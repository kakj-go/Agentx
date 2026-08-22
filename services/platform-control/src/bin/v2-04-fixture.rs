use std::{collections::BTreeSet, env};

use agentx_bundle_builder::{WorkPackageBuildSource, build_work_package, compile_workflow_version};
use agentx_control_infrastructure::{
    ControlInfrastructureSettings, connect_control_mysql, control_object_store,
};
use agentx_domain::WorkflowDefinition;
use agentx_runtime_contracts::{
    CancelWorkPackageRequestV1, ControlRole, ExecuteWorkPackageRequestV1,
    PrepareWorkPackageRequestV1, RuntimeAuthorizationSnapshotV1, RuntimeCallPurposeV1,
    RuntimeEvaluationCaseV1, RuntimeEvaluatorV1, RuntimePolicyV1, RuntimeSkillProgramV1,
    RuntimeWorkPackageOverlayV1, RuntimeWorkPackageSpecV1, ServiceClaimsV1, WorkPackagePurpose,
    issue_service_token, now_unix,
};
use anyhow::{Context, Result};
use bytes::Bytes;
use ed25519_dalek::{SigningKey, pkcs8::DecodePrivateKey};
use object_store::{ObjectStore, path::Path as ObjectPath};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

const TENANT: &str = "018f0000-0000-7000-8000-000000000001";
const USER: &str = "018f0000-0000-7000-8000-000000000002";
const DEPARTMENT: &str = "018f0000-0000-7000-8000-000000000005";
const ENVIRONMENT: &str = "018f0000-0000-7000-8000-000000000007";
const WORKFLOW: &str = "018f0000-0000-7000-8000-000000000401";
const IDENTITY: &str = "018f0000-0000-7000-8000-000000000402";
const VERSION: &str = "018f0000-0000-7000-8000-000000000403";
const WORKFLOW_DEPLOYMENT: &str = "018f0000-0000-7000-8000-000000000404";
const APPLICATION: &str = "018f0000-0000-7000-8000-000000000405";
const CHILD_WORKFLOW: &str = "018f0000-0000-7000-8000-000000000411";
const CHILD_VERSION: &str = "018f0000-0000-7000-8000-000000000412";
const GRANDCHILD_WORKFLOW: &str = "018f0000-0000-7000-8000-000000000413";
const GRANDCHILD_VERSION: &str = "018f0000-0000-7000-8000-000000000414";
const MODEL: &str = "018f0000-0000-7000-8000-000000000421";
const MODEL_VERSION: &str = "018f0000-0000-7000-8000-000000000422";
const MCP: &str = "018f0000-0000-7000-8000-000000000423";
const MCP_VERSION: &str = "018f0000-0000-7000-8000-000000000424";
const RAG: &str = "018f0000-0000-7000-8000-000000000425";
const RAG_VERSION: &str = "018f0000-0000-7000-8000-000000000426";
const MEMORY: &str = "018f0000-0000-7000-8000-000000000427";
const MEMORY_VERSION: &str = "018f0000-0000-7000-8000-000000000428";
const SKILL: &str = "018f0000-0000-7000-8000-000000000429";
const SKILL_VERSION: &str = "018f0000-0000-7000-8000-00000000042a";
const SANDBOX_PROFILE: &str = "018f0000-0000-7000-8000-00000000042b";
const SANDBOX_VERSION: &str = "018f0000-0000-7000-8000-00000000042c";
const WAIT_WORKFLOW: &str = "018f0000-0000-7000-8000-000000000431";
const WAIT_IDENTITY: &str = "018f0000-0000-7000-8000-000000000432";
const APPROVAL_WORKFLOW: &str = "018f0000-0000-7000-8000-000000000433";
const APPROVAL_IDENTITY: &str = "018f0000-0000-7000-8000-000000000434";
const EVALUATION_PACKAGE: &str = "018f0000-0000-7000-8000-000000000441";
const CANCELLED_EVALUATION_PACKAGE: &str = "018f0000-0000-7000-8000-000000000442";

