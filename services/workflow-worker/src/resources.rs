use std::{collections::BTreeMap, sync::Arc};

use agentx_application::{
    ArtifactStore, ArtifactWrite, McpToolRequest, McpToolRuntime, MemoryOperation, MemoryRequest,
    MemoryRuntime, ModelRequest, ModelRuntime, RagOperation, RagRequest, RagRuntime,
    RuntimeContext, RuntimeCredentialHandle, RuntimeError, RuntimeResult, SandboxCommand,
    SandboxCreateRequest, SandboxCredentialHandle, SandboxEvent, SandboxRuntime, SkillRuntime,
    TraceSink,
};
use agentx_domain::{
    ArtifactId, AttemptId, ExecutionId, NodeExecutionId, ResourceReference, ResourceType, TenantId,
    TraceId, WorkflowId, WorkflowServiceIdentityId, WorkflowVersionId,
};
use agentx_infrastructure::operations_projection::MySqlOperationsProjection;
use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use agentx_node_protocol::{BinaryReference, Item};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct ResourceRuntimes {
    pub model: Arc<dyn ModelRuntime>,
    pub mcp: Arc<dyn McpToolRuntime>,
    pub skill: Arc<dyn SkillRuntime>,
    pub rag: Arc<dyn RagRuntime>,
    pub memory: Arc<dyn MemoryRuntime>,
    pub sandbox: Option<Arc<dyn SandboxRuntime>>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub trace: MySqlOperationsProjection,
}

impl ResourceRuntimes {
    pub async fn execute(
        &self,
        task: &RuntimeTask,
        worker_lease: Uuid,
        credential_handles: BTreeMap<Uuid, RuntimeCredentialHandle>,
        cancellation: CancellationToken,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let context = runtime_context(task, worker_lease, credential_handles, cancellation);
        let result = match task.capability.as_str() {
            "model" => self.execute_model(task, &context, parameters).await,
            "mcp_tool" => self.execute_mcp(task, &context, parameters).await,
            "skill" => self.execute_skill(task, &context).await,
            "rag" => self.execute_rag(task, &context, parameters).await,
            "memory" => self.execute_memory(task, &context, parameters).await,
            "sandbox" => self.execute_code(task, &context, parameters).await,
            _ => Err(RuntimeError::new(
                "WORKER_CAPABILITY_UNSUPPORTED",
                "This Worker does not support the requested resource capability",
            )),
        };
        self.emit_trace(task, &result).await;
        result
    }

