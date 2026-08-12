use agentx_domain::{WorkflowDefinition, canonical_content_hash};
use agentx_infrastructure::{
    config::InfrastructureSettings,
    credential::{CredentialKeyring, PlainSecret},
    mysql,
};
use agentx_runtime::{CompileContext, NodeRegistry, WorkflowCompiler};
use anyhow::{Context, Result};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let settings = InfrastructureSettings::from_env()?;
    let pool = mysql::connect(&settings.mysql).await?;
    let owner = sqlx::query("SELECT u.tenant_id,u.id user_id,ud.department_id FROM users u JOIN user_departments ud ON ud.tenant_id=u.tenant_id AND ud.user_id=u.id WHERE u.username='admin' AND u.status='active' ORDER BY u.created_at LIMIT 1")
        .fetch_optional(&pool).await?.context("E2E company admin is missing")?;
    let tenant_id: Uuid = owner.try_get("tenant_id")?;
    let user_id: Uuid = owner.try_get("user_id")?;
    let department_id: Uuid = owner.try_get("department_id")?;
    let endpoint = std::env::var("AGENTX_REMOTE_NODE_ENDPOINT")
        .unwrap_or_else(|_| "http://echo-node:8080".into());
    let credential_id = ensure_fixture_credential(&pool, tenant_id, user_id, department_id).await?;

    let child = create_workflow(&pool, tenant_id, user_id, department_id, "M4 Child Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("child-output", "set", "Child Output", 320, 120, json!({"values":{"childCompleted":true}}))
        ],
        "connections":[],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Runtime Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "settings":{"executionOrder":"deterministic","activationBudget":100},
        "nodes":[
            node("left", "set", "Left Branch", 280, 100, json!({"values":{"left":true}})),
            node("right", "set", "Right Branch", 280, 260, json!({"values":{"right":true}})),
            node("merge", "merge", "Required Merge", 500, 180, json!({"mode":"combine_by_position"})),
            node("loop", "loop_over_items", "Loop Over Items", 700, 180, json!({"batchSize":1})),
            node("child", "sub_workflow", "Fixed Sub-workflow", 900, 180, json!({"workflowVersionId":child.to_string()})),
            node("remote", "remote_action", "Remote Echo", 1110, 180, json!({"endpoint":endpoint,"fixtureMode":"echo"}))
        ],
        "connections":[
            edge("left-merge", "left", "main", "merge", "main:0"),
            edge("right-merge", "right", "main", "merge", "main:1"),
            edge("merge-loop", "merge", "main", "loop", "main"),
            edge("loop-back", "loop", "loop", "loop", "main"),
            edge_with_order("loop-done", "loop", "done", "child", "main", 1),
            edge("child-remote", "child", "main", "remote", "main")
        ],
        "end":{"outputs":{}}
    })).await?;

    let broker_version = create_workflow(&pool, tenant_id, user_id, department_id, "M4 Broker Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            {"id":"broker","key":"broker","type":"remote_action","typeVersion":1,"name":"Remote Broker Probe","parameters":{"endpoint":endpoint,"fixtureMode":"broker"},"resourceReferences":[{"resourceType":"credential","resourceId":credential_id,"operation":"use"}]}
        ],
        "connections":[],
        "end":{"outputs":{}}
    })).await?;
    ensure_credential_snapshot(&pool, tenant_id, user_id, broker_version, credential_id).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Fault Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            {"id":"delayed-remote","key":"delayed_remote","type":"remote_action","typeVersion":1,"name":"Delayed Remote Echo","parameters":{"endpoint":endpoint,"fixtureMode":"delay","delayMs":15000},"settings":{"retryOnFail":true,"maxTries":2,"waitBetweenTriesMs":250}}
        ],
        "connections":[],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(
        &pool,
        tenant_id,
        user_id,
        department_id,
        "M4 Cycle Budget Fixture",
        json!({
            "schemaVersion":"4.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "settings":{"executionOrder":"deterministic","activationBudget":7},
            "nodes":[
                node("root", "no_op", "Cycle Root", 80, 160, json!({})),
                node("step", "set", "Cycle Step", 300, 160, json!({"values":{"cycled":true}})),
                node("branch", "if", "Cycle Branch", 540, 160, json!({"condition":true}))
            ],
            "connections":[
                edge("cycle-start", "root", "main", "step", "main"),
                edge("cycle-forward", "step", "main", "branch", "main"),
                edge("cycle-back", "branch", "true", "step", "main")
            ],
            "end":{"outputs":{}}
        }),
    )
    .await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Wait Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("wait", "wait", "Signed Webhook Wait", 340, 160, json!({"kind":"webhook","authenticationMode":"signed"})),
            node("resumed", "set", "Resume Output", 620, 160, json!({"values":{"resumed":true}}))
        ],
        "connections":[
            edge("wait-resume", "wait", "resumed", "resumed", "main")
        ],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Timer Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("timer", "wait", "Duration Wait", 340, 160, json!({"kind":"duration","durationMs":2500})),
            node("resumed", "set", "Timer Output", 620, 160, json!({"values":{"timerCompleted":true}}))
        ],
        "connections":[
            edge("timer-resume", "timer", "resumed", "resumed", "main")
        ],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Datetime Wait Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("datetime", "wait", "Datetime Wait", 340, 160, json!({"kind":"datetime","resumeAt":"2026-01-01T00:00:00Z"})),
            node("resumed", "set", "Datetime Output", 620, 160, json!({"values":{"datetimeCompleted":true}}))
        ],
        "connections":[
            edge("datetime-resume", "datetime", "resumed", "resumed", "main")
        ],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Form Wait Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("form", "wait", "Signed Form Wait", 340, 160, json!({"kind":"form","authenticationMode":"signed","payloadSchema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}})),
            node("resumed", "set", "Form Output", 620, 160, json!({"values":{"formCompleted":true}}))
        ],
        "connections":[
            edge("form-resume", "form", "resumed", "resumed", "main")
        ],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Approval Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("approval", "approval", "Charge Approval", 340, 160, json!({"title":"M4 Charge Approval","description":"Approve the M4 recovery fixture"})),
            node("approved", "set", "Approved Output", 620, 100, json!({"values":{"approved":true}})),
            node("rejected", "set", "Rejected Output", 620, 240, json!({"values":{"approved":false}}))
        ],
        "connections":[
            edge("approval-yes", "approval", "approved", "approved", "main"),
            edge_with_order("approval-no", "approval", "rejected", "rejected", "main", 1)
        ],
        "end":{"outputs":{}}
    })).await?;

    create_workflow(&pool, tenant_id, user_id, department_id, "M4 Approval Timeout Fixture", json!({
        "schemaVersion":"4.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[
            node("approval", "approval", "Expiring Approval", 340, 160, json!({"title":"M4 Expiring Approval","description":"This approval must time out","timeoutMs":2500})),
            node("timed-out", "set", "Approval Timeout Output", 620, 160, json!({"values":{"timedOut":true}}))
        ],
        "connections":[
            edge("approval-timeout-output", "approval", "timed_out", "timed-out", "main")
        ],
        "end":{"outputs":{}}
    })).await?;
    Ok(())
}