struct ResourceFixture {
    node_id: &'static str,
    resource_type: &'static str,
    resource_id: Uuid,
    version_id: Uuid,
    operation: &'static str,
    snapshot: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mode = env::var("AGENTX_V2_FIXTURE_MODE").unwrap_or_else(|_| "seed".into());
    if mode == "evaluation" {
        let seed = env::var("AGENTX_V2_FIXTURE_ID_SEED").unwrap_or_default();
        let success_id = if seed.is_empty() {
            id(EVALUATION_PACKAGE)?
        } else {
            fixture_id(&seed, "evaluation-success")
        };
        let cancellation_id = if seed.is_empty() {
            id(CANCELLED_EVALUATION_PACKAGE)?
        } else {
            fixture_id(&seed, "evaluation-cancelled")
        };
        let evaluation = publish_evaluation_packages(
            success_id,
            cancellation_id,
            &grandchild_definition()?,
            &suspension_definition("wait")?,
        )
        .await?;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "apiVersion":"agentx.io/v2-04-fixture/v1",
                "mode":"evaluation",
                "evaluation":evaluation
            }))?
        );
        return Ok(());
    }
    anyhow::ensure!(
        mode == "seed",
        "AGENTX_V2_FIXTURE_MODE must be seed or evaluation"
    );
    let dependencies_namespace = env::var("AGENTX_V2_FIXTURE_DEPENDENCIES_NAMESPACE")
        .context("AGENTX_V2_FIXTURE_DEPENDENCIES_NAMESPACE is required")?;
    anyhow::ensure!(
        dependencies_namespace
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '-'),
        "dependencies Namespace is not a DNS label"
    );
    let settings = ControlInfrastructureSettings::from_env()?;
    let pool = connect_control_mysql(&settings.mysql).await?;
    let objects = control_object_store(&settings.object_storage)?;
    let tenant = id(TENANT)?;
    let workflow_version = id(VERSION)?;
    let child_version = id(CHILD_VERSION)?;
    let skill_version = id(SKILL_VERSION)?;

    let skill = RuntimeSkillProgramV1 {
        schema_version: 1,
        instructions: "Return the immutable V2-04 Skill result.".into(),
        dependency_object_ids: vec![],
    };
    let skill_bytes = agentx_runtime_contracts::canonical_bytes(&skill)?;
    let skill_hash = agentx_runtime_contracts::content_hash(&skill)?;
    let skill_source_key = format!(
        "control/{tenant}/{skill_version}/{}",
        skill_hash
            .as_str()
            .strip_prefix("sha256:")
            .expect("ContentHash has sha256 prefix")
    );
    objects
        .put(
            &ObjectPath::from(skill_source_key.clone()),
            Bytes::from(skill_bytes.clone()).into(),
        )
        .await?;

    let child_definition = child_definition()?;
    compile_workflow_version(&child_definition, child_version)
        .context("V2-04 child Workflow does not compile")?;
    let grandchild_definition = grandchild_definition()?;
    compile_workflow_version(&grandchild_definition, id(GRANDCHILD_VERSION)?)
        .context("V2-04 grandchild Workflow does not compile")?;
    let definition = full_definition()?;
    compile_workflow_version(&definition, workflow_version)
        .context("V2-04 full Workflow does not compile")?;
    let wait_definition = suspension_definition("wait")?;
    compile_workflow_version(&wait_definition, id(WAIT_WORKFLOW)?)
        .context("V2-04 Wait Debug Workflow does not compile")?;
    let approval_definition = suspension_definition("approval")?;
    compile_workflow_version(&approval_definition, id(APPROVAL_WORKFLOW)?)
        .context("V2-04 Approval Debug Workflow does not compile")?;
    let resources = resource_fixtures(
        &dependencies_namespace,
        tenant,
        &skill_source_key,
        skill_hash.as_str(),
        skill_bytes.len() as u64,
    )?;

    let mut tx = pool.begin().await?;
    seed_workflows(
        &mut tx,
        &definition,
        &child_definition,
        &grandchild_definition,
        &wait_definition,
        &approval_definition,
    )
    .await?;
    seed_resources(&mut tx, &resources).await?;
    tx.commit().await?;

    println!(
        "{}",
        serde_json::to_string(&json!({
            "apiVersion":"agentx.io/v2-04-fixture/v1",
            "tenantId":TENANT,
            "applicationId":APPLICATION,
            "applicationSlug":"v2-04-runtime-engine",
            "workflowId":WORKFLOW,
            "workflowVersionId":VERSION,
            "environmentId":ENVIRONMENT,
            "serviceIdentityId":IDENTITY,
            "childWorkflowVersionId":CHILD_VERSION,
            "grandchildWorkflowVersionId":GRANDCHILD_VERSION,
            "waitWorkflowId":WAIT_WORKFLOW,
            "approvalWorkflowId":APPROVAL_WORKFLOW,
            "skillObjectId":SKILL_VERSION,
            "resources":{
                "model":MODEL,
                "mcp":MCP,
                "rag":RAG,
                "memory":MEMORY,
                "skill":SKILL,
                "sandboxProfile":SANDBOX_PROFILE
            }
        }))?
    );
    Ok(())
}

