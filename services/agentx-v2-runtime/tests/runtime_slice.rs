use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use agentx_bundle_builder::{
    BundleBuildSource, WorkPackageBuildSource, build_bundle, build_work_package,
    compile_workflow_version, composite_ir_object_id,
};
use agentx_domain::WorkflowDefinition;
use agentx_runtime_contracts::{
    ActivateDeploymentRequestV1, ActivationManifestV1, AdmissionStatusV1, AdmissionTargetV1,
    ApiKeyAdmissionV1, ApplicationRouteAdmissionV1, ApprovalDecisionValueV1,
    CancelWorkPackageRequestV1, CommandEnvelopeV1, ControlRole, CreateSessionRequestV1,
    DelegationClaimsV1, DisableDeploymentRequestV1, ExecuteWorkPackageRequestV1,
    ExecutionSearchRequestV1, InvocationResponseV1, MessagePartInputV1, MessageRequestV1,
    MessageResponseV1, Plane, PrepareBundleRequestV1, PrepareWorkPackageRequestV1,
    PublishReceiptStatusV1, RollbackDeploymentRequestV1, RuntimeAdmissionCommandV1,
    RuntimeApprovalDecisionV1, RuntimeAuthorizationSnapshotV1, RuntimeCallPurposeV1,
    RuntimeEventPayloadV1, RuntimeGrantStateV1, RuntimeObjectReferenceV1,
    RuntimeObjectUploadMetadataV1, RuntimePolicyV1, RuntimeResourceKindV1,
    RuntimeRetentionDataTypeV1, RuntimeRetentionPolicyV1, RuntimeTriggerConfigurationV1,
    RuntimeTriggerSpecV1, RuntimeUserWorkflowGrantV1, RuntimeWorkPackageOverlayV1, ServiceClaimsV1,
    ServiceIdentityAdmissionV1, SessionResponseV1, SideEffectResolutionV1, StorageDomain,
    WorkPackagePurpose, WorkerResultStatusV1, WorkerResultV1, issue_delegation_token,
    issue_service_token, now_unix,
};
use agentx_v2_runtime::{
    RuntimeState,
    auth::RuntimeTrust,
    error::RuntimeError,
    execution::{
        InvocationRequestV1, authenticate_api_key, claim_commands, claim_dispatch,
        complete_dispatch, create_invocation, process_command, process_command_with_state,
        recover_dispatches, release_dispatch,
    },
    gc::{cleanup_expired_temporary_objects, mark_collectable, sweep_one},
    internal_engine::{
        apply_runtime_command, cancel_work_package, execute_work_package, prepare_work_package,
    },
    object_upload::persist_upload,
    publish::{
        activate_deployment, apply_admission, disable_deployment, prepare_bundle,
        rollback_deployment,
    },
    query::{get_execution, get_execution_artifact, search_executions},
    retention::run_once as run_retention_once,
    trigger::{TriggerProvider, TriggerProviderResponse},
    worker_runtime::{RuntimeWorker, WorkerProvider, WorkerProviderError, WorkerProviderResponse},
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, State},
    http::{HeaderMap, Request, header::AUTHORIZATION},
    routing::{get, post},
};
use bytes::Bytes;
use ed25519_dalek::SigningKey;
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, PutMultipartOpts, PutOptions,
    PutPayload, PutResult,
};
use object_store::{ObjectStore, memory::InMemory, path::Path as ObjectPath};
use rand::rngs::OsRng;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row, mysql::MySqlPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use tower::ServiceExt;
use uuid::Uuid;

const PRIVATE_KEY: &[u8] =
    include_bytes!("../../../crates/agentx-runtime-contracts/tests/fixtures/service-private.pem");
const PUBLIC_KEY: &[u8] =
    include_bytes!("../../../crates/agentx-runtime-contracts/tests/fixtures/service-public.pem");

struct Fixture {
    state: RuntimeState,
    signing_key: SigningKey,
    work_package_signing_key: SigningKey,
    tenant_id: Uuid,
    application_id: Uuid,
    workflow_id: Uuid,
    identity_id: Uuid,
    key_id: Uuid,
    api_key: String,
}

struct StubTriggerProvider {
    delay: Duration,
    response: Result<TriggerProviderResponse, String>,
}

enum StubWorkerMode {
    Reject,
    Agent(Arc<std::sync::atomic::AtomicUsize>),
    Evaluator,
}

struct StubWorkerProvider {
    mode: StubWorkerMode,
}

#[async_trait::async_trait]
impl WorkerProvider for StubWorkerProvider {
    async fn post_json(
        &self,
        endpoint: &str,
        _context: agentx_v2_runtime::egress::EgressRequestContext,
        _timeout: Duration,
        _headers: reqwest::header::HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let payload = match &self.mode {
            StubWorkerMode::Reject => {
                return Err(WorkerProviderError::Denied(
                    "unexpected provider request in test".into(),
                ));
            }
            StubWorkerMode::Evaluator => json!({
                "text":"{\"passed\":true,\"score\":0.95,\"reason\":\"fixture accepted the target output\",\"usage\":{\"tokens\":7,\"costMicros\":23}}",
                "message":{"role":"assistant","content":"{\"passed\":true,\"score\":0.95,\"reason\":\"fixture accepted the target output\",\"usage\":{\"tokens\":7,\"costMicros\":23}}"},
                "reasoningContent":null,
                "structuredOutput":{"passed":true,"score":0.95,"reason":"fixture accepted the target output","usage":{"tokens":7,"costMicros":23}},
                "citations":[],
                "toolCalls":[],
                "files":[],
                "usage":{"inputTokens":0,"outputTokens":7,"tokens":7,"costMicros":23},
                "finishReason":"stop",
                "partial":false
            }),
            StubWorkerMode::Agent(calls) if endpoint.ends_with("/model") => {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    json!({"toolCall":{"query":"agentx"},"usage":{"tokens":10,"costMicros":5}})
                } else {
                    json!({"done":true,"answer":"agentx-v2","usage":{"tokens":10,"costMicros":5}})
                }
            }
            StubWorkerMode::Agent(_) if endpoint.ends_with("/mcp") => {
                if body.get("id").is_none() {
                    Value::Null
                } else if body.get("method").and_then(Value::as_str) == Some("initialize") {
                    json!({"jsonrpc":"2.0","id":body["id"],"result":{}})
                } else {
                    json!({"jsonrpc":"2.0","id":body["id"],"result":{"content":{"value":"tool-result"}}})
                }
            }
            StubWorkerMode::Agent(_) => {
                return Err(WorkerProviderError::Denied(format!(
                    "unexpected Agent fixture endpoint: {endpoint}"
                )));
            }
        };
        Ok(WorkerProviderResponse {
            status: reqwest::StatusCode::OK,
            headers: reqwest::header::HeaderMap::new(),
            body: Bytes::from(serde_json::to_vec(&payload).unwrap()),
        })
    }
}

fn test_worker(fixture: &Fixture, mode: StubWorkerMode) -> RuntimeWorker {
    RuntimeWorker::new_with_provider(
        fixture.state.pool.clone(),
        fixture.state.objects.clone(),
        Arc::new(StubWorkerProvider { mode }),
    )
}

#[async_trait::async_trait]
impl TriggerProvider for StubTriggerProvider {
    async fn post_json(
        &self,
        _endpoint: &str,
        _context: agentx_v2_runtime::egress::EgressRequestContext,
        _timeout: Duration,
        _idempotency_key: Option<&str>,
        _input: &Value,
    ) -> Result<TriggerProviderResponse, String> {
        tokio::time::sleep(self.delay).await;
        self.response.clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn v2_publish_execution_query_recovery_and_gc_are_fenced_and_idempotent() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx_runtime")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("Runtime MySQL container should start");
    let port = container.get_host_port_ipv4(3306.tcp()).await.unwrap();
    let pool = connect_with_retry(port).await;
    agentx_runtime_infrastructure::migrate_runtime_mysql(&pool)
        .await
        .unwrap();
    trace_watermarks_are_atomic_under_concurrency(&pool).await;
    composite_timeout_commands_are_idempotent(&pool).await;
    quota_projection_claim_is_single_owner(&pool).await;
    trigger_claim_takeover_and_provider_failure_are_fenced(&pool).await;

    let fixture = Fixture::new(pool);
    command_claim_returns_only_the_current_batch(&fixture).await;
    authentication_failures_do_not_write_receipts(&fixture).await;
    let first = fixture.bundle(1).await;
    object_upload_is_immutable_and_replayable(&fixture, &first).await;
    prepare_is_idempotent_and_does_not_route_traffic(&fixture, &first).await;
    apply_initial_admission(&fixture, 1).await;
    concurrent_admission_delivery_converges_to_one_receipt(&fixture).await;
    activate(&fixture, &first, None, 1, 1).await;
    authentication_requires_active_route_tenant_and_head(&fixture).await;
    let before_invalid: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM application_invocations WHERE tenant_id=?")
            .bind(fixture.tenant_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert!(matches!(
        create_invocation(
            &fixture.state.pool,
            fixture.tenant_id,
            fixture.application_id,
            fixture.key_id,
            &InvocationRequestV1 {
                input: json!({"message":"x"}),
                idempotency_key: "runtime-slice-invalid-input".into(),
            },
        )
        .await,
        Err(RuntimeError::InvalidRequest(
            "INPUT_SCHEMA_VALIDATION_FAILED",
            _
        ))
    ));
    let after_invalid: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM application_invocations WHERE tenant_id=?")
            .bind(fixture.tenant_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(
        before_invalid, after_invalid,
        "invalid input must not create Runtime facts"
    );

    let accepted = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"agentx-v2"}),
            idempotency_key: "runtime-slice-invocation".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(accepted.bundle_id, first.payload.bundle_id);
    assert_eq!(accepted.admission_epoch, 1);

    invocation_and_dispatch_recovery_are_fenced(&fixture, accepted.execution_id).await;
    retry_policy_creates_a_second_attempt_and_trace(&fixture).await;
    session_message_appends_one_assistant_response(&fixture).await;
    fork_uses_checkpoint_machine_and_preserves_source(&fixture, accepted.execution_id).await;
    query_is_tenant_application_and_execution_scoped(&fixture, accepted.execution_id).await;
    deterministic_start_rejection_is_terminal(&fixture).await;
    revoked_grant_rejection_is_terminal_and_monotonic(&fixture).await;
    expired_attempt_deadline_is_terminal_and_not_requeued(&fixture).await;

    let second = fixture.bundle(2).await;
    fixture
        .upload_bundle_object(&second, "bundle-object:second")
        .await;
    prepare(&fixture, &second, "prepare:second").await;
    let before_activation = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"still-first"}),
            idempotency_key: "prepare-only-cannot-route".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(before_activation.bundle_id, first.payload.bundle_id);

    activate(&fixture, &second, Some(1), 2, 1).await;
    stale_head_and_sequence_are_rejected(&fixture, &first).await;
    rollback(&fixture, &first, Some(2), 3, 1).await;

    apply_api_key(&fixture, 2, AdmissionStatusV1::Revoked).await;
    assert!(matches!(
        authenticate_api_key(&fixture.state.pool, "runtime-slice", &fixture.api_key).await,
        Err(RuntimeError::Unauthorized)
    ));
    let rejected = rollback_request(&fixture, &second, Some(3), 4, 1, "rollback:revoked").await;
    assert_eq!(rejected.status, PublishReceiptStatusV1::Rejected);
    assert!(matches!(
        rejected.rejection.unwrap().code,
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::AdmissionPrerequisiteMissing
    ));
    let key_status: String =
        sqlx::query_scalar("SELECT status FROM api_key_admission WHERE tenant_id=? AND key_id=?")
            .bind(fixture.tenant_id)
            .bind(fixture.key_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(key_status, "revoked");

    gc_protects_heads_references_and_holds_then_sweeps(&fixture, &second).await;
    gc_object_delete_failure_is_recorded_and_retryable(&fixture).await;
    ready_orphan_objects_are_swept_and_reuploadable(&fixture).await;
    expired_temporary_objects_are_removed(&fixture).await;
    disable_is_scoped_idempotent_and_preserves_the_head(&fixture, &first).await;
    work_package_prepare_execute_cancel_are_independently_signed_and_idempotent(&fixture).await;
    evaluation_work_package_creates_cases_converges_and_cancels_atomically(&fixture).await;
    wait_and_approval_resume_exactly_once(&fixture).await;
    large_worker_results_are_externalized_and_verified(&fixture).await;
    composite_child_uses_immutable_runtime_snapshot_and_merges_on_success(&fixture).await;
    sandbox_manager_is_fenced_and_idempotent(&fixture).await;
    skill_worker_loads_and_verifies_the_runtime_object_closure(&fixture).await;
    agent_worker_runs_a_bounded_tool_loop_and_persists_usage(&fixture).await;
    quota_projection_covers_all_dimensions_and_has_no_terminal_residue(&fixture).await;
    retention_dry_run_reference_block_and_object_sweep_are_fenced(&fixture).await;
    event_sequencer_quarantines_invalid_payload_without_blocking_valid_events(&fixture).await;
}