    async fn emit_trace(&self, task: &RuntimeTask, result: &RuntimeResult<TaskResult>) {
        let resource = task.resource_references.iter().find(|value| {
            value.resource_type.as_str()
                == match task.capability.as_str() {
                    "model" => "model",
                    "mcp_tool" => "mcp_tool",
                    "rag" => "rag",
                    "memory" => "memory",
                    "skill" => "skill",
                    "sandbox" => "sandbox_profile",
                    _ => "",
                }
        });
        let (
            status,
            error_code,
            error_message,
            attributes,
            sandbox_id,
            input_tokens,
            output_tokens,
            cost,
            partial,
            content_ref,
        ) = match result {
            Ok(TaskResult::Completed(outputs)) => {
                let item = outputs.values().flatten().next();
                let value = item.map(|item| item.json.clone()).unwrap_or(Value::Null);
                let artifact_refs = item
                    .into_iter()
                    .flat_map(|item| item.binary.values())
                    .map(|value| value.artifact_handle.clone())
                    .collect::<Vec<_>>();
                let mut attributes = value.clone();
                if !artifact_refs.is_empty() {
                    if let Value::Object(values) = &mut attributes {
                        values.insert("artifactRefs".into(), json!(artifact_refs.clone()));
                    }
                }
                (
                    "succeeded",
                    None,
                    None,
                    attributes,
                    value
                        .get("sandboxId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    value.pointer("/usage/inputTokens").and_then(Value::as_u64),
                    value.pointer("/usage/outputTokens").and_then(Value::as_u64),
                    value
                        .pointer("/usage/costMicros")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    value
                        .get("partial")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    artifact_refs
                        .first()
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .map(ArtifactId::from_uuid),
                )
            }
            Ok(TaskResult::Failed { code, message, .. }) => (
                "failed",
                Some(code.clone()),
                Some(message.clone()),
                json!({}),
                None,
                None,
                None,
                0,
                false,
                None,
            ),
            Ok(TaskResult::Suspended(_)) => (
                "suspended",
                None,
                None,
                json!({}),
                None,
                None,
                None,
                0,
                false,
                None,
            ),
            Err(error) => (
                "failed",
                Some(error.code.clone()),
                Some(error.message.clone()),
                error.attributes.clone(),
                error
                    .attributes
                    .get("sandboxId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                None,
                None,
                0,
                error.partial,
                error
                    .attributes
                    .get("artifactRefs")
                    .and_then(Value::as_array)
                    .and_then(|values| values.first())
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .map(ArtifactId::from_uuid),
            ),
        };
        let event_type = match task.capability.as_str() {
            "model" => "model.call",
            "mcp_tool" => "mcp.tool",
            "rag" => "rag.call",
            "memory" => "memory.call",
            "skill" => "skill.load",
            "sandbox" => "sandbox.command",
            _ => "runtime.call",
        };
        let event = agentx_domain::TraceEvent {
            event_id: Uuid::now_v7(),
            tenant_id: TenantId::from_uuid(task.tenant_id),
            trace_id: TraceId::from_uuid(task.trace_id),
            span_id: Uuid::now_v7(),
            parent_span_id: None,
            execution_id: ExecutionId::from_uuid(task.execution_id),
            workflow_id: WorkflowId::from_uuid(task.workflow_id),
            workflow_version_id: task.workflow_version_id.map(WorkflowVersionId::from_uuid),
            node_execution_id: Some(NodeExecutionId::from_uuid(task.node_execution_id)),
            attempt_id: Some(AttemptId::from_uuid(task.attempt_id)),
            agent_run_id: None,
            runtime_call_id: None,
            sandbox_id,
            resource_type: resource.map(|value| value.resource_type.as_str().to_owned()),
            resource_id: resource.map(|value| value.resource_id),
            resource_version_id: resource.and_then(|value| value.resource_version_id),
            event_type: event_type.into(),
            status: status.into(),
            event_time: time::OffsetDateTime::now_utc(),
            run_index: task.run_index,
            iteration_index: task.iteration_index,
            duration_ms: None,
            model_name: None,
            provider_name: None,
            mcp_tool_name: None,
            input_tokens,
            output_tokens,
            cost_micros: cost,
            error_code,
            error_message,
            stop_reason: None,
            partial,
            content_ref,
            attributes,
        };
        let _ = self.trace.append(event).await;
    }

    async fn execute_model(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let resource = reference(task, ResourceType::Model)?;
        let messages = messages(&parameters, task);
        let response = self
            .model
            .complete(
                context,
                ModelRequest {
                    resource,
                    messages,
                    tools: Vec::new(),
                    parameters: json!({}),
                },
            )
            .await?;
        let text = response
            .message
            .get("content")
            .and_then(Value::as_str)
            .or_else(|| response.message.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| response.message.to_string());
        let structured_json = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        Ok(completed(item(
            task,
            json!({"text":text,"message":response.message,"structuredJson":structured_json,"citations":[],"toolCalls":response.tool_calls,"usage":{"inputTokens":response.input_tokens,"outputTokens":response.output_tokens,"costMicros":response.cost_micros,"estimated":response.usage_estimated},"finishReason":response.stop_reason,"stopReason":response.stop_reason,"partial":response.partial}),
        )))
    }

    async fn execute_mcp(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let resource = reference(task, ResourceType::McpTool)?;
        let arguments = parameters
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| first_input(task));
        let response = self
            .mcp
            .call(
                context,
                McpToolRequest {
                    resource,
                    arguments,
                },
            )
            .await?;
        Ok(completed(item(
            task,
            json!({"content":response.content,"textContent":response.content,"structuredContent":response.structured_content,"isError":response.is_error}),
        )))
    }

    async fn execute_skill(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
    ) -> RuntimeResult<TaskResult> {
        let resource = reference(task, ResourceType::Skill)?;
        let bundle = self.skill.load(context, resource).await?;
        Ok(completed(item(
            task,
            json!({"instructions":bundle.instructions,"files":bundle.files,"dependencies":bundle.dependencies}),
        )))
    }

    async fn execute_rag(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let resource = reference(task, ResourceType::Rag)?;
        let operation = match parameters
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("query")
        {
            "query" => RagOperation::Query,
            "retrieve" => RagOperation::Retrieve,
            "insert" => RagOperation::Insert,
            "delete" => RagOperation::Delete,
            "health_check" => RagOperation::HealthCheck,
            _ => {
                return Err(RuntimeError::new(
                    "RAG_OPERATION_INVALID",
                    "RAG operation is invalid",
                ));
            }
        };
        let mut value = self
            .rag
            .execute(
                context,
                RagRequest {
                    resource,
                    operation,
                    input: parameters
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| first_input(task)),
                },
            )
            .await?;
        if let Some(result) = value.as_object_mut() {
            result.entry("documents").or_insert_with(|| json!([]));
            result.entry("chunks").or_insert_with(|| json!([]));
            result.entry("citations").or_insert_with(|| json!([]));
            result.entry("recordIds").or_insert_with(|| json!([]));
        }
        Ok(completed(item(task, value)))
    }

