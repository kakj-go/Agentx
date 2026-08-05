use std::{collections::BTreeMap, sync::Arc, time::Duration as StdDuration};

use agentx_application::{
    ArtifactStore, ArtifactWrite, RuntimeContext, RuntimeResourceSnapshot, SkillRuntime,
};
use agentx_domain::{
    AttemptId, ExecutionId, NodeExecutionId, ResourceOperation, ResourceReference, ResourceType,
    TenantId, TraceId, WorkflowDefinition, WorkflowId, WorkflowServiceIdentityId,
    WorkflowVersionId,
};
use agentx_infrastructure::{
    artifact::MySqlObjectArtifactStore,
    config::MySqlSettings,
    credential::{CredentialKeyring, PlainSecret},
    mysql,
    runtime_broker::{InvocationBroker, InvocationBrokerError, InvocationScope},
    runtime_repository::RuntimeRepository,
    runtime_resources::MySqlResourceAuthorizer,
    skill_runtime::SnapshotSkillRuntime,
};
use agentx_node_protocol::{BinaryReference, InvocationResourceRequest, Item};
use agentx_runtime::{CompileContext, ExecutionMachine, NodeRegistry, WorkflowCompiler};
use object_store::memory::InMemory;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::{MySqlPool, Row};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration, OffsetDateTime};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn invocation_handles_and_checkpoint_artifacts_enforce_runtime_boundaries() {
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("MySQL container should start");
    let port = container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("mapped MySQL port");
    let settings = MySqlSettings {
        host: "127.0.0.1".into(),
        port,
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("agentx-test-password".to_owned()),
        max_connections: 5,
        tls_mode: agentx_infrastructure::config::MySqlTlsMode::Disabled,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    };
    let pool = connect(&settings).await;
    mysql::run_migrations(&pool).await.expect("run migrations");
    let root_pool = connect(&MySqlSettings {
        username: "root".into(),
        password: SecretString::from("agentx-root-password".to_owned()),
        ..settings.clone()
    })
    .await;
    sqlx::raw_sql("SET GLOBAL log_bin_trust_function_creators=1")
        .execute(&root_pool)
        .await
        .expect("allow fixture trigger");

    let ids = seed_runtime(&pool).await;
    let keyring = Arc::new(test_keyring());
    seed_credential(&pool, &keyring, &ids).await;
    let object_store = Arc::new(InMemory::new());
    let artifact_store: Arc<dyn ArtifactStore> =
        Arc::new(MySqlObjectArtifactStore::new(pool.clone(), object_store));
    let input_artifact = artifact_store
        .put(ArtifactWrite {
            tenant_id: TenantId::from_uuid(ids.tenant),
            content_type: "text/plain".into(),
            content: b"artifact-value".to_vec(),
        })
        .await
        .expect("seed artifact");
    let broker = InvocationBroker::new(
        pool.clone(),
        keyring,
        artifact_store.clone(),
        "http://workflow-worker:8080".into(),
        300,
    );
    let item = Item {
        binary: BTreeMap::from([(
            "data".into(),
            BinaryReference {
                artifact_handle: input_artifact.id.to_string(),
                file_name: Some("fixture.txt".into()),
                content_type: Some("text/plain".into()),
                size_bytes: 14,
            },
        )]),
        ..Item::default()
    };
    let scope = InvocationScope {
        tenant_id: ids.tenant,
        execution_id: ids.execution,
        node_execution_id: ids.node_execution,
        attempt_id: ids.attempt,
        lease_token: ids.lease,
        deadline: OffsetDateTime::now_utc() + Duration::minutes(5),
    };
    let issued = broker
        .issue(
            &scope,
            &[ResourceReference {
                resource_type: ResourceType::Credential,
                resource_id: ids.credential,
                resource_version_id: None,
                operation: ResourceOperation::Use,
            }],
            &[agentx_application::RuntimeResourceSnapshot {
                node_id: "remote".into(),
                reference: ResourceReference {
                    resource_type: ResourceType::Credential,
                    resource_id: ids.credential,
                    resource_version_id: None,
                    operation: ResourceOperation::Use,
                },
                snapshot_hash: "fixture".into(),
                snapshot: serde_json::json!({"secretVersion":1}),
            }],
            [&item].into_iter(),
        )
        .await
        .expect("issue invocation handles");
    assert_eq!(issued.credential_handles.len(), 1);
    assert_eq!(issued.artifact_handles.len(), 1);
    assert!(issued.cancellation_url.contains("workflow-worker:8080"));
    let raw_handle = &issued.credential_handles[0].handle;
    let stored_hash: String = sqlx::query_scalar(
        "SELECT token_hash FROM node_invocation_handles WHERE handle_kind='credential'",
    )
    .fetch_one(&pool)
    .await
    .expect("stored handle hash");
    assert_ne!(stored_hash, *raw_handle);

    let request = InvocationResourceRequest {
        handle: raw_handle.clone(),
        tenant_id: TenantId::from_uuid(ids.tenant),
        node_execution_id: agentx_domain::NodeExecutionId::from_uuid(ids.node_execution),
        attempt_id: ids.attempt,
    };
    let mut cross_attempt = request.clone();
    cross_attempt.attempt_id = Uuid::now_v7();
    assert!(matches!(
        broker.resolve(&cross_attempt).await,
        Err(InvocationBrokerError::Invalid)
    ));
    let mut cross_tenant = request.clone();
    cross_tenant.tenant_id = TenantId::new();
    assert!(matches!(
        broker.resolve(&cross_tenant).await,
        Err(InvocationBrokerError::Invalid)
    ));
    let credential =
        serde_json::to_value(broker.resolve(&request).await.expect("resolve credential"))
            .expect("serialize credential response");
    assert_eq!(credential["kind"], "credential");
    assert_eq!(credential["value"]["token"], "runtime-secret");
    assert!(matches!(
        broker.resolve(&request).await,
        Err(InvocationBrokerError::Replayed)
    ));
    let artifact_request = InvocationResourceRequest {
        handle: issued.artifact_handles[0].handle.clone(),
        ..request.clone()
    };
    let artifact = serde_json::to_value(
        broker
            .resolve(&artifact_request)
            .await
            .expect("resolve artifact"),
    )
    .expect("serialize artifact response");
    assert_eq!(artifact["kind"], "artifact");
    assert_eq!(artifact["contentBase64"], "YXJ0aWZhY3QtdmFsdWU=");
    let cancellation_token = issued
        .cancellation_url
        .rsplit('/')
        .next()
        .expect("cancellation token");
    let cancellation = broker
        .cancellation_status(cancellation_token)
        .await
        .expect("active cancellation status");
    assert!(cancellation.lease_valid);
    assert!(!cancellation.cancellation_requested);
    sqlx::query(
        "UPDATE worker_leases SET released_at=CURRENT_TIMESTAMP(6) WHERE node_attempt_id=?",
    )
    .bind(ids.attempt)
    .execute(&pool)
    .await
    .expect("release lease");
    let cancellation = broker
        .cancellation_status(cancellation_token)
        .await
        .expect("released cancellation status");
    assert!(!cancellation.lease_valid);
    assert!(cancellation.cancellation_requested);

    verify_checkpoint_externalization(&pool, artifact_store, &ids).await;
}

