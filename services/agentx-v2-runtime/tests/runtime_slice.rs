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
    ApiKeyAdmissionV1, ApplicationRouteAdmissionV1, ApplyChatMappingRequestV1,
    ApprovalDecisionValueV1, CancelWorkPackageRequestV1, ChatMappingV1, CommandEnvelopeV1,
    ControlRole, CreateSessionRequestV1, DelegationClaimsV1, DisableDeploymentRequestV1,
    ExecuteWorkPackageRequestV1, ExecutionSearchRequestV1, InvocationResponseV1,
    MessagePartInputV1, MessageRequestV1, MessageResponseV1, Plane, PrepareBundleRequestV1,
    PrepareWorkPackageRequestV1, PublishReceiptStatusV1, RollbackDeploymentRequestV1,
    RuntimeAdmissionCommandV1, RuntimeApprovalDecisionV1, RuntimeAuthorizationSnapshotV1,
    RuntimeCallPurposeV1, RuntimeEventPayloadV1, RuntimeGrantStateV1, RuntimeObjectReferenceV1,
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
        InvocationCaller, InvocationRequestV1, authenticate_api_key, claim_commands,
        claim_dispatch, complete_dispatch, create_invocation, create_runtime_invocation_tx,
        process_command, process_command_with_state, recover_dispatches, release_dispatch,
    },
    gc::{cleanup_expired_temporary_objects, mark_collectable, sweep_one},
    internal_engine::{
        apply_runtime_command, cancel_work_package, execute_work_package, prepare_work_package,
    },
    object_upload::persist_upload,
    publish::{
        activate_deployment, apply_admission, apply_chat_mapping, disable_deployment,
        prepare_bundle, rollback_deployment,
    },
    query::{
        get_execution, get_execution_artifact, get_execution_runtime_details, search_executions,
    },
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
                "reasoningContent":null,
                "structuredOutput":{"passed":true,"score":0.95,"reason":"fixture accepted the target output","usage":{"tokens":7,"costMicros":23}},
                "citations":[],
                "files":[],
                "usage":{"inputTokens":0,"outputTokens":7,"totalTokens":7,"costMicros":23},
                "finishReason":"stop",
                "partial":false
            }),
            StubWorkerMode::Agent(calls) if endpoint.ends_with("/model") => {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    json!({"toolCall":{"query":"agentx"},"usage":{"inputTokens":5,"outputTokens":5,"totalTokens":10}})
                } else {
                    json!({"done":true,"answer":"agentx-v2","usage":{"inputTokens":5,"outputTokens":5,"totalTokens":10}})
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

include!("runtime_slice/publish_and_suspension.rs");
include!("runtime_slice/fork_sandbox_and_retention.rs");
include!("runtime_slice/lifecycle_and_work_packages.rs");
include!("runtime_slice/session_recovery_and_gc.rs");