async fn publish_evaluation_packages(
    success_id: Uuid,
    cancellation_id: Uuid,
    success_definition: &WorkflowDefinition,
    cancellation_definition: &WorkflowDefinition,
) -> Result<Value> {
    let settings = ControlInfrastructureSettings::from_env()?;
    let pool = connect_control_mysql(&settings.mysql).await?;
    let runtime_url = env::var("AGENTX_RUNTIME_INTERNAL_URL")
        .context("AGENTX_RUNTIME_INTERNAL_URL is required")?
        .trim_end_matches('/')
        .to_owned();
    let signing_key = SigningKey::from_pkcs8_pem(&env::var(
        "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM",
    )?)?;
    let signing_kid = env::var("AGENTX_CONTROL_WORK_PACKAGE_KEY_ID")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    persist_evaluation_intent(&pool, success_id, "V2 Evaluation Fixture").await?;
    let success = build_evaluation_package(
        success_id,
        success_definition.clone(),
        2,
        &signing_kid,
        &signing_key,
    )?;
    runtime_post::<agentx_runtime_contracts::PublishReceiptV1, _>(
        &client,
        &runtime_url,
        "runtime.work-packages.prepare",
        "/internal/runtime/v1/work-packages:prepare",
        &PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("v2-04:evaluation:prepare:{success_id}"),
            work_package: success,
        },
    )
    .await?;
    let success_receipt = runtime_post::<agentx_runtime_contracts::ApplyReceiptV1, _>(
        &client,
        &runtime_url,
        "runtime.work-packages.execute",
        &format!("/internal/runtime/v1/work-packages/{success_id}:execute"),
        &ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("v2-04:evaluation:execute:{success_id}"),
            package_id: success_id,
            input: Value::Null,
        },
    )
    .await?;

    persist_evaluation_intent(&pool, cancellation_id, "V2 Cancelled Evaluation Fixture").await?;
    let cancellation = build_evaluation_package(
        cancellation_id,
        cancellation_definition.clone(),
        1,
        &signing_kid,
        &signing_key,
    )?;
    runtime_post::<agentx_runtime_contracts::PublishReceiptV1, _>(
        &client,
        &runtime_url,
        "runtime.work-packages.prepare",
        "/internal/runtime/v1/work-packages:prepare",
        &PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("v2-04:evaluation:prepare:{cancellation_id}"),
            work_package: cancellation,
        },
    )
    .await?;
    let cancellation_receipt = runtime_post::<agentx_runtime_contracts::ApplyReceiptV1, _>(
        &client,
        &runtime_url,
        "runtime.work-packages.execute",
        &format!("/internal/runtime/v1/work-packages/{cancellation_id}:execute"),
        &ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("v2-04:evaluation:execute:{cancellation_id}"),
            package_id: cancellation_id,
            input: Value::Null,
        },
    )
    .await?;
    let cancellation_version = cancellation_receipt.object_version;
    let cancelled = runtime_post::<agentx_runtime_contracts::ApplyReceiptV1, _>(
        &client,
        &runtime_url,
        "runtime.work-packages.cancel",
        &format!("/internal/runtime/v1/work-packages/{cancellation_id}:cancel"),
        &CancelWorkPackageRequestV1 {
            api_version: 1,
            tenant_id: id(TENANT)?,
            package_id: cancellation_id,
            expected_version: cancellation_version,
            idempotency_key: format!("v2-04:evaluation:cancel:{cancellation_id}"),
        },
    )
    .await?;
    Ok(json!({
        "completedPackageId":success_id,
        "completedExecutionIds":success_receipt.result.get("executionIds").cloned().unwrap_or(Value::Null),
        "cancelledPackageId":cancellation_id,
        "cancelledExecutionIds":cancellation_receipt.result.get("executionIds").cloned().unwrap_or(Value::Null),
        "cancelReceiptApplied":cancelled.applied
    }))
}

async fn persist_evaluation_intent(pool: &MySqlPool, package_id: Uuid, name: &str) -> Result<()> {
    sqlx::query("INSERT INTO evaluation_runs(id,tenant_id,name,workflow_version_id,dataset_version_id,evaluation_profile_version_id,work_package_id,intent_version,parameters_json,status,created_by,owner_department_id,visibility) VALUES(?,?,?,?,?,?,?,1,?,'queued',?,?,'private') ON DUPLICATE KEY UPDATE name=VALUES(name),work_package_id=VALUES(work_package_id)")
        .bind(fixture_id(&package_id.to_string(), "control-evaluation-run"))
        .bind(id(TENANT)?)
        .bind(name)
        .bind(id(VERSION)?)
        .bind(fixture_id(&package_id.to_string(), "dataset-version"))
        .bind(fixture_id(&package_id.to_string(), "profile-version"))
        .bind(package_id)
        .bind(json!({"fixture": "v2-evaluation", "packageId": package_id}))
        .bind(id(USER)?)
        .bind(id(DEPARTMENT)?)
        .execute(pool)
        .await?;
    Ok(())
}