    async fn execute_memory(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let resource = reference(task, ResourceType::Memory)?;
        let operation = match parameters
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("search")
        {
            "get" => MemoryOperation::Get,
            "search" => MemoryOperation::Search,
            "add" => MemoryOperation::Add,
            "update" => MemoryOperation::Update,
            "delete" => MemoryOperation::Delete,
            _ => {
                return Err(RuntimeError::new(
                    "MEMORY_OPERATION_INVALID",
                    "Memory operation is invalid",
                ));
            }
        };
        let mut value = self
            .memory
            .execute(
                context,
                MemoryRequest {
                    resource,
                    operation,
                    input: parameters
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| first_input(task)),
                },
            )
            .await?;
        if let Some(result) = value.as_object_mut() {
            result.entry("records").or_insert_with(|| json!([]));
            result.entry("recordIds").or_insert_with(|| json!([]));
        }
        Ok(completed(item(task, value)))
    }

    async fn execute_code(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let sandbox = self.sandbox.as_ref().ok_or_else(|| {
            RuntimeError::new(
                "SANDBOX_RUNTIME_UNAVAILABLE",
                "Sandbox Manager is not configured for this Worker",
            )
        })?;
        let profile = task
            .resource_snapshots
            .iter()
            .find(|value| value.reference.resource_type == ResourceType::SandboxProfile)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    "SANDBOX_PROFILE_MISSING",
                    "Code node has no Sandbox Profile snapshot",
                )
            })?;
        let runner = parameters
            .get("runner")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RuntimeError::new("SANDBOX_RUNNER_INVALID", "Code runner is required")
            })?;
        if profile.snapshot.get("runner").and_then(Value::as_str) != Some(runner) {
            return Err(RuntimeError::new(
                "SANDBOX_PROFILE_RUNNER_MISMATCH",
                "Code runner does not match the Sandbox Profile",
            ));
        }
        let source = parameters
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RuntimeError::new("SANDBOX_SOURCE_INVALID", "Code source is required")
            })?;
        let (workspace, path, program) = match runner {
            "python" => ("/workspace", "/workspace/main.py", "python3"),
            "javascript" => ("/workspace", "/workspace/main.js", "node"),
            "shell" => ("/workspace", "/workspace/main.sh", "/bin/sh"),
            // The pinned OpenSandbox Playwright image ships Python Playwright and
            // Chromium under its non-root /home/playwright workspace.
            "browser" => ("/home/playwright", "/home/playwright/browser.py", "python3"),
            _ => {
                return Err(RuntimeError::new(
                    "SANDBOX_PROTOCOL_UNSUPPORTED",
                    "The requested Code runner is unsupported",
                ));
            }
        };
        let network_policy = parameters
            .get("networkPolicy")
            .cloned()
            .unwrap_or(Value::Null);
        let credential_names = parameters
            .get("credentialFiles")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let credentials = parameters
            .get("_credentialHandles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| {
                let resource_id = value
                    .get("resourceId")
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "SANDBOX_CREDENTIAL_HANDLE_INVALID",
                            "Credential Handle resource is invalid",
                        )
                    })?;
                let environment_name = credential_names
                    .iter()
                    .find_map(|(name, configured)| {
                        (configured.as_str() == Some(resource_id.to_string().as_str()))
                            .then(|| name.clone())
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "AGENTX_CREDENTIAL_{}_FILE",
                            resource_id.simple().to_string().to_ascii_uppercase()
                        )
                    });
                if !valid_environment_name(&environment_name) {
                    return Err(RuntimeError::new(
                        "SANDBOX_CREDENTIAL_ENVIRONMENT_INVALID",
                        "Credential file environment names must use uppercase letters, digits, and underscores",
                    ));
                }
                Ok(SandboxCredentialHandle {
                    resource_id,
                    handle: value
                        .get("handle")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "SANDBOX_CREDENTIAL_HANDLE_INVALID",
                                "Credential Handle is missing",
                            )
                        })?
                        .to_owned(),
                    environment_name,
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        let lease = sandbox
            .create(
                context,
                SandboxCreateRequest {
                    profile: profile.clone(),
                    labels: json!({"agentx-node-type":"code"}),
                    network_policy,
                    credentials,
                },
            )
            .await?;
        let result = async {
            sandbox
                .upload(context, &lease, path, source.as_bytes().to_vec())
                .await?;
            let mut argv = vec![program.to_owned(), path.to_owned()];
            if let Some(arguments) = parameters.get("arguments").and_then(Value::as_array) {
                for argument in arguments {
                    argv.push(
                        argument
                            .as_str()
                            .ok_or_else(|| {
                                RuntimeError::new(
                                    "SANDBOX_COMMAND_INVALID",
                                    "Code arguments must be strings",
                                )
                            })?
                            .to_owned(),
                    );
                }
            }
            let mut stream = sandbox
                .execute(
                    context,
                    SandboxCommand {
                        lease: lease.clone(),
                        argv,
                        environment: json!({}),
                        working_directory: workspace.into(),
                    },
                )
                .await?;
            let limit = profile
                .snapshot
                .get("outputLimitBytes")
                .and_then(Value::as_u64)
                .unwrap_or(1_048_576) as usize;
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdout_full = Vec::new();
            let mut stderr_full = Vec::new();
            let mut exit_code = None;
            let mut partial = false;
            let mut stream_error = None;
            loop {
                let event = tokio::select! {
                    () = context.cancellation.cancelled() => {
                        partial = true;
                        let _ = sandbox.interrupt(context, &lease).await;
                        break;
                    }
                    event = stream.next() => event,
                };
                let Some(event) = event else { break };
                match event {
                    Ok(SandboxEvent::Stdout { data, .. }) => {
                        append_output(&mut stdout, &mut stdout_full, &data, limit, &mut partial)
                    }
                    Ok(SandboxEvent::Stderr { data, .. }) => {
                        append_output(&mut stderr, &mut stderr_full, &data, limit, &mut partial)
                    }
                    Ok(SandboxEvent::Completed {
                        exit_code: value,
                        partial: value_partial,
                    }) => {
                        exit_code = Some(value);
                        partial |= value_partial;
                    }
                    Err(error) => {
                        partial |= error.partial;
                        stream_error = Some(error);
                        break;
                    }
                }
            }
            let mut binary = BTreeMap::new();
            if stdout_full.len() > limit {
                let artifact = self
                    .artifacts
                    .put(ArtifactWrite {
                        tenant_id: context.tenant_id,
                        content_type: "text/plain; charset=utf-8".into(),
                        content: stdout_full,
                    })
                    .await
                    .map_err(|error| {
                        RuntimeError::new("ARTIFACT_WRITE_FAILED", error.to_string())
                            .retryable(true)
                    })?;
                binary.insert(
                    "stdout".into(),
                    BinaryReference {
                        artifact_handle: artifact.id.to_string(),
                        file_name: Some("stdout.txt".into()),
                        content_type: Some(artifact.content_type),
                        size_bytes: artifact.content.len() as u64,
                    },
                );
            }
            if stderr_full.len() > limit {
                let artifact = self
                    .artifacts
                    .put(ArtifactWrite {
                        tenant_id: context.tenant_id,
                        content_type: "text/plain; charset=utf-8".into(),
                        content: stderr_full,
                    })
                    .await
                    .map_err(|error| {
                        RuntimeError::new("ARTIFACT_WRITE_FAILED", error.to_string())
                            .retryable(true)
                    })?;
                binary.insert(
                    "stderr".into(),
                    BinaryReference {
                        artifact_handle: artifact.id.to_string(),
                        file_name: Some("stderr.txt".into()),
                        content_type: Some(artifact.content_type),
                        size_bytes: artifact.content.len() as u64,
                    },
                );
            }
            if stream_error.is_none()
                && let Some(paths) = parameters.get("outputPaths").and_then(Value::as_array)
            {
                for (index, path) in paths.iter().enumerate() {
                    let path = path.as_str().ok_or_else(|| {
                        RuntimeError::new("SANDBOX_PATH_FORBIDDEN", "Output path must be a string")
                    })?;
                    validate_download_target(sandbox.as_ref(), context, &lease, workspace, path)
                        .await?;
                    let content = sandbox.download(context, &lease, path).await?;
                    let artifact = self
                        .artifacts
                        .put(ArtifactWrite {
                            tenant_id: context.tenant_id,
                            content_type: "application/octet-stream".into(),
                            content,
                        })
                        .await
                        .map_err(|error| {
                            RuntimeError::new("ARTIFACT_WRITE_FAILED", error.to_string())
                                .retryable(true)
                        })?;
                    binary.insert(
                        format!("output{index}"),
                        BinaryReference {
                            artifact_handle: artifact.id.to_string(),
                            file_name: path.rsplit('/').next().map(str::to_owned),
                            content_type: Some(artifact.content_type),
                            size_bytes: artifact.content.len() as u64,
                        },
                    );
                }
            }
            Ok::<_, RuntimeError>((
                stdout,
                stderr,
                exit_code.unwrap_or(1),
                partial,
                binary,
                stream_error,
            ))
        }
        .await;
        let cleanup = sandbox.terminate(context, &lease).await;
        if let Err(error) = cleanup {
            return Err(
                RuntimeError::new("SANDBOX_CLEANUP_PENDING", error.to_string())
                    .retryable(true)
                    .outcome_unknown(true),
            );
        }
        let (stdout, stderr, exit_code, partial, binary, stream_error) = result?;
        if let Some(mut error) = stream_error {
            error.partial |= partial;
            error.attributes = command_error_attributes(
                &stdout,
                &stderr,
                exit_code,
                partial,
                &lease.sandbox_id,
                &binary,
            );
            return Err(error);
        }
        if exit_code != 0 {
            let mut error = RuntimeError::new(
                "SANDBOX_COMMAND_FAILED",
                format!("Sandbox command exited with code {exit_code}"),
            )
            .partial(partial);
            error.attributes = command_error_attributes(
                &stdout,
                &stderr,
                exit_code,
                partial,
                &lease.sandbox_id,
                &binary,
            );
            return Err(error);
        }
        let stdout_text = String::from_utf8_lossy(&stdout).into_owned();
        let structured_outputs = serde_json::from_str::<Value>(&stdout_text)
            .ok()
            .and_then(|value| value.get("outputs").cloned())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let downloaded_artifacts = binary
            .iter()
            .map(|(name, value)| json!({"name":name,"artifactId":value.artifact_handle,"fileName":value.file_name,"contentType":value.content_type,"sizeBytes":value.size_bytes}))
            .collect::<Vec<_>>();
        let mut output = item(
            task,
            json!({"stdout":stdout_text,"stderr":String::from_utf8_lossy(&stderr),"exitCode":exit_code,"partial":partial,"sandboxId":lease.sandbox_id,"downloadedArtifacts":downloaded_artifacts,"structuredOutputs":structured_outputs}),
        );
        output.binary = binary;
        Ok(completed(output))
    }
}