#[tokio::test]
async fn skill_runtime_rechecks_revoked_grants_and_rejects_cross_tenant_contexts() {
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("MySQL container should start");
    let port = container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("mapped MySQL port");
    let pool = connect(&MySqlSettings {
        host: "127.0.0.1".into(),
        port,
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("agentx-test-password".to_owned()),
        max_connections: 5,
        tls_mode: agentx_infrastructure::config::MySqlTlsMode::Disabled,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    })
    .await;
    mysql::run_migrations(&pool).await.expect("run migrations");
    let ids = seed_runtime(&pool).await;
    let artifacts: Arc<dyn ArtifactStore> = Arc::new(MySqlObjectArtifactStore::new(
        pool.clone(),
        Arc::new(InMemory::new()),
    ));
    let skill_file = artifacts
        .put(ArtifactWrite {
            tenant_id: TenantId::from_uuid(ids.tenant),
            content_type: "text/markdown".into(),
            content: b"Use the published fixture.".to_vec(),
        })
        .await
        .expect("seed skill artifact");
    let reference = ResourceReference {
        resource_type: ResourceType::Skill,
        resource_id: Uuid::now_v7(),
        resource_version_id: Some(Uuid::now_v7()),
        operation: ResourceOperation::Use,
    };
    sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,'skill',?,?, 'use',?)")
        .bind(Uuid::now_v7())
        .bind(ids.tenant)
        .bind(ids.identity)
        .bind(reference.resource_id)
        .bind(reference.resource_version_id)
        .bind(ids.user)
        .execute(&pool)
        .await
        .expect("grant skill");
    let snapshot = RuntimeResourceSnapshot {
        node_id: "skill".into(),
        reference: reference.clone(),
        snapshot_hash: "skill-fixture".into(),
        snapshot: json!({
            "manifest":{"instructions":"Use the published fixture."},
            "files":[{
                "artifactId":skill_file.id,
                "contentHash":format!("sha256:{}", skill_file.sha256),
                "path":"SKILL.md",
                "mimeType":"text/markdown"
            }]
        }),
    };
    let runtime = SnapshotSkillRuntime::new(MySqlResourceAuthorizer::new(pool.clone()), artifacts);
    let context = skill_context(&ids, TenantId::from_uuid(ids.tenant), snapshot.clone());
    runtime
        .load(&context, reference.clone())
        .await
        .expect("authorized skill should load");

    let cross_tenant = skill_context(&ids, TenantId::new(), snapshot);
    let denied = runtime
        .load(&cross_tenant, reference.clone())
        .await
        .expect_err("cross-tenant context must not use the original grant");
    assert_eq!(denied.code, "RESOURCE_GRANT_MISSING");

    sqlx::query(
        "DELETE FROM resource_grants WHERE tenant_id=? AND resource_type='skill' AND resource_id=?",
    )
    .bind(ids.tenant)
    .bind(reference.resource_id)
    .execute(&pool)
    .await
    .expect("revoke skill");
    let revoked = runtime
        .load(&context, reference.clone())
        .await
        .expect_err("revoked skill must fail before loading artifacts");
    assert_eq!(revoked.code, "RESOURCE_GRANT_MISSING");
}