async fn trace_watermarks_are_atomic_under_concurrency(pool: &MySqlPool) {
    const EVENT_COUNT: u64 = 16;
    let tenant_id = Uuid::now_v7();
    let workflow_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(?,?,?,?,?,'debug','running',UTC_TIMESTAMP(6))")
        .bind(execution_id)
        .bind(tenant_id)
        .bind(workflow_id)
        .bind(version_id)
        .bind(Uuid::now_v7())
        .execute(pool)
        .await
        .unwrap();

    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..EVENT_COUNT {
        let pool = pool.clone();
        tasks.spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let mut draft = agentx_v2_runtime::trace_delivery::TraceDraft::execution(
                tenant_id,
                execution_id,
                format!("execution.concurrent_{index}"),
                "running",
            );
            draft.attributes = json!({"index":index});
            let sequence = agentx_v2_runtime::trace_delivery::enqueue(&mut tx, draft)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            sequence
        });
    }
    let mut sequences = Vec::with_capacity(EVENT_COUNT as usize);
    while let Some(result) = tasks.join_next().await {
        sequences.push(result.unwrap());
    }
    sequences.sort_unstable();
    assert_eq!(sequences, (1..=EVENT_COUNT).collect::<Vec<_>>());
    let watermark: u64 =
        sqlx::query_scalar("SELECT trace_watermark FROM workflow_executions WHERE id=?")
            .bind(execution_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(watermark, EVENT_COUNT);
    let persisted: Vec<u64> = sqlx::query_scalar(
        "SELECT execution_sequence FROM trace_outbox WHERE tenant_id=? AND execution_id=? ORDER BY execution_sequence",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(persisted, sequences);

    sqlx::query("DELETE FROM trace_outbox WHERE tenant_id=? AND execution_id=?")
        .bind(tenant_id)
        .bind(execution_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM execution_events WHERE tenant_id=? AND execution_id=?")
        .bind(tenant_id)
        .bind(execution_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_executions WHERE tenant_id=? AND id=?")
        .bind(tenant_id)
        .bind(execution_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn composite_timeout_commands_are_idempotent(pool: &MySqlPool) {
    let tenant_id = Uuid::now_v7();
    let workflow_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let bundle_id = Uuid::now_v7();
    let parent_execution_id = Uuid::now_v7();
    let child_execution_id = Uuid::now_v7();
    let parent_node_execution_id = Uuid::now_v7();
    for (execution_id, trace_id, status, trigger_type) in [
        (
            parent_execution_id,
            Uuid::now_v7(),
            "waiting",
            "application",
        ),
        (child_execution_id, Uuid::now_v7(), "running", "composite"),
    ] {
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,bundle_id,admission_epoch,state_version,trace_id,trigger_type,status,started_at) VALUES(?,?,?,?,?,1,1,?,?,?,UTC_TIMESTAMP(6))")
            .bind(execution_id)
            .bind(tenant_id)
            .bind(workflow_id)
            .bind(version_id)
            .bind(bundle_id)
            .bind(trace_id)
            .bind(trigger_type)
            .bind(status)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO execution_children(tenant_id,parent_execution_id,parent_node_execution_id,child_execution_id,child_bundle_id,relationship,context_overlay_json,context_overlay_hash,deadline_at) VALUES(?,?,?,?,?,'composite',JSON_OBJECT(),'sha256:0000000000000000000000000000000000000000000000000000000000000000',DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND))")
        .bind(tenant_id)
        .bind(parent_execution_id)
        .bind(parent_node_execution_id)
        .bind(child_execution_id)
        .bind(bundle_id)
        .execute(pool)
        .await
        .unwrap();

    assert_eq!(
        agentx_v2_runtime::composite_execution::enqueue_overdue(pool, 100)
            .await
            .unwrap(),
        1
    );
    agentx_v2_runtime::composite_execution::enqueue_overdue(pool, 100)
        .await
        .unwrap();
    let commands: Vec<String> = sqlx::query_scalar(
        "SELECT command_type FROM runtime_commands WHERE tenant_id=? ORDER BY command_type",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(commands, vec!["cancel_execution", "resume_execution"]);

    sqlx::query("DELETE FROM runtime_commands WHERE tenant_id=?")
        .bind(tenant_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM execution_children WHERE tenant_id=?")
        .bind(tenant_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_executions WHERE tenant_id=?")
        .bind(tenant_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn event_sequencer_quarantines_invalid_payload_without_blocking_valid_events(
    fixture: &Fixture,
) {
    let owner = Uuid::now_v7();
    for _ in 0..10_000 {
        match agentx_v2_runtime::event_export::sequence_one(&fixture.state.pool, owner).await {
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(error) => panic!("a Runtime-produced Integration Event was invalid: {error}"),
        }
    }
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_outbox WHERE status='pending' AND message_type='runtime_event'",
    )
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        remaining, 0,
        "the Event Sequencer did not drain its valid backlog"
    );

    let locked_execution_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_executions WHERE application_id IS NOT NULL AND bundle_id IS NOT NULL ORDER BY created_at,id LIMIT 1",
    )
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let raced_event_id = Uuid::now_v7();
    sqlx::query("INSERT INTO execution_outbox(id,tenant_id,execution_id,message_type,payload_json,status) VALUES(?,?,?,'runtime_event',?,'pending')")
        .bind(raced_event_id)
        .bind(fixture.tenant_id)
        .bind(locked_execution_id)
        .bind(json!({"type":"invocation.accepted","commandId":Uuid::now_v7()}))
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let mut execution_lock = fixture.state.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(fixture.tenant_id)
        .bind(locked_execution_id)
        .fetch_one(&mut *execution_lock)
        .await
        .unwrap();
    let raced = tokio::time::timeout(
        Duration::from_secs(2),
        agentx_v2_runtime::event_export::sequence_one(&fixture.state.pool, owner),
    )
    .await
    .expect("Event Sequencing must not wait for the Execution row lock")
    .expect("the locked Execution must remain readable from its committed snapshot");
    assert!(raced.is_some());
    execution_lock.rollback().await.unwrap();
    let raced_status: String = sqlx::query_scalar("SELECT status FROM execution_outbox WHERE id=?")
        .bind(raced_event_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(raced_status, "published");

    let invalid_id = Uuid::now_v7();
    sqlx::query("INSERT INTO execution_outbox(id,tenant_id,execution_id,message_type,payload_json,status) VALUES(?,?,NULL,'runtime_event',?,'pending')")
        .bind(invalid_id)
        .bind(fixture.tenant_id)
        .bind(json!({"kind":"retention_changed","unknownField":true}))
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    assert!(
        agentx_v2_runtime::event_export::sequence_one(&fixture.state.pool, owner)
            .await
            .is_err()
    );
    let invalid_status: (String, Option<String>) =
        sqlx::query_as("SELECT status,last_error FROM execution_outbox WHERE id=?")
            .bind(invalid_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(invalid_status.0, "failed");
    assert!(
        invalid_status
            .1
            .as_deref()
            .is_some_and(|error| error.starts_with("EVENT_PAYLOAD_INVALID:"))
    );

    let valid_id = Uuid::now_v7();
    let run_id = Uuid::now_v7();
    let payload = RuntimeEventPayloadV1::RetentionChanged {
        run_id,
        run_version: 1,
        status: "completed".into(),
        marked_count: 0,
        deleted_count: 0,
        failed_count: 0,
        dry_run: true,
        items: vec![],
    };
    sqlx::query("INSERT INTO execution_outbox(id,tenant_id,execution_id,message_type,event_type,aggregate_type,aggregate_id,aggregate_version,correlation_id,payload_json,status) VALUES(?,?,NULL,'runtime_event','retention_changed','retention',?,1,?,?, 'pending')")
        .bind(valid_id)
        .bind(fixture.tenant_id)
        .bind(run_id.to_string())
        .bind(run_id)
        .bind(serde_json::to_value(payload).unwrap())
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    assert!(
        agentx_v2_runtime::event_export::sequence_one(&fixture.state.pool, owner)
            .await
            .unwrap()
            .is_some()
    );
    let valid_status: String = sqlx::query_scalar("SELECT status FROM execution_outbox WHERE id=?")
        .bind(valid_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(valid_status, "published");
}

async fn quota_projection_claim_is_single_owner(pool: &MySqlPool) {
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            agentx_v2_runtime::quota::claim_projection(&pool, Uuid::now_v7()).await
        }));
    }
    let mut claims = Vec::new();
    for task in tasks {
        if let Some(claim) = task.await.unwrap().unwrap() {
            claims.push(claim);
        }
    }
    assert_eq!(
        claims.len(),
        1,
        "only one Quota Projection replica may lead"
    );
    let stale = claims[0];
    sqlx::query(
        "UPDATE runtime_role_leases SET locked_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE role_key='quota_projection'",
    )
    .execute(pool)
    .await
    .unwrap();
    let replacement = agentx_v2_runtime::quota::claim_projection(pool, Uuid::now_v7())
        .await
        .unwrap()
        .expect("expired Quota Projection Lease must be taken over");
    assert!(replacement.fencing_token > stale.fencing_token);
    assert!(
        agentx_v2_runtime::quota::heartbeat_projection(pool, stale)
            .await
            .is_err(),
        "stale Quota Projection fencing token must be rejected"
    );
}

async fn command_claim_returns_only_the_current_batch(fixture: &Fixture) {
    let command_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status) VALUES(?,?,'claim_fixture','execution',?,?,JSON_OBJECT(),'pending')",
    )
    .bind(command_id)
    .bind(fixture.tenant_id)
    .bind(execution_id.to_string())
    .bind(format!("claim-fixture:{command_id}"))
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let owner = Uuid::now_v7();
    let first = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap();
    assert!(first.iter().any(|claim| claim.command_id == command_id));
    let second = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap();
    assert!(
        second.iter().all(|claim| claim.command_id != command_id),
        "an in-flight command must not be returned again to the same owner"
    );
    sqlx::query("DELETE FROM runtime_commands WHERE id=?")
        .bind(command_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
}

async fn deterministic_start_rejection_is_terminal(fixture: &Fixture) {
    let accepted = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"stale-authorization"}),
            idempotency_key: "runtime-slice-stale-authorization".into(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE service_identity_projection SET updated_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 73 HOUR) WHERE identity_id=?")
        .bind(fixture.identity_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let owner = Uuid::now_v7();
    let claim = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == accepted.execution_id)
        .expect("stale-authorization Start Command must be claimable");
    process_command_with_state(&fixture.state, &claim)
        .await
        .unwrap();
    let facts = sqlx::query(
        "SELECT e.status,CAST(JSON_UNQUOTE(JSON_EXTRACT(e.error_json,'$.code')) AS CHAR) AS error_code,c.status AS command_status,(SELECT COUNT(*) FROM bundle_references r WHERE r.tenant_id=e.tenant_id AND r.reference_kind='active_execution' AND r.owner_id=e.id AND r.released_at IS NULL) AS live_references FROM workflow_executions e JOIN runtime_commands c ON c.id=? WHERE e.id=?",
    )
    .bind(claim.command_id)
    .bind(accepted.execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(facts.get::<String, _>("status"), "failed");
    assert_eq!(
        facts.get::<String, _>("error_code"),
        "RUNTIME_AUTHORIZATION_STALE"
    );
    assert_eq!(facts.get::<String, _>("command_status"), "failed");
    assert_eq!(facts.get::<i64, _>("live_references"), 0);
    sqlx::query(
        "UPDATE service_identity_projection SET updated_at=UTC_TIMESTAMP(6) WHERE identity_id=?",
    )
    .bind(fixture.identity_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
}

async fn revoked_grant_rejection_is_terminal_and_monotonic(fixture: &Fixture) {
    let accepted = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"revoked-grant"}),
            idempotency_key: "runtime-slice-revoked-grant".into(),
        },
    )
    .await
    .unwrap();
    let grant_id = Uuid::now_v7();
    let resource_id = Uuid::now_v7();
    let mut authorization: RuntimeAuthorizationSnapshotV1 = serde_json::from_value(
        sqlx::query_scalar(
            "SELECT authorization_snapshot_json FROM execution_snapshots WHERE execution_id=?",
        )
        .bind(accepted.execution_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap(),
    )
    .unwrap();
    authorization.grant_ids.push(grant_id);
    sqlx::query(
        "UPDATE execution_snapshots SET authorization_snapshot_json=? WHERE execution_id=?",
    )
    .bind(serde_json::to_value(authorization).unwrap())
    .bind(accepted.execution_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let grant_target = |policy_epoch, enabled| AdmissionTargetV1::ResourceGrant {
        state: RuntimeGrantStateV1 {
            tenant_id: fixture.tenant_id,
            identity_id: fixture.identity_id,
            grant_id,
            resource_kind: RuntimeResourceKindV1::Model,
            resource_id,
            operations: BTreeSet::from(["use".into()]),
            policy_epoch,
            enabled,
        },
    };
    apply_target(fixture, 2, grant_target(2, false)).await;
    apply_target(fixture, 1, grant_target(1, true)).await;
    let projection: (String, u64) = sqlx::query_as(
        "SELECT status,policy_epoch FROM resource_grant_projection WHERE tenant_id=? AND grant_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(grant_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(projection, ("revoked".into(), 2));

    let owner = Uuid::now_v7();
    let claim = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == accepted.execution_id)
        .expect("revoked Grant Start Command must be claimable");
    process_command_with_state(&fixture.state, &claim)
        .await
        .unwrap();
    let facts: (String, String, String, String) = sqlx::query_as(
        "SELECT e.status,CAST(JSON_UNQUOTE(JSON_EXTRACT(e.error_json,'$.code')) AS CHAR),i.status,c.status FROM workflow_executions e JOIN application_invocations i ON i.execution_id=e.id JOIN runtime_commands c ON c.id=? WHERE e.id=?",
    )
    .bind(claim.command_id)
    .bind(accepted.execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        facts,
        (
            "failed".into(),
            "RUNTIME_GRANT_REVOKED".into(),
            "failed".into(),
            "failed".into(),
        )
    );
}

async fn quota_projection_covers_all_dimensions_and_has_no_terminal_residue(fixture: &Fixture) {
    let dimensions = [
        "execution_concurrency",
        "node_concurrency",
        "sandbox_concurrency",
        "agent_iterations",
        "tokens",
        "cost_micros",
        "artifact_bytes",
        "cpu_millis",
        "memory_bytes",
        "pids",
        "disk_bytes",
        "ttl_seconds",
    ];
    for dimension in dimensions {
        sqlx::query(
            "INSERT INTO quota_policy_projection(tenant_id,dimension_key,hard_limit,period_seconds,version,updated_by) VALUES(?,?,1000000000,3600,1,?) ON DUPLICATE KEY UPDATE hard_limit=VALUES(hard_limit),period_seconds=VALUES(period_seconds)",
        )
        .bind(fixture.tenant_id)
        .bind(dimension)
        .bind(Uuid::now_v7())
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    }
    let projection = agentx_v2_runtime::quota::counter_projection(&fixture.state.pool)
        .await
        .unwrap()
        .into_iter()
        .filter(|counter| counter.tenant_id == fixture.tenant_id)
        .collect::<Vec<_>>();
    assert_eq!(projection.len(), dimensions.len());
    assert!(projection.iter().all(|counter| counter.hard_limit > 0));
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM quota_reservations WHERE tenant_id=? AND status='active' AND expires_at>UTC_TIMESTAMP(6)",
    )
    .bind(fixture.tenant_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(active, 0);
}

async fn wait_and_approval_resume_exactly_once(fixture: &Fixture) {
    let wait_execution = start_suspending_work_package(fixture, "wait").await;
    let wait: (Uuid, Uuid, Value) = sqlx::query_as(
        "SELECT w.id,w.node_execution_id,t.response_json FROM wait_subscriptions w JOIN execution_resume_tokens t ON t.id=w.resume_token_id WHERE w.tenant_id=? AND w.execution_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(wait_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let token = wait.2["resumeToken"].as_str().unwrap();
    let request_body = json!({"outputPort":"resumed","payload":{"message":"resumed-once"}});
    let router = agentx_v2_runtime::gateway::router().with_state(fixture.state.clone());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/waits/{token}/resume"))
                .header("content-type", "application/json")
                .header("idempotency-key", "wait:resume:once")
                .header("x-agentx-signature", "runtime-slice-signature")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);
    let replay = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/waits/{token}/resume"))
                .header("content-type", "application/json")
                .header("idempotency-key", "wait:resume:once")
                .header("x-agentx-signature", "runtime-slice-signature")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), axum::http::StatusCode::ACCEPTED);
    let conflict = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/waits/{token}/resume"))
                .header("content-type", "application/json")
                .header("idempotency-key", "wait:resume:once")
                .header("x-agentx-signature", "runtime-slice-signature")
                .body(Body::from(
                    serde_json::to_vec(
                        &json!({"outputPort":"resumed","payload":{"message":"different"}}),
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conflict.status(), axum::http::StatusCode::CONFLICT);
    process_execution_commands(fixture, wait_execution).await;
    let wait_state: (String, String, i64) = sqlx::query_as(
        "SELECT e.status,w.status,(SELECT COUNT(*) FROM bundle_references r WHERE r.tenant_id=w.tenant_id AND r.reference_kind='pending_wait' AND r.owner_id=w.id AND r.released_at IS NULL) FROM workflow_executions e JOIN wait_subscriptions w ON w.execution_id=e.id AND w.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=?",
    )
    .bind(fixture.tenant_id)
    .bind(wait_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(wait_state, ("succeeded".into(), "resumed".into(), 0));

    let cancelled_wait_execution = start_suspending_work_package(fixture, "wait").await;
    let cancelled_wait_response: Value = sqlx::query_scalar(
        "SELECT t.response_json FROM wait_subscriptions w JOIN execution_resume_tokens t ON t.id=w.resume_token_id WHERE w.tenant_id=? AND w.execution_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(cancelled_wait_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let cancelled_wait_token = cancelled_wait_response["resumeToken"].as_str().unwrap();
    let response = agentx_v2_runtime::gateway::router()
        .with_state(fixture.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/waits/{cancelled_wait_token}/resume"))
                .header("content-type", "application/json")
                .header("idempotency-key", "wait:resume:cancel-race")
                .header("x-agentx-signature", "runtime-slice-signature")
                .body(Body::from(
                    serde_json::to_vec(
                        &json!({"outputPort":"resumed","payload":{"message":"too-late"}}),
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);
    let cancel_command_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status) VALUES(?,?,'cancel_execution','execution',?,?,JSON_OBJECT(),'pending')",
    )
    .bind(cancel_command_id)
    .bind(fixture.tenant_id)
    .bind(cancelled_wait_execution.to_string())
    .bind(format!("wait:cancel:{cancelled_wait_execution}"))
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let race_owner = Uuid::now_v7();
    let mut commands = claim_commands(&fixture.state.pool, race_owner, 100)
        .await
        .unwrap();
    let cancel = commands
        .iter()
        .find(|command| command.command_id == cancel_command_id)
        .cloned()
        .unwrap();
    let resume = commands
        .drain(..)
        .find(|command| {
            command.execution_id == cancelled_wait_execution
                && command.command_type == "resume_wait"
        })
        .unwrap();
    process_command(&fixture.state.pool, &cancel).await.unwrap();
    process_command(&fixture.state.pool, &resume).await.unwrap();
    let cancelled_state: (String, String, String) = sqlx::query_as(
        "SELECT e.status,w.status,c.status FROM workflow_executions e JOIN wait_subscriptions w ON w.execution_id=e.id AND w.tenant_id=e.tenant_id JOIN runtime_commands c ON c.id=? WHERE e.tenant_id=? AND e.id=?",
    )
    .bind(resume.command_id)
    .bind(fixture.tenant_id)
    .bind(cancelled_wait_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        cancelled_state,
        ("cancelled".into(), "cancelled".into(), "completed".into()),
        "a Resume command that loses the cancellation race must converge without retrying"
    );

    let approval_execution = start_suspending_work_package(fixture, "approval").await;
    let task: (Uuid, u64) = sqlx::query_as(
        "SELECT id,version FROM approval_tasks WHERE tenant_id=? AND execution_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(approval_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let notification_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE tenant_id=? AND notification_type='approval_reassigned' AND target_type='user' AND target_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fixture.identity_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        notification_count, 1,
        "a single-candidate Approval must create one deterministic Runtime notification"
    );
    let approved = admission_request(
        fixture,
        100,
        AdmissionTargetV1::ApprovalDecision {
            state: RuntimeApprovalDecisionV1 {
                task_id: task.0,
                task_version: task.1,
                decision: ApprovalDecisionValueV1::Approved,
                decided_by: fixture.identity_id,
                reason: Some("approved by runtime slice".into()),
            },
        },
        "approval:decision:approved",
    );
    let rejected = admission_request(
        fixture,
        100,
        AdmissionTargetV1::ApprovalDecision {
            state: RuntimeApprovalDecisionV1 {
                task_id: task.0,
                task_version: task.1,
                decision: ApprovalDecisionValueV1::Rejected,
                decided_by: Uuid::now_v7(),
                reason: Some("concurrent rejection".into()),
            },
        },
        "approval:decision:rejected",
    );
    let (approved_result, rejected_result) = tokio::join!(
        apply_admission(
            State(fixture.state.clone()),
            publisher_headers("runtime.admission.apply"),
            Json(approved.clone())
        ),
        apply_admission(
            State(fixture.state.clone()),
            publisher_headers("runtime.admission.apply"),
            Json(rejected.clone())
        )
    );
    let approved_receipt = approved_result.unwrap().0;
    let rejected_receipt = rejected_result.unwrap().0;
    assert_ne!(approved_receipt.applied, rejected_receipt.applied);
    let winner = if approved_receipt.applied {
        approved
    } else {
        rejected
    };
    let replay = apply_admission(
        State(fixture.state.clone()),
        publisher_headers("runtime.admission.apply"),
        Json(winner),
    )
    .await
    .unwrap()
    .0;
    assert!(replay.replayed);
    let decision_commands: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runtime_commands WHERE tenant_id=? AND aggregate_id=? AND command_type='resume_execution'",
    )
    .bind(fixture.tenant_id)
    .bind(approval_execution.to_string())
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(decision_commands, 1);
    process_execution_commands(fixture, approval_execution).await;
    let approval_state: (String, String, String, u64) = sqlx::query_as(
        "SELECT e.status,a.status,a.resume_status,a.version FROM workflow_executions e JOIN approval_tasks a ON a.execution_id=e.id AND a.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=?",
    )
    .bind(fixture.tenant_id)
    .bind(approval_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(approval_state.0, "succeeded");
    assert!(matches!(approval_state.1.as_str(), "approved" | "rejected"));
    assert_eq!(approval_state.2, "succeeded");
    // Decision acceptance and the subsequent resume completion are distinct
    // authoritative transitions and therefore advance the task version twice.
    assert_eq!(approval_state.3, 3);

    let invalid_approval_execution = start_suspending_work_package_with_parameters(
        fixture,
        "approval",
        json!({
            "title":"Invalid approval",
            "candidateUserId":{
                "kind":"reference",
                "selector":{
                    "namespace":"inputs",
                    "run":{"kind":"current"},
                    "item":{"kind":"current"},
                    "path":["missingCandidate"]
                },
                "missingPolicy":{"kind":"error"}
            }
        }),
    )
    .await;
    let invalid_state: (String, String, String, String, String, i64) = sqlx::query_as(
        "SELECT e.status,e.error_code,n.status,n.error_code,a.error_code,(SELECT COUNT(*) FROM approval_tasks t WHERE t.tenant_id=e.tenant_id AND t.execution_id=e.id) FROM workflow_executions e JOIN node_executions n ON n.tenant_id=e.tenant_id AND n.execution_id=e.id JOIN node_attempts a ON a.tenant_id=n.tenant_id AND a.node_execution_id=n.id WHERE e.tenant_id=? AND e.id=?",
    )
    .bind(fixture.tenant_id)
    .bind(invalid_approval_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        invalid_state,
        (
            "failed".into(),
            "DYNAMIC_VALUE_EVALUATION_FAILED".into(),
            "failed".into(),
            "DYNAMIC_VALUE_EVALUATION_FAILED".into(),
            "DYNAMIC_VALUE_EVALUATION_FAILED".into(),
            0,
        )
    );

    let duration_execution = start_suspending_work_package_with_parameters(
        fixture,
        "wait",
        json!({"kind":"duration","durationMs":1}),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert_eq!(
        agentx_v2_runtime::enqueue_due_waits(&fixture.state.pool, Uuid::now_v7(), 100)
            .await
            .unwrap(),
        1
    );
    process_execution_commands(fixture, duration_execution).await;
    let duration_state: (String, String) = sqlx::query_as(
        "SELECT e.status,w.status FROM workflow_executions e JOIN wait_subscriptions w ON w.tenant_id=e.tenant_id AND w.execution_id=e.id WHERE e.tenant_id=? AND e.id=?",
    )
    .bind(fixture.tenant_id)
    .bind(duration_execution)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(duration_state, ("succeeded".into(), "resumed".into()));
}

async fn start_suspending_work_package(fixture: &Fixture, node_type: &str) -> Uuid {
    let parameters = if node_type == "approval" {
        json!({
            "title":"Runtime approval",
            "timeoutMs":300000,
            "candidateUserId":{"kind":"literal","value":fixture.identity_id}
        })
    } else {
        json!({"kind":"webhook","authenticationMode":"signed"})
    };
    start_suspending_work_package_with_parameters(fixture, node_type, parameters).await
}

async fn start_suspending_work_package_with_parameters(
    fixture: &Fixture,
    node_type: &str,
    parameters: Value,
) -> Uuid {
    let now = OffsetDateTime::now_utc();
    let package_id = Uuid::now_v7();
    let mut source = fixture.work_package_source(package_id, now, now + time::Duration::hours(1));
    source.source_revision = format!("{node_type}:1");
    source.definition = suspension_definition(node_type, parameters);
    let compiled = compile_workflow_version(&source.definition, package_id).unwrap();
    source.spec = agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Debug {
        draft_revision: 1,
        debug_plan: agentx_runtime_contracts::RuntimeDebugPlanV1::whole(&compiled),
    };
    let package = build_work_package(
        source,
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("{node_type}:prepare:{package_id}"),
            work_package: package,
        }),
    )
    .await
    .unwrap();
    let started = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("{node_type}:execute:{package_id}"),
            package_id,
            input: json!({"message":"suspend"}),
        }),
    )
    .await
    .unwrap()
    .0;
    let execution_id = serde_json::from_value(started.result["executionId"].clone()).unwrap();
    process_execution_commands(fixture, execution_id).await;
    execution_id
}

async fn process_execution_commands(fixture: &Fixture, execution_id: Uuid) {
    let owner = Uuid::now_v7();
    let commands = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap();
    let mut matched = 0;
    for command in commands {
        if command.execution_id == execution_id {
            process_command(&fixture.state.pool, &command)
                .await
                .unwrap();
            matched += 1;
        }
    }
    assert_eq!(
        matched, 1,
        "expected one Runtime Command for {execution_id}"
    );
}

fn suspension_definition(node_type: &str, parameters: Value) -> WorkflowDefinition {
    let mut connections = vec![
        json!({"id":"start-suspend","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"suspend","targetHandle":"main","order":0}),
    ];
    if node_type == "approval" {
        connections.push(json!({"id":"approved-end","sourceNodeId":"suspend","sourceHandle":"approved","targetNodeId":"__end__","targetHandle":"main","order":0}));
        connections.push(json!({"id":"rejected-end","sourceNodeId":"suspend","sourceHandle":"rejected","targetNodeId":"__end__","targetHandle":"main","order":1}));
    } else {
        connections.push(json!({"id":"resumed-end","sourceNodeId":"suspend","sourceHandle":"resumed","targetNodeId":"__end__","targetHandle":"main","order":0}));
    }
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{"missingCandidate":{"type":"string"}},"additionalProperties":true},"contexts":{}},
        "nodes":[{
            "id":"suspend",
            "key":"suspend",
            "type":node_type,
            "typeVersion":1,
            "name":"Suspend",
            "parameters":parameters,
            "outputProjection":{},
            "contextWrites":[],
            "resourceReferences":[]
        }],
        "connections":connections,
        "end":{"outputs":{}},
        "settings":{"activationBudget":20,"executionOrder":"deterministic"}
    }))
    .unwrap()
}