fn fixture_id(seed: &str, purpose: &str) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(seed.as_bytes());
    digest.update([0]);
    digest.update(purpose.as_bytes());
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn build_evaluation_package(
    package_id: Uuid,
    definition: WorkflowDefinition,
    case_count: usize,
    signing_kid: &str,
    signing_key: &SigningKey,
) -> Result<agentx_runtime_contracts::RuntimeWorkPackageV1> {
    let created_at = OffsetDateTime::now_utc();
    let cases = (0..case_count)
        .map(|index| RuntimeEvaluationCaseV1 {
            case_id: Uuid::now_v7(),
            input: json!({"message":format!("v2-04-evaluation-{}",index + 1)}),
            expected_output: None,
        })
        .collect();
    Ok(build_work_package(
        WorkPackageBuildSource {
            package_id,
            tenant_id: id(TENANT)?,
            origin: agentx_runtime_contracts::ExecutionOriginV1::system(None),
            purpose: WorkPackagePurpose::Evaluation,
            call_purpose: RuntimeCallPurposeV1::Evaluation,
            spec: RuntimeWorkPackageSpecV1::Evaluation {
                dataset_version_id: Uuid::now_v7(),
                profile_version_id: Uuid::now_v7(),
                cases,
                evaluators: vec![RuntimeEvaluatorV1::DeterministicRule {
                    evaluator_id: Uuid::now_v7(),
                    expression: "not_null".into(),
                }],
            },
            source_revision: format!("v2-04-evaluation:{package_id}"),
            definition,
            dependency_versions: Default::default(),
            supported_capabilities: BTreeSet::from(["builtin".into()]),
            overlay: RuntimeWorkPackageOverlayV1::default(),
            resources: vec![],
            authorization: RuntimeAuthorizationSnapshotV1 {
                schema_version: 1,
                tenant_id: id(TENANT)?,
                service_identity_id: id(IDENTITY)?,
                workflow_id: id(WORKFLOW)?,
                policy_epoch: 1,
                grant_ids: vec![],
                capabilities: BTreeSet::from(["builtin".into()]),
                maximum_policy_staleness_seconds: 72 * 60 * 60,
                captured_at: created_at,
            },
            objects: vec![],
            runtime_policy: RuntimePolicyV1::default(),
            created_at,
            expires_at: created_at + time::Duration::hours(24),
        },
        signing_kid,
        signing_key,
    )?)
}

