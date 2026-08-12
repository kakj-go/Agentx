use std::{collections::BTreeMap, env, sync::Arc, time::Duration};

mod agent;
mod builtins;
mod resources;

use agentx_application::{
    ArtifactStore, CredentialResolver, RuntimeCredentialHandle, SandboxRuntime,
};
use agentx_domain::{ExecutionId, NodeExecutionId, ResourceType, TenantId, WorkflowVersionId};
use agentx_infrastructure::{
    artifact::MySqlObjectArtifactStore,
    clients,
    config::{RuntimeInfrastructureSettings, SecretProviderMode, secret_provider_mode},
    credential::{
        CredentialKeyring, CredentialSource, MySqlCredentialResolver, RemoteSecretProvider,
    },
    knowledge_runtime::{LightRagRuntime, Mem0Runtime},
    mcp_runtime::HttpMcpToolRuntime,
    model_runtime::OpenAiCompatibleRuntime,
    mysql,
    quota::QuotaAdmission,
    runtime_broker::{InvocationBroker, InvocationBrokerError, InvocationScope},
    runtime_queue::{QueueItem, RuntimeQueue},
    runtime_repository::{ClaimedTask, RuntimeRepository, RuntimeTask, TaskResult},
    runtime_resources::MySqlResourceAuthorizer,
    sandbox_runtime::SandboxManagerRuntime,
    skill_runtime::SnapshotSkillRuntime,
};
use agentx_node_protocol::{
    ExecutionMode, GroupedInput, InvocationResourceRequest, InvocationResourceResponse, Item,
    NODE_PROTOCOL_VERSION, NodeActionRequest, NodeActionResult, ResolvedParameters, TraceContext,
};
use agentx_runtime::{COMPILER_VERSION, ExpressionContext, ExpressionEngine};
use agentx_runtime_rpc::v1::{
    CancelExecutionRequest, HeartbeatLeaseRequest, ReportNodeResultRequest,
    RequestExecutionRequest, runtime_coordinator_client::RuntimeCoordinatorClient,
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use futures::StreamExt;
use secrecy::SecretString;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Channel, Endpoint};
use tracing::{error, info, warn};
use uuid::Uuid;

use agent::AgentRunner;
use resources::{ResourceRuntimes, failed, runtime_context};