async fn large_worker_results_are_externalized_and_verified(fixture: &Fixture) {
    let package_id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let mut source = fixture.work_package_source(package_id, now, now + time::Duration::hours(1));
    let message = "v2-large-result".repeat(6_000);
    source.overlay.input = json!({"message":message});
    let package = build_work_package(
        source,
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("large:prepare:{package_id}"),
            work_package: package,
        }),
    )
    .await
    .unwrap();
    let started = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("large:execute:{package_id}"),
            package_id,
            input: Value::Null,
        }),
    )
    .await
    .unwrap()
    .0;
    let execution_id: Uuid = serde_json::from_value(started.result["executionId"].clone()).unwrap();
    let owner = Uuid::now_v7();
    let command = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &command)
        .await
        .unwrap();
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let task = dispatch.task().unwrap();
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let worker_id = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        &task,
    )
    .await
    .unwrap()
    .unwrap();
    let worker = test_worker(fixture, StubWorkerMode::Reject);
    let execution = worker.execute(&claim).await;
    let result = worker.build_result(&claim, execution).await.unwrap();
    let object = result.output_object.clone().unwrap();
    assert!(result.outputs.is_empty());
    agentx_v2_runtime::engine::submit_worker_result_with_objects(
        &fixture.state.pool,
        fixture.state.objects.clone(),
        &result,
    )
    .await
    .unwrap();
    let stored: (Option<Value>, Option<Uuid>) =
        sqlx::query_as("SELECT output_json,result_object_id FROM node_attempts WHERE id=?")
            .bind(result.attempt_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert!(stored.0.is_none());
    assert_eq!(stored.1, Some(object.object_id));
    let terminal: Value = sqlx::query_scalar(
        "SELECT terminal_result_json FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(terminal["message"], message);
    while agentx_v2_runtime::artifact::externalize_one(&fixture.state)
        .await
        .unwrap()
    {}
    let terminal_object_id: Uuid = sqlx::query_scalar(
        "SELECT terminal_result_object_id FROM workflow_executions WHERE tenant_id=? AND id=? AND terminal_result_json IS NULL",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_ne!(terminal_object_id, object.object_id);
    let input_artifact_id: Uuid = sqlx::query_scalar(
        "SELECT r.artifact_id FROM artifact_references r JOIN node_attempts a ON a.node_execution_id=UUID_TO_BIN(r.owner_id) WHERE r.tenant_id=? AND a.id=? AND r.reference_role=? LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .bind(result.attempt_id)
    .bind(format!("trace_input:{}", result.attempt_id))
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_ne!(input_artifact_id, object.object_id);
    let trace_refs: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT CAST(JSON_UNQUOTE(JSON_EXTRACT(payload_json,'$.contentRef')) AS CHAR(36)) FROM trace_outbox WHERE tenant_id=? AND execution_id=? AND JSON_EXTRACT(payload_json,'$.contentRef') IS NOT NULL",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .fetch_all(&fixture.state.pool)
    .await
    .unwrap();
    assert!(trace_refs.contains(&input_artifact_id.to_string()));
    assert!(trace_refs.contains(&object.object_id.to_string()));
    let artifact_subject = Uuid::now_v7();
    sqlx::query("INSERT INTO runtime_user_admission(tenant_id,user_id,token_version,status,tenant_query_enabled,admission_epoch) VALUES(?,?,1,'active',FALSE,1)")
        .bind(fixture.tenant_id).bind(artifact_subject).execute(&fixture.state.pool).await.unwrap();
    sqlx::query("INSERT INTO runtime_user_application_grants(tenant_id,user_id,application_id,grant_version,status,can_invoke,can_query,admission_epoch) VALUES(?,?,?,1,'active',TRUE,TRUE,1)")
        .bind(fixture.tenant_id).bind(artifact_subject).bind(fixture.application_id).execute(&fixture.state.pool).await.unwrap();
    sqlx::query("INSERT INTO runtime_user_workflow_grants(tenant_id,user_id,workflow_id,grant_version,status,admission_epoch) VALUES(?,?,?,1,'active',1)")
        .bind(fixture.tenant_id).bind(artifact_subject).bind(fixture.workflow_id).execute(&fixture.state.pool).await.unwrap();
    let artifact_hash = agentx_runtime_contracts::content_hash(
        &json!({"operation":"execution_artifact","executionId":execution_id}),
    )
    .unwrap();
    let downloaded = get_execution_artifact(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            artifact_subject,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([execution_id]),
            artifact_hash.clone(),
        ),
        Path((execution_id, input_artifact_id)),
    )
    .await
    .unwrap();
    assert!(
        !to_bytes(downloaded.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty()
    );
    let denied = get_execution_artifact(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            artifact_subject,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([Uuid::now_v7()]),
            artifact_hash,
        ),
        Path((execution_id, input_artifact_id)),
    )
    .await;
    assert!(matches!(denied, Err(RuntimeError::Unauthorized)));
    let checkpoint: (Uuid, Uuid) = sqlx::query_as(
        "SELECT id,payload_artifact_id FROM checkpoints WHERE tenant_id=? AND execution_id=? AND payload_json IS NULL ORDER BY sequence_number DESC LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let source_version: u64 = sqlx::query_scalar(
        "SELECT state_version FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let fork_command_id = Uuid::now_v7();
    let _ = apply_runtime_command(
        State(fixture.state.clone()),
        publisher_headers("runtime.commands.apply"),
        Json(agentx_runtime_contracts::RuntimeCommandApplyRequestV1 {
            api_version: 1,
            command_id: fork_command_id,
            tenant_id: fixture.tenant_id,
            object_version: source_version,
            idempotency_key: format!("large:fork:{execution_id}"),
            command: agentx_runtime_contracts::ExecutionCommandV1::Fork {
                source_execution_id: execution_id,
                checkpoint_id: checkpoint.0,
                mode: agentx_runtime_contracts::PartialExecutionModeV1::Whole,
                node_id: None,
                side_effect_resolution: SideEffectResolutionV1::Execute,
            },
        }),
    )
    .await
    .unwrap();
    let fork_claim = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.command_id == fork_command_id)
        .unwrap();
    process_command(&fixture.state.pool, &fork_claim)
        .await
        .unwrap();
    let fork_execution_id: Uuid = sqlx::query_scalar(
        "SELECT fork_execution_id FROM execution_forks WHERE tenant_id=? AND source_execution_id=? AND source_checkpoint_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .bind(checkpoint.0)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let start_claim = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == fork_execution_id)
        .unwrap();
    agentx_v2_runtime::execution::process_command_with_state(&fixture.state, &start_claim)
        .await
        .unwrap();
    let fork_status: String =
        sqlx::query_scalar("SELECT status FROM workflow_executions WHERE id=?")
            .bind(fork_execution_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(fork_status, "running");
    let fork_dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let fork_task = fork_dispatch.task().unwrap();
    assert_eq!(fork_task.execution_id, fork_execution_id);
    complete_dispatch(&fixture.state.pool, &fork_dispatch)
        .await
        .unwrap();
    let fork_worker_id = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        fork_worker_id,
        fork_task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let fork_worker_claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        fork_worker_id,
        fork_task.capability.as_str(),
        &fork_task,
    )
    .await
    .unwrap()
    .unwrap();
    let fork_execution = worker.execute(&fork_worker_claim).await;
    let fork_result = worker
        .build_result(&fork_worker_claim, fork_execution)
        .await
        .unwrap();
    agentx_v2_runtime::engine::submit_worker_result_with_objects(
        &fixture.state.pool,
        fixture.state.objects.clone(),
        &fork_result,
    )
    .await
    .unwrap();
}

async fn fork_uses_checkpoint_machine_and_preserves_source(
    fixture: &Fixture,
    source_execution_id: Uuid,
) {
    let checkpoint_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM checkpoints WHERE tenant_id=? AND execution_id=? ORDER BY sequence_number DESC LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .bind(source_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let source_version: u64 = sqlx::query_scalar(
        "SELECT state_version FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(source_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let command_id = Uuid::now_v7();
    let _ = apply_runtime_command(
        State(fixture.state.clone()),
        publisher_headers("runtime.commands.apply"),
        Json(agentx_runtime_contracts::RuntimeCommandApplyRequestV1 {
            api_version: 1,
            tenant_id: fixture.tenant_id,
            command_id,
            object_version: source_version,
            idempotency_key: format!("fork:{source_execution_id}:pass"),
            command: agentx_runtime_contracts::ExecutionCommandV1::Fork {
                source_execution_id,
                checkpoint_id,
                mode: agentx_runtime_contracts::PartialExecutionModeV1::Node,
                node_id: Some("pass".into()),
                side_effect_resolution: SideEffectResolutionV1::ReuseOutput,
            },
        }),
    )
    .await
    .unwrap();
    let owner = Uuid::now_v7();
    let fork_command = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|command| command.command_id == command_id)
        .unwrap();
    process_command(&fixture.state.pool, &fork_command)
        .await
        .unwrap();
    let fork_execution_id: Uuid = sqlx::query_scalar(
        "SELECT fork_execution_id FROM execution_forks WHERE tenant_id=? AND source_execution_id=? AND source_checkpoint_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(source_execution_id)
    .bind(checkpoint_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let start = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|command| command.execution_id == fork_execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &start).await.unwrap();
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let task = dispatch.task().unwrap();
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let worker_id = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        &task,
    )
    .await
    .unwrap()
    .unwrap();
    agentx_v2_runtime::engine::submit_worker_result(
        &fixture.state.pool,
        &successful_worker_result(&claim),
    )
    .await
    .unwrap();
    let fork_result: Value = sqlx::query_scalar(
        "SELECT terminal_result_json FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fork_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(fork_result["message"], "agentx-v2");
    let source_status: String =
        sqlx::query_scalar("SELECT status FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(fixture.tenant_id)
            .bind(source_execution_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(source_status, "succeeded");

    let mut checkpoint_payload: Value =
        sqlx::query_scalar("SELECT payload_json FROM checkpoints WHERE tenant_id=? AND id=?")
            .bind(fixture.tenant_id)
            .bind(checkpoint_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    checkpoint_payload["machine"]["workflow"]["nodes"][0]["sideEffectLevel"] =
        json!("irreversible");
    let checkpoint_hash = agentx_runtime_contracts::content_hash(&checkpoint_payload).unwrap();
    sqlx::query(
        "UPDATE checkpoints SET payload_json=?,payload_hash=?,state_hash=? WHERE tenant_id=? AND id=?",
    )
    .bind(&checkpoint_payload)
    .bind(checkpoint_hash.as_str())
    .bind(checkpoint_hash.as_str())
    .bind(fixture.tenant_id)
    .bind(checkpoint_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();

    let dry_run_command_id = Uuid::now_v7();
    let _ = apply_runtime_command(
        State(fixture.state.clone()),
        publisher_headers("runtime.commands.apply"),
        Json(agentx_runtime_contracts::RuntimeCommandApplyRequestV1 {
            api_version: 1,
            tenant_id: fixture.tenant_id,
            command_id: dry_run_command_id,
            object_version: source_version,
            idempotency_key: format!("fork:{source_execution_id}:dry-run"),
            command: agentx_runtime_contracts::ExecutionCommandV1::Fork {
                source_execution_id,
                checkpoint_id,
                mode: agentx_runtime_contracts::PartialExecutionModeV1::Whole,
                node_id: None,
                side_effect_resolution: SideEffectResolutionV1::DryRun,
            },
        }),
    )
    .await
    .unwrap();
    let fork_command = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|command| command.command_id == dry_run_command_id)
        .unwrap();
    process_command(&fixture.state.pool, &fork_command)
        .await
        .unwrap();
    let dry_run_execution_id: Uuid = sqlx::query_scalar(
        "SELECT fork_execution_id FROM execution_forks WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(format!("runtime-command:{dry_run_command_id}"))
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let start = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|command| command.execution_id == dry_run_execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &start).await.unwrap();
    let dry_run_result: Value = sqlx::query_scalar(
        "SELECT terminal_result_json FROM workflow_executions WHERE tenant_id=? AND id=? AND status='succeeded'",
    )
    .bind(fixture.tenant_id)
    .bind(dry_run_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(dry_run_result["message"], "agentx-v2");
    let dry_run_dispatches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_outbox WHERE tenant_id=? AND execution_id=? AND message_type='dispatch_node'",
    )
    .bind(fixture.tenant_id)
    .bind(dry_run_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        dry_run_dispatches, 0,
        "dry-run Fork must not dispatch the side-effect node"
    );
}

async fn composite_child_uses_immutable_runtime_snapshot_and_merges_on_success(fixture: &Fixture) {
    let now = OffsetDateTime::now_utc();
    let package_id = Uuid::now_v7();
    let child_version_id = Uuid::now_v7();
    let child_definition = definition();
    let child_ir = compile_workflow_version(&child_definition, child_version_id).unwrap();
    let definition_bytes = agentx_runtime_contracts::canonical_bytes(&child_definition).unwrap();
    let ir_bytes = agentx_runtime_contracts::canonical_bytes(&child_ir).unwrap();
    let definition_hash = agentx_runtime_contracts::content_hash(&child_definition).unwrap();
    let ir_hash = agentx_runtime_contracts::content_hash(&child_ir).unwrap();
    let ir_object_id = composite_ir_object_id(child_version_id);
    let definition_object = RuntimeObjectReferenceV1 {
        tenant_id: fixture.tenant_id,
        storage_domain: StorageDomain::Runtime,
        object_id: child_version_id,
        object_key: RuntimeObjectReferenceV1::canonical_key(
            fixture.tenant_id,
            child_version_id,
            &definition_hash,
        ),
        content_hash: definition_hash,
        size_bytes: definition_bytes.len() as u64,
        media_type: "application/vnd.agentx.workflow-definition+json".into(),
    };
    let ir_object = RuntimeObjectReferenceV1 {
        tenant_id: fixture.tenant_id,
        storage_domain: StorageDomain::Runtime,
        object_id: ir_object_id,
        object_key: RuntimeObjectReferenceV1::canonical_key(
            fixture.tenant_id,
            ir_object_id,
            &ir_hash,
        ),
        content_hash: ir_hash,
        size_bytes: ir_bytes.len() as u64,
        media_type: "application/vnd.agentx.compiled-workflow.v1+json".into(),
    };
    for (object, bytes) in [
        (definition_object.clone(), definition_bytes),
        (ir_object.clone(), ir_bytes),
    ] {
        persist_upload(
            &fixture.state,
            RuntimeObjectUploadMetadataV1 {
                api_version: 1,
                idempotency_key: format!("composite:upload:{}", object.object_id),
                tenant_id: object.tenant_id,
                object_id: object.object_id,
                content_hash: object.content_hash.clone(),
                size_bytes: object.size_bytes,
                media_type: object.media_type.clone(),
            },
            Bytes::from(bytes),
        )
        .await
        .unwrap();
    }
    let configuration = agentx_runtime_contracts::RuntimeResourceConfigurationV1::Composite {
        workflow_version_id: child_version_id,
        definition_object_id: child_version_id,
        ir_object_id,
    };
    let mut source = fixture.work_package_source(package_id, now, now + time::Duration::hours(1));
    source.definition = composite_definition(child_version_id);
    source
        .dependency_versions
        .insert(child_version_id, child_definition);
    let compiled = agentx_bundle_builder::compile_workflow_version_with_dependencies(
        &source.definition,
        package_id,
        &source.dependency_versions,
    )
    .unwrap();
    source.spec = agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Debug {
        draft_revision: 1,
        debug_plan: agentx_runtime_contracts::RuntimeDebugPlanV1::whole(&compiled),
    };
    source.objects = vec![definition_object, ir_object];
    source.resources = vec![agentx_runtime_contracts::RuntimeResourceBindingV1 {
        resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::Composite,
        resource_id: child_version_id,
        resource_version: child_version_id.to_string(),
        state_epoch: 1,
        content_hash: agentx_runtime_contracts::content_hash(&configuration).unwrap(),
        configuration,
        object_ids: vec![child_version_id, ir_object_id],
    }];
    let package = build_work_package(
        source,
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("composite:prepare:{package_id}"),
            work_package: package,
        }),
    )
    .await
    .unwrap();
    let started = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("composite:execute:{package_id}"),
            package_id,
            input: json!({"message":"composite-v2"}),
        }),
    )
    .await
    .unwrap()
    .0;
    let parent_execution_id =
        Uuid::parse_str(started.result["executionId"].as_str().unwrap()).unwrap();
    let owner = Uuid::now_v7();
    let parent_start = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == parent_execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &parent_start)
        .await
        .unwrap();
    let child_execution_id: Uuid = sqlx::query_scalar(
        "SELECT child_execution_id FROM execution_children WHERE tenant_id=? AND parent_execution_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(parent_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let child_start = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == child_execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &child_start)
        .await
        .unwrap();
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let task = dispatch.task().unwrap();
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let worker_id = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        &task,
    )
    .await
    .unwrap()
    .unwrap();
    agentx_v2_runtime::engine::submit_worker_result(
        &fixture.state.pool,
        &successful_worker_result(&claim),
    )
    .await
    .unwrap();
    let parent_resume = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|command| command.execution_id == parent_execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &parent_resume)
        .await
        .unwrap();
    let relation: (String, Value) = sqlx::query_as(
        "SELECT merge_status,context_overlay_json FROM execution_children WHERE tenant_id=? AND child_execution_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(child_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(relation.0, "merged");
    let result: Value = sqlx::query_scalar(
        "SELECT terminal_result_json FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(parent_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(result["message"], "composite-v2");
}

async fn agent_worker_runs_a_bounded_tool_loop_and_persists_usage(fixture: &Fixture) {
    let model_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let endpoint = "https://provider.example.test";
    let model_configuration = agentx_runtime_contracts::RuntimeResourceConfigurationV1::Model {
        provider: "fixture".into(),
        endpoint: format!("{endpoint}/model"),
        model: "fixture-model".into(),
        price_version: "price:1".into(),
        credential: None,
    };
    let mcp_configuration = agentx_runtime_contracts::RuntimeResourceConfigurationV1::Mcp {
        endpoint: format!("{endpoint}/mcp"),
        tool_name: "search".into(),
        tool_version: "1".into(),
        input_schema_hash: agentx_runtime_contracts::content_hash(&json!({"type":"object"}))
            .unwrap(),
        credential: None,
    };
    let attempt_id = Uuid::now_v7();
    let node_execution_id = Uuid::now_v7();
    let claim = agentx_v2_runtime::engine::ClaimedWorkerAttempt {
        lease: agentx_runtime_contracts::WorkerAttemptLeaseV1 {
            protocol_version: 1,
            attempt_id,
            worker_id: Uuid::now_v7(),
            fencing_token: 1,
            locked_until: OffsetDateTime::now_utc() + time::Duration::seconds(30),
        },
        task: agentx_runtime_contracts::WorkerTaskV1 {
            protocol_version: 1,
            task_id: Uuid::now_v7(),
            tenant_id: fixture.tenant_id,
            execution_id: Uuid::now_v7(),
            node_execution_id,
            attempt_id,
            capability: agentx_node_protocol::NodeCapability::Agent,
            bundle_id: Uuid::now_v7(),
            work_package_id: None,
            state_version: 1,
            compatibility_hash: agentx_runtime_contracts::content_hash(&json!({"agent":1}))
                .unwrap(),
            deadline_at: OffsetDateTime::now_utc() + time::Duration::seconds(30),
        },
        node_type: "agent".into(),
        node_version: 1,
        run_index: 0,
        iteration_index: 0,
        node_parameters: json!({"budget":{"maxIterations":3,"maxTokens":100,"maxCostMicros":100}}),
        inputs: BTreeMap::from([(
            "main".into(),
            vec![agentx_node_protocol::Item {
                json: json!({"question":"agentx"}),
                ..Default::default()
            }],
        )]),
        resources: vec![
            agentx_runtime_contracts::RuntimeResourceBindingV1 {
                resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::Model,
                resource_id: Uuid::now_v7(),
                resource_version: "model:1".into(),
                state_epoch: 1,
                content_hash: agentx_runtime_contracts::content_hash(&model_configuration).unwrap(),
                configuration: model_configuration,
                object_ids: vec![],
            },
            agentx_runtime_contracts::RuntimeResourceBindingV1 {
                resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::Mcp,
                resource_id: Uuid::now_v7(),
                resource_version: "mcp:1".into(),
                state_epoch: 1,
                content_hash: agentx_runtime_contracts::content_hash(&mcp_configuration).unwrap(),
                configuration: mcp_configuration,
                object_ids: vec![],
            },
        ],
        context: json!({}),
    };
    let worker = test_worker(fixture, StubWorkerMode::Agent(model_calls.clone()));
    let output = worker.execute(&claim).await;
    assert_eq!(
        output.status,
        WorkerResultStatusV1::Succeeded,
        "skill failed: {:?} {:?}",
        output.error_code,
        output.error_message
    );
    assert_eq!(model_calls.load(Ordering::SeqCst), 2);
    let run = sqlx::query(
        "SELECT iteration_count,model_call_count,tool_call_count,input_tokens,cost_micros,status FROM agent_runs WHERE node_execution_id=?",
    )
    .bind(node_execution_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(run.try_get::<u32, _>("iteration_count").unwrap(), 2);
    assert_eq!(run.try_get::<u32, _>("model_call_count").unwrap(), 2);
    assert_eq!(run.try_get::<u32, _>("tool_call_count").unwrap(), 1);
    assert_eq!(run.try_get::<u64, _>("input_tokens").unwrap(), 20);
    assert_eq!(run.try_get::<u64, _>("cost_micros").unwrap(), 10);
    assert_eq!(run.try_get::<String, _>("status").unwrap(), "succeeded");
    let call_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runtime_calls WHERE attempt_id=?")
            .bind(attempt_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(call_count, 3);
}

async fn skill_worker_loads_and_verifies_the_runtime_object_closure(fixture: &Fixture) {
    let entrypoint_id = Uuid::now_v7();
    let program = agentx_runtime_contracts::RuntimeSkillProgramV1 {
        schema_version: 1,
        instructions: "Return the immutable Skill result".into(),
        dependency_object_ids: vec![],
    };
    let bytes = agentx_runtime_contracts::canonical_bytes(&program).unwrap();
    let raw_hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    let object = RuntimeObjectReferenceV1 {
        storage_domain: StorageDomain::Runtime,
        tenant_id: fixture.tenant_id,
        object_id: entrypoint_id,
        content_hash: agentx_runtime_contracts::ContentHash::parse(&raw_hash).unwrap(),
        size_bytes: bytes.len() as u64,
        media_type: "application/vnd.agentx.skill-program.v1+json".into(),
        object_key: format!(
            "runtime/{}/{}/{}",
            fixture.tenant_id,
            entrypoint_id,
            raw_hash.trim_start_matches("sha256:")
        ),
    };
    persist_upload(
        &fixture.state,
        RuntimeObjectUploadMetadataV1 {
            api_version: 1,
            idempotency_key: format!("skill:{entrypoint_id}"),
            tenant_id: fixture.tenant_id,
            object_id: entrypoint_id,
            content_hash: object.content_hash.clone(),
            size_bytes: object.size_bytes,
            media_type: object.media_type.clone(),
        },
        Bytes::from(bytes),
    )
    .await
    .unwrap();
    let configuration = agentx_runtime_contracts::RuntimeResourceConfigurationV1::Skill {
        entrypoint_object_id: entrypoint_id,
        dependency_object_ids: vec![],
    };
    let skill_binding_id = Uuid::now_v7();
    let attempt_id = Uuid::now_v7();
    let claim = agentx_v2_runtime::engine::ClaimedWorkerAttempt {
        lease: agentx_runtime_contracts::WorkerAttemptLeaseV1 {
            protocol_version: 1,
            attempt_id,
            worker_id: Uuid::now_v7(),
            fencing_token: 1,
            locked_until: OffsetDateTime::now_utc() + time::Duration::seconds(30),
        },
        task: agentx_runtime_contracts::WorkerTaskV1 {
            protocol_version: 1,
            task_id: Uuid::now_v7(),
            tenant_id: fixture.tenant_id,
            execution_id: Uuid::now_v7(),
            node_execution_id: Uuid::now_v7(),
            attempt_id,
            capability: agentx_node_protocol::NodeCapability::Skill,
            bundle_id: Uuid::now_v7(),
            work_package_id: None,
            state_version: 1,
            compatibility_hash: agentx_runtime_contracts::content_hash(&json!({"skill":1}))
                .unwrap(),
            deadline_at: OffsetDateTime::now_utc() + time::Duration::seconds(30),
        },
        node_type: "skill".into(),
        node_version: 1,
        run_index: 0,
        iteration_index: 0,
        node_parameters: json!({"resourceId":skill_binding_id}),
        inputs: BTreeMap::from([(
            "main".into(),
            vec![agentx_node_protocol::Item {
                json: json!({"message":"skill-input"}),
                ..Default::default()
            }],
        )]),
        resources: vec![agentx_runtime_contracts::RuntimeResourceBindingV1 {
            resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::Skill,
            resource_id: skill_binding_id,
            resource_version: "skill:1".into(),
            state_epoch: 1,
            content_hash: agentx_runtime_contracts::content_hash(&configuration).unwrap(),
            configuration,
            object_ids: vec![entrypoint_id],
        }],
        context: json!({}),
    };
    let worker = test_worker(fixture, StubWorkerMode::Reject);
    let output = worker.execute(&claim).await;
    assert_eq!(
        output.status,
        WorkerResultStatusV1::Succeeded,
        "skill failed: {:?} {:?}",
        output.error_code,
        output.error_message
    );
    assert_eq!(
        output.outputs["main"][0].json["instructions"],
        "Return the immutable Skill result"
    );
}

async fn sandbox_manager_is_fenced_and_idempotent(fixture: &Fixture) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_endpoint = format!("http://{}", listener.local_addr().unwrap());
    let execd_endpoint = provider_endpoint.clone();
    let takeover_command_blocked = Arc::new(AtomicBool::new(false));
    let command_block = takeover_command_blocked.clone();
    let provider = Router::new()
        .route(
            "/v1/sandboxes",
            post(|| async {
                Json(json!({
                    "id":"sandbox-v2",
                    "status":{"state":"Running"},
                    "metadata":{}
                }))
            }),
        )
        .route(
            "/v1/sandboxes/{id}/endpoints/44772",
            get(move |Path(id): Path<String>| {
                let endpoint = execd_endpoint.clone();
                async move {
                    Json(json!({
                        "endpoint":format!("{endpoint}/v1/sandboxes/{id}/proxy/44772"),
                        "headers":{}
                    }))
                }
            }),
        )
        .route(
            "/v1/sandboxes/{id}/proxy/44772/command",
            post(move |Json(body): Json<Value>| {
                let command_block = command_block.clone();
                async move {
                    let command = body.get("command").and_then(Value::as_str).unwrap_or_default();
                    if command.contains("cHJpbnRmIHRha2VvdmVy")
                        && !command_block.swap(true, Ordering::SeqCst)
                    {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                    if command.contains("ZXhpdCAx") {
                        (
                            axum::http::StatusCode::OK,
                            "data: {\"type\":\"stderr\",\"text\":\"failed\"}\n\ndata: {\"type\":\"result\",\"exit_code\":1}\n\n",
                        )
                    } else {
                        (
                            axum::http::StatusCode::OK,
                            "data: {\"type\":\"stdout\",\"text\":\"ok\"}\n\ndata: {\"type\":\"result\",\"exit_code\":0}\n\n",
                        )
                    }
                }
            }),
        )
        .route(
            "/v1/sandboxes/{id}",
            axum::routing::delete(|Path(id): Path<String>| async move {
                Json(json!({"sandboxId":id,"terminated":true}))
            }),
        );
    let provider_task = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    let worker_id = Uuid::now_v7();
    let execution_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_executions WHERE tenant_id=? ORDER BY created_at,id LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let node_execution_id = Uuid::now_v7();
    let attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,worker_protocol_version,ir_schema_version,compiler_version,manifest_version,status,idempotency_key,worker_instance_id,locked_until,fencing_token,deadline_at,input_json) VALUES(?,?,?,?,1,'sandbox',1,1,? ,?,'running',?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),7,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),JSON_OBJECT())",
    )
    .bind(attempt_id)
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .bind(node_execution_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .bind(format!("sandbox-test:{attempt_id}"))
    .bind(worker_id.to_string())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let configuration = agentx_runtime_contracts::RuntimeResourceConfigurationV1::SandboxProfile {
        provider: "opensandbox".into(),
        image: "python:3.13".into(),
        cpu_millis: 500,
        memory_bytes: 256 * 1024 * 1024,
        disk_bytes: 1024 * 1024 * 1024,
        pid_limit: 64,
        egress_mode: agentx_runtime_contracts::SandboxEgressModeV1::None,
        maximum_ttl_seconds: 300,
    };
    let request = agentx_v2_runtime::sandbox::SandboxExecuteRequestV1 {
        api_version: 1,
        tenant_id: fixture.tenant_id,
        execution_id,
        node_execution_id,
        attempt_id,
        worker_id,
        fencing_token: 7,
        idempotency_key: format!("sandbox:execute:{attempt_id}"),
        profile: agentx_runtime_contracts::RuntimeResourceBindingV1 {
            resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::SandboxProfile,
            resource_id: Uuid::now_v7(),
            resource_version: "profile:1".into(),
            state_epoch: 1,
            content_hash: agentx_runtime_contracts::content_hash(&configuration).unwrap(),
            configuration,
            object_ids: vec![],
        },
        input: json!({"code":"print('ok')"}),
        parameters: json!({"runner":"shell","source":"printf ok"}),
    };
    let manager_state = agentx_v2_runtime::sandbox::SandboxManagerState {
        pool: fixture.state.pool.clone(),
        client: reqwest::Client::new(),
        provider_endpoint,
        provider_api_key: None,
        provider_secure_access: false,
        owner: Uuid::now_v7(),
    };
    let manager = agentx_v2_runtime::sandbox::router(manager_state.clone());
    let first = call_sandbox_manager(manager.clone(), &request).await;
    assert_eq!(first.0, axum::http::StatusCode::OK);
    assert!(!first.1.replayed);
    let replay = call_sandbox_manager(manager.clone(), &request).await;
    assert_eq!(replay.0, axum::http::StatusCode::OK);
    assert!(replay.1.replayed);
    let mut conflicting = request.clone();
    conflicting.input = json!({"code":"different"});
    let conflict = sandbox_request(manager.clone(), &conflicting).await;
    assert_eq!(conflict.status(), axum::http::StatusCode::CONFLICT);
    let mut stale = request.clone();
    stale.idempotency_key.push_str(":stale");
    stale.fencing_token = 6;
    let stale = sandbox_request(manager.clone(), &stale).await;
    assert_eq!(stale.status(), axum::http::StatusCode::CONFLICT);
    let replacement_worker_id = Uuid::now_v7();
    sqlx::query(
        "UPDATE node_attempts SET worker_instance_id=?,lease_token=?,fencing_token=8,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND) WHERE id=?",
    )
    .bind(replacement_worker_id.to_string())
    .bind(replacement_worker_id)
    .bind(attempt_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let mut replacement = request.clone();
    replacement.worker_id = replacement_worker_id;
    replacement.fencing_token = 8;
    let replay_after_takeover = call_sandbox_manager(manager.clone(), &replacement).await;
    assert_eq!(replay_after_takeover.0, axum::http::StatusCode::OK);
    assert!(replay_after_takeover.1.replayed);
    assert_eq!(replay_after_takeover.1.output, first.1.output);

    let abandoned_attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,worker_protocol_version,ir_schema_version,compiler_version,manifest_version,status,idempotency_key,worker_instance_id,locked_until,fencing_token,deadline_at,input_json) VALUES(?,?,?,?,2,'sandbox',1,1,? ,?,'running',?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),7,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 5 MINUTE),JSON_OBJECT())",
    )
    .bind(abandoned_attempt_id)
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .bind(node_execution_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .bind(format!("sandbox-test:{abandoned_attempt_id}"))
    .bind(worker_id.to_string())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let mut abandoned_request = request.clone();
    abandoned_request.attempt_id = abandoned_attempt_id;
    abandoned_request.idempotency_key = format!("sandbox:execute:{abandoned_attempt_id}");
    abandoned_request.parameters = json!({"runner":"shell","source":"printf takeover"});
    let abandoned_manager = manager.clone();
    let abandoned_payload = abandoned_request.clone();
    let abandoned =
        tokio::spawn(async move { sandbox_request(abandoned_manager, &abandoned_payload).await });
    for _ in 0..100 {
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM sandbox_leases WHERE tenant_id=? AND idempotency_key=?",
        )
        .bind(fixture.tenant_id)
        .bind(&abandoned_request.idempotency_key)
        .fetch_optional(&fixture.state.pool)
        .await
        .unwrap();
        if status.as_deref() == Some("interrupting") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    abandoned.abort();
    let _ = abandoned.await;
    let abandoned_status: String = sqlx::query_scalar(
        "SELECT status FROM sandbox_leases WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(&abandoned_request.idempotency_key)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(abandoned_status, "interrupting");
    let takeover_worker_id = Uuid::now_v7();
    sqlx::query(
        "UPDATE node_attempts SET worker_instance_id=?,lease_token=?,fencing_token=8,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND) WHERE id=?",
    )
    .bind(takeover_worker_id.to_string())
    .bind(takeover_worker_id)
    .bind(abandoned_attempt_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE sandbox_leases SET locked_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(&abandoned_request.idempotency_key)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    abandoned_request.worker_id = takeover_worker_id;
    abandoned_request.fencing_token = 8;
    let recovered = call_sandbox_manager(manager.clone(), &abandoned_request).await;
    assert_eq!(recovered.0, axum::http::StatusCode::OK);
    assert!(recovered.1.replayed);
    let recovered_lease: (String, u64) = sqlx::query_as(
        "SELECT status,fencing_token FROM sandbox_leases WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(&abandoned_request.idempotency_key)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(recovered_lease, ("terminated".into(), 2));

    let failed_attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,worker_protocol_version,ir_schema_version,compiler_version,manifest_version,status,idempotency_key,worker_instance_id,locked_until,fencing_token,deadline_at,input_json) VALUES(?,?,?,?,3,'sandbox',1,1,? ,?,'running',?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),7,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),JSON_OBJECT())",
    )
    .bind(failed_attempt_id)
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .bind(node_execution_id)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .bind(format!("sandbox-test:{failed_attempt_id}"))
    .bind(worker_id.to_string())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let mut failed = request;
    failed.attempt_id = failed_attempt_id;
    failed.idempotency_key = format!("sandbox:execute:{failed_attempt_id}");
    failed.input = json!({"fail":true});
    failed.parameters = json!({"runner":"shell","source":"exit 1"});
    let failed_response = sandbox_request(manager, &failed).await;
    assert_eq!(
        failed_response.status(),
        axum::http::StatusCode::BAD_GATEWAY
    );
    let failed_status: String = sqlx::query_scalar(
        "SELECT status FROM sandbox_leases WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(&failed.idempotency_key)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(failed_status, "failed");
    let status: String = sqlx::query_scalar("SELECT status FROM sandbox_leases WHERE id=?")
        .bind(first.1.lease_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(status, "terminated");
    let orphan_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sandbox_leases(id,tenant_id,execution_id,node_execution_id,attempt_id,worker_lease_token,sandbox_id,lease_token_hash,profile_version_id,idempotency_key,status,provider_labels_json,request_hash,expires_at,fencing_token,outcome_unknown) VALUES(?,?,?,?,?,?,?,?,?,?,'orphaned',?, ?,DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND),1,TRUE)",
    )
    .bind(orphan_id)
    .bind(fixture.tenant_id)
    .bind(execution_id)
    .bind(node_execution_id)
    .bind(attempt_id)
    .bind(Uuid::now_v7())
    .bind("orphan-v2")
    .bind("b".repeat(64))
    .bind(Uuid::now_v7())
    .bind(format!("sandbox:orphan:{orphan_id}"))
    .bind(json!({"agentxLeaseId":orphan_id}))
    .bind(agentx_runtime_contracts::content_hash(&json!({"orphan":orphan_id})).unwrap().as_str())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert!(
        agentx_v2_runtime::sandbox::reconcile_one(&manager_state)
            .await
            .unwrap()
    );
    let reconciled: (String, u64, u64) = sqlx::query_as(
        "SELECT status,fencing_token,termination_attempts FROM sandbox_leases WHERE id=?",
    )
    .bind(orphan_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(reconciled, ("terminated".into(), 2, 1));
    for (status, last_error, expected_event) in [
        (
            "interrupting",
            Some("execution_cancelled"),
            "sandbox.cancelled",
        ),
        ("running", None, "sandbox.timed_out"),
    ] {
        let lease_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO sandbox_leases(id,tenant_id,execution_id,node_execution_id,attempt_id,worker_lease_token,sandbox_id,lease_token_hash,profile_version_id,idempotency_key,status,provider_labels_json,request_hash,expires_at,fencing_token,outcome_unknown,last_error) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND),1,FALSE,?)",
        )
        .bind(lease_id)
        .bind(fixture.tenant_id)
        .bind(execution_id)
        .bind(node_execution_id)
        .bind(attempt_id)
        .bind(Uuid::now_v7())
        .bind(format!("sandbox-{lease_id}"))
        .bind(format!("{:x}", Sha256::digest(lease_id.as_bytes())))
        .bind(Uuid::now_v7())
        .bind(format!("sandbox:terminal:{lease_id}"))
        .bind(status)
        .bind(json!({"agentxLeaseId":lease_id}))
        .bind(
            agentx_runtime_contracts::content_hash(&json!({"terminal":lease_id}))
                .unwrap()
                .as_str(),
        )
        .bind(last_error)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
        let mut reconciled_target = false;
        for _ in 0..16 {
            assert!(
                agentx_v2_runtime::sandbox::reconcile_one(&manager_state)
                    .await
                    .unwrap()
            );
            let lease_status: String =
                sqlx::query_scalar("SELECT status FROM sandbox_leases WHERE id=?")
                    .bind(lease_id)
                    .fetch_one(&fixture.state.pool)
                    .await
                    .unwrap();
            if lease_status == "terminated" {
                reconciled_target = true;
                break;
            }
        }
        assert!(
            reconciled_target,
            "Sandbox Reaper must reach the target Lease"
        );
        let traced: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM trace_outbox WHERE tenant_id=? AND execution_id=? AND JSON_UNQUOTE(JSON_EXTRACT(payload_json,'$.sandboxLeaseId'))=? AND JSON_UNQUOTE(JSON_EXTRACT(payload_json,'$.eventType'))=?",
        )
        .bind(fixture.tenant_id)
        .bind(execution_id)
        .bind(lease_id.to_string())
        .bind(expected_event)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
        assert_eq!(traced, 1, "Sandbox terminal path must close its Span");
    }
    provider_task.abort();
}