fn skill_context(
    ids: &RuntimeIds,
    tenant_id: TenantId,
    snapshot: RuntimeResourceSnapshot,
) -> RuntimeContext {
    RuntimeContext {
        tenant_id,
        workflow_service_identity_id: WorkflowServiceIdentityId::from_uuid(ids.identity),
        workflow_id: WorkflowId::from_uuid(ids.workflow),
        workflow_version_id: WorkflowVersionId::from_uuid(ids.workflow_version),
        execution_id: ExecutionId::from_uuid(ids.execution),
        node_execution_id: NodeExecutionId::from_uuid(ids.node_execution),
        attempt_id: AttemptId::from_uuid(ids.attempt),
        lease_token: ids.lease,
        trace_id: TraceId::new(),
        span_id: Uuid::now_v7(),
        deadline: OffsetDateTime::now_utc() + Duration::minutes(5),
        cancellation: CancellationToken::new(),
        idempotency_key: "skill-runtime-fixture".into(),
        resources: vec![snapshot],
    }
}

async fn verify_checkpoint_externalization(
    pool: &MySqlPool,
    artifact_store: Arc<dyn ArtifactStore>,
    ids: &RuntimeIds,
) {
    let definition: WorkflowDefinition = serde_json::from_value(json!({
        "schemaVersion":"2.0",
        "nodes":[{"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","position":{"x":0,"y":0},"parameters":{}}],
        "connections":[]
    }))
    .expect("workflow definition");
    let compiled = WorkflowCompiler::new(&NodeRegistry::m4_defaults())
        .compile(&definition, &CompileContext::default())
        .expect("compile workflow");
    let machine = ExecutionMachine::new(
        compiled,
        vec![Item {
            json: json!({"payload":"x".repeat(8 * 1024)}),
            ..Item::default()
        }],
    )
    .expect("create execution machine");
    let payload = serde_json::to_value(&machine).expect("serialize machine");
    let checkpoint_id = Uuid::now_v7();
    sqlx::query("INSERT INTO checkpoints(id,tenant_id,execution_id,sequence_number,checkpoint_type,state_hash,payload_json) VALUES(?,?,?,1,'execution_start','fixture-state',?)")
        .bind(checkpoint_id).bind(ids.tenant).bind(ids.execution).bind(&payload)
        .execute(pool).await.expect("insert checkpoint");
    let repository = RuntimeRepository::new(pool.clone())
        .with_checkpoint_artifacts(artifact_store.clone(), 1024);

    sqlx::raw_sql("CREATE TRIGGER reject_checkpoint_externalization BEFORE UPDATE ON checkpoints FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='fixture switch failure'")
        .execute(pool).await.expect("create rejection trigger");
    assert!(repository.externalize_checkpoints(10).await.is_err());
    let inline: bool = sqlx::query_scalar("SELECT payload_json IS NOT NULL AND payload_artifact_id IS NULL FROM checkpoints WHERE id=?")
        .bind(checkpoint_id).fetch_one(pool).await.expect("inline checkpoint remains");
    assert!(inline);
    let active_artifacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE tenant_id=? AND content_type='application/vnd.agentx.checkpoint+json' AND deleted_at IS NULL")
        .bind(ids.tenant).fetch_one(pool).await.expect("compensated artifacts");
    assert_eq!(active_artifacts, 0);
    sqlx::raw_sql("DROP TRIGGER reject_checkpoint_externalization")
        .execute(pool)
        .await
        .expect("drop rejection trigger");

    assert_eq!(
        repository
            .externalize_checkpoints(10)
            .await
            .expect("externalize checkpoint"),
        1
    );
    let row = sqlx::query(
        "SELECT payload_json,payload_artifact_id,state_hash FROM checkpoints WHERE id=?",
    )
    .bind(checkpoint_id)
    .fetch_one(pool)
    .await
    .expect("externalized checkpoint row");
    assert!(
        row.try_get::<Option<Value>, _>("payload_json")
            .unwrap()
            .is_none()
    );
    assert!(
        row.try_get::<Option<Uuid>, _>("payload_artifact_id")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        row.try_get::<String, _>("state_hash").unwrap(),
        "fixture-state"
    );
    let restored = repository
        .load_checkpoint_machine(ids.tenant, checkpoint_id)
        .await
        .expect("restore externalized checkpoint");
    assert_eq!(serde_json::to_value(restored).unwrap(), payload);
}