pub fn runtime_context(
    task: &RuntimeTask,
    worker_lease: Uuid,
    credential_handles: BTreeMap<Uuid, RuntimeCredentialHandle>,
    cancellation: CancellationToken,
) -> RuntimeContext {
    RuntimeContext {
        tenant_id: TenantId::from_uuid(task.tenant_id),
        workflow_service_identity_id: WorkflowServiceIdentityId::from_uuid(
            task.workflow_service_identity_id,
        ),
        workflow_id: WorkflowId::from_uuid(task.workflow_id),
        workflow_version_id: task.workflow_version_id.map(WorkflowVersionId::from_uuid),
        execution_id: ExecutionId::from_uuid(task.execution_id),
        node_execution_id: NodeExecutionId::from_uuid(task.node_execution_id),
        attempt_id: AttemptId::from_uuid(task.attempt_id),
        lease_token: worker_lease,
        trace_id: TraceId::from_uuid(task.trace_id),
        span_id: Uuid::now_v7(),
        deadline: task.deadline,
        cancellation,
        idempotency_key: task.idempotency_key.clone(),
        resources: task.resource_snapshots.clone(),
        credential_handles,
    }
}
pub fn reference(task: &RuntimeTask, kind: ResourceType) -> RuntimeResult<ResourceReference> {
    task.resource_references
        .iter()
        .find(|value| value.resource_type == kind)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(
                "RESOURCE_REFERENCE_MISSING",
                format!("{} resource reference is missing", kind.as_str()),
            )
        })
}
fn messages(parameters: &Value, task: &RuntimeTask) -> Vec<Value> {
    let mut messages = configured_model_messages(parameters);
    if messages.is_empty() {
        messages.push(json!({"role":"user","content":first_input(task)}));
    }
    messages
}