#[derive(Clone)]
struct WorkerState {
    repository: RuntimeRepository,
    queue: RuntimeQueue,
    coordinator: RuntimeCoordinatorClient<Channel>,
    http: reqwest::Client,
    remote_node_auth_token: Option<String>,
    instance_id: String,
    capabilities: Vec<String>,
    concurrency: Arc<Semaphore>,
    quota_admission: QuotaAdmission,
    broker: InvocationBroker,
    runtimes: ResourceRuntimes,
    agent: AgentRunner,
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings = RuntimeInfrastructureSettings::from_env()?;
    let health_settings = settings.clone();
    let pool = mysql::connect(&settings.mysql).await?;
    let quota_admission = QuotaAdmission::new(settings.redis.clone());
    let objects = clients::object_store(&settings.object_storage)?;
    let artifact_store: Arc<dyn ArtifactStore> = Arc::new(
        MySqlObjectArtifactStore::new(pool.clone(), objects)
            .with_quota_admission(quota_admission.clone()),
    );
    let credential_source = match secret_provider_mode()? {
        SecretProviderMode::VaultKvV2 => {
            CredentialSource::external(Arc::new(RemoteSecretProvider::from_env()?))
        }
        SecretProviderMode::LocalEncrypted => {
            let keyring =
                CredentialKeyring::from_json(
                    env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID").context(
                        "AGENTX_CREDENTIAL_ACTIVE_KEY_ID is required by the invocation broker",
                    )?,
                    &SecretString::from(env::var("AGENTX_CREDENTIAL_KEYS_JSON").context(
                        "AGENTX_CREDENTIAL_KEYS_JSON is required by the invocation broker",
                    )?),
                )?;
            CredentialSource::local(Arc::new(keyring))
        }
    };
    let broker = InvocationBroker::new_with_credential_source(
        pool.clone(),
        credential_source.clone(),
        artifact_store.clone(),
        env::var("AGENTX_NODE_BROKER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
        env::var("AGENTX_NODE_HANDLE_TTL_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(300),
    );
    let endpoint = env::var("AGENTX_RUNTIME_COORDINATOR_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9090".into());
    let channel = Endpoint::from_shared(endpoint.clone())?.connect_lazy();
    let capabilities = env::var("AGENTX_WORKER_CAPABILITIES")
        .unwrap_or_else(|_| "builtin,declarative_http,remote_action".into())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let concurrency = env::var("AGENTX_WORKER_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16_usize)
        .clamp(1, 256);
    let credentials: Arc<dyn CredentialResolver> = Arc::new(
        MySqlCredentialResolver::new_with_source(pool.clone(), credential_source),
    );
    let authorizer = MySqlResourceAuthorizer::new(pool.clone());
    let sandbox: Option<Arc<dyn SandboxRuntime>> = match (
        env::var("AGENTX_SANDBOX_MANAGER_URL").ok(),
        env::var("AGENTX_SANDBOX_RPC_TOKEN").ok(),
    ) {
        (Some(endpoint), Some(token)) => Some(Arc::new(
            SandboxManagerRuntime::connect_lazy(&endpoint, &token).map_err(anyhow::Error::new)?,
        )),
        _ => None,
    };
    let runtimes = ResourceRuntimes {
        model: Arc::new(
            OpenAiCompatibleRuntime::new(authorizer.clone(), credentials.clone())
                .map_err(anyhow::Error::new)?,
        ),
        mcp: Arc::new(
            HttpMcpToolRuntime::new(authorizer.clone(), credentials.clone())
                .map_err(anyhow::Error::new)?,
        ),
        skill: Arc::new(SnapshotSkillRuntime::new(
            authorizer.clone(),
            artifact_store.clone(),
        )),
        rag: Arc::new(
            LightRagRuntime::new(authorizer.clone(), credentials.clone())
                .map_err(anyhow::Error::new)?,
        ),
        memory: Arc::new(Mem0Runtime::new(authorizer, credentials).map_err(anyhow::Error::new)?),
        sandbox,
        artifacts: artifact_store,
        trace: agentx_infrastructure::operations_projection::MySqlOperationsProjection::new(
            pool.clone(),
        ),
    };
    let agent = AgentRunner::new(pool.clone(), runtimes.clone());
    let state = WorkerState {
        repository: RuntimeRepository::new(pool.clone())
            .with_quota_admission(quota_admission.clone()),
        queue: RuntimeQueue::new(settings.redis),
        coordinator: RuntimeCoordinatorClient::new(channel),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        remote_node_auth_token: env::var("AGENTX_REMOTE_NODE_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.is_empty()),
        instance_id: env::var("HOSTNAME").unwrap_or_else(|_| format!("worker-{}", Uuid::now_v7())),
        capabilities,
        concurrency: Arc::new(Semaphore::new(concurrency)),
        quota_admission,
        broker,
        runtimes,
        agent,
    };
    state.queue.ensure_groups().await?;

    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.register("redis", true).await;
    health.register("coordinator", true).await;
    health.register("object_storage", true).await;
    tokio::spawn(consume_loop(state.clone()));
    tokio::spawn(heartbeat_service_loop(state.clone(), health.clone()));
    tokio::spawn(worker_dependency_health_loop(
        health.clone(),
        health_settings,
        pool,
        endpoint,
    ));
    let router = Router::new()
        .route(
            "/agentx/runtime/v1/invocation-resources/resolve",
            post(resolve_invocation_resource),
        )
        .route(
            "/agentx/runtime/v1/invocations/cancellation/{token}",
            get(invocation_cancellation),
        )
        .with_state(state);
    agentx_service_kit::serve("workflow-worker", router, health).await
}

async fn consume_loop(state: WorkerState) {
    loop {
        match state
            .queue
            .read(&state.capabilities, &state.instance_id, 5_000)
            .await
        {
            Ok(items) => {
                for item in items {
                    let state = state.clone();
                    let permit = state
                        .concurrency
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("worker semaphore open");
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(error) = process_queue_item(&state, &item).await {
                            error!(%error, stream_id=%item.stream_id, "runtime task processing failed");
                        }
                    });
                }
            }
            Err(error) => {
                error!(%error, "runtime queue read failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn process_queue_item(state: &WorkerState, item: &QueueItem) -> Result<()> {
    if !supports_dispatch(&item.message) {
        warn!(
            stream_id=%item.stream_id,
            node_protocol=%item.message.node_protocol_version,
            compiler_version=%item.message.compiler_version,
            ir_schema_version=%item.message.ir_schema_version,
            "runtime task requires an incompatible worker"
        );
        return Ok(());
    }
    let Some(claimed) = state
        .repository
        .claim_task(&item.message, &state.instance_id, 30)
        .await?
    else {
        state.queue.ack(item).await?;
        return Ok(());
    };
    info!(execution_id=%claimed.task.execution_id,node_execution_id=%claimed.task.node_execution_id,"runtime task claimed");
    let result = match agentx_infrastructure::quota::reserve_attempt_with_admission(
        state.repository.pool(),
        &claimed.task,
        Some(&state.quota_admission),
    )
    .await
    {
        Ok(()) => execute_with_heartbeat(state, &claimed).await,
        Err(error) => {
            let message = error.to_string();
            let admission_unavailable = message.contains("Redis quota admission")
                || message.contains("QUOTA_ADMISSION_UNAVAILABLE");
            TaskResult::Failed {
                code: if admission_unavailable {
                    "QUOTA_ADMISSION_UNAVAILABLE"
                } else {
                    "QUOTA_EXCEEDED"
                }
                .into(),
                message,
                retryable: admission_unavailable,
            }
        }
    };
    let report = report_result(state, &claimed, result).await;
    match report {
        Ok(_) => {
            agentx_infrastructure::quota::settle_attempt_with_admission(
                state.repository.pool(),
                &claimed.task,
                Some(&state.quota_admission),
            )
            .await?;
            state.queue.ack(item).await?;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn supports_dispatch(message: &agentx_infrastructure::runtime_repository::DispatchMessage) -> bool {
    message.node_protocol_version == NODE_PROTOCOL_VERSION
        && message.compiler_version == COMPILER_VERSION
        && message.ir_schema_version == "4.0"
}

async fn execute_with_heartbeat(state: &WorkerState, claimed: &ClaimedTask) -> TaskResult {
    let cancellation = CancellationToken::new();
    let execution = execute_task(state, claimed, cancellation.clone());
    tokio::pin!(execution);
    let timeout = tokio::time::sleep(Duration::from_millis(
        (claimed.task.deadline - OffsetDateTime::now_utc())
            .whole_milliseconds()
            .max(1) as u64,
    ));
    tokio::pin!(timeout);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            result = &mut execution => return result.unwrap_or_else(|error| TaskResult::Failed { code:"WORKER_ERROR".into(), message:error.to_string(), retryable:true }),
            _ = &mut timeout => {
                cancellation.cancel();
                let cleanup=tokio::time::timeout(Duration::from_secs(15),&mut execution).await;
                if let Ok(Ok(TaskResult::Failed{code,message,retryable}))=cleanup {if code=="SANDBOX_CLEANUP_PENDING"{return TaskResult::Failed{code,message,retryable};}}
                return TaskResult::Failed{code:"NODE_TIMEOUT".into(),message:"Node execution exceeded its deadline".into(),retryable:true};
            },
            _ = heartbeat.tick() => {
                let response=state.coordinator.clone().heartbeat_lease(HeartbeatLeaseRequest{
                    tenant_id:claimed.task.tenant_id.to_string(),attempt_id:claimed.task.attempt_id.to_string(),
                    lease_token:claimed.lease_token.to_string(),worker_instance_id:state.instance_id.clone(),
                }).await;
                match response {
                    Ok(response) if response.get_ref().valid && !response.get_ref().cancellation_requested => {}
                    Ok(_) => {
                        cancellation.cancel();
                        let cleanup=tokio::time::timeout(Duration::from_secs(15),&mut execution).await;
                        if let Ok(Ok(TaskResult::Failed{code,message,retryable}))=cleanup {if code=="SANDBOX_CLEANUP_PENDING"{return TaskResult::Failed{code,message,retryable};}}
                        return TaskResult::Failed{code:"EXECUTION_CANCELLED".into(),message:"Execution was cancelled or lease was lost".into(),retryable:false};
                    },
                    Err(error) => warn!(%error,"lease heartbeat failed; current lease deadline remains authoritative"),
                }
            }
        }
    }
}

async fn execute_task(
    state: &WorkerState,
    claimed: &ClaimedTask,
    cancellation: CancellationToken,
) -> Result<TaskResult> {
    let task = &claimed.task;
    match task.capability.as_str() {
        "builtin" => execute_builtin(state, task, cancellation).await,
        "declarative_http" => execute_http(state, task).await,
        "remote_action" => execute_remote(state, task, claimed.lease_token).await,
        "model" | "mcp_tool" | "skill" | "rag" | "memory" | "sandbox" => {
            let (credential_handles, sandbox_handles) =
                issue_runtime_credentials(state, task, claimed.lease_token).await?;
            let first = flatten_inputs(&task.inputs)
                .into_iter()
                .next()
                .unwrap_or_default();
            let mut parameters = ExpressionEngine
                .resolve_parameters(&task.node_parameters, &expression_context(task, &first, 0))?;
            if task.capability == "sandbox" {
                parameters
                    .as_object_mut()
                    .context("Sandbox node parameters must be an object")?
                    .insert("_credentialHandles".into(), Value::Array(sandbox_handles));
            }
            Ok(state
                .runtimes
                .execute(
                    task,
                    claimed.lease_token,
                    credential_handles,
                    cancellation,
                    parameters,
                )
                .await
                .unwrap_or_else(failed))
        }
        "agent" => {
            let (credential_handles, _) =
                issue_runtime_credentials(state, task, claimed.lease_token).await?;
            let first = flatten_inputs(&task.inputs)
                .into_iter()
                .next()
                .unwrap_or_default();
            let parameters = ExpressionEngine
                .resolve_parameters(&task.node_parameters, &expression_context(task, &first, 0))?;
            let context =
                runtime_context(task, claimed.lease_token, credential_handles, cancellation);
            Ok(state
                .agent
                .execute(task, &context, parameters)
                .await
                .unwrap_or_else(failed))
        }
        other => anyhow::bail!("unsupported worker capability {other}"),
    }
}

async fn issue_runtime_credentials(
    state: &WorkerState,
    task: &RuntimeTask,
    lease_token: Uuid,
) -> Result<(BTreeMap<Uuid, RuntimeCredentialHandle>, Vec<Value>)> {
    if !task
        .resource_references
        .iter()
        .any(|reference| reference.resource_type == ResourceType::Credential)
    {
        return Ok((BTreeMap::new(), Vec::new()));
    }
    let issued = state
        .broker
        .issue(
            &InvocationScope {
                tenant_id: task.tenant_id,
                execution_id: task.execution_id,
                node_execution_id: task.node_execution_id,
                attempt_id: task.attempt_id,
                lease_token,
                deadline: task.deadline,
            },
            &task.resource_references,
            &task.resource_snapshots,
            task.inputs.values().flatten(),
        )
        .await?;
    let mut runtime_handles = BTreeMap::new();
    let mut sandbox_handles = Vec::new();
    for binding in issued.credential_bindings {
        sandbox_handles.push(json!({
            "resourceId": binding.resource_id,
            "version": binding.version,
            "handle": binding.invocation.handle,
        }));
        runtime_handles.insert(
            binding.resource_id,
            RuntimeCredentialHandle {
                version: binding.version,
                handle: binding.invocation.handle,
                expires_at: binding.invocation.expires_at,
            },
        );
    }
    Ok((runtime_handles, sandbox_handles))
}

async fn execute_builtin(
    state: &WorkerState,
    task: &RuntimeTask,
    cancellation: CancellationToken,
) -> Result<TaskResult> {
    if let Some(result) = builtins::execute(task)? {
        return Ok(result);
    }
    let items = flatten_inputs(&task.inputs);
    if task.node_type.starts_with("workflow.") {
        return execute_subworkflow(state, task, items, cancellation).await;
    }
    match task.node_type.as_str() {
        "set" => {
            let mut output = Vec::with_capacity(items.len());
            for (index, mut item) in items.into_iter().enumerate() {
                let context = expression_context(task, &item, index);
                let resolved =
                    ExpressionEngine.resolve_parameters(&task.node_parameters, &context)?;
                let keep_only_set = resolved
                    .get("keepOnlySet")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let values = resolved
                    .get("values")
                    .cloned()
                    .unwrap_or_else(|| resolved.clone());
                if keep_only_set {
                    item.json = values;
                } else if let (Some(target), Some(values)) =
                    (item.json.as_object_mut(), values.as_object())
                {
                    target.extend(values.clone());
                }
                output.push(item);
            }
            Ok(completed("main", output))
        }
        "error_handler" => Ok(execute_error_handler(
            items,
            task.node_parameters
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("recover"),
        )),
        "if" => {
            let mut truthy = Vec::new();
            let mut falsy = Vec::new();
            for (index, item) in items.into_iter().enumerate() {
                let context = expression_context(task, &item, index);
                let condition = task
                    .node_parameters
                    .get("condition")
                    .context("IF condition is required")?;
                let resolved = ExpressionEngine.resolve_parameters(condition, &context)?;
                if resolved
                    .as_bool()
                    .context("IF condition must resolve to boolean")?
                {
                    truthy.push(item);
                } else {
                    falsy.push(item);
                }
            }
            Ok(TaskResult::Completed(BTreeMap::from([
                ("true".into(), truthy),
                ("false".into(), falsy),
            ])))
        }
        "switch" => {
            let rules = task
                .node_parameters
                .get("rules")
                .and_then(Value::as_array)
                .context("Switch rules are required")?;
            let mut outputs = BTreeMap::<String, Vec<Item>>::new();
            for (item_index, item) in items.into_iter().enumerate() {
                let mut matched = false;
                for (rule_index, rule) in rules.iter().enumerate() {
                    let condition = rule
                        .get("condition")
                        .context("Switch rule condition is required")?;
                    if ExpressionEngine
                        .resolve_parameters(
                            condition,
                            &expression_context(task, &item, item_index),
                        )?
                        .as_bool()
                        .unwrap_or(false)
                    {
                        outputs
                            .entry(format!("case:{rule_index}"))
                            .or_default()
                            .push(item.clone());
                        matched = true;
                        if !task
                            .node_parameters
                            .get("sendToAllMatches")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            break;
                        }
                    }
                }
                if !matched {
                    outputs.entry("fallback".into()).or_default().push(item);
                }
            }
            Ok(TaskResult::Completed(outputs))
        }
        "loop_over_items" => execute_loop(task, items),
        "wait" => Ok(TaskResult::Suspended(wait_contract(task)?)),
        "approval" => Ok(TaskResult::Suspended(approval_contract(task)?)),
        "sub_workflow" => execute_subworkflow(state, task, items, cancellation).await,
        other => Ok(TaskResult::Failed {
            code: "BUILTIN_NOT_IMPLEMENTED".into(),
            message: format!("Builtin node {other} is not implemented"),
            retryable: false,
        }),
    }
}

fn execute_error_handler(items: Vec<Item>, mode: &str) -> TaskResult {
    if mode != "fail" {
        return completed("recovered", items);
    }
    let error = items.first().and_then(|item| item.json.get("error"));
    TaskResult::Failed {
        code: error
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("HANDLED_ERROR")
            .into(),
        message: error
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Error Handler rethrew the incoming error")
            .into(),
        retryable: false,
    }
}

async fn execute_http(state: &WorkerState, task: &RuntimeTask) -> Result<TaskResult> {
    let first = flatten_inputs(&task.inputs)
        .into_iter()
        .next()
        .unwrap_or_default();
    let parameters = ExpressionEngine
        .resolve_parameters(&task.node_parameters, &expression_context(task, &first, 0))?;
    let url = parameters
        .get("url")
        .and_then(Value::as_str)
        .context("HTTP url is required")?;
    let method = parameters
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .parse()?;
    let mut request = state
        .http
        .request(method, url)
        .timeout(Duration::from_millis(
            (task.deadline - OffsetDateTime::now_utc())
                .whole_milliseconds()
                .max(1) as u64,
        ));
    if let Some(headers) = parameters.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }
    }
    if let Some(body) = parameters.get("body") {
        request = request.json(body);
    }
    let response = request.send().await?;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                Value::String(v.to_str().unwrap_or("[binary]").into()),
            )
        })
        .collect::<Map<_, _>>();
    let bytes = response.bytes().await?;
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    if !status.is_success() {
        return Ok(TaskResult::Failed {
            code: format!("HTTP_{}", status.as_u16()),
            message: body.to_string(),
            retryable: status.is_server_error(),
        });
    }
    Ok(completed(
        "main",
        vec![Item {
            json: json!({"status":status.as_u16(),"statusCode":status.as_u16(),"headers":headers,"body":body,"responseArtifact":null}),
            ..Item::default()
        }],
    ))
}