struct RuntimeIds {
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    workflow: Uuid,
    identity: Uuid,
    workflow_version: Uuid,
    execution: Uuid,
    node_execution: Uuid,
    attempt: Uuid,
    lease: Uuid,
    credential: Uuid,
}

async fn seed_runtime(pool: &MySqlPool) -> RuntimeIds {
    let ids = RuntimeIds {
        tenant: Uuid::now_v7(),
        user: Uuid::now_v7(),
        department: Uuid::now_v7(),
        workflow: Uuid::now_v7(),
        identity: Uuid::now_v7(),
        workflow_version: Uuid::now_v7(),
        execution: Uuid::now_v7(),
        node_execution: Uuid::now_v7(),
        attempt: Uuid::now_v7(),
        lease: Uuid::now_v7(),
        credential: Uuid::now_v7(),
    };
    sqlx::query(
        "INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Runtime Test','runtime test')",
    )
    .bind(ids.tenant)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
        .bind(ids.department).bind(ids.tenant).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,'runtime','runtime','Runtime')")
        .bind(ids.user).bind(ids.tenant).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Runtime',?,?)")
        .bind(ids.workflow).bind(ids.tenant).bind(ids.user).bind(ids.department).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id,status) VALUES(?,?,?,'active')")
        .bind(ids.identity).bind(ids.tenant).bind(ids.workflow).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'2.0',JSON_OBJECT(),'fixture',?)")
        .bind(ids.workflow_version).bind(ids.tenant).bind(ids.workflow).bind(ids.user).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(?,?,?,?,?,'manual','running',CURRENT_TIMESTAMP(6))")
        .bind(ids.execution).bind(ids.tenant).bind(ids.workflow).bind(ids.workflow_version).bind(Uuid::now_v7()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES(?,?,?,'remote','Remote','remote_action',1,0,0,0,'running','remote_action')")
        .bind(ids.node_execution).bind(ids.tenant).bind(ids.execution).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,lease_token,deadline_at) VALUES(?,?,?,?,1,'running','runtime-attempt',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
        .bind(ids.attempt).bind(ids.tenant).bind(ids.execution).bind(ids.node_execution).bind(ids.lease).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO worker_leases(node_attempt_id,tenant_id,node_execution_id,lease_token,worker_instance_id,capability,acquired_at,heartbeat_at,expires_at) VALUES(?,?,?,?,'worker-test','remote_action',CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6),DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
        .bind(ids.attempt).bind(ids.tenant).bind(ids.node_execution).bind(ids.lease).execute(pool).await.unwrap();
    ids
}

async fn seed_credential(pool: &MySqlPool, keyring: &CredentialKeyring, ids: &RuntimeIds) {
    let version = 1_u64;
    let aad = format!("{}/{}/{}", ids.tenant, ids.credential, version);
    let encrypted = keyring
        .encrypt(
            &PlainSecret::new(serde_json::to_vec(&json!({"token":"runtime-secret"})).unwrap()),
            aad.as_bytes(),
        )
        .unwrap();
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,'Runtime Secret','bearer','****',?,?)")
        .bind(ids.credential).bind(ids.tenant).bind(ids.department).bind(ids.user).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,1,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(ids.tenant).bind(ids.credential).bind(encrypted.algorithm).bind(encrypted.key_id)
        .bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(ids.user).execute(pool).await.unwrap();
}

fn test_keyring() -> CredentialKeyring {
    CredentialKeyring::from_json(
        "test-v1".into(),
        &SecretString::from(
            r#"{"keys":{"test-v1":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#.to_owned(),
        ),
    )
    .unwrap()
}

async fn connect(settings: &MySqlSettings) -> MySqlPool {
    for _ in 0..30 {
        if let Ok(pool) = mysql::connect(settings).await {
            return pool;
        }
        tokio::time::sleep(StdDuration::from_millis(500)).await;
    }
    panic!("connect to test MySQL")
}