fn configured_model_messages(parameters: &Value) -> Vec<Value> {
    let mut messages = Vec::new();
    if let Some(prompt) = parameters
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        messages.push(json!({"role":"user","content":prompt}));
    }
    if let Some(question) = parameters
        .get("userQuestion")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|question| !question.is_empty())
    {
        messages.push(json!({"role":"user","content":question}));
    }
    messages
}
pub fn first_input(task: &RuntimeTask) -> Value {
    task.inputs
        .values()
        .flatten()
        .next()
        .map(|item| item.json.clone())
        .unwrap_or(Value::Null)
}
fn item(task: &RuntimeTask, json: Value) -> Item {
    let mut value = task
        .inputs
        .values()
        .flatten()
        .next()
        .cloned()
        .unwrap_or_default();
    value.json = json;
    value
}
fn completed(item: Item) -> TaskResult {
    TaskResult::Completed(BTreeMap::from([("main".into(), vec![item])]))
}
fn append_limited(target: &mut Vec<u8>, data: &[u8], limit: usize, partial: &mut bool) {
    let remaining = limit.saturating_sub(target.len());
    target.extend_from_slice(&data[..data.len().min(remaining)]);
    if data.len() > remaining {
        *partial = true;
    }
}

fn append_output(
    visible: &mut Vec<u8>,
    full: &mut Vec<u8>,
    data: &[u8],
    limit: usize,
    partial: &mut bool,
) {
    full.extend_from_slice(data);
    append_limited(visible, data, limit, partial);
}