async fn execute_remote(
    state: &WorkerState,
    task: &RuntimeTask,
    lease_token: Uuid,
) -> Result<TaskResult> {
    let endpoint = task
        .node_parameters
        .get("endpoint")
        .and_then(Value::as_str)
        .context("Remote node endpoint is required")?;
    let items = flatten_inputs(&task.inputs);
    let common = ExpressionEngine.resolve_parameters(
        &task.node_parameters,
        &expression_context(task, items.first().unwrap_or(&Item::default()), 0),
    )?;
    let per_item = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            ExpressionEngine.resolve_parameters(
                &task.node_parameters,
                &expression_context(task, item, index),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let invocation = state
        .broker
        .issue(
            &InvocationScope {
                tenant_id: task.tenant_id,
                execution_id: task.execution_id,
                node_execution_id: task.node_execution_id,
                attempt_id: task.attempt_id,
                lease_token,
                deadline: task.deadline,
            },
            &task.resource_references,
            &task.resource_snapshots,
            task.inputs.values().flatten(),
        )
        .await?;
    let request = NodeActionRequest {
        protocol_version: NODE_PROTOCOL_VERSION.into(),
        node_type: task.node_type.clone(),
        node_version: task.node_version,
        tenant_id: TenantId::from_uuid(task.tenant_id),
        workflow_version_id: task.workflow_version_id.map(WorkflowVersionId::from_uuid),
        execution_id: ExecutionId::from_uuid(task.execution_id),
        node_execution_id: NodeExecutionId::from_uuid(task.node_execution_id),
        attempt_id: task.attempt_id,
        run_index: task.run_index,
        iteration_index: task.iteration_index,
        mode: execution_mode(&task.mode),
        inputs: task
            .inputs
            .iter()
            .enumerate()
            .map(|(index, (port, items))| GroupedInput {
                port: port.clone(),
                branch_index: index as u32,
                items: items.clone(),
            })
            .collect(),
        parameters: ResolvedParameters { common, per_item },
        artifact_handles: invocation.artifact_handles,
        credential_handles: invocation.credential_handles,
        idempotency_key: task.idempotency_key.clone(),
        deadline: task.deadline,
        cancellation_url: Some(invocation.cancellation_url),
        trace_context: TraceContext {
            trace_id: task.trace_id.to_string(),
            span_id: Uuid::now_v7().to_string(),
            trace_flags: None,
        },
    };
    let mut request_builder = state
        .http
        .post(format!(
            "{}/agentx/node/v1/actions/execute",
            endpoint.trim_end_matches('/')
        ))
        .json(&request)
        .timeout(Duration::from_millis(
            (task.deadline - OffsetDateTime::now_utc())
                .whole_milliseconds()
                .max(1) as u64,
        ));
    if let Some(token) = &state.remote_node_auth_token {
        request_builder = request_builder.bearer_auth(token);
    }
    let response = request_builder.send().await?;
    if !response.status().is_success() {
        let status = response.status();
        return Ok(TaskResult::Failed {
            code: format!("REMOTE_HTTP_{}", status.as_u16()),
            message: response.text().await.unwrap_or_default(),
            retryable: status.is_server_error(),
        });
    }
    Ok(match response.json::<NodeActionResult>().await? {
        NodeActionResult::Completed { outputs, .. } => TaskResult::Completed(
            outputs
                .into_iter()
                .enumerate()
                .map(|(index, items)| {
                    (
                        if index == 0 {
                            "main".into()
                        } else {
                            format!("main:{index}")
                        },
                        items,
                    )
                })
                .collect(),
        ),
        NodeActionResult::Failed { error } => TaskResult::Failed {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        },
        NodeActionResult::Suspended { resume, checkpoint } => {
            let timeout_at = format_optional_time(resume.timeout_at)?;
            TaskResult::Suspended(
                json!({"kind":resume.kind,"timeoutAt":timeout_at,"payloadSchema":resume.payload_schema,"checkpoint":checkpoint}),
            )
        }
    })
}

async fn resolve_invocation_resource(
    State(state): State<WorkerState>,
    Json(request): Json<InvocationResourceRequest>,
) -> Result<Json<InvocationResourceResponse>, (StatusCode, Json<Value>)> {
    state
        .broker
        .resolve(&request)
        .await
        .map(Json)
        .map_err(broker_error)
}

async fn invocation_cancellation(
    State(state): State<WorkerState>,
    Path(token): Path<String>,
) -> Result<Json<agentx_node_protocol::InvocationCancellationStatus>, (StatusCode, Json<Value>)> {
    state
        .broker
        .cancellation_status(&token)
        .await
        .map(Json)
        .map_err(broker_error)
}

fn broker_error(error: InvocationBrokerError) -> (StatusCode, Json<Value>) {
    let status = match error {
        InvocationBrokerError::Invalid => StatusCode::NOT_FOUND,
        InvocationBrokerError::Expired | InvocationBrokerError::Replayed => StatusCode::GONE,
        InvocationBrokerError::LeaseInvalid => StatusCode::CONFLICT,
        InvocationBrokerError::ResourceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, Json(json!({"code":error.to_string()})))
}

async fn execute_subworkflow(
    state: &WorkerState,
    task: &RuntimeTask,
    items: Vec<Item>,
    cancellation: CancellationToken,
) -> Result<TaskResult> {
    let parameters = ExpressionEngine.resolve_parameters(
        &task.node_parameters,
        &expression_context(task, items.first().unwrap_or(&Item::default()), 0),
    )?;
    let version = parameters
        .get("workflowVersionId")
        .and_then(Value::as_str)
        .context("Sub-workflow version is required")?;
    let parent = sqlx::query(
        "SELECT session_id,requested_by,status FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(task.tenant_id)
    .bind(task.execution_id)
    .fetch_one(state.repository.pool())
    .await?;
    let session_id: Option<Uuid> = parent.try_get("session_id")?;
    let requested_by: Option<Uuid> = parent.try_get("requested_by")?;
    let accepted = state
        .coordinator
        .clone()
        .request_execution(RequestExecutionRequest {
            tenant_id: task.tenant_id.to_string(),
            source: Some(
                agentx_runtime_rpc::v1::request_execution_request::Source::Version(
                    agentx_runtime_rpc::v1::VersionSource {
                        version_id: version.into(),
                    },
                ),
            ),
            invocation_id: None,
            session_id: session_id.map(|value| value.to_string()),
            requested_by: requested_by.map(|value| value.to_string()),
            trigger_type: "sub_workflow".into(),
            input_json: serde_json::to_string(
                parameters
                    .get("inputs")
                    .unwrap_or(&Value::Object(Default::default())),
            )?,
            debug_plan_json: "{}".into(),
            debug_overlay_json: "{}".into(),
            resource_snapshots_json: "[]".into(),
            context_json: serde_json::to_string(&task.contexts)?,
            idempotency_key: Some(format!(
                "sub:{}:{}",
                task.execution_id, task.node_execution_id
            )),
            caller_execution_id: Some(task.execution_id.to_string()),
            parent_execution_id: Some(task.execution_id.to_string()),
            trace_id: Some(task.trace_id.to_string()),
            caller_node_execution_id: Some(task.node_execution_id.to_string()),
        })
        .await?
        .into_inner();
    let child = Uuid::parse_str(&accepted.execution_id)?;
    loop {
        let row = sqlx::query("SELECT status FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(task.tenant_id)
            .bind(child)
            .fetch_one(state.repository.pool())
            .await?;
        let status: String = row.try_get("status")?;
        match status.as_str() {
            "succeeded" => {
                let result = sqlx::query_scalar::<_, Value>(
                    "SELECT result_json FROM workflow_executions WHERE tenant_id=? AND id=?",
                )
                .bind(task.tenant_id)
                .bind(child)
                .fetch_one(state.repository.pool())
                .await?;
                let output = result.get("outputs").cloned().unwrap_or(Value::Null);
                return Ok(completed(
                    "main",
                    vec![Item {
                        json: output,
                        ..Item::default()
                    }],
                ));
            }
            "failed" | "cancelled" | "timed_out" => {
                let result = sqlx::query_scalar::<_, Value>(
                    "SELECT result_json FROM workflow_executions WHERE tenant_id=? AND id=?",
                )
                .bind(task.tenant_id)
                .bind(child)
                .fetch_optional(state.repository.pool())
                .await?
                .unwrap_or(Value::Null);
                let primary = result
                    .get("error")
                    .and_then(|error| error.get("primaryError"))
                    .cloned()
                    .unwrap_or_else(|| {
                        json!({
                            "code": "SUBWORKFLOW_FAILED",
                            "message": format!("Sub-workflow ended with {status}"),
                            "details": {"childExecutionId": child},
                            "retryable": false
                        })
                    });
                let item = json!({
                    "code": primary.get("code").and_then(Value::as_str).unwrap_or("SUBWORKFLOW_FAILED"),
                    "message": primary.get("message").and_then(Value::as_str).unwrap_or("Sub-workflow failed"),
                    "details": {
                        "childExecutionId": child,
                        "childError": primary.get("details").cloned().unwrap_or(Value::Null)
                    },
                    "sourceNodeId": task.node_execution_id,
                    "sourceNodeKey": "sub_workflow",
                    "nodeExecutionId": task.node_execution_id,
                    "runIndex": 0,
                    "iterationIndex": 0,
                    "retryable": primary.get("retryable").and_then(Value::as_bool).unwrap_or(false)
                });
                return Ok(completed(
                    "error",
                    vec![Item {
                        json: item,
                        ..Item::default()
                    }],
                ));
            }
            _ => {
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        let _ = state.coordinator.clone().cancel_execution(CancelExecutionRequest {
                            tenant_id: task.tenant_id.to_string(),
                            execution_id: child.to_string(),
                            actor_user_id: requested_by.map(|value| value.to_string()),
                        }).await;
                        return Ok(TaskResult::Failed {
                            code: "SUBWORKFLOW_CANCELLED".into(),
                            message: "Parent execution was cancelled or timed out".into(),
                            retryable: false,
                        });
                    }
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
            }
        }
    }
}

async fn report_result(
    state: &WorkerState,
    claimed: &ClaimedTask,
    result: TaskResult,
) -> Result<()> {
    let (status, outputs, error_code, error_message, retryable, suspend) = match result {
        TaskResult::Completed(outputs) => (
            "completed",
            serde_json::to_string(&outputs)?,
            None,
            None,
            false,
            "{}".into(),
        ),
        TaskResult::Failed {
            code,
            message,
            retryable,
        } => (
            "failed",
            "{}".into(),
            Some(code),
            Some(message),
            retryable,
            "{}".into(),
        ),
        TaskResult::Suspended(value) => (
            "suspended",
            "{}".into(),
            None,
            None,
            false,
            serde_json::to_string(&value)?,
        ),
    };
    state
        .coordinator
        .clone()
        .report_node_result(ReportNodeResultRequest {
            tenant_id: claimed.task.tenant_id.to_string(),
            execution_id: claimed.task.execution_id.to_string(),
            node_execution_id: claimed.task.node_execution_id.to_string(),
            attempt_id: claimed.task.attempt_id.to_string(),
            lease_token: claimed.lease_token.to_string(),
            status: status.into(),
            outputs_json: outputs,
            error_code,
            error_message,
            retryable,
            suspend_json: suspend,
        })
        .await?;
    Ok(())
}

fn flatten_inputs(inputs: &BTreeMap<String, Vec<Item>>) -> Vec<Item> {
    inputs.values().flatten().cloned().collect()
}
fn completed(port: &str, items: Vec<Item>) -> TaskResult {
    TaskResult::Completed(BTreeMap::from([(port.into(), items)]))
}
fn expression_context(task: &RuntimeTask, item: &Item, index: usize) -> ExpressionContext {
    ExpressionContext {
        json: item.json.clone(),
        input: serde_json::to_value(flatten_inputs(&task.inputs)).unwrap_or(Value::Null),
        item_index: index,
        run_index: task.run_index,
        linked_nodes: task.linked_nodes.clone(),
        inputs: task.workflow_inputs.clone(),
        outputs: task.linked_nodes.clone(),
        contexts: task.contexts.clone(),
        loop_context: serde_json::json!({"iteration": task.iteration_index, "itemIndex": index}),
        execution: serde_json::json!({
            "executionId": task.execution_id,
            "nodeExecutionId": task.node_execution_id,
            "runIndex": task.run_index,
            "iterationIndex": task.iteration_index,
            "contextVersion": task.context_version,
        }),
    }
}
fn execute_loop(task: &RuntimeTask, mut items: Vec<Item>) -> Result<TaskResult> {
    let batch = task
        .node_parameters
        .get("batchSize")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let resumed = items
        .first()
        .is_some_and(|item| item.metadata.contains_key("agentx.loop.all"));
    let all = if resumed {
        items[0]
            .metadata
            .remove("agentx.loop.all")
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default()
    } else {
        items.clone()
    };
    let mut remaining = if resumed {
        items[0]
            .metadata
            .remove("agentx.loop.remaining")
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default()
    } else {
        let split = batch.min(items.len());
        items.split_off(split)
    };
    if resumed {
        if remaining.is_empty() {
            return Ok(completed("done", all));
        }
        let split = batch.min(remaining.len());
        items = remaining.drain(..split).collect();
    }
    for item in &mut items {
        item.metadata
            .insert("agentx.loop.all".into(), serde_json::to_value(&all)?);
        item.metadata.insert(
            "agentx.loop.remaining".into(),
            serde_json::to_value(&remaining)?,
        );
    }
    Ok(completed("loop", items))
}
fn wait_contract(task: &RuntimeTask) -> Result<Value> {
    let first = flatten_inputs(&task.inputs)
        .into_iter()
        .next()
        .unwrap_or_default();
    let p = ExpressionEngine
        .resolve_parameters(&task.node_parameters, &expression_context(task, &first, 0))?;
    let kind = p.get("kind").and_then(Value::as_str).unwrap_or("duration");
    let wake = if kind == "duration" {
        Some(
            OffsetDateTime::now_utc()
                + time::Duration::milliseconds(
                    p.get("durationMs")
                        .and_then(Value::as_i64)
                        .unwrap_or(1000)
                        .max(1),
                ),
        )
    } else {
        p.get("resumeAt").and_then(Value::as_str).and_then(|v| {
            OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339).ok()
        })
    };
    anyhow::ensure!(
        !matches!(kind, "duration" | "datetime") || wake.is_some(),
        "WAIT_WAKE_AT_INVALID"
    );
    let auth = p
        .get("authenticationMode")
        .and_then(Value::as_str)
        .unwrap_or("signed");
    let auth_hash = p
        .get("authenticationValue")
        .and_then(Value::as_str)
        .map(|value| format!("{:x}", Sha256::digest(value.as_bytes())));
    let wake_at = format_optional_time(wake)?;
    Ok(
        json!({"kind":if matches!(kind,"duration"|"datetime"){"time"}else{kind},"waitKind":kind,"wakeAt":wake_at,"timeoutAt":p.get("timeoutAt"),"payloadSchema":p.get("payloadSchema").cloned().unwrap_or_else(||json!({"type":"object"})),"authenticationMode":auth,"authenticationConfigHash":auth_hash}),
    )
}

fn format_optional_time(value: Option<OffsetDateTime>) -> Result<Option<String>> {
    value
        .map(|value| value.format(&time::format_description::well_known::Rfc3339))
        .transpose()
        .map_err(Into::into)
}
fn approval_contract(task: &RuntimeTask) -> Result<Value> {
    let first = flatten_inputs(&task.inputs)
        .into_iter()
        .next()
        .unwrap_or_default();
    let p = ExpressionEngine
        .resolve_parameters(&task.node_parameters, &expression_context(task, &first, 0))?;
    let relative_timeout = p
        .get("timeoutMs")
        .and_then(Value::as_i64)
        .map(|value| OffsetDateTime::now_utc() + time::Duration::milliseconds(value.max(1)));
    let timeout_at = match p.get("timeoutAt").and_then(Value::as_str) {
        Some(value) => Some(
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .context("APPROVAL_TIMEOUT_AT_INVALID")?,
        ),
        None => relative_timeout,
    };
    Ok(
        json!({"kind":"approval","timeoutAt":format_optional_time(timeout_at)?,"payloadSchema":{"type":"object"},"title":p.get("title"),"description":p.get("description"),"candidateUserId":p.get("candidateUserId")}),
    )
}
fn execution_mode(value: &str) -> ExecutionMode {
    match value {
        "partial" | "node" | "to_node" | "from_node" | "fork" => ExecutionMode::Partial,
        "evaluation" => ExecutionMode::Evaluation,
        "production" => ExecutionMode::Production,
        _ => ExecutionMode::Manual,
    }
}

async fn worker_dependency_health_loop(
    health: agentx_service_kit::HealthRegistry,
    settings: RuntimeInfrastructureSettings,
    pool: sqlx::MySqlPool,
    coordinator_endpoint: String,
) {
    loop {
        health
            .set_status(
                "mysql",
                if mysql::ping(&pool).await.is_ok() {
                    "ready"
                } else {
                    "unavailable"
                },
            )
            .await;
        health
            .set_status(
                "redis",
                if clients::connect_redis(&settings.redis).await.is_ok() {
                    "ready"
                } else {
                    "unavailable"
                },
            )
            .await;
        let object_ready = match clients::object_store(&settings.object_storage) {
            Ok(store) => matches!(store.list(None).next().await, Some(Ok(_)) | None),
            Err(_) => false,
        };
        health
            .set_status(
                "object_storage",
                if object_ready { "ready" } else { "unavailable" },
            )
            .await;
        let coordinator_ready = match Endpoint::from_shared(coordinator_endpoint.clone()) {
            Ok(endpoint) => endpoint.connect().await.is_ok(),
            Err(_) => false,
        };
        health
            .set_status(
                "coordinator",
                if coordinator_ready {
                    "ready"
                } else {
                    "unavailable"
                },
            )
            .await;
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn heartbeat_service_loop(state: WorkerState, health: agentx_service_kit::HealthRegistry) {
    loop {
        let overall_status = health.overall_status().await;
        let capability_status = if overall_status == "ready" {
            "ready"
        } else {
            "unavailable"
        };
        let manifest_hashes: Value = sqlx::query_scalar(
            "SELECT COALESCE(JSON_ARRAYAGG(manifest_hash),JSON_ARRAY()) FROM node_definition_versions",
        )
        .fetch_one(state.repository.pool())
        .await
        .unwrap_or_else(|_| json!([]));
        for capability in &state.capabilities {
            let _ = sqlx::query("INSERT INTO worker_capabilities(instance_id,capability,node_protocol_version,ir_schema_versions_json,compiler_version_min,compiler_version_max,manifest_hashes_json,status,heartbeat_at) VALUES(?,?,?,?,?,?,?,?,CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE node_protocol_version=VALUES(node_protocol_version),ir_schema_versions_json=VALUES(ir_schema_versions_json),compiler_version_min=VALUES(compiler_version_min),compiler_version_max=VALUES(compiler_version_max),manifest_hashes_json=VALUES(manifest_hashes_json),status=VALUES(status),heartbeat_at=CURRENT_TIMESTAMP(6)")
                .bind(&state.instance_id)
                .bind(capability)
                .bind(NODE_PROTOCOL_VERSION)
                .bind(json!(["4.0"]))
                .bind(COMPILER_VERSION)
                .bind(COMPILER_VERSION)
                .bind(&manifest_hashes)
                .bind(capability_status)
                .execute(state.repository.pool())
                .await;
        }
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM tenants")
            .fetch_all(state.repository.pool())
            .await
        {
            Ok(tenants) => {
                let status = &overall_status;
                for tenant in tenants {
                    let _=sqlx::query("INSERT INTO runtime_service_heartbeats(tenant_id,service_type,instance_id,status,detail_json,heartbeat_at) VALUES(?,'worker',?,?,?,CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE status=VALUES(status),detail_json=VALUES(detail_json),heartbeat_at=CURRENT_TIMESTAMP(6)").bind(tenant).bind(&state.instance_id).bind(status).bind(json!({"capabilities":state.capabilities})).execute(state.repository.pool()).await;
                }
            }
            Err(error) => warn!(%error,"worker heartbeat tenant query failed"),
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspended_contract_times_use_rfc3339_strings() {
        let value = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        assert_eq!(
            format_optional_time(Some(value)).unwrap(),
            Some("2023-11-14T22:13:20Z".into())
        );
    }

    #[test]
    fn loop_processes_every_batch_before_done() {
        let task = RuntimeTask {
            tenant_id: Uuid::nil(),
            workflow_id: Uuid::nil(),
            workflow_version_id: None,
            workflow_service_identity_id: Uuid::nil(),
            execution_id: Uuid::nil(),
            node_execution_id: Uuid::nil(),
            attempt_id: Uuid::nil(),
            attempt_number: 1,
            node_type: "loop_over_items".into(),
            node_version: 1,
            node_parameters: json!({"batchSize":1}),
            workflow_inputs: json!({}),
            contexts: json!({}),
            context_version: 0,
            inputs: BTreeMap::new(),
            run_index: 0,
            iteration_index: 0,
            capability: "builtin".into(),
            idempotency_key: "test".into(),
            deadline: OffsetDateTime::now_utc() + time::Duration::minutes(1),
            mode: "manual".into(),
            trace_id: Uuid::nil(),
            linked_nodes: json!({}),
            resource_references: vec![],
            resource_snapshots: vec![],
        };
        let mut batch = vec![
            Item {
                json: json!({"value":1}),
                ..Item::default()
            },
            Item {
                json: json!({"value":2}),
                ..Item::default()
            },
            Item {
                json: json!({"value":3}),
                ..Item::default()
            },
        ];
        for expected in [1, 2, 3] {
            let TaskResult::Completed(mut outputs) = execute_loop(&task, batch).unwrap() else {
                panic!("loop result")
            };
            batch = outputs.remove("loop").expect("loop batch");
            assert_eq!(batch[0].json["value"], expected);
        }
        let TaskResult::Completed(mut outputs) = execute_loop(&task, batch).unwrap() else {
            panic!("done result")
        };
        assert_eq!(outputs.remove("done").expect("done items").len(), 3);
    }

    #[test]
    fn error_handler_recovers_or_rethrows_the_original_error() {
        let items = vec![Item {
            json: json!({"error":{"code":"MODEL_FAILED","message":"model unavailable"},"sourceNode":"agent"}),
            ..Item::default()
        }];
        let TaskResult::Completed(mut outputs) = execute_error_handler(items.clone(), "recover")
        else {
            panic!("recover result")
        };
        assert_eq!(outputs.remove("recovered").unwrap(), items);
        assert!(matches!(
            execute_error_handler(items, "fail"),
            TaskResult::Failed { code, message, retryable: false }
                if code == "MODEL_FAILED" && message == "model unavailable"
        ));
    }
}