async fn call_sandbox_manager(
    manager: Router,
    request: &agentx_v2_runtime::sandbox::SandboxExecuteRequestV1,
) -> (
    axum::http::StatusCode,
    agentx_v2_runtime::sandbox::SandboxExecuteResponseV1,
) {
    let response = sandbox_request(manager, request).await;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let parsed = serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "Sandbox Manager returned {status} with an unexpected body: {error}; {}",
            String::from_utf8_lossy(&body)
        )
    });
    (status, parsed)
}

async fn sandbox_request(
    manager: Router,
    request: &agentx_v2_runtime::sandbox::SandboxExecuteRequestV1,
) -> axum::response::Response {
    manager
        .oneshot(
            Request::post("/internal/runtime/v1/sandboxes:execute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn retention_dry_run_reference_block_and_object_sweep_are_fenced(fixture: &Fixture) {
    let deletable = Uuid::now_v7();
    let protected = Uuid::now_v7();
    for (artifact_id, key) in [
        (deletable, format!("artifacts/{deletable}")),
        (protected, format!("artifacts/{protected}")),
    ] {
        fixture
            .state
            .objects
            .put(
                &ObjectPath::from(key.clone()),
                Bytes::from_static(b"retention").into(),
            )
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key,created_at) VALUES(?,?,'application/octet-stream',9,REPEAT('a',64),?,DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 DAY))",
        )
        .bind(artifact_id)
        .bind(fixture.tenant_id)
        .bind(key)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'execution','retention-test','output')",
    )
    .bind(fixture.tenant_id)
    .bind(protected)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let retained_execution: Uuid = sqlx::query_scalar(
        "SELECT target_execution_id FROM evaluation_run_cases WHERE tenant_id=? AND status='completed' ORDER BY created_at LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM checkpoints WHERE tenant_id=? AND execution_id=?")
        .bind(fixture.tenant_id)
        .bind(retained_execution)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_executions SET ended_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 DAY) WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(retained_execution)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let retained_message = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO application_messages(id,tenant_id,session_id,sequence_number,role,created_at) VALUES(?,?,?,1,'assistant',DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 DAY))",
    )
    .bind(retained_message)
    .bind(fixture.tenant_id)
    .bind(Uuid::now_v7())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let retained_evaluation: Uuid = sqlx::query_scalar(
        "SELECT id FROM evaluation_runs WHERE tenant_id=? AND status='completed' ORDER BY created_at LIMIT 1",
    )
    .bind(fixture.tenant_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE evaluation_runs SET completed_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 DAY) WHERE id=?",
    )
    .bind(retained_evaluation)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    apply_target(
        fixture,
        3,
        AdmissionTargetV1::RetentionPolicy {
            state: RuntimeRetentionPolicyV1 {
                tenant_id: fixture.tenant_id,
                policy_version: 1,
                retention_days: BTreeMap::from([
                    (RuntimeRetentionDataTypeV1::Artifact, 14),
                    (RuntimeRetentionDataTypeV1::Execution, 14),
                    (RuntimeRetentionDataTypeV1::ApplicationMessage, 14),
                    (RuntimeRetentionDataTypeV1::EvaluationReport, 14),
                ]),
                enabled: true,
            },
        },
    )
    .await;

    let dry_run = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,policy_version,idempotency_key,status,requested_by) VALUES(?,?,TRUE,1,?,'queued',?)",
    )
    .bind(dry_run)
    .bind(fixture.tenant_id)
    .bind(format!("retention:dry:{dry_run}"))
    .bind(Uuid::now_v7())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert!(
        run_retention_once(&fixture.state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let dry_status: String = sqlx::query_scalar("SELECT status FROM retention_runs WHERE id=?")
        .bind(dry_run)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(dry_status, "completed");
    let dry_deleted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE id=? AND deleted_at IS NOT NULL")
            .bind(deletable)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(dry_deleted, 0);

    let run = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,policy_version,idempotency_key,status,requested_by) VALUES(?,?,FALSE,1,?,'queued',?)",
    )
    .bind(run)
    .bind(fixture.tenant_id)
    .bind(["retention:delete:", &run.to_string()].concat())
    .bind(Uuid::now_v7())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert!(
        run_retention_once(&fixture.state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let rows = sqlx::query(
        "SELECT target_id,status FROM retention_items WHERE retention_run_id=? ORDER BY target_id",
    )
    .bind(run)
    .fetch_all(&fixture.state.pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.try_get::<String, _>("target_id").unwrap(),
            row.try_get::<String, _>("status").unwrap(),
        )
    })
    .collect::<HashMap<_, _>>();
    assert_eq!(
        rows.get(&deletable.to_string()).map(String::as_str),
        Some("deleted")
    );
    assert_eq!(
        rows.get(&protected.to_string()).map(String::as_str),
        Some("blocked")
    );
    assert!(
        fixture
            .state
            .objects
            .get(&ObjectPath::from(format!("artifacts/{deletable}")))
            .await
            .is_err()
    );
    assert!(
        fixture
            .state
            .objects
            .get(&ObjectPath::from(format!("artifacts/{protected}")))
            .await
            .is_ok()
    );
    for (table, id) in [
        ("workflow_executions", retained_execution),
        ("application_messages", retained_message),
        ("evaluation_runs", retained_evaluation),
    ] {
        let deleted: bool = match table {
            "workflow_executions" => sqlx::query_scalar(
                "SELECT retention_deleted_at IS NOT NULL FROM workflow_executions WHERE id=?",
            )
            .bind(id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap(),
            "application_messages" => sqlx::query_scalar(
                "SELECT retention_deleted_at IS NOT NULL FROM application_messages WHERE id=?",
            )
            .bind(id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap(),
            "evaluation_runs" => sqlx::query_scalar(
                "SELECT retention_deleted_at IS NOT NULL FROM evaluation_runs WHERE id=?",
            )
            .bind(id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap(),
            _ => unreachable!(),
        };
        assert!(deleted, "{table} retention candidate was not tombstoned");
    }

    let retryable = Uuid::now_v7();
    let retryable_key = format!("artifacts/{retryable}");
    fixture
        .state
        .objects
        .put(
            &ObjectPath::from(retryable_key.clone()),
            Bytes::from_static(b"retention").into(),
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key,created_at) VALUES(?,?,'application/octet-stream',9,REPEAT('b',64),?,DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 DAY))",
    )
    .bind(retryable)
    .bind(fixture.tenant_id)
    .bind(&retryable_key)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let retry_run = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,policy_version,idempotency_key,status,requested_by) VALUES(?,?,FALSE,1,?,'queued',?)",
    )
    .bind(retry_run)
    .bind(fixture.tenant_id)
    .bind(format!("retention:retry:{retry_run}"))
    .bind(Uuid::now_v7())
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    let failing_state = RuntimeState {
        pool: fixture.state.pool.clone(),
        objects: Arc::new(FailOnceDeleteStore::new(fixture.state.objects.clone())),
        trust: fixture.state.trust.clone(),
        wakeups: Default::default(),
        vault: None,
    };
    assert!(
        run_retention_once(&failing_state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let failed = sqlx::query(
        "SELECT status,attempt_count FROM retention_items WHERE retention_run_id=? AND object_id=?",
    )
    .bind(retry_run)
    .bind(retryable)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(failed.get::<String, _>("status"), "failed");
    assert_eq!(failed.get::<u32, _>("attempt_count"), 1);
    sqlx::query(
        "UPDATE retention_runs SET locked_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE id=?",
    )
    .bind(retry_run)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert!(
        run_retention_once(&fixture.state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let retried = sqlx::query(
        "SELECT status,attempt_count FROM retention_items WHERE retention_run_id=? AND object_id=?",
    )
    .bind(retry_run)
    .bind(retryable)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(retried.get::<String, _>("status"), "deleted");
    assert_eq!(retried.get::<u32, _>("attempt_count"), 2);
}

async fn trigger_claim_takeover_and_provider_failure_are_fenced(pool: &MySqlPool) {
    let tenant_id = Uuid::now_v7();
    let application_id = Uuid::now_v7();
    let binding_id = Uuid::now_v7();
    let bundle_id = Uuid::now_v7();
    let specification = RuntimeTriggerSpecV1 {
        schema_version: 1,
        trigger_id: binding_id,
        application_id,
        node_id: "poll-source".into(),
        revision: 1,
        configuration_hash: agentx_runtime_contracts::content_hash(&json!({"poll":"v1"})).unwrap(),
        enabled: true,
        configuration: RuntimeTriggerConfigurationV1::Poll {
            interval_seconds: 60,
            provider_endpoint: "http://127.0.0.1:1/unavailable".into(),
            input: json!({}),
        },
    };
    sqlx::query("INSERT INTO trigger_bindings(id,tenant_id,application_id,application_deployment_id,bundle_id,workflow_version_id,node_id,configuration_revision,configuration_hash,trigger_kind,configuration_json,status,next_poll_at) VALUES(?,?,?,?,?,?,?,?,?,'poll',?,'active',UTC_TIMESTAMP(6))")
        .bind(binding_id).bind(tenant_id).bind(application_id).bind(Uuid::now_v7()).bind(bundle_id).bind(Uuid::now_v7()).bind("poll-source").bind(1_u64).bind(specification.configuration_hash.as_str()).bind(serde_json::to_value(&specification).unwrap()).execute(pool).await.unwrap();

    let first_owner = Uuid::now_v7();
    let second_owner = Uuid::now_v7();
    let (first, second) = tokio::join!(
        agentx_v2_runtime::trigger::claim(pool, first_owner, 100),
        agentx_v2_runtime::trigger::claim(pool, second_owner, 100),
    );
    let mut claims = first.unwrap();
    claims.extend(second.unwrap());
    assert_eq!(claims.len(), 1, "two Trigger replicas claimed one Binding");
    let stale = claims.pop().unwrap();
    assert_eq!(stale.fencing_token, 1);

    sqlx::query("UPDATE trigger_bindings SET locked_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE id=?")
        .bind(binding_id).execute(pool).await.unwrap();
    let current_owner = if stale.owner == first_owner {
        second_owner
    } else {
        first_owner
    };
    let current = agentx_v2_runtime::trigger::claim(pool, current_owner, 1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(current.fencing_token, 2);
    assert!(
        agentx_v2_runtime::trigger::heartbeat(pool, &stale)
            .await
            .is_err()
    );

    let provider = StubTriggerProvider {
        delay: Duration::ZERO,
        response: Err("fixture unavailable".into()),
    };
    agentx_v2_runtime::trigger::execute_with_provider(pool, &current, &provider)
        .await
        .unwrap();
    let row = sqlx::query("SELECT locked_by,cursor_value,last_error,next_poll_at>UTC_TIMESTAMP(6) retry_delayed FROM trigger_bindings WHERE id=?")
        .bind(binding_id).fetch_one(pool).await.unwrap();
    assert!(
        row.try_get::<Option<Uuid>, _>("locked_by")
            .unwrap()
            .is_none()
    );
    assert!(
        row.try_get::<Option<String>, _>("cursor_value")
            .unwrap()
            .is_none()
    );
    assert!(
        row.try_get::<String, _>("last_error")
            .unwrap()
            .starts_with("POLL_PROVIDER_ERROR")
    );
    assert!(row.try_get::<bool, _>("retry_delayed").unwrap());
}

#[tokio::test]
async fn lifecycle_response_from_an_old_revision_cannot_create_an_invocation() {
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx_runtime")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(3306.tcp()).await.unwrap();
    let pool = connect_with_retry(port).await;
    agentx_runtime_infrastructure::migrate_runtime_mysql(&pool)
        .await
        .unwrap();
    let endpoint = "https://provider.example.test/lifecycle".to_owned();
    let tenant_id = Uuid::now_v7();
    let application_id = Uuid::now_v7();
    let bundle_id = Uuid::now_v7();
    let binding_id = Uuid::now_v7();
    let specification = RuntimeTriggerSpecV1 {
        schema_version: 1,
        trigger_id: binding_id,
        application_id,
        node_id: "remote:lifecycle:activate".into(),
        revision: 1,
        configuration_hash: agentx_runtime_contracts::content_hash(&json!({"revision":1})).unwrap(),
        enabled: true,
        configuration: RuntimeTriggerConfigurationV1::Lifecycle {
            operation: agentx_runtime_contracts::LifecycleOperationV1::Activate,
            provider_endpoint: endpoint,
            input: json!({"operation":"activate"}),
        },
    };
    sqlx::query("INSERT INTO trigger_bindings(id,tenant_id,application_id,application_deployment_id,bundle_id,workflow_version_id,node_id,configuration_revision,configuration_hash,trigger_kind,configuration_json,status,next_poll_at) VALUES(?,?,?,?,?,?,?,?,?,'lifecycle',?,'active',UTC_TIMESTAMP(6))")
        .bind(binding_id).bind(tenant_id).bind(application_id).bind(Uuid::now_v7()).bind(bundle_id).bind(Uuid::now_v7()).bind(&specification.node_id).bind(1_u64).bind(specification.configuration_hash.as_str()).bind(serde_json::to_value(&specification).unwrap()).execute(&pool).await.unwrap();
    let claim = agentx_v2_runtime::trigger::claim(&pool, Uuid::now_v7(), 1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let execute_pool = pool.clone();
    let provider = Arc::new(StubTriggerProvider {
        delay: Duration::from_millis(100),
        response: Ok(TriggerProviderResponse {
            success: true,
            status: "200 OK".into(),
            cursor: None,
            body: json!({"accepted":true,"state":{"active":true}}),
        }),
    });
    let task = tokio::spawn(async move {
        agentx_v2_runtime::trigger::execute_with_provider(&execute_pool, &claim, provider.as_ref())
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    sqlx::query("UPDATE trigger_bindings SET configuration_revision=2 WHERE id=?")
        .bind(binding_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
    let invocations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM application_invocations WHERE tenant_id=? AND caller_id=?",
    )
    .bind(tenant_id)
    .bind(binding_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(invocations, 0);
}

async fn authentication_failures_do_not_write_receipts(fixture: &Fixture) {
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publish_receipts")
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    let bundle = fixture.bundle(99).await;
    let rejected = prepare_bundle(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.activate"),
        Json(PrepareBundleRequestV1 {
            api_version: 1,
            idempotency_key: "auth-must-not-write".into(),
            bundle,
        }),
    )
    .await;
    assert!(matches!(rejected, Err(RuntimeError::Unauthorized)));
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publish_receipts")
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(after, before);
}

async fn authentication_requires_active_route_tenant_and_head(fixture: &Fixture) {
    assert!(
        authenticate_api_key(&fixture.state.pool, "runtime-slice", &fixture.api_key)
            .await
            .is_ok()
    );

    sqlx::query(
        "UPDATE application_routes SET status='disabled' WHERE tenant_id=? AND application_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fixture.application_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert_authentication_rejected(fixture).await;
    sqlx::query(
        "UPDATE application_routes SET status='active' WHERE tenant_id=? AND application_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fixture.application_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();

    sqlx::query("UPDATE tenant_admission SET status='disabled' WHERE tenant_id=?")
        .bind(fixture.tenant_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    assert_authentication_rejected(fixture).await;
    sqlx::query("UPDATE tenant_admission SET status='active' WHERE tenant_id=?")
        .bind(fixture.tenant_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    sqlx::query("UPDATE deployment_bundles SET status='superseded' WHERE tenant_id=? AND id=(SELECT bundle_id FROM deployment_heads WHERE tenant_id=? AND application_id=?)")
        .bind(fixture.tenant_id)
        .bind(fixture.tenant_id)
        .bind(fixture.application_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    assert_authentication_rejected(fixture).await;
    sqlx::query("UPDATE deployment_bundles SET status='active' WHERE tenant_id=? AND id=(SELECT bundle_id FROM deployment_heads WHERE tenant_id=? AND application_id=?)")
        .bind(fixture.tenant_id)
        .bind(fixture.tenant_id)
        .bind(fixture.application_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
}

async fn assert_authentication_rejected(fixture: &Fixture) {
    assert!(matches!(
        authenticate_api_key(&fixture.state.pool, "runtime-slice", &fixture.api_key).await,
        Err(RuntimeError::Unauthorized | RuntimeError::NotFound)
    ));
}

impl Fixture {
    fn new(pool: MySqlPool) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let work_package_signing_key = SigningKey::generate(&mut OsRng);
        let trust = RuntimeTrust::new(
            "agentx-control",
            "agentx-runtime-internal",
            HashMap::from([("publisher-current".into(), PUBLIC_KEY.to_vec())]),
        )
        .with_bundle_key("bundle-current", signing_key.verifying_key())
        .with_work_package_key(
            "work-package-current",
            work_package_signing_key.verifying_key(),
        );
        Self {
            state: RuntimeState {
                pool,
                objects: Arc::new(InMemory::new()),
                trust: Arc::new(trust),
                wakeups: Default::default(),
                vault: None,
            },
            signing_key,
            work_package_signing_key,
            tenant_id: Uuid::now_v7(),
            application_id: Uuid::now_v7(),
            workflow_id: Uuid::now_v7(),
            identity_id: Uuid::now_v7(),
            key_id: Uuid::now_v7(),
            api_key: "axk_runtime-slice-secret-value".into(),
        }
    }

    async fn bundle(&self, sequence: u64) -> agentx_runtime_contracts::ExecutionSpecBundleV1 {
        let definition = definition();
        let workflow_version_id = Uuid::now_v7();
        let bytes = agentx_runtime_contracts::canonical_bytes(&definition).unwrap();
        let hash = agentx_runtime_contracts::content_hash(&definition).unwrap();
        let object = RuntimeObjectReferenceV1 {
            tenant_id: self.tenant_id,
            storage_domain: StorageDomain::Runtime,
            object_id: workflow_version_id,
            object_key: RuntimeObjectReferenceV1::canonical_key(
                self.tenant_id,
                workflow_version_id,
                &hash,
            ),
            content_hash: hash,
            size_bytes: bytes.len() as u64,
            media_type: "application/vnd.agentx.workflow-definition+json".into(),
        };
        build_bundle(
            BundleBuildSource {
                bundle_id: Uuid::now_v7(),
                tenant_id: self.tenant_id,
                application_id: self.application_id,
                deployment_id: Uuid::now_v7(),
                workflow_id: self.workflow_id,
                workflow_version_id,
                sequence,
                definition,
                dependency_versions: BTreeMap::new(),
                supported_capabilities: BTreeSet::from(["builtin".into()]),
                input_contract: json!({"type":"object","required":["message"],"properties":{"message":{"type":"string","minLength":2}},"additionalProperties":true}),
                output_contract: json!({"type":"object"}),
                resources: vec![],
                authorization: RuntimeAuthorizationSnapshotV1 {
                    schema_version: 1,
                    tenant_id: self.tenant_id,
                    service_identity_id: self.identity_id,
                    workflow_id: self.workflow_id,
                    policy_epoch: 1,
                    capabilities: BTreeSet::from(["builtin".into()]),
                    grant_ids: vec![],
                    maximum_policy_staleness_seconds: 72 * 60 * 60,
                    captured_at: OffsetDateTime::UNIX_EPOCH,
                },
                triggers: vec![],
                runtime_policy: RuntimePolicyV1 {
                    timeout_seconds: 30,
                    operation_deadline_seconds: 30,
                    ..RuntimePolicyV1::default()
                },
                objects: vec![object],
                created_at: OffsetDateTime::UNIX_EPOCH,
            },
            "bundle-current",
            &self.signing_key,
        )
        .unwrap()
    }

    fn work_package_source(
        &self,
        package_id: Uuid,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> WorkPackageBuildSource {
        let definition = definition();
        let compiled = agentx_bundle_builder::compile_workflow_version(&definition, package_id)
            .expect("debug Workflow compiles");
        WorkPackageBuildSource {
            package_id,
            tenant_id: self.tenant_id,
            purpose: WorkPackagePurpose::Debug,
            call_purpose: RuntimeCallPurposeV1::Debug,
            spec: agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Debug {
                draft_revision: 1,
                debug_plan: agentx_runtime_contracts::RuntimeDebugPlanV1::whole(&compiled),
            },
            source_revision: "draft:1".into(),
            definition,
            dependency_versions: BTreeMap::new(),
            supported_capabilities: BTreeSet::from(["builtin".into()]),
            overlay: RuntimeWorkPackageOverlayV1 {
                input: json!({"message":"work-package"}),
                ..RuntimeWorkPackageOverlayV1::default()
            },
            resources: vec![],
            authorization: RuntimeAuthorizationSnapshotV1 {
                schema_version: 1,
                tenant_id: self.tenant_id,
                service_identity_id: self.identity_id,
                workflow_id: self.workflow_id,
                policy_epoch: 1,
                capabilities: BTreeSet::from(["builtin".into()]),
                grant_ids: vec![],
                maximum_policy_staleness_seconds: 72 * 60 * 60,
                captured_at: created_at,
            },
            objects: vec![],
            runtime_policy: RuntimePolicyV1::default(),
            created_at,
            expires_at,
        }
    }

    fn evaluation_work_package_source(
        &self,
        package_id: Uuid,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> WorkPackageBuildSource {
        let mut source = self.work_package_source(package_id, created_at, expires_at);
        source.purpose = WorkPackagePurpose::Evaluation;
        source.call_purpose = RuntimeCallPurposeV1::Evaluation;
        source.source_revision = "evaluation:1".into();
        source.spec = agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Evaluation {
            dataset_version_id: Uuid::now_v7(),
            profile_version_id: Uuid::now_v7(),
            cases: vec![
                agentx_runtime_contracts::RuntimeEvaluationCaseV1 {
                    case_id: Uuid::now_v7(),
                    input: json!({"message":"case-one"}),
                    expected_output: Some(json!({"message":"case-one"})),
                },
                agentx_runtime_contracts::RuntimeEvaluationCaseV1 {
                    case_id: Uuid::now_v7(),
                    input: json!({"message":"case-two"}),
                    expected_output: Some(json!({"message":"case-two"})),
                },
            ],
            evaluators: vec![
                agentx_runtime_contracts::RuntimeEvaluatorV1::DeterministicRule {
                    evaluator_id: Uuid::now_v7(),
                    expression: "exact_match".into(),
                },
            ],
        };
        source
    }

    async fn upload_bundle_object(
        &self,
        bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
        idempotency_key: &str,
    ) -> agentx_runtime_contracts::RuntimeObjectUploadReceiptV1 {
        let object = bundle.payload.objects[0].clone();
        let bytes = agentx_runtime_contracts::canonical_bytes(&bundle.payload.definition).unwrap();
        persist_upload(
            &self.state,
            RuntimeObjectUploadMetadataV1 {
                api_version: 1,
                idempotency_key: idempotency_key.into(),
                tenant_id: object.tenant_id,
                object_id: object.object_id,
                content_hash: object.content_hash,
                size_bytes: object.size_bytes,
                media_type: object.media_type,
            },
            Bytes::from(bytes),
        )
        .await
        .unwrap()
    }
}

async fn work_package_prepare_execute_cancel_are_independently_signed_and_idempotent(
    fixture: &Fixture,
) {
    let now = OffsetDateTime::now_utc();
    let package_id = Uuid::now_v7();
    let source = fixture.work_package_source(package_id, now, now + time::Duration::hours(1));
    let wrongly_signed =
        build_work_package(source.clone(), "work-package-current", &fixture.signing_key).unwrap();
    let rejected = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: "work-package:wrong-key".into(),
            work_package: wrongly_signed,
        }),
    )
    .await;
    assert!(matches!(
        rejected,
        Err(RuntimeError::BadRequest(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::InvalidSignature,
            _
        ))
    ));

    let package = build_work_package(
        source.clone(),
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let request = PrepareWorkPackageRequestV1 {
        api_version: 1,
        idempotency_key: "work-package:prepare".into(),
        work_package: package,
    };
    let prepared = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(request.clone()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(prepared.status, PublishReceiptStatusV1::Accepted);
    assert!(!prepared.receipt.replayed);
    let replayed = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(request),
    )
    .await
    .unwrap()
    .0;
    assert!(replayed.receipt.replayed);
    let call_purpose: String =
        sqlx::query_scalar("SELECT call_purpose FROM runtime_work_packages WHERE id=?")
            .bind(package_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(call_purpose, "debug");

    let mut conflicting_source = source;
    conflicting_source.source_revision = "draft:2".into();
    let conflict = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: "work-package:prepare".into(),
            work_package: build_work_package(
                conflicting_source,
                "work-package-current",
                &fixture.work_package_signing_key,
            )
            .unwrap(),
        }),
    )
    .await;
    assert!(matches!(conflict, Err(RuntimeError::Conflict(_, _))));

    let execute_request = ExecuteWorkPackageRequestV1 {
        api_version: 1,
        idempotency_key: "work-package:execute".into(),
        package_id,
        input: Value::Null,
    };
    let executed = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(execute_request.clone()),
    )
    .await
    .unwrap()
    .0;
    assert!(!executed.replayed);
    let execute_replay = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(execute_request),
    )
    .await
    .unwrap()
    .0;
    assert!(execute_replay.replayed);
    let execute_conflict = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: "work-package:execute".into(),
            package_id,
            input: json!({"message":"different"}),
        }),
    )
    .await;
    assert!(matches!(
        execute_conflict,
        Err(RuntimeError::Conflict(_, _))
    ));

    let cancel_request = CancelWorkPackageRequestV1 {
        api_version: 1,
        tenant_id: fixture.tenant_id,
        package_id,
        expected_version: executed.object_version,
        idempotency_key: "work-package:cancel".into(),
    };
    let cancelled = cancel_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.cancel"),
        Json(cancel_request.clone()),
    )
    .await
    .unwrap()
    .0;
    assert!(!cancelled.replayed);
    let cancel_replay = cancel_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.cancel"),
        Json(cancel_request),
    )
    .await
    .unwrap()
    .0;
    assert!(cancel_replay.replayed);

    let expired_source = fixture.work_package_source(
        Uuid::now_v7(),
        now - time::Duration::hours(2),
        now - time::Duration::hours(1),
    );
    let expired = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: "work-package:expired".into(),
            work_package: build_work_package(
                expired_source,
                "work-package-current",
                &fixture.work_package_signing_key,
            )
            .unwrap(),
        }),
    )
    .await;
    assert!(matches!(expired, Err(RuntimeError::Conflict(_, _))));
}

async fn evaluation_work_package_creates_cases_converges_and_cancels_atomically(fixture: &Fixture) {
    let now = OffsetDateTime::now_utc();
    let package_id = Uuid::now_v7();
    let package = build_work_package(
        fixture.evaluation_work_package_source(package_id, now, now + time::Duration::hours(24)),
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:prepare:{package_id}"),
            work_package: package,
        }),
    )
    .await
    .unwrap();
    let execute_request = ExecuteWorkPackageRequestV1 {
        api_version: 1,
        idempotency_key: format!("evaluation:execute:{package_id}"),
        package_id,
        input: Value::Null,
    };
    let executed = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(execute_request.clone()),
    )
    .await
    .unwrap()
    .0;
    let replayed = execute_work_package(
        State(fixture.state.clone()),
        Path(package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(execute_request),
    )
    .await
    .unwrap()
    .0;
    assert!(replayed.replayed);
    assert_eq!(executed.result, replayed.result);
    let execution_ids = executed.result["executionIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| serde_json::from_value::<Uuid>(id.clone()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(execution_ids.len(), 2);
    let case_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evaluation_run_cases c JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id WHERE r.tenant_id=? AND r.work_package_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(package_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(case_count, 2);
    complete_work_package_executions(fixture, &execution_ids).await;
    let run_status: String = sqlx::query_scalar(
        "SELECT status FROM evaluation_runs WHERE tenant_id=? AND work_package_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(package_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(run_status, "completed");
    let rule_failures: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id WHERE r.work_package_id=? AND rr.status<>'passed'",
    )
    .bind(package_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(rule_failures, 0);
    let package_status: String =
        sqlx::query_scalar("SELECT status FROM runtime_work_packages WHERE id=?")
            .bind(package_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(package_status, "succeeded");

    let endpoint = "https://provider.example.test/evaluate".to_owned();
    let model_package_id = Uuid::now_v7();
    let model_id = Uuid::now_v7();
    let evaluator_id = Uuid::now_v7();
    let prompt_object_id = Uuid::now_v7();
    let prompt = Bytes::from_static(br#"{"instruction":"return passed and score"}"#);
    let prompt_hash = agentx_runtime_contracts::ContentHash::parse(format!(
        "sha256:{:x}",
        Sha256::digest(&prompt)
    ))
    .unwrap();
    let prompt_reference = RuntimeObjectReferenceV1 {
        tenant_id: fixture.tenant_id,
        storage_domain: StorageDomain::Runtime,
        object_id: prompt_object_id,
        object_key: RuntimeObjectReferenceV1::canonical_key(
            fixture.tenant_id,
            prompt_object_id,
            &prompt_hash,
        ),
        content_hash: prompt_hash.clone(),
        size_bytes: prompt.len() as u64,
        media_type: "application/json".into(),
    };
    persist_upload(
        &fixture.state,
        RuntimeObjectUploadMetadataV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:prompt:{prompt_object_id}"),
            tenant_id: fixture.tenant_id,
            object_id: prompt_object_id,
            content_hash: prompt_hash,
            size_bytes: prompt.len() as u64,
            media_type: "application/json".into(),
        },
        prompt,
    )
    .await
    .unwrap();
    let mut model_source = fixture.evaluation_work_package_source(
        model_package_id,
        now,
        now + time::Duration::hours(24),
    );
    model_source.supported_capabilities.insert("model".into());
    model_source
        .authorization
        .capabilities
        .insert("model".into());
    model_source.objects.push(prompt_reference);
    model_source
        .resources
        .push(agentx_runtime_contracts::RuntimeResourceBindingV1 {
            resource_kind: agentx_runtime_contracts::RuntimeResourceKindV1::Model,
            resource_id: model_id,
            resource_version: "model-fixture:1".into(),
            state_epoch: 1,
            content_hash: agentx_runtime_contracts::content_hash(&json!({
                "modelId":model_id,
                "version":1
            }))
            .unwrap(),
            configuration: agentx_runtime_contracts::RuntimeResourceConfigurationV1::Model {
                provider: "fixture".into(),
                endpoint,
                model: "evaluator-fixture".into(),
                price_version: "price:1".into(),
                credential: None,
            },
            object_ids: vec![],
        });
    let agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Evaluation { evaluators, .. } =
        &mut model_source.spec
    else {
        unreachable!()
    };
    evaluators.push(agentx_runtime_contracts::RuntimeEvaluatorV1::Model {
        evaluator_id,
        resource_id: model_id,
        prompt_object_id,
    });
    let model_package = build_work_package(
        model_source,
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:prepare:{model_package_id}"),
            work_package: model_package,
        }),
    )
    .await
    .unwrap();
    let model_started = execute_work_package(
        State(fixture.state.clone()),
        Path(model_package_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:execute:{model_package_id}"),
            package_id: model_package_id,
            input: Value::Null,
        }),
    )
    .await
    .unwrap()
    .0;
    let model_targets = model_started.result["executionIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| serde_json::from_value::<Uuid>(id.clone()).unwrap())
        .collect::<Vec<_>>();
    complete_work_package_executions(fixture, &model_targets).await;
    complete_model_evaluators(fixture, model_package_id).await;
    let model_results: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),CAST(COALESCE(SUM(rr.evaluator_execution_id IS NOT NULL AND rr.evaluator_command_id IS NOT NULL),0) AS SIGNED),CAST(COALESCE(SUM(rr.cost_micros),0) AS SIGNED) FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id WHERE r.tenant_id=? AND r.work_package_id=? AND JSON_UNQUOTE(JSON_EXTRACT(rr.detail_json,'$.kind'))='model' AND rr.status='passed'",
    )
    .bind(fixture.tenant_id)
    .bind(model_package_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    let model_debug: Vec<(String, String, Option<Value>, Option<Value>, Value)> = sqlx::query_as(
        "SELECT rr.status,e.status,e.output_json,e.error_json,rr.detail_json FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id LEFT JOIN workflow_executions e ON e.id=rr.evaluator_execution_id AND e.tenant_id=rr.tenant_id WHERE r.tenant_id=? AND r.work_package_id=? AND JSON_UNQUOTE(JSON_EXTRACT(rr.detail_json,'$.kind'))='model' ORDER BY rr.created_at,rr.id",
    )
    .bind(fixture.tenant_id)
    .bind(model_package_id)
    .fetch_all(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        model_results.0, 2,
        "model evaluator state: {model_debug:#?}"
    );
    assert_eq!(model_results.1, 2);
    assert_eq!(model_results.2, 46);
    let model_package_status: String =
        sqlx::query_scalar("SELECT status FROM runtime_work_packages WHERE id=?")
            .bind(model_package_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(model_package_status, "succeeded");
    let cancelled_id = Uuid::now_v7();
    let cancelled_package = build_work_package(
        fixture.evaluation_work_package_source(cancelled_id, now, now + time::Duration::hours(24)),
        "work-package-current",
        &fixture.work_package_signing_key,
    )
    .unwrap();
    let _ = prepare_work_package(
        State(fixture.state.clone()),
        publisher_headers("runtime.work-packages.prepare"),
        Json(PrepareWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:prepare:{cancelled_id}"),
            work_package: cancelled_package,
        }),
    )
    .await
    .unwrap();
    let started = execute_work_package(
        State(fixture.state.clone()),
        Path(cancelled_id),
        publisher_headers("runtime.work-packages.execute"),
        Json(ExecuteWorkPackageRequestV1 {
            api_version: 1,
            idempotency_key: format!("evaluation:execute:{cancelled_id}"),
            package_id: cancelled_id,
            input: Value::Null,
        }),
    )
    .await
    .unwrap()
    .0;
    let cancelled_execution_ids = started.result["executionIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| serde_json::from_value::<Uuid>(id.clone()).unwrap())
        .collect::<Vec<_>>();
    let start_owner = Uuid::now_v7();
    for command in claim_commands(&fixture.state.pool, start_owner, 100)
        .await
        .unwrap()
    {
        process_command(&fixture.state.pool, &command)
            .await
            .unwrap();
    }
    let _ = cancel_work_package(
        State(fixture.state.clone()),
        Path(cancelled_id),
        publisher_headers("runtime.work-packages.cancel"),
        Json(CancelWorkPackageRequestV1 {
            api_version: 1,
            tenant_id: fixture.tenant_id,
            package_id: cancelled_id,
            expected_version: started.object_version,
            idempotency_key: format!("evaluation:cancel:{cancelled_id}"),
        }),
    )
    .await
    .unwrap();
    let cancel_owner = Uuid::now_v7();
    for command in claim_commands(&fixture.state.pool, cancel_owner, 100)
        .await
        .unwrap()
    {
        process_command(&fixture.state.pool, &command)
            .await
            .unwrap();
    }
    let live_cases: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evaluation_run_cases c JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id WHERE r.work_package_id=? AND c.status<>'cancelled'",
    )
    .bind(cancelled_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(live_cases, 0);
    let cancelled_executions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_executions WHERE tenant_id=? AND work_package_id=? AND status='cancelled'",
    )
    .bind(fixture.tenant_id)
    .bind(cancelled_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(cancelled_executions, cancelled_execution_ids.len() as i64);
    let active_reservations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM quota_reservations WHERE tenant_id=? AND status='active' AND ((scope_type='execution' AND scope_id IN (?,?)) OR (scope_type='attempt' AND scope_id IN (SELECT CAST(BIN_TO_UUID(id) AS CHAR) COLLATE utf8mb4_0900_ai_ci FROM node_attempts WHERE tenant_id=? AND execution_id IN (?,?))))",
    )
    .bind(fixture.tenant_id)
    .bind(cancelled_execution_ids[0].to_string())
    .bind(cancelled_execution_ids[1].to_string())
    .bind(fixture.tenant_id)
    .bind(cancelled_execution_ids[0])
    .bind(cancelled_execution_ids[1])
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(active_reservations, 0);
}

async fn complete_work_package_executions(fixture: &Fixture, execution_ids: &[Uuid]) {
    let owner = Uuid::now_v7();
    let commands = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap();
    for execution_id in execution_ids {
        let command = commands
            .iter()
            .find(|claim| claim.execution_id == *execution_id)
            .unwrap();
        process_command(&fixture.state.pool, command).await.unwrap();
        let dispatch = claim_dispatch(&fixture.state.pool, owner)
            .await
            .unwrap()
            .unwrap();
        let task = dispatch.task().unwrap();
        assert_eq!(task.execution_id, *execution_id);
        complete_dispatch(&fixture.state.pool, &dispatch)
            .await
            .unwrap();
        let worker_id = Uuid::now_v7();
        agentx_v2_runtime::engine::register_worker(
            &fixture.state.pool,
            worker_id,
            task.capability.as_str(),
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .unwrap();
        let claim = agentx_v2_runtime::engine::claim_worker_attempt(
            &fixture.state.pool,
            worker_id,
            task.capability.as_str(),
            &task,
        )
        .await
        .unwrap()
        .unwrap();
        agentx_v2_runtime::engine::submit_worker_result(
            &fixture.state.pool,
            &successful_worker_result(&claim),
        )
        .await
        .unwrap();
    }
}

async fn complete_model_evaluators(fixture: &Fixture, package_id: Uuid) {
    let execution_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT rr.evaluator_execution_id FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id JOIN evaluation_runs r ON r.id=c.evaluation_run_id AND r.tenant_id=c.tenant_id WHERE r.tenant_id=? AND r.work_package_id=? AND rr.status='running' ORDER BY rr.created_at,rr.id",
    )
    .bind(fixture.tenant_id)
    .bind(package_id)
    .fetch_all(&fixture.state.pool)
    .await
    .unwrap();
    let owner = Uuid::now_v7();
    let commands = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap();
    let worker = test_worker(fixture, StubWorkerMode::Evaluator);
    for execution_id in execution_ids {
        let command = commands
            .iter()
            .find(|claim| claim.execution_id == execution_id)
            .unwrap();
        process_command(&fixture.state.pool, command).await.unwrap();
        let dispatch = claim_dispatch(&fixture.state.pool, owner)
            .await
            .unwrap()
            .unwrap();
        let task = dispatch.task().unwrap();
        assert_eq!(task.execution_id, execution_id);
        complete_dispatch(&fixture.state.pool, &dispatch)
            .await
            .unwrap();
        let worker_id = Uuid::now_v7();
        agentx_v2_runtime::engine::register_worker(
            &fixture.state.pool,
            worker_id,
            "model",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .unwrap();
        let claim = agentx_v2_runtime::engine::claim_worker_attempt(
            &fixture.state.pool,
            worker_id,
            "model",
            &task,
        )
        .await
        .unwrap()
        .unwrap();
        let execution = worker.execute(&claim).await;
        let result_hash = agentx_v2_runtime::engine::worker_result_hash(
            execution.status,
            &execution.outputs,
            None,
            execution.error_code.as_deref(),
            execution.error_message.as_deref(),
            None,
        )
        .unwrap();
        agentx_v2_runtime::engine::submit_worker_result(
            &fixture.state.pool,
            &WorkerResultV1 {
                protocol_version: 1,
                attempt_id: claim.task.attempt_id,
                worker_id,
                fencing_token: claim.lease.fencing_token,
                status: execution.status,
                result_hash,
                outputs: execution.outputs,
                output_object: None,
                error_code: execution.error_code,
                error_message: execution.error_message,
                partial_output_object: None,
            },
        )
        .await
        .unwrap();
    }
}

async fn object_upload_is_immutable_and_replayable(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
) {
    let first = fixture
        .upload_bundle_object(bundle, "bundle-object:first")
        .await;
    assert!(!first.replayed);
    let replay = fixture
        .upload_bundle_object(bundle, "bundle-object:first")
        .await;
    assert!(replay.replayed);
    assert_eq!(replay.object.object_key, first.object.object_key);

    let object = &bundle.payload.objects[0];
    let wrong = persist_upload(
        &fixture.state,
        RuntimeObjectUploadMetadataV1 {
            api_version: 1,
            idempotency_key: "bundle-object:first".into(),
            tenant_id: object.tenant_id,
            object_id: object.object_id,
            content_hash: object.content_hash.clone(),
            size_bytes: object.size_bytes + 1,
            media_type: object.media_type.clone(),
        },
        Bytes::from_static(b"different"),
    )
    .await;
    assert!(matches!(wrong, Err(RuntimeError::BadRequest(_, _))));
}

async fn prepare_is_idempotent_and_does_not_route_traffic(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
) {
    let first = prepare(fixture, bundle, "prepare:first").await;
    let replay = prepare(fixture, bundle, "prepare:first").await;
    assert_eq!(first.bundle_id, replay.bundle_id);
    let heads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deployment_heads")
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(heads, 0);

    let mut changed = bundle.clone();
    changed.payload.compiled_ir.activation_budget = 99;
    let conflict = prepare_bundle(
        State(fixture.state.clone()),
        publisher_headers("runtime.bundles.prepare"),
        Json(PrepareBundleRequestV1 {
            api_version: 1,
            idempotency_key: "prepare:first".into(),
            bundle: changed,
        }),
    )
    .await;
    assert!(matches!(conflict, Err(RuntimeError::Conflict(_, _))));
}

async fn prepare(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
    key: &str,
) -> agentx_runtime_contracts::PublishReceiptV1 {
    prepare_bundle(
        State(fixture.state.clone()),
        publisher_headers("runtime.bundles.prepare"),
        Json(PrepareBundleRequestV1 {
            api_version: 1,
            idempotency_key: key.into(),
            bundle: bundle.clone(),
        }),
    )
    .await
    .unwrap()
    .0
}

async fn apply_initial_admission(fixture: &Fixture, epoch: u64) {
    for target in [
        AdmissionTargetV1::Tenant { enabled: true },
        AdmissionTargetV1::ApplicationRoute {
            state: ApplicationRouteAdmissionV1 {
                tenant_id: fixture.tenant_id,
                application_id: fixture.application_id,
                route_key: "runtime-slice".into(),
                status: AdmissionStatusV1::Active,
            },
        },
        api_key_target(fixture, AdmissionStatusV1::Active),
        AdmissionTargetV1::ServiceIdentity {
            state: ServiceIdentityAdmissionV1 {
                tenant_id: fixture.tenant_id,
                workflow_id: fixture.workflow_id,
                identity_id: fixture.identity_id,
                policy_epoch: epoch,
                status: AdmissionStatusV1::Active,
                capabilities: vec!["builtin".into()],
                grant_ids: vec![],
            },
        },
    ] {
        apply_target(fixture, epoch, target).await;
    }
}

async fn apply_api_key(fixture: &Fixture, epoch: u64, status: AdmissionStatusV1) {
    apply_target(fixture, epoch, api_key_target(fixture, status)).await;
}

fn api_key_target(fixture: &Fixture, status: AdmissionStatusV1) -> AdmissionTargetV1 {
    AdmissionTargetV1::ApiKey {
        state: ApiKeyAdmissionV1 {
            tenant_id: fixture.tenant_id,
            application_id: fixture.application_id,
            key_id: fixture.key_id,
            key_prefix: fixture.api_key.chars().take(12).collect(),
            secret_hash: format!("sha256:{:x}", Sha256::digest(fixture.api_key.as_bytes())),
            status,
            expires_at: None,
        },
    }
}

async fn apply_target(fixture: &Fixture, epoch: u64, target: AdmissionTargetV1) {
    let event_id = Uuid::now_v7();
    let receipt = apply_admission(
        State(fixture.state.clone()),
        publisher_headers("runtime.admission.apply"),
        Json(admission_request(
            fixture,
            epoch,
            target,
            &format!("admission:{event_id}"),
        )),
    )
    .await
    .unwrap()
    .0;
    assert!(receipt.applied);
}

fn admission_request(
    fixture: &Fixture,
    epoch: u64,
    target: AdmissionTargetV1,
    idempotency_key: &str,
) -> RuntimeAdmissionCommandV1 {
    let event_id = Uuid::now_v7();
    let hash = agentx_runtime_contracts::content_hash(&target).unwrap();
    RuntimeAdmissionCommandV1 {
        api_version: 1,
        command: CommandEnvelopeV1 {
            schema_version: 1,
            event_id,
            source_plane: Plane::Control,
            tenant_id: fixture.tenant_id,
            aggregate_type: "admission".into(),
            aggregate_id: fixture.application_id.to_string(),
            object_version: epoch,
            occurred_at: OffsetDateTime::now_utc(),
            payload: json!({}),
            content_hash: hash,
            correlation_id: event_id,
            causation_id: None,
            idempotency_key: idempotency_key.into(),
        },
        admission_epoch: epoch,
        target,
    }
}

async fn concurrent_admission_delivery_converges_to_one_receipt(fixture: &Fixture) {
    const CONCURRENCY: usize = 16;
    let request = admission_request(
        fixture,
        1,
        AdmissionTargetV1::Tenant { enabled: true },
        "admission:concurrent-delivery",
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(CONCURRENCY));
    let mut deliveries = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let state = fixture.state.clone();
        let request = request.clone();
        let barrier = barrier.clone();
        deliveries.push(tokio::spawn(async move {
            barrier.wait().await;
            apply_admission(
                State(state),
                publisher_headers("runtime.admission.apply"),
                Json(request),
            )
            .await
            .map(|receipt| receipt.0)
        }));
    }
    let mut fresh = 0;
    let mut replayed = 0;
    for delivery in deliveries {
        let receipt = delivery
            .await
            .expect("concurrent Admission task should join")
            .expect("concurrent Admission delivery should converge");
        assert!(receipt.applied);
        if receipt.replayed {
            replayed += 1;
        } else {
            fresh += 1;
        }
    }
    assert_eq!(fresh, 1);
    assert_eq!(replayed, CONCURRENCY - 1);
    let receipt_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM publish_receipts WHERE tenant_id=? AND operation='admission' AND idempotency_key=?",
    )
    .bind(fixture.tenant_id)
    .bind(&request.command.idempotency_key)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(receipt_count, 1);
}