async fn runtime_post<T: DeserializeOwned, B: Serialize>(
    client: &reqwest::Client,
    runtime_url: &str,
    scope: &str,
    path: &str,
    body: &B,
) -> Result<T> {
    let now = now_unix();
    let token = issue_service_token(
        &env::var("AGENTX_CONTROL_PUBLISHER_JWT_KID")?,
        env::var("AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM")?.as_bytes(),
        &ServiceClaimsV1 {
            iss: "agentx-control".into(),
            aud: "agentx-runtime-internal".into(),
            sub: "v2-04-fixture".into(),
            role: ControlRole::Publisher,
            scope: BTreeSet::from([scope.into()]),
            iat: now,
            exp: now + 300,
            jti: Uuid::now_v7(),
        },
    )?;
    let response = client
        .post(format!("{runtime_url}{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    anyhow::ensure!(
        status.is_success(),
        "Runtime Work Package request {path} failed with HTTP {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn id(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(Into::into)
}

fn reference(
    binding_id: &str,
    binding_role: Option<&str>,
    resource_type: &str,
    resource_id: &str,
    resource_version_id: &str,
    operation: &str,
) -> Value {
    let mut value = json!({
        "bindingId":binding_id,
        "resourceType":resource_type,
        "resourceId":resource_id,
        "resourceVersionId":resource_version_id,
        "operation":operation
    });
    if let Some(role) = binding_role {
        value["bindingRole"] = json!(role);
    }
    value
}

fn full_definition() -> Result<WorkflowDefinition> {
    let nodes = vec![
        json!({"id":"model","key":"model","type":"model","typeVersion":1,"name":"Model","parameters":{"prompt":"你叫 kakj","userQuestion":"你好，你叫什么"},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("model-main",None,"model",MODEL,MODEL_VERSION,"use")]}),
        json!({"id":"mcp","key":"mcp","type":"mcp_tool","typeVersion":1,"name":"MCP","parameters":{"arguments":{"text":"agentx-v2-04"}},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("mcp-main",None,"mcp_tool",MCP,MCP_VERSION,"use")]}),
        json!({"id":"rag","key":"rag","type":"rag","typeVersion":1,"name":"RAG","parameters":{"operation":"query","input":{"query":"agentx-v2-04","mode":"naive"}},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("rag-main",None,"rag",RAG,RAG_VERSION,"read")]}),
        json!({"id":"memory","key":"memory","type":"memory","typeVersion":1,"name":"Memory","parameters":{"operation":"add","input":{"text":"agentx-v2-04-memory"}},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("memory-main",None,"memory",MEMORY,MEMORY_VERSION,"write")]}),
        json!({"id":"skill","key":"skill","type":"skill","typeVersion":1,"name":"Skill","parameters":{"resourceId":SKILL},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("skill-main",None,"skill",SKILL,SKILL_VERSION,"use")]}),
        json!({"id":"agent","key":"agent","type":"agent","typeVersion":1,"name":"Agent","parameters":{"systemPrompt":"Use the fixed tool once.","userQuestion":"agentx-v2-04","maxIterations":3,"maxModelCalls":3,"maxToolCalls":2,"maxTotalTokens":1000,"maxOutputTokens":256,"maxCostMicros":1000,"maxDurationMs":30000,"limitAction":"fail"},"outputProjection":{},"contextWrites":[],"resourceReferences":[
            reference("agent-model",Some("ai_model"),"model",MODEL,MODEL_VERSION,"use"),
            reference("agent-tool",Some("ai_tool"),"mcp_tool",MCP,MCP_VERSION,"use"),
            reference("agent-rag",Some("ai_retriever"),"rag",RAG,RAG_VERSION,"read"),
            reference("agent-memory",Some("ai_memory"),"memory",MEMORY,MEMORY_VERSION,"read"),
            reference("agent-skill",Some("ai_skill"),"skill",SKILL,SKILL_VERSION,"use")
        ]}),
        json!({"id":"composite","key":"composite","type":"sub_workflow","typeVersion":1,"name":"Composite","parameters":{"workflowVersionId":CHILD_VERSION},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}),
        json!({"id":"code","key":"code","type":"code","typeVersion":1,"name":"Sandbox","parameters":{"runner":"python","source":"import json\nimport time\ntime.sleep(5)\nprint(json.dumps({'message':'agentx-v2-04'}))","arguments":[],"networkPolicy":{"egressMode":"none"}},"outputProjection":{},"contextWrites":[],"resourceReferences":[reference("sandbox-main",None,"sandbox_profile",SANDBOX_PROFILE,SANDBOX_VERSION,"use")]}),
    ];
    let mut connections = Vec::new();
    let sequence = [
        "model",
        "mcp",
        "rag",
        "memory",
        "skill",
        "agent",
        "composite",
        "code",
    ];
    connections.push(json!({"id":"start-model","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"model","targetHandle":"main","order":0}));
    for pair in sequence.windows(2) {
        connections.push(json!({"id":format!("{}-{}",pair[0],pair[1]),"sourceNodeId":pair[0],"sourceHandle":"main","targetNodeId":pair[1],"targetHandle":"main","order":0}));
    }
    connections.push(json!({"id":"code-end","sourceNodeId":"code","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}));
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"contexts":{}},
        "nodes":nodes,
        "connections":connections,
        "end":{"outputs":{"stdout":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"code","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["stdout"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true}}},
        "settings":{"activationBudget":64,"executionOrder":"deterministic"}
    }))
    .map_err(Into::into)
}

fn child_definition() -> Result<WorkflowDefinition> {
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","additionalProperties":true},"contexts":{}},
        "nodes":[{"id":"grandchild","key":"grandchild","type":"sub_workflow","typeVersion":1,"name":"Fixed Grandchild","parameters":{"workflowVersionId":GRANDCHILD_VERSION},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],
        "connections":[
            {"id":"child-start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"grandchild","targetHandle":"main","order":0},
            {"id":"child-end","sourceNodeId":"grandchild","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{}},
        "settings":{"activationBudget":8,"executionOrder":"deterministic"}
    }))
    .map_err(Into::into)
}

fn grandchild_definition() -> Result<WorkflowDefinition> {
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","additionalProperties":true},"contexts":{}},
        "nodes":[{"id":"grandchild-pass","key":"grandchild_pass","type":"no_op","typeVersion":1,"name":"Grandchild Pass","parameters":{},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],
        "connections":[
            {"id":"grandchild-start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"grandchild-pass","targetHandle":"main","order":0},
            {"id":"grandchild-end","sourceNodeId":"grandchild-pass","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{}},
        "settings":{"activationBudget":8,"executionOrder":"deterministic"}
    }))
    .map_err(Into::into)
}

fn suspension_definition(node_type: &str) -> Result<WorkflowDefinition> {
    let parameters = if node_type == "approval" {
        json!({"title":"V2-04 Runtime approval","description":"Kubernetes E2E decision","candidateUserId":USER,"timeoutMs":300000})
    } else {
        json!({"kind":"webhook","authenticationMode":"signed","payloadSchema":{"type":"object"}})
    };
    let connections = if node_type == "approval" {
        vec![
            json!({"id":"start-suspend","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"suspend","targetHandle":"main","order":0}),
            json!({"id":"approved-end","sourceNodeId":"suspend","sourceHandle":"approved","targetNodeId":"__end__","targetHandle":"main","order":0}),
            json!({"id":"rejected-end","sourceNodeId":"suspend","sourceHandle":"rejected","targetNodeId":"__end__","targetHandle":"main","order":1}),
        ]
    } else {
        vec![
            json!({"id":"start-suspend","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"suspend","targetHandle":"main","order":0}),
            json!({"id":"resumed-end","sourceNodeId":"suspend","sourceHandle":"resumed","targetNodeId":"__end__","targetHandle":"main","order":0}),
        ]
    };
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","additionalProperties":true},"contexts":{}},
        "nodes":[{"id":"suspend","key":"suspend","type":node_type,"typeVersion":1,"name":"Suspend","parameters":parameters,"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],
        "connections":connections,
        "end":{"outputs":{}},
        "settings":{"activationBudget":20,"executionOrder":"deterministic"}
    }))
    .map_err(Into::into)
}

fn vault_reference(tenant: Uuid, credential: &str) -> Value {
    json!({
        "mount":"secret",
        "path":format!("tenants/{tenant}/runtime-credentials/{credential}"),
        "key":"value",
        "version":1
    })
}

fn resource_fixtures(
    dependencies_namespace: &str,
    tenant: Uuid,
    skill_source_key: &str,
    skill_hash: &str,
    skill_size: u64,
) -> Result<Vec<ResourceFixture>> {
    let echo = format!("http://echo-mcp.{dependencies_namespace}.svc:8090");
    let rag = format!("http://lightrag.{dependencies_namespace}.svc:9621");
    let memory = format!("http://mem0.{dependencies_namespace}.svc:8000");
    let mcp_schema_hash = agentx_runtime_contracts::content_hash(&json!({"type":"object"}))?;
    let skill_object = json!({
        "objectId":SKILL_VERSION,
        "sourceKey":skill_source_key,
        "contentHash":skill_hash,
        "sizeBytes":skill_size,
        "mediaType":"application/vnd.agentx.skill-program.v1+json"
    });
    Ok(vec![
        resource(
            "model",
            "model",
            MODEL,
            MODEL_VERSION,
            "use",
            json!({"providerType":"fixture","endpoint":format!("{echo}/v2/runtime/model"),"modelName":"agentx-v2-04-model","price":{"versionId":"fixture-v1"},"resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"model")}),
        )?,
        resource(
            "mcp",
            "mcp_tool",
            MCP,
            MCP_VERSION,
            "use",
            json!({"endpoint":format!("{echo}/mcp"),"toolName":"echo","schemaHash":mcp_schema_hash,"resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"model")}),
        )?,
        resource(
            "rag",
            "rag",
            RAG,
            RAG_VERSION,
            "read",
            json!({"endpoint":rag,"externalResourceId":"v2-04","resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"rag")}),
        )?,
        resource(
            "memory",
            "memory",
            MEMORY,
            MEMORY_VERSION,
            "write",
            json!({"endpoint":memory,"externalNamespace":"v2-04","resourceVersion":1}),
        )?,
        resource(
            "skill",
            "skill",
            SKILL,
            SKILL_VERSION,
            "use",
            json!({"resourceVersion":1,"dependencyObjectIds":[],"runtimeObjects":[skill_object]}),
        )?,
        resource(
            "code",
            "sandbox_profile",
            SANDBOX_PROFILE,
            SANDBOX_VERSION,
            "use",
            json!({"provider":"opensandbox","imageDigest":"opensandbox/code-interpreter:latest","cpuMillis":500,"memoryBytes":536870912_u64,"diskBytes":1073741824_u64,"pidsLimit":256,"networkPolicy":{"defaultAction":"deny"},"timeoutSeconds":120,"resourceVersion":1}),
        )?,
        resource(
            "agent",
            "model",
            MODEL,
            MODEL_VERSION,
            "use",
            json!({"providerType":"fixture","endpoint":format!("{echo}/v2/runtime/model"),"modelName":"agentx-v2-04-model","price":{"versionId":"fixture-v1"},"resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"model")}),
        )?,
        resource(
            "agent",
            "mcp_tool",
            MCP,
            MCP_VERSION,
            "use",
            json!({"endpoint":format!("{echo}/mcp"),"toolName":"echo","schemaHash":mcp_schema_hash,"resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"model")}),
        )?,
        resource(
            "agent",
            "rag",
            RAG,
            RAG_VERSION,
            "read",
            json!({"endpoint":rag,"externalResourceId":"v2-04","resourceVersion":1,"vaultSecretRef":vault_reference(tenant,"rag")}),
        )?,
        resource(
            "agent",
            "memory",
            MEMORY,
            MEMORY_VERSION,
            "read",
            json!({"endpoint":memory,"externalNamespace":"v2-04","resourceVersion":1}),
        )?,
        resource(
            "agent",
            "skill",
            SKILL,
            SKILL_VERSION,
            "use",
            json!({"resourceVersion":1,"dependencyObjectIds":[],"runtimeObjects":[skill_object]}),
        )?,
    ])
}

fn resource(
    node_id: &'static str,
    resource_type: &'static str,
    resource_id: &str,
    version_id: &str,
    operation: &'static str,
    snapshot: Value,
) -> Result<ResourceFixture> {
    Ok(ResourceFixture {
        node_id,
        resource_type,
        resource_id: id(resource_id)?,
        version_id: id(version_id)?,
        operation,
        snapshot,
    })
}

async fn seed_workflows(
    tx: &mut Transaction<'_, MySql>,
    definition: &WorkflowDefinition,
    child_definition: &WorkflowDefinition,
    grandchild_definition: &WorkflowDefinition,
    wait_definition: &WorkflowDefinition,
    approval_definition: &WorkflowDefinition,
) -> Result<()> {
    let tenant = id(TENANT)?;
    let user = id(USER)?;
    let department = id(DEPARTMENT)?;
    let workflow = id(WORKFLOW)?;
    let child_workflow = id(CHILD_WORKFLOW)?;
    let grandchild_workflow = id(GRANDCHILD_WORKFLOW)?;
    let version = id(VERSION)?;
    let child_version = id(CHILD_VERSION)?;
    let grandchild_version = id(GRANDCHILD_VERSION)?;
    let environment = id(ENVIRONMENT)?;
    let deployment = id(WORKFLOW_DEPLOYMENT)?;
    let application = id(APPLICATION)?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(?,?,'V2-04 Runtime Engine','active','private',?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)")
        .bind(workflow).bind(tenant).bind(user).bind(department).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(?,?,'V2-04 Composite Child','active','private',?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)")
        .bind(child_workflow).bind(tenant).bind(user).bind(department).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(?,?,'V2-04 Composite Grandchild','active','private',?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)")
        .bind(grandchild_workflow).bind(tenant).bind(user).bind(department).execute(&mut **tx).await?;
    for (workflow_id, name, identity_id, draft) in [
        (
            id(WAIT_WORKFLOW)?,
            "V2-04 Wait Debug",
            id(WAIT_IDENTITY)?,
            wait_definition,
        ),
        (
            id(APPROVAL_WORKFLOW)?,
            "V2-04 Approval Debug",
            id(APPROVAL_IDENTITY)?,
            approval_definition,
        ),
    ] {
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,status,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,'active','private',?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)")
            .bind(workflow_id).bind(tenant).bind(name).bind(user).bind(department).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id,status,version) VALUES(?,?,?,'active',1) ON DUPLICATE KEY UPDATE status='active',version=1")
            .bind(identity_id).bind(tenant).bind(workflow_id).execute(&mut **tx).await?;
        let draft_value = serde_json::to_value(draft)?;
        let draft_hash = agentx_runtime_contracts::content_hash(draft)?;
        sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,revision,definition_json,content_hash,updated_by) VALUES(?,?,?,'5.0',1,?,?,?) ON DUPLICATE KEY UPDATE revision=1,definition_json=VALUES(definition_json),content_hash=VALUES(content_hash),updated_by=VALUES(updated_by)")
            .bind(Uuid::now_v7()).bind(tenant).bind(workflow_id).bind(draft_value).bind(draft_hash.as_str()).bind(user).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id,status,version) VALUES(?,?,?,'active',1) ON DUPLICATE KEY UPDATE status='active',version=1")
        .bind(id(IDENTITY)?).bind(tenant).bind(workflow).execute(&mut **tx).await?;
    for (version_id, workflow_id, value, hash) in [
        (
            version,
            workflow,
            serde_json::to_value(definition)?,
            agentx_runtime_contracts::content_hash(definition)?.to_string(),
        ),
        (
            child_version,
            child_workflow,
            serde_json::to_value(child_definition)?,
            agentx_runtime_contracts::content_hash(child_definition)?.to_string(),
        ),
        (
            grandchild_version,
            grandchild_workflow,
            serde_json::to_value(grandchild_definition)?,
            agentx_runtime_contracts::content_hash(grandchild_definition)?.to_string(),
        ),
    ] {
        sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'5.0',?,?,?) ON DUPLICATE KEY UPDATE definition_json=VALUES(definition_json),content_hash=VALUES(content_hash)")
            .bind(version_id).bind(tenant).bind(workflow_id).bind(value).bind(hash).bind(user).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO workflow_deployments(id,tenant_id,workflow_id,environment_id,workflow_version_id,sequence_number,status,source,created_by) VALUES(?,?,?,?,?,1,'active','publish',?) ON DUPLICATE KEY UPDATE status='active'")
        .bind(deployment).bind(tenant).bind(workflow).bind(environment).bind(version).bind(user).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_deployment_heads(tenant_id,workflow_id,environment_id,active_deployment_id,version) VALUES(?,?,?,?,1) ON DUPLICATE KEY UPDATE active_deployment_id=VALUES(active_deployment_id),version=version+1")
        .bind(tenant).bind(workflow).bind(environment).bind(deployment).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO applications(id,tenant_id,workflow_id,name,slug,visibility,owner_user_id,owner_department_id,status) VALUES(?,?,?,'V2-04 Runtime Engine','v2-04-runtime-engine','private',?,?,'draft') ON DUPLICATE KEY UPDATE status='draft'")
        .bind(application).bind(tenant).bind(workflow).bind(user).bind(department).execute(&mut **tx).await?;
    Ok(())
}

async fn seed_resources(
    tx: &mut Transaction<'_, MySql>,
    resources: &[ResourceFixture],
) -> Result<()> {
    let tenant = id(TENANT)?;
    let identity = id(IDENTITY)?;
    let user = id(USER)?;
    let version = id(VERSION)?;
    for (index, resource) in resources.iter().enumerate() {
        let snapshot_hash = agentx_runtime_contracts::content_hash(&resource.snapshot)?;
        sqlx::query("INSERT INTO workflow_version_resources(id,tenant_id,workflow_version_id,node_id,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash) VALUES(?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE snapshot_json=VALUES(snapshot_json),snapshot_hash=VALUES(snapshot_hash)")
            .bind(Uuid::now_v7()).bind(tenant).bind(version).bind(resource.node_id)
            .bind(resource.resource_type).bind(resource.resource_id).bind(resource.version_id)
            .bind(resource.operation).bind(&resource.snapshot).bind(snapshot_hash.as_str())
            .execute(&mut **tx).await?;
        let grant_operation = if resource.operation == "write" {
            "write"
        } else if resource.operation == "read" {
            "read"
        } else {
            "use"
        };
        sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,?,?,?,?,?) ON DUPLICATE KEY UPDATE resource_version_id=VALUES(resource_version_id)")
            .bind(Uuid::now_v7()).bind(tenant).bind(identity).bind(resource.resource_type)
            .bind(resource.resource_id).bind(resource.version_id).bind(grant_operation).bind(user)
            .execute(&mut **tx).await?;
        if index == 0 {
            anyhow::ensure!(snapshot_hash.as_str().starts_with("sha256:"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspension_fixtures_follow_the_current_node_manifests() {
        for (node_type, workflow_id) in [("wait", WAIT_WORKFLOW), ("approval", APPROVAL_WORKFLOW)] {
            let definition = suspension_definition(node_type).unwrap();
            compile_workflow_version(&definition, id(workflow_id).unwrap()).unwrap();
        }
    }

    #[test]
    fn seeded_fixture_ids_are_deterministic_and_purpose_scoped() {
        assert_eq!(fixture_id("run", "success"), fixture_id("run", "success"));
        assert_ne!(fixture_id("run", "success"), fixture_id("run", "cancelled"));
    }
}