async fn ensure_fixture_credential(
    pool: &MySqlPool,
    tenant_id: Uuid,
    user_id: Uuid,
    department_id: Uuid,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM credentials WHERE tenant_id=? AND name='M4 Broker Credential'",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    let version = 1_u64;
    let keyring = CredentialKeyring::from_json(
        std::env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")?,
        &SecretString::from(std::env::var("AGENTX_CREDENTIAL_KEYS_JSON")?),
    )?;
    let secret = PlainSecret::new(serde_json::to_vec(&json!({"token":"m4-broker-secret"}))?);
    let encrypted = keyring.encrypt(&secret, format!("{tenant_id}/{id}/{version}").as_bytes())?;
    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,'M4 Broker Credential','bearer','****',?,?)")
        .bind(id).bind(tenant_id).bind(department_id).bind(user_id)
        .execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,1,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(id).bind(encrypted.algorithm).bind(encrypted.key_id)
        .bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(user_id)
        .execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(id)
}

async fn ensure_credential_snapshot(
    pool: &MySqlPool,
    tenant_id: Uuid,
    user_id: Uuid,
    workflow_version_id: Uuid,
    credential_id: Uuid,
) -> Result<()> {
    let identity_id: Uuid = sqlx::query_scalar("SELECT i.id FROM workflow_service_identities i JOIN workflow_versions v ON v.workflow_id=i.workflow_id AND v.tenant_id=i.tenant_id WHERE v.tenant_id=? AND v.id=?")
        .bind(tenant_id)
        .bind(workflow_version_id)
        .fetch_one(pool)
        .await?;
    let snapshot = json!({
        "id": credential_id,
        "credentialType": "bearer",
        "secretVersion": 1,
        "resourceVersion": 1
    });
    let snapshot_hash = canonical_content_hash(&snapshot)?;
    let mut transaction = pool.begin().await?;
    let snapshot_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_version_resources WHERE tenant_id=? AND workflow_version_id=? AND node_id='broker' AND resource_type='credential' AND resource_id=? AND operation_key='use')")
        .bind(tenant_id)
        .bind(workflow_version_id)
        .bind(credential_id)
        .fetch_one(&mut *transaction)
        .await?;
    if !snapshot_exists {
        sqlx::query("INSERT INTO workflow_version_resources(id,tenant_id,workflow_version_id,node_id,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash) VALUES(?,?,?,'broker','credential',?,NULL,'use',?,?)")
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(workflow_version_id)
            .bind(credential_id)
            .bind(snapshot)
            .bind(snapshot_hash)
            .execute(&mut *transaction)
            .await?;
    }
    let grant_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_grants WHERE tenant_id=? AND subject_type='workflow_service_identity' AND subject_id=? AND resource_type='credential' AND resource_id=? AND operation_key='use')")
        .bind(tenant_id)
        .bind(identity_id)
        .bind(credential_id)
        .fetch_one(&mut *transaction)
        .await?;
    if !grant_exists {
        sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,'credential',?,NULL,'use',?)")
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(identity_id)
            .bind(credential_id)
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn create_workflow(
    pool: &MySqlPool,
    tenant_id: Uuid,
    user_id: Uuid,
    department_id: Uuid,
    name: &str,
    definition: Value,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT v.id FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE v.tenant_id=? AND w.name=? ORDER BY v.version_number DESC LIMIT 1")
        .bind(tenant_id).bind(name).fetch_optional(pool).await?
    {
        return Ok(id);
    }
    let workflow_id = Uuid::now_v7();
    let draft_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let parsed: WorkflowDefinition = serde_json::from_value(definition.clone())?;
    let compiled = WorkflowCompiler::new(&NodeRegistry::m4_defaults())
        .compile(
            &parsed,
            &CompileContext {
                current_workflow_version_id: Some(version_id.to_string()),
                ancestor_workflow_version_ids: Default::default(),
            },
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{}: {}",
                name,
                serde_json::to_string(&error.issues).unwrap_or_else(|_| error.to_string())
            )
        })?;
    let content_hash = canonical_content_hash(&definition)?;
    let mut tx = pool.begin().await?;
    insert_workflow(
        &mut tx,
        tenant_id,
        user_id,
        department_id,
        workflow_id,
        draft_id,
        version_id,
        name,
        definition,
        content_hash,
        serde_json::to_value(&compiled)?,
        &compiled.canonical_hash,
        &compiled.compiler_version,
    )
    .await?;
    tx.commit().await?;
    Ok(version_id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_workflow(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    user_id: Uuid,
    department_id: Uuid,
    workflow_id: Uuid,
    draft_id: Uuid,
    version_id: Uuid,
    name: &str,
    definition: Value,
    content_hash: String,
    compiled: Value,
    compiled_hash: &str,
    compiler_version: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,'M4 Kubernetes E2E fixture','company',?,?)")
        .bind(workflow_id).bind(tenant_id).bind(name).bind(user_id).bind(department_id).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)")
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(workflow_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO workflow_members(tenant_id,workflow_id,user_id,member_role,created_by) VALUES(?,?,?,'manager',?)")
        .bind(tenant_id).bind(workflow_id).bind(user_id).bind(user_id).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,revision,definition_json,content_hash,updated_by) VALUES(?,?,?,'4.0',1,?,?,?)")
        .bind(draft_id).bind(tenant_id).bind(workflow_id).bind(&definition).bind(&content_hash).bind(user_id).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,compiled_ir_json,compiled_ir_hash,compiler_version,compiled_at,created_by) VALUES(?,?,?,1,1,'4.0',?,?,?,?,?,CURRENT_TIMESTAMP(6),?)")
        .bind(version_id).bind(tenant_id).bind(workflow_id).bind(definition).bind(content_hash).bind(compiled).bind(compiled_hash).bind(compiler_version).bind(user_id).execute(&mut **tx).await?;
    Ok(())
}

fn node(id: &str, node_type: &str, name: &str, _x: i32, _y: i32, parameters: Value) -> Value {
    json!({"id":id,"key":id.replace('-', "_"),"type":node_type,"typeVersion":1,"name":name,"parameters":parameters})
}

fn edge(id: &str, source: &str, source_handle: &str, target: &str, target_handle: &str) -> Value {
    edge_with_order(id, source, source_handle, target, target_handle, 0)
}

fn edge_with_order(
    id: &str,
    source: &str,
    source_handle: &str,
    target: &str,
    target_handle: &str,
    order: u32,
) -> Value {
    json!({"id":id,"sourceNodeId":source,"sourceHandle":source_handle,"targetNodeId":target,"targetHandle":target_handle,"order":order})
}