fn command_error_attributes(
    stdout: &[u8],
    stderr: &[u8],
    exit_code: i32,
    partial: bool,
    sandbox_id: &str,
    binary: &BTreeMap<String, BinaryReference>,
) -> Value {
    let artifact_refs = binary
        .values()
        .map(|value| value.artifact_handle.clone())
        .collect::<Vec<_>>();
    json!({
        "stdout": String::from_utf8_lossy(stdout),
        "stderr": String::from_utf8_lossy(stderr),
        "exitCode": exit_code,
        "partial": partial,
        "sandboxId": sandbox_id,
        "artifactRefs": artifact_refs,
    })
}

async fn validate_download_target(
    sandbox: &dyn SandboxRuntime,
    context: &RuntimeContext,
    lease: &agentx_application::SandboxLease,
    workspace: &str,
    path: &str,
) -> RuntimeResult<()> {
    let mut stream = sandbox
        .execute(
            context,
            SandboxCommand {
                lease: lease.clone(),
                argv: vec!["readlink".into(), "-f".into(), "--".into(), path.into()],
                environment: json!({}),
                working_directory: workspace.into(),
            },
        )
        .await?;
    let mut canonical = Vec::new();
    let mut exit_code = None;
    while let Some(event) = stream.next().await {
        match event? {
            SandboxEvent::Stdout { data, .. } if canonical.len() + data.len() <= 4096 => {
                canonical.extend(data)
            }
            SandboxEvent::Stdout { .. } => {
                return Err(RuntimeError::new(
                    "SANDBOX_PATH_FORBIDDEN",
                    "Resolved output path is too long",
                ));
            }
            SandboxEvent::Stderr { .. } => {}
            SandboxEvent::Completed {
                exit_code: value,
                partial,
            } => {
                if partial {
                    return Err(RuntimeError::new(
                        "SANDBOX_PATH_FORBIDDEN",
                        "Output path validation was interrupted",
                    )
                    .partial(true));
                }
                exit_code = Some(value);
            }
        }
    }
    let canonical = std::str::from_utf8(&canonical).unwrap_or_default().trim();
    if exit_code != Some(0) || !path_is_within_workspace(canonical, workspace) {
        return Err(RuntimeError::new(
            "SANDBOX_PATH_FORBIDDEN",
            "Output path resolves outside the Sandbox workspace",
        ));
    }
    Ok(())
}