async fn activate(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
    expected: Option<u64>,
    sequence: u64,
    epoch: u64,
) {
    let request = activation_request(
        bundle,
        expected,
        sequence,
        epoch,
        format!("activate:{sequence}"),
    );
    let receipt = activate_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.activate"),
        Json(request.clone()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(receipt.status, PublishReceiptStatusV1::Accepted);
    let replay = activate_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.activate"),
        Json(request),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(replay.bundle_id, receipt.bundle_id);
}

fn activation_request(
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
    expected: Option<u64>,
    sequence: u64,
    epoch: u64,
    key: String,
) -> ActivateDeploymentRequestV1 {
    ActivateDeploymentRequestV1 {
        api_version: 1,
        idempotency_key: key,
        manifest: ActivationManifestV1 {
            api_version: 1,
            tenant_id: bundle.payload.tenant_id,
            application_id: bundle.payload.application_id,
            deployment_id: bundle.payload.deployment_id,
            bundle_id: bundle.payload.bundle_id,
            expected_head_version: expected,
            activation_sequence: sequence,
            minimum_admission_epoch: epoch,
            runtime_config_revision: 1,
            runtime_policy: agentx_runtime_contracts::ApplicationRuntimePolicyV1 {
                session_version_policy: agentx_runtime_contracts::SessionVersionPolicyV1::Pinned,
                synchronous_wait_seconds: 30,
                maximum_json_bytes: 1_048_576,
                maximum_multipart_bytes: 52_428_800,
            },
        },
    }
}

async fn rollback(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
    expected: Option<u64>,
    sequence: u64,
    epoch: u64,
) {
    let receipt = rollback_request(
        fixture,
        bundle,
        expected,
        sequence,
        epoch,
        &format!("rollback:{sequence}"),
    )
    .await;
    assert_eq!(receipt.status, PublishReceiptStatusV1::Accepted);
}

async fn rollback_request(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
    expected: Option<u64>,
    sequence: u64,
    epoch: u64,
    key: &str,
) -> agentx_runtime_contracts::PublishReceiptV1 {
    rollback_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.rollback"),
        Json(RollbackDeploymentRequestV1 {
            api_version: 1,
            idempotency_key: key.into(),
            manifest: ActivationManifestV1 {
                api_version: 1,
                tenant_id: bundle.payload.tenant_id,
                application_id: bundle.payload.application_id,
                deployment_id: bundle.payload.deployment_id,
                bundle_id: bundle.payload.bundle_id,
                expected_head_version: expected,
                activation_sequence: sequence,
                minimum_admission_epoch: epoch,
                runtime_config_revision: 1,
                runtime_policy: agentx_runtime_contracts::ApplicationRuntimePolicyV1 {
                    session_version_policy:
                        agentx_runtime_contracts::SessionVersionPolicyV1::Pinned,
                    synchronous_wait_seconds: 30,
                    maximum_json_bytes: 1_048_576,
                    maximum_multipart_bytes: 52_428_800,
                },
            },
        }),
    )
    .await
    .unwrap()
    .0
}

async fn stale_head_and_sequence_are_rejected(
    fixture: &Fixture,
    first: &agentx_runtime_contracts::ExecutionSpecBundleV1,
) {
    let stale = rollback_request(fixture, first, Some(1), 3, 1, "rollback:stale-head").await;
    assert_eq!(stale.status, PublishReceiptStatusV1::Rejected);
    assert!(matches!(
        stale.rejection.unwrap().code,
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::HeadVersionConflict
    ));
    let unordered = rollback_request(fixture, first, Some(2), 2, 1, "rollback:old-sequence").await;
    assert_eq!(unordered.status, PublishReceiptStatusV1::Rejected);
    assert!(matches!(
        unordered.rejection.unwrap().code,
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::ActivationSequenceConflict
    ));
}

async fn session_message_appends_one_assistant_response(fixture: &Fixture) {
    let router = agentx_v2_runtime::gateway::router().with_state(fixture.state.clone());
    let session_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/applications/runtime-slice/sessions")
                .header(AUTHORIZATION, format!("Bearer {}", fixture.api_key))
                .header("content-type", "application/json")
                .header("idempotency-key", "runtime-slice-session")
                .body(Body::from(
                    serde_json::to_vec(&CreateSessionRequestV1 {
                        external_user_id: Some("runtime-slice-user".into()),
                        title: Some("Runtime slice session".into()),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session_response.status(), axum::http::StatusCode::CREATED);
    let session: SessionResponseV1 = serde_json::from_slice(
        &to_bytes(session_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();

    let message_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/sessions/{}/messages", session.id))
                .header(AUTHORIZATION, format!("Bearer {}", fixture.api_key))
                .header("content-type", "application/json")
                .header("idempotency-key", "runtime-slice-session-message")
                .body(Body::from(
                    serde_json::to_vec(&MessageRequestV1 {
                        parts: vec![MessagePartInputV1 {
                            part_type: "text".into(),
                            content: Some(json!("session-message")),
                            artifact_id: None,
                        }],
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(message_response.status(), axum::http::StatusCode::ACCEPTED);
    let invocation: InvocationResponseV1 = serde_json::from_slice(
        &to_bytes(message_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(invocation.session_id, Some(session.id));
    let result = invocation_and_dispatch_recovery_are_fenced(
        fixture,
        invocation
            .execution_id
            .expect("Session Invocation has an Execution"),
    )
    .await;
    assert!(
        agentx_v2_runtime::engine::submit_worker_result(&fixture.state.pool, &result)
            .await
            .unwrap(),
        "a duplicate Worker Result must replay its original Receipt"
    );

    let rows = sqlx::query(
        "SELECT sequence_number,role,invocation_id FROM application_messages WHERE tenant_id=? AND session_id=? ORDER BY sequence_number,id",
    )
    .bind(fixture.tenant_id)
    .bind(session.id)
    .fetch_all(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].try_get::<u64, _>("sequence_number").unwrap(), 1);
    assert_eq!(rows[0].try_get::<String, _>("role").unwrap(), "user");
    assert_eq!(rows[1].try_get::<u64, _>("sequence_number").unwrap(), 2);
    assert_eq!(rows[1].try_get::<String, _>("role").unwrap(), "assistant");
    assert!(rows.iter().all(|row| {
        row.try_get::<Option<Uuid>, _>("invocation_id").unwrap() == Some(invocation.id)
    }));

    let list_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/sessions/{}/messages", session.id))
                .header(AUTHORIZATION, format!("Bearer {}", fixture.api_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), axum::http::StatusCode::OK);
    let messages: Vec<MessageResponseV1> = serde_json::from_slice(
        &to_bytes(list_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.sequence, message.role.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "user"), (2, "assistant")]
    );
    assert_eq!(messages[1].parts[0].part_type, "text");
    assert_eq!(messages[1].parts[0].content, Some(json!("session-message")));
}

async fn invocation_and_dispatch_recovery_are_fenced(
    fixture: &Fixture,
    execution_id: Uuid,
) -> WorkerResultV1 {
    let owner = Uuid::now_v7();
    let command = claim_commands(&fixture.state.pool, owner, 10)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &command)
        .await
        .unwrap();
    let failed_dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    release_dispatch(
        &fixture.state.pool,
        &failed_dispatch,
        "Redis connection was rebuilt",
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let task = dispatch.task().unwrap();
    assert_eq!(dispatch.id, failed_dispatch.id);
    assert_eq!(task.attempt_id, failed_dispatch.task().unwrap().attempt_id);
    assert!(dispatch.fencing_token > failed_dispatch.fencing_token);
    assert!(matches!(
        complete_dispatch(&fixture.state.pool, &failed_dispatch).await,
        Err(RuntimeError::Conflict(_, _))
    ));
    let attempt_id = task.attempt_id;
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    assert!(matches!(
        complete_dispatch(&fixture.state.pool, &dispatch).await,
        Err(RuntimeError::Conflict(_, _))
    ));
    sqlx::query("UPDATE execution_outbox SET published_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 6 SECOND) WHERE id=?")
        .bind(dispatch.id).execute(&fixture.state.pool).await.unwrap();
    let recovered = recover_dispatches(&fixture.state.pool, 100).await.unwrap();
    assert!(
        recovered
            .iter()
            .any(|message| message.attempt_id == attempt_id)
    );

    let workers = (0..20).map(|_| Uuid::now_v7()).collect::<Vec<_>>();
    for worker in &workers {
        agentx_v2_runtime::engine::register_worker(
            &fixture.state.pool,
            *worker,
            task.capability.as_str(),
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .unwrap();
    }
    let mut contenders = tokio::task::JoinSet::new();
    for worker in workers {
        let pool = fixture.state.pool.clone();
        let task = task.clone();
        contenders.spawn(async move {
            agentx_v2_runtime::engine::claim_worker_attempt(
                &pool,
                worker,
                task.capability.as_str(),
                &task,
            )
            .await
        });
    }
    let mut claims = Vec::new();
    while let Some(result) = contenders.join_next().await {
        if let Some(claim) = result.unwrap().unwrap() {
            claims.push(claim);
        }
    }
    assert_eq!(
        claims.len(),
        1,
        "20 concurrent Workers must produce one Lease"
    );
    let first_claim = claims.pop().unwrap();
    sqlx::query("UPDATE node_attempts SET locked_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE id=?")
        .bind(attempt_id).execute(&fixture.state.pool).await.unwrap();
    recover_dispatches(&fixture.state.pool, 100).await.unwrap();
    let replacement = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        replacement,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let replacement_claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        replacement,
        task.capability.as_str(),
        &task,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(replacement_claim.lease.fencing_token > first_claim.lease.fencing_token);
    let first_result = successful_worker_result(&first_claim);
    assert!(matches!(
        agentx_v2_runtime::engine::submit_worker_result(&fixture.state.pool, &first_result).await,
        Err(RuntimeError::Conflict(_, _))
    ));
    let replacement_result = successful_worker_result(&replacement_claim);
    agentx_v2_runtime::engine::submit_worker_result(&fixture.state.pool, &replacement_result)
        .await
        .unwrap();
    replacement_result
}

async fn expired_attempt_deadline_is_terminal_and_not_requeued(fixture: &Fixture) {
    let accepted = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"deadline"}),
            idempotency_key: "runtime-slice-expired-deadline".into(),
        },
    )
    .await
    .unwrap();
    let owner = Uuid::now_v7();
    let command = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == accepted.execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &command)
        .await
        .unwrap();
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let task = dispatch.task().unwrap();
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let worker_id = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let claim = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        worker_id,
        task.capability.as_str(),
        &task,
    )
    .await
    .unwrap()
    .unwrap();
    sqlx::query(
        "UPDATE node_attempts SET deadline_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE tenant_id=? AND id=?",
    )
    .bind(fixture.tenant_id)
    .bind(claim.task.attempt_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();

    let recovered = recover_dispatches(&fixture.state.pool, 100).await.unwrap();
    assert!(
        !recovered
            .iter()
            .any(|message| message.attempt_id == task.attempt_id)
    );
    let state: (String, String, String, bool) = sqlx::query_as(
        "SELECT e.status,a.status,n.status,l.released_at IS NOT NULL FROM workflow_executions e JOIN node_attempts a ON a.execution_id=e.id AND a.tenant_id=e.tenant_id JOIN node_executions n ON n.id=a.node_execution_id AND n.tenant_id=a.tenant_id JOIN worker_leases l ON l.node_attempt_id=a.id AND l.tenant_id=a.tenant_id WHERE e.tenant_id=? AND e.id=? AND a.id=?",
    )
    .bind(fixture.tenant_id)
    .bind(accepted.execution_id)
    .bind(task.attempt_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        state,
        (
            "timed_out".into(),
            "timed_out".into(),
            "timed_out".into(),
            true
        )
    );

    let stale_worker = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        stale_worker,
        task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    assert!(
        agentx_v2_runtime::engine::claim_worker_attempt(
            &fixture.state.pool,
            stale_worker,
            task.capability.as_str(),
            &task,
        )
        .await
        .unwrap()
        .is_none(),
        "a stale dispatch message must be ACK-safe after the deadline becomes terminal"
    );
}

async fn retry_policy_creates_a_second_attempt_and_trace(fixture: &Fixture) {
    let accepted = create_invocation(
        &fixture.state.pool,
        fixture.tenant_id,
        fixture.application_id,
        fixture.key_id,
        &InvocationRequestV1 {
            input: json!({"message":"retry-me"}),
            idempotency_key: "runtime-slice-retry".into(),
        },
    )
    .await
    .unwrap();
    let owner = Uuid::now_v7();
    let command = claim_commands(&fixture.state.pool, owner, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.execution_id == accepted.execution_id)
        .unwrap();
    process_command(&fixture.state.pool, &command)
        .await
        .unwrap();
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let first_task = dispatch.task().unwrap();
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let first_worker = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        first_worker,
        first_task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let first = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        first_worker,
        first_task.capability.as_str(),
        &first_task,
    )
    .await
    .unwrap()
    .unwrap();
    let failed_status = WorkerResultStatusV1::Failed;
    let failed = WorkerResultV1 {
        protocol_version: 1,
        attempt_id: first.task.attempt_id,
        worker_id: first.lease.worker_id,
        fencing_token: first.lease.fencing_token,
        status: failed_status,
        result_hash: agentx_v2_runtime::engine::worker_result_hash(
            failed_status,
            &BTreeMap::new(),
            None,
            Some("CONTROLLED_FIRST_FAILURE"),
            Some("The first Attempt fails for retry verification"),
            None,
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        output_object: None,
        error_code: Some("CONTROLLED_FIRST_FAILURE".into()),
        error_message: Some("The first Attempt fails for retry verification".into()),
        partial_output_object: None,
    };
    agentx_v2_runtime::engine::submit_worker_result(&fixture.state.pool, &failed)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let attempts = sqlx::query("SELECT id,attempt_number,status FROM node_attempts WHERE tenant_id=? AND execution_id=? ORDER BY attempt_number")
        .bind(fixture.tenant_id).bind(accepted.execution_id).fetch_all(&fixture.state.pool).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].try_get::<u16, _>("attempt_number").unwrap(), 1);
    assert_eq!(
        attempts[0].try_get::<String, _>("status").unwrap(),
        "failed"
    );
    assert_eq!(attempts[1].try_get::<u16, _>("attempt_number").unwrap(), 2);
    let dispatch = claim_dispatch(&fixture.state.pool, owner)
        .await
        .unwrap()
        .unwrap();
    let second_task = dispatch.task().unwrap();
    assert_eq!(
        second_task.attempt_id,
        attempts[1].try_get::<Uuid, _>("id").unwrap()
    );
    complete_dispatch(&fixture.state.pool, &dispatch)
        .await
        .unwrap();
    let second_worker = Uuid::now_v7();
    agentx_v2_runtime::engine::register_worker(
        &fixture.state.pool,
        second_worker,
        second_task.capability.as_str(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let second = agentx_v2_runtime::engine::claim_worker_attempt(
        &fixture.state.pool,
        second_worker,
        second_task.capability.as_str(),
        &second_task,
    )
    .await
    .unwrap()
    .unwrap();
    agentx_v2_runtime::engine::submit_worker_result(
        &fixture.state.pool,
        &successful_worker_result(&second),
    )
    .await
    .unwrap();
    let execution_status: String =
        sqlx::query_scalar("SELECT status FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(fixture.tenant_id)
            .bind(accepted.execution_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(execution_status, "succeeded");
    let traced_attempts: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT JSON_UNQUOTE(JSON_EXTRACT(payload_json,'$.attemptId'))) FROM trace_outbox WHERE tenant_id=? AND execution_id=? AND JSON_UNQUOTE(JSON_EXTRACT(payload_json,'$.spanKind'))='attempt'")
        .bind(fixture.tenant_id).bind(accepted.execution_id).fetch_one(&fixture.state.pool).await.unwrap();
    assert_eq!(traced_attempts, 2);
}

fn successful_worker_result(
    claim: &agentx_v2_runtime::engine::ClaimedWorkerAttempt,
) -> WorkerResultV1 {
    let outputs = BTreeMap::from([(
        "main".into(),
        claim.inputs.get("main").cloned().unwrap_or_default(),
    )]);
    let status = WorkerResultStatusV1::Succeeded;
    let result_hash =
        agentx_v2_runtime::engine::worker_result_hash(status, &outputs, None, None, None, None)
            .unwrap();
    WorkerResultV1 {
        protocol_version: 1,
        attempt_id: claim.task.attempt_id,
        worker_id: claim.lease.worker_id,
        fencing_token: claim.lease.fencing_token,
        status,
        result_hash,
        outputs,
        output_object: None,
        error_code: None,
        error_message: None,
        partial_output_object: None,
    }
}

async fn query_is_tenant_application_and_execution_scoped(fixture: &Fixture, execution_id: Uuid) {
    let subject_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runtime_user_admission(tenant_id,user_id,token_version,status,tenant_query_enabled,admission_epoch) VALUES(?,?,1,'active',FALSE,1)",
    )
    .bind(fixture.tenant_id)
    .bind(subject_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO runtime_user_application_grants(tenant_id,user_id,application_id,grant_version,status,can_invoke,can_query,admission_epoch) VALUES(?,?,?,1,'active',TRUE,TRUE,1)",
    )
    .bind(fixture.tenant_id)
    .bind(subject_id)
    .bind(fixture.application_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();

    let exact = get_execution(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([execution_id]),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"get_execution","executionId":execution_id}),
            )
            .unwrap(),
        ),
        Path(execution_id),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(exact.summary.execution_id, execution_id);

    let denied = get_execution(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([Uuid::now_v7()]),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"get_execution","executionId":execution_id}),
            )
            .unwrap(),
        ),
        Path(execution_id),
    )
    .await;
    assert!(matches!(denied, Err(RuntimeError::Unauthorized)));

    sqlx::query("UPDATE workflow_executions SET application_id=NULL WHERE id=?")
        .bind(execution_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    apply_target(
        fixture,
        10,
        AdmissionTargetV1::RuntimeUserWorkflowGrant {
            state: RuntimeUserWorkflowGrantV1 {
                tenant_id: fixture.tenant_id,
                user_id: subject_id,
                workflow_id: fixture.workflow_id,
                grant_version: 10,
                can_query: true,
            },
        },
    )
    .await;
    let app_less = get_execution(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([execution_id]),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"get_execution","executionId":execution_id}),
            )
            .unwrap(),
        ),
        Path(execution_id),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(app_less.summary.execution_id, execution_id);

    apply_target(
        fixture,
        11,
        AdmissionTargetV1::RuntimeUserWorkflowGrant {
            state: RuntimeUserWorkflowGrantV1 {
                tenant_id: fixture.tenant_id,
                user_id: subject_id,
                workflow_id: fixture.workflow_id,
                grant_version: 11,
                can_query: false,
            },
        },
    )
    .await;
    apply_target(
        fixture,
        10,
        AdmissionTargetV1::RuntimeUserWorkflowGrant {
            state: RuntimeUserWorkflowGrantV1 {
                tenant_id: fixture.tenant_id,
                user_id: subject_id,
                workflow_id: fixture.workflow_id,
                grant_version: 10,
                can_query: true,
            },
        },
    )
    .await;
    let workflow_grant: (String, u64) = sqlx::query_as(
        "SELECT status,admission_epoch FROM runtime_user_workflow_grants WHERE tenant_id=? AND user_id=? AND workflow_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(subject_id)
    .bind(fixture.workflow_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(workflow_grant, ("revoked".into(), 11));
    let revoked = get_execution(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.execution".into()]),
            BTreeSet::new(),
            BTreeSet::from([execution_id]),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"get_execution","executionId":execution_id}),
            )
            .unwrap(),
        ),
        Path(execution_id),
    )
    .await;
    assert!(matches!(revoked, Err(RuntimeError::Unauthorized)));
    sqlx::query("UPDATE workflow_executions SET application_id=? WHERE id=?")
        .bind(fixture.application_id)
        .bind(execution_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    let search_request = ExecutionSearchRequestV1 {
        api_version: 1,
        tenant_id: fixture.tenant_id,
        application_ids: vec![fixture.application_id],
        workflow_ids: vec![],
        statuses: vec!["succeeded".into()],
        created_after: None,
        created_before: None,
        search: None,
        cursor: None,
        limit: 50,
    };
    let page = search_executions(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.executions".into()]),
            BTreeSet::from([fixture.application_id]),
            BTreeSet::new(),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"execution-search","request":search_request}),
            )
            .unwrap(),
        ),
        Json(search_request),
    )
    .await
    .unwrap()
    .0;
    assert!(
        page.items
            .iter()
            .any(|item| item.execution_id == execution_id)
    );

    let wrong_application_request = ExecutionSearchRequestV1 {
        api_version: 1,
        tenant_id: fixture.tenant_id,
        application_ids: vec![fixture.application_id],
        workflow_ids: vec![],
        statuses: vec![],
        created_after: None,
        created_before: None,
        search: None,
        cursor: None,
        limit: 50,
    };
    let wrong_application = search_executions(
        State(fixture.state.clone()),
        delegation_headers(
            fixture.tenant_id,
            subject_id,
            BTreeSet::from(["runtime.query.executions".into()]),
            BTreeSet::from([Uuid::now_v7()]),
            BTreeSet::new(),
            agentx_runtime_contracts::content_hash(
                &json!({"operation":"execution-search","request":wrong_application_request}),
            )
            .unwrap(),
        ),
        Json(wrong_application_request),
    )
    .await;
    assert!(matches!(wrong_application, Err(RuntimeError::Unauthorized)));
}