fn path_is_within_workspace(path: &str, workspace: &str) -> bool {
    path == workspace
        || path
            .strip_prefix(workspace)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn valid_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_uppercase())
        && chars.all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

pub fn failed(error: RuntimeError) -> TaskResult {
    TaskResult::Failed {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_output_marks_partial() {
        let mut target = Vec::new();
        let mut partial = false;
        append_limited(&mut target, b"abcdef", 3, &mut partial);
        assert_eq!(target, b"abc");
        assert!(partial);
    }

    #[test]
    fn bounded_output_keeps_full_artifact_content() {
        let mut visible = Vec::new();
        let mut full = Vec::new();
        let mut partial = false;
        append_output(&mut visible, &mut full, b"abcdef", 3, &mut partial);
        assert_eq!(visible, b"abc");
        assert_eq!(full, b"abcdef");
        assert!(partial);
    }

    #[test]
    fn model_prompt_and_user_question_become_ordered_user_messages() {
        assert_eq!(
            configured_model_messages(&json!({
                "prompt": "Summarize the input",
                "userQuestion": "First question"
            })),
            vec![
                json!({"role":"user","content":"Summarize the input"}),
                json!({"role":"user","content":"First question"}),
            ]
        );
    }

    #[test]
    fn download_paths_must_resolve_inside_the_selected_workspace() {
        assert!(path_is_within_workspace(
            "/home/playwright/browser.png",
            "/home/playwright"
        ));
        assert!(path_is_within_workspace("/workspace", "/workspace"));
        assert!(!path_is_within_workspace(
            "/home/playwright-escape/output.txt",
            "/home/playwright"
        ));
        assert!(!path_is_within_workspace(
            "/workspace-escape/output.txt",
            "/workspace"
        ));
    }
}