async fn gc_protects_heads_references_and_holds_then_sweeps(
    fixture: &Fixture,
    bundle: &agentx_runtime_contracts::ExecutionSpecBundleV1,
) {
    sqlx::query("UPDATE deployment_bundles SET retained_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 DAY) WHERE id=?")
        .bind(bundle.payload.bundle_id).execute(&fixture.state.pool).await.unwrap();
    let reference = Uuid::now_v7();
    sqlx::query("INSERT INTO bundle_references(id,tenant_id,bundle_id,reference_kind,owner_id) VALUES(?,?,?,'checkpoint_fork_source',?)")
        .bind(reference).bind(fixture.tenant_id).bind(bundle.payload.bundle_id).bind(Uuid::now_v7())
        .execute(&fixture.state.pool).await.unwrap();
    let hold = Uuid::now_v7();
    sqlx::query("INSERT INTO bundle_retention_holds(id,tenant_id,bundle_id,reason,held_by) VALUES(?,?,?,'test','runtime-slice')")
        .bind(hold).bind(fixture.tenant_id).bind(bundle.payload.bundle_id)
        .execute(&fixture.state.pool).await.unwrap();
    assert_eq!(
        mark_collectable(&fixture.state, Uuid::now_v7())
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE bundle_references SET released_at=UTC_TIMESTAMP(6) WHERE id=?")
        .bind(reference)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE bundle_retention_holds SET released_at=UTC_TIMESTAMP(6) WHERE id=?")
        .bind(hold)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let run = Uuid::now_v7();
    assert_eq!(mark_collectable(&fixture.state, run).await.unwrap(), 1);
    assert!(
        sweep_one(&fixture.state, run, Uuid::now_v7())
            .await
            .unwrap()
    );
    assert!(
        !sweep_one(&fixture.state, run, Uuid::now_v7())
            .await
            .unwrap()
    );
    let bundles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deployment_bundles WHERE id=?")
        .bind(bundle.payload.bundle_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(bundles, 0);
    let object_status: String =
        sqlx::query_scalar("SELECT status FROM runtime_objects WHERE object_id=?")
            .bind(bundle.payload.objects[0].object_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(object_status, "deleted");
    let run_status: String = sqlx::query_scalar("SELECT status FROM bundle_gc_runs WHERE id=?")
        .bind(run)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(run_status, "completed");
}

async fn expired_temporary_objects_are_removed(fixture: &Fixture) {
    let object_id = Uuid::now_v7();
    let key = format!("temporary/{}/{}", fixture.tenant_id, object_id);
    fixture
        .state
        .objects
        .put(
            &ObjectPath::from(key.clone()),
            Bytes::from_static(b"orphan").into(),
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO runtime_objects(object_id,tenant_id,object_key,content_hash,size_bytes,media_type,status,temporary_key,idempotency_key,request_hash,temporary_expires_at) VALUES(?,?,?,CONCAT('sha256:',REPEAT('0',64)),6,'application/octet-stream','uploading',?,'orphan','sha256:orphan',DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 SECOND))")
        .bind(object_id).bind(fixture.tenant_id).bind(format!("runtime/{}/{}/{}",fixture.tenant_id,object_id,"0".repeat(64))).bind(&key)
        .execute(&fixture.state.pool).await.unwrap();
    assert_eq!(
        cleanup_expired_temporary_objects(&fixture.state, 100)
            .await
            .unwrap(),
        1
    );
    assert!(
        fixture
            .state
            .objects
            .get(&ObjectPath::from(key))
            .await
            .is_err()
    );
}

async fn gc_object_delete_failure_is_recorded_and_retryable(fixture: &Fixture) {
    let bundle = fixture.bundle(50).await;
    fixture
        .upload_bundle_object(&bundle, "bundle-object:gc-retry")
        .await;
    prepare(fixture, &bundle, "prepare:gc-retry").await;
    sqlx::query("UPDATE deployment_bundles SET status='retained',retained_until=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 1 DAY) WHERE id=?")
        .bind(bundle.payload.bundle_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let run = Uuid::now_v7();
    let failing = Arc::new(FailOnceDeleteStore::new(fixture.state.objects.clone()));
    let state = RuntimeState {
        pool: fixture.state.pool.clone(),
        objects: failing.clone(),
        trust: fixture.state.trust.clone(),
        wakeups: Default::default(),
        vault: None,
    };
    assert_eq!(mark_collectable(&state, run).await.unwrap(), 1);
    assert!(sweep_one(&state, run, Uuid::now_v7()).await.unwrap());
    let failed: String =
        sqlx::query_scalar("SELECT status FROM bundle_gc_items WHERE gc_run_id=? AND bundle_id=?")
            .bind(run)
            .bind(bundle.payload.bundle_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(failed, "failed");
    assert!(sweep_one(&state, run, Uuid::now_v7()).await.unwrap());
    let deleted: String =
        sqlx::query_scalar("SELECT status FROM bundle_gc_items WHERE gc_run_id=? AND bundle_id=?")
            .bind(run)
            .bind(bundle.payload.bundle_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(deleted, "deleted");
    assert!(failing.failed.load(Ordering::SeqCst));
}

async fn ready_orphan_objects_are_swept_and_reuploadable(fixture: &Fixture) {
    let orphan = fixture.bundle(60).await;
    let first = fixture
        .upload_bundle_object(&orphan, "bundle-object:ready-orphan")
        .await;
    sqlx::query(
        "UPDATE runtime_objects SET ready_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 2 HOUR) WHERE tenant_id=? AND object_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(first.object.object_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();

    let run = Uuid::now_v7();
    assert_eq!(mark_collectable(&fixture.state, run).await.unwrap(), 1);
    let kind: String = sqlx::query_scalar(
        "SELECT item_kind FROM bundle_gc_items WHERE gc_run_id=? AND object_id=?",
    )
    .bind(run)
    .bind(first.object.object_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(kind, "object");
    assert!(
        sweep_one(&fixture.state, run, Uuid::now_v7())
            .await
            .unwrap()
    );
    let deleted: String =
        sqlx::query_scalar("SELECT status FROM runtime_objects WHERE object_id=?")
            .bind(first.object.object_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(deleted, "deleted");

    let restored = fixture
        .upload_bundle_object(&orphan, "bundle-object:ready-orphan")
        .await;
    assert!(!restored.replayed);
    let ready: String = sqlx::query_scalar("SELECT status FROM runtime_objects WHERE object_id=?")
        .bind(first.object.object_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(ready, "ready");

    let retry_orphan = fixture.bundle(62).await;
    let retry_receipt = fixture
        .upload_bundle_object(&retry_orphan, "bundle-object:orphan-gc-retry")
        .await;
    sqlx::query("UPDATE runtime_objects SET ready_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 2 HOUR) WHERE tenant_id=? AND object_id=?")
        .bind(fixture.tenant_id)
        .bind(retry_receipt.object.object_id)
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let failing = Arc::new(FailOnceDeleteStore::new(fixture.state.objects.clone()));
    let failing_state = RuntimeState {
        pool: fixture.state.pool.clone(),
        objects: failing,
        trust: fixture.state.trust.clone(),
        wakeups: Default::default(),
        vault: None,
    };
    let retry_run = Uuid::now_v7();
    assert_eq!(
        mark_collectable(&failing_state, retry_run).await.unwrap(),
        1
    );
    assert!(
        sweep_one(&failing_state, retry_run, Uuid::now_v7())
            .await
            .unwrap()
    );
    let failed: String =
        sqlx::query_scalar("SELECT status FROM bundle_gc_items WHERE gc_run_id=? AND object_id=?")
            .bind(retry_run)
            .bind(retry_receipt.object.object_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(failed, "failed");
    assert!(
        sweep_one(&failing_state, retry_run, Uuid::now_v7())
            .await
            .unwrap()
    );
    let retried: String =
        sqlx::query_scalar("SELECT status FROM runtime_objects WHERE object_id=?")
            .bind(retry_receipt.object.object_id)
            .fetch_one(&fixture.state.pool)
            .await
            .unwrap();
    assert_eq!(retried, "deleted");

    let bound = fixture.bundle(61).await;
    fixture
        .upload_bundle_object(&bound, "bundle-object:bound-not-orphan")
        .await;
    prepare(fixture, &bound, "prepare:bound-not-orphan").await;
    sqlx::query(
        "UPDATE runtime_objects SET ready_at=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 2 HOUR) WHERE tenant_id=? AND object_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(bound.payload.objects[0].object_id)
    .execute(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(
        mark_collectable(&fixture.state, Uuid::now_v7())
            .await
            .unwrap(),
        0
    );
}

async fn disable_is_scoped_idempotent_and_preserves_the_head(
    fixture: &Fixture,
    active: &agentx_runtime_contracts::ExecutionSpecBundleV1,
) {
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publish_receipts")
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    let wrong_scope = disable_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.activate"),
        Json(DisableDeploymentRequestV1 {
            api_version: 1,
            idempotency_key: "disable:wrong-scope".into(),
            tenant_id: fixture.tenant_id,
            application_id: fixture.application_id,
            admission_epoch: 3,
        }),
    )
    .await;
    assert!(matches!(wrong_scope, Err(RuntimeError::Unauthorized)));
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publish_receipts")
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(after, before);

    apply_api_key(fixture, 3, AdmissionStatusV1::Active).await;
    let request = DisableDeploymentRequestV1 {
        api_version: 1,
        idempotency_key: "disable:accepted".into(),
        tenant_id: fixture.tenant_id,
        application_id: fixture.application_id,
        admission_epoch: 4,
    };
    let receipt = disable_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.disable"),
        Json(request.clone()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(receipt.status, PublishReceiptStatusV1::Accepted);
    let replay = disable_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.disable"),
        Json(request),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(replay.bundle_id, receipt.bundle_id);

    let head: Uuid = sqlx::query_scalar(
        "SELECT bundle_id FROM deployment_heads WHERE tenant_id=? AND application_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fixture.application_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(head, active.payload.bundle_id);
    let route: String = sqlx::query_scalar(
        "SELECT status FROM application_routes WHERE tenant_id=? AND application_id=?",
    )
    .bind(fixture.tenant_id)
    .bind(fixture.application_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert_eq!(route, "disabled");
    let retained: bool = sqlx::query_scalar(
        "SELECT status='disabled' AND retained_until>=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 13 DAY) FROM deployment_bundles WHERE id=?",
    )
    .bind(active.payload.bundle_id)
    .fetch_one(&fixture.state.pool)
    .await
    .unwrap();
    assert!(retained);
    assert!(matches!(
        create_invocation(
            &fixture.state.pool,
            fixture.tenant_id,
            fixture.application_id,
            fixture.key_id,
            &InvocationRequestV1 {
                input: json!({"message":"disabled"}),
                idempotency_key: "invocation:after-disable".into(),
            },
        )
        .await,
        Err(RuntimeError::Unauthorized | RuntimeError::NotFound)
    ));

    let stale = disable_deployment(
        State(fixture.state.clone()),
        publisher_headers("runtime.deployments.disable"),
        Json(DisableDeploymentRequestV1 {
            api_version: 1,
            idempotency_key: "disable:stale-epoch".into(),
            tenant_id: fixture.tenant_id,
            application_id: fixture.application_id,
            admission_epoch: 4,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(stale.status, PublishReceiptStatusV1::Rejected);
    let rejection = stale.rejection.unwrap();
    assert!(matches!(
        rejection.code,
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::ActivationSequenceConflict
    ));
    let persisted: String = sqlx::query_scalar("SELECT status FROM publish_receipts WHERE tenant_id=? AND operation='disable' AND idempotency_key='disable:stale-epoch'")
        .bind(fixture.tenant_id)
        .fetch_one(&fixture.state.pool)
        .await
        .unwrap();
    assert_eq!(persisted, "rejected");
}

#[derive(Debug)]
struct FailOnceDeleteStore {
    inner: Arc<dyn ObjectStore>,
    failed: AtomicBool,
}

impl FailOnceDeleteStore {
    fn new(inner: Arc<dyn ObjectStore>) -> Self {
        Self {
            inner,
            failed: AtomicBool::new(false),
        }
    }
}

impl fmt::Display for FailOnceDeleteStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fail-once-delete")
    }
}

#[async_trait::async_trait]
impl ObjectStore for FailOnceDeleteStore {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(location, payload, options).await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        options: PutMultipartOpts,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        self.inner.get_opts(location, options).await
    }

    async fn delete(&self, location: &ObjectPath) -> object_store::Result<()> {
        if !self.failed.swap(true, Ordering::SeqCst) {
            return Err(object_store::Error::Generic {
                store: "fail-once-delete",
                source: "injected delete failure".into(),
            });
        }
        self.inner.delete(location).await
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> futures::stream::BoxStream<'_, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy(&self, from: &ObjectPath, to: &ObjectPath) -> object_store::Result<()> {
        self.inner.copy(from, to).await
    }

    async fn copy_if_not_exists(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
    ) -> object_store::Result<()> {
        self.inner.copy_if_not_exists(from, to).await
    }
}

fn publisher_headers(scope: &str) -> HeaderMap {
    let now = now_unix();
    let token = issue_service_token(
        "publisher-current",
        PRIVATE_KEY,
        &ServiceClaimsV1 {
            iss: "agentx-control".into(),
            aud: "agentx-runtime-internal".into(),
            sub: "publisher-test".into(),
            role: ControlRole::Publisher,
            scope: BTreeSet::from([scope.into()]),
            iat: now,
            exp: now + 300,
            jti: Uuid::now_v7(),
        },
    )
    .unwrap();
    bearer_headers(token)
}

fn delegation_headers(
    tenant_id: Uuid,
    subject_id: Uuid,
    scope: BTreeSet<String>,
    application_ids: BTreeSet<Uuid>,
    execution_ids: BTreeSet<Uuid>,
    request_hash: agentx_runtime_contracts::ContentHash,
) -> HeaderMap {
    let now = now_unix();
    bearer_headers(
        issue_delegation_token(
            "publisher-current",
            PRIVATE_KEY,
            &DelegationClaimsV1 {
                iss: "agentx-control".into(),
                aud: "agentx-runtime-internal".into(),
                sub: subject_id,
                tenant_id,
                token_version: 1,
                tenant_wide: false,
                scope,
                application_ids,
                workflow_ids: BTreeSet::new(),
                execution_ids,
                session_ids: BTreeSet::new(),
                request_hash,
                iat: now,
                exp: now + 60,
                jti: Uuid::now_v7(),
            },
        )
        .unwrap(),
    )
}

fn bearer_headers(token: String) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    headers
}

fn definition() -> WorkflowDefinition {
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"contexts":{}},
        "nodes":[{"id":"pass","key":"pass","type":"no_op","typeVersion":1,"name":"Pass","parameters":{},"settings":{"retryOnFail":true,"maxTries":2,"waitBetweenTriesMs":5},"outputProjection":{},"contextWrites":[],"resourceReferences":[]}],
        "connections":[
            {"id":"start-pass","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"pass","targetHandle":"main","order":0},
            {"id":"pass-end","sourceNodeId":"pass","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{"message":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"pass","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["message"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true}}},
        "settings":{"activationBudget":20,"executionOrder":"deterministic"}
    }))
    .unwrap()
}

fn composite_definition(child_version_id: Uuid) -> WorkflowDefinition {
    serde_json::from_value(json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"contexts":{}},
        "nodes":[{
            "id":"child",
            "key":"child",
            "type":"sub_workflow",
            "typeVersion":1,
            "name":"Child",
            "parameters":{"workflowVersionId":child_version_id},
            "outputProjection":{},
            "contextWrites":[],
            "resourceReferences":[]
        }],
        "connections":[
            {"id":"start-child","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"child","targetHandle":"main","order":0},
            {"id":"child-end","sourceNodeId":"child","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{"message":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"child","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["message"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true}}},
        "settings":{"activationBudget":20,"executionOrder":"deterministic"}
    }))
    .unwrap()
}

async fn connect_with_retry(port: u16) -> MySqlPool {
    let url = format!("mysql://agentx:agentx-test-password@127.0.0.1:{port}/agentx_runtime");
    let mut last_error = None;
    for _ in 0..40 {
        match MySqlPoolOptions::new()
            .max_connections(20)
            .connect(&url)
            .await
        {
            Ok(pool) => return pool,
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("connect to Runtime MySQL: {last_error:?}");
}
