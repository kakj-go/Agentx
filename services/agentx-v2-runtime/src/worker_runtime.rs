use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use agentx_domain::{ExecutionId, NodeExecutionId, TenantId};
use agentx_node_protocol::{
    ExecutionMode, GroupedInput, Item, NODE_PROTOCOL_VERSION, NodeActionRequest, NodeActionResult,
    NodeCapability, ResolvedParameters, TraceContext,
};
use agentx_runtime_contracts::{
    ContentHash, RuntimeObjectReferenceV1, RuntimeResourceBindingV1,
    RuntimeResourceConfigurationV1, RuntimeResourceKindV1, RuntimeSkillProgramV1, StorageDomain,
    WorkerResultStatusV1, WorkerResultV1,
};
use bytes::Bytes;
use object_store::{ObjectStore, path::Path as ObjectPath};
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use crate::{
    egress::{EgressRequestContext, ProviderHttpClient},
    engine::ClaimedWorkerAttempt,
    vault::RuntimeVault,
    worker_support::{
        openai_chat_completions_endpoint, provider_secret_header, raw_hash,
        runtime_call_fingerprint, runtime_call_span_name, stable_id,
    },
};

mod mcp;
#[path = "worker_runtime_output.rs"]
mod output;

use output::{
    effective_agent_budget, openai_chat_request, openai_execution_output, provider_usage,
    provider_usage_detail, runtime_call_is_replayable, sandbox_execution_output,
};

pub struct WorkerExecution {
    pub status: WorkerResultStatusV1,
    pub outputs: BTreeMap<String, Vec<Item>>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorkerProviderResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone, Debug)]
pub enum WorkerProviderError {
    Denied(String),
    Request { message: String, is_connect: bool },
}

#[async_trait::async_trait]
pub trait WorkerProvider: Send + Sync {
    async fn post_json(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError>;

    async fn post_sandbox_manager_json(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        self.post_json(endpoint, context, timeout, headers, body)
            .await
    }
}

#[async_trait::async_trait]
impl WorkerProvider for ProviderHttpClient {
    async fn post_json(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let mut request = self
            .post(endpoint, context, timeout)
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        let response =
            request
                .json(body)
                .send()
                .await
                .map_err(|error| WorkerProviderError::Request {
                    message: error.to_string(),
                    is_connect: error.is_connect(),
                })?;
        Ok(WorkerProviderResponse {
            status: response.status(),
            headers: response.headers().clone(),
            body: response
                .bytes()
                .await
                .map_err(|error| WorkerProviderError::Request {
                    message: error.to_string(),
                    is_connect: error.is_connect(),
                })?,
        })
    }

    async fn post_sandbox_manager_json(
        &self,
        endpoint: &str,
        _context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let mut request = self
            .post_sandbox_manager(endpoint, timeout)
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        let response =
            request
                .json(body)
                .send()
                .await
                .map_err(|error| WorkerProviderError::Request {
                    message: error.to_string(),
                    is_connect: error.is_connect(),
                })?;
        Ok(WorkerProviderResponse {
            status: response.status(),
            headers: response.headers().clone(),
            body: response
                .bytes()
                .await
                .map_err(|error| WorkerProviderError::Request {
                    message: error.to_string(),
                    is_connect: error.is_connect(),
                })?,
        })
    }
}

#[derive(Clone, Copy)]
enum RuntimeHttpTransport {
    Provider,
    SandboxManager,
}

impl WorkerExecution {
    fn succeeded(value: Value) -> Self {
        Self {
            status: WorkerResultStatusV1::Succeeded,
            outputs: BTreeMap::from([(
                "main".into(),
                vec![Item {
                    json: value,
                    ..Item::default()
                }],
            )]),
            error_code: None,
            error_message: None,
        }
    }

    fn failed(code: &str, message: impl Into<String>, outcome_unknown: bool) -> Self {
        Self {
            status: if outcome_unknown {
                WorkerResultStatusV1::OutcomeUnknown
            } else {
                WorkerResultStatusV1::Failed
            },
            outputs: BTreeMap::new(),
            error_code: Some(code.into()),
            error_message: Some(message.into()),
        }
    }

    fn suspended(resume: Value) -> Self {
        Self {
            status: WorkerResultStatusV1::Suspended,
            outputs: BTreeMap::from([(
                "main".into(),
                vec![Item {
                    json: resume,
                    ..Item::default()
                }],
            )]),
            error_code: None,
            error_message: None,
        }
    }
}

pub struct RuntimeWorker {
    pool: MySqlPool,
    provider: Arc<dyn WorkerProvider>,
    vault: Option<RuntimeVault>,
    objects: Arc<dyn ObjectStore>,
}

impl RuntimeWorker {
    pub fn new(pool: MySqlPool, objects: Arc<dyn ObjectStore>) -> anyhow::Result<Self> {
        Ok(Self {
            pool,
            provider: Arc::new(ProviderHttpClient::from_env(
                agentx_runtime_contracts::EgressRole::WorkflowWorker,
            )?),
            vault: RuntimeVault::from_env().ok(),
            objects,
        })
    }

    #[doc(hidden)]
    #[must_use]
    pub fn new_with_provider(
        pool: MySqlPool,
        objects: Arc<dyn ObjectStore>,
        provider: Arc<dyn WorkerProvider>,
    ) -> Self {
        Self {
            pool,
            provider,
            vault: RuntimeVault::from_env().ok(),
            objects,
        }
    }

    pub async fn execute(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        match claim.task.capability {
            NodeCapability::Builtin => self.execute_builtin(claim),
            NodeCapability::DeclarativeHttp => self.execute_declarative_http(claim).await,
            NodeCapability::RemoteAction => self.execute_remote_action(claim).await,
            NodeCapability::Agent => self.execute_agent(claim).await,
            NodeCapability::Model
            | NodeCapability::McpTool
            | NodeCapability::Rag
            | NodeCapability::Memory
            | NodeCapability::Skill
            | NodeCapability::Sandbox => self.execute_resource(claim).await,
        }
    }

    #[must_use]
    pub fn object_store(&self) -> Arc<dyn ObjectStore> {
        self.objects.clone()
    }

    pub async fn build_result(
        &self,
        claim: &ClaimedWorkerAttempt,
        execution: WorkerExecution,
    ) -> anyhow::Result<WorkerResultV1> {
        crate::trace_artifact::externalize_attempt_input(&self.pool, &self.objects, claim).await;
        let encoded = agentx_runtime_contracts::canonical_bytes(&execution.outputs)?;
        let (outputs, output_object) =
            if encoded.len() > agentx_runtime_contracts::INLINE_RESULT_LIMIT_BYTES as usize {
                (
                    BTreeMap::new(),
                    Some(self.persist_attempt_output(claim, encoded).await?),
                )
            } else {
                (execution.outputs, None)
            };
        let result_hash = crate::engine::worker_result_hash(
            execution.status,
            &outputs,
            output_object.as_ref(),
            execution.error_code.as_deref(),
            execution.error_message.as_deref(),
            None,
        )?;
        Ok(WorkerResultV1 {
            protocol_version: 1,
            attempt_id: claim.task.attempt_id,
            worker_id: claim.lease.worker_id,
            fencing_token: claim.lease.fencing_token,
            status: execution.status,
            result_hash,
            outputs,
            output_object,
            error_code: execution.error_code,
            error_message: execution.error_message,
            partial_output_object: None,
        })
    }

    async fn persist_attempt_output(
        &self,
        claim: &ClaimedWorkerAttempt,
        encoded: Vec<u8>,
    ) -> anyhow::Result<RuntimeObjectReferenceV1> {
        let object_id = stable_id(claim.task.attempt_id, b"worker-result-object");
        let content_hash =
            ContentHash::parse(format!("sha256:{:x}", Sha256::digest(encoded.as_slice())))?;
        let object_key =
            RuntimeObjectReferenceV1::canonical_key(claim.task.tenant_id, object_id, &content_hash);
        if let Some(row) = sqlx::query(
            "SELECT content_hash,size_bytes,media_type,status FROM runtime_objects WHERE tenant_id=? AND object_id=?",
        )
        .bind(claim.task.tenant_id)
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await?
        {
            anyhow::ensure!(
                row.try_get::<String, _>("content_hash")? == content_hash.as_str()
                    && row.try_get::<u64, _>("size_bytes")? == encoded.len() as u64
                    && row.try_get::<String, _>("media_type")? == "application/json"
                    && row.try_get::<String, _>("status")? == "ready",
                "attempt output object identity conflicts with existing content"
            );
            let artifact = RuntimeObjectReferenceV1 {
                tenant_id: claim.task.tenant_id,
                storage_domain: StorageDomain::Runtime,
                object_id,
                object_key,
                content_hash,
                size_bytes: encoded.len() as u64,
                media_type: "application/json".into(),
            };
            crate::trace_artifact::register_artifact(
                &self.pool,
                &artifact,
                claim.task.execution_id,
                claim.task.node_execution_id,
                &format!("trace_output:{}", claim.task.attempt_id),
            )
            .await?;
            return Ok(artifact);
        }
        let path = ObjectPath::from(object_key.clone());
        self.objects
            .put(&path, bytes::Bytes::from(encoded.clone()).into())
            .await?;
        let request_hash = agentx_runtime_contracts::content_hash(&json!({
            "attemptId":claim.task.attempt_id,
            "contentHash":content_hash,
            "sizeBytes":encoded.len(),
        }))?;
        let inserted = sqlx::query(
            "INSERT INTO runtime_objects(object_id,tenant_id,object_key,content_hash,size_bytes,media_type,status,idempotency_key,request_hash,temporary_expires_at,ready_at) VALUES(?,?,?,?,?,'application/json','ready',?,?,UTC_TIMESTAMP(6),UTC_TIMESTAMP(6))",
        )
        .bind(object_id)
        .bind(claim.task.tenant_id)
        .bind(&object_key)
        .bind(content_hash.as_str())
        .bind(encoded.len() as u64)
        .bind(format!("worker-result:{}", claim.task.attempt_id))
        .bind(request_hash.as_str())
        .execute(&self.pool)
        .await;
        if let Err(error) = inserted {
            let _ = self.objects.delete(&path).await;
            return Err(error.into());
        }
        let artifact = RuntimeObjectReferenceV1 {
            tenant_id: claim.task.tenant_id,
            storage_domain: StorageDomain::Runtime,
            object_id,
            object_key,
            content_hash,
            size_bytes: encoded.len() as u64,
            media_type: "application/json".into(),
        };
        crate::trace_artifact::register_artifact(
            &self.pool,
            &artifact,
            claim.task.execution_id,
            claim.task.node_execution_id,
            &format!("trace_output:{}", claim.task.attempt_id),
        )
        .await?;
        Ok(artifact)
    }

    fn execute_builtin(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        let value = first_input(claim).unwrap_or(Value::Null);
        execute_builtin_node(&claim.node_type, &claim.node_parameters, value)
    }

    async fn execute_agent(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        let run_id = stable_id(claim.task.attempt_id, b"agent-run");
        let mut state = first_input(claim).unwrap_or(Value::Null);
        let before_hash = raw_hash(&state);
        let budget = effective_agent_budget(&claim.node_parameters);
        if let Err(error) = sqlx::query(
            "INSERT INTO agent_runs(id,tenant_id,execution_id,node_execution_id,attempt_id,status,budget_json,state_hash) VALUES(?,?,?,?,?,'running',?,?) ON DUPLICATE KEY UPDATE id=id",
        )
        .bind(run_id)
        .bind(claim.task.tenant_id)
        .bind(claim.task.execution_id)
        .bind(claim.task.node_execution_id)
        .bind(claim.task.attempt_id)
        .bind(&budget)
        .bind(&before_hash)
        .execute(&self.pool)
        .await
        {
            return WorkerExecution::failed("AGENT_STATE_UNAVAILABLE", error.to_string(), false);
        }
        self.emit_agent_span(
            run_id,
            None,
            agentx_runtime_contracts::TraceEventKindV1::Started,
            "running",
            None,
            Some(&state),
        )
        .await;
        let maximum_iterations = budget["maxIterations"].as_u64().unwrap_or(1) as u32;
        let maximum_tokens = budget["maxTokens"].as_u64().unwrap_or(4096);
        let maximum_cost = budget["maxCostMicros"].as_u64().unwrap_or(1_000_000);
        let Some(model) = claim
            .resources
            .iter()
            .find(|binding| binding.resource_kind == RuntimeResourceKindV1::Model)
        else {
            return self
                .finish_agent_failure(
                    run_id,
                    0,
                    0,
                    0,
                    &before_hash,
                    "model_binding_missing",
                    "AGENT_MODEL_BINDING_MISSING",
                )
                .await;
        };
        let tool = mcp_tool_binding(&claim.resources);
        let mut seen = BTreeSet::from([before_hash]);
        let mut tokens = 0_u64;
        let mut cost_micros = 0_u64;
        let mut tool_calls = 0_u32;
        for iteration in 0..maximum_iterations {
            let state_before_hash = raw_hash(&state);
            let iteration_id = stable_id(run_id, format!("iteration-{iteration}").as_bytes());
            if let Err(error) = sqlx::query(
                "INSERT INTO agent_iterations(id,tenant_id,agent_run_id,iteration_index,status,state_before_hash) VALUES(?,?,?,?,'running',?) ON DUPLICATE KEY UPDATE id=id",
            )
            .bind(iteration_id)
            .bind(claim.task.tenant_id)
            .bind(run_id)
            .bind(iteration)
            .bind(&state_before_hash)
            .execute(&self.pool)
            .await
            {
                return WorkerExecution::failed("AGENT_STATE_UNAVAILABLE", error.to_string(), false);
            }
            self.emit_agent_span(
                run_id,
                Some(iteration_id),
                agentx_runtime_contracts::TraceEventKindV1::Started,
                "running",
                None,
                Some(&state),
            )
            .await;
            let model_result = self
                .execute_provider_call(claim, model, state.clone(), iteration * 2)
                .await;
            let Some(model_output) = successful_value(&model_result) else {
                self.finish_iteration(
                    iteration_id,
                    "failed",
                    &state_before_hash,
                    "provider_failed",
                )
                .await;
                self.finish_agent(
                    run_id,
                    "failed",
                    iteration + 1,
                    tokens,
                    cost_micros,
                    tool_calls,
                    &state_before_hash,
                    "provider_failed",
                )
                .await;
                return model_result;
            };
            let usage = provider_usage(&model_output);
            tokens = tokens.saturating_add(usage.0);
            cost_micros = cost_micros.saturating_add(usage.1);
            if tokens > maximum_tokens || cost_micros > maximum_cost {
                self.finish_iteration(
                    iteration_id,
                    "failed",
                    &state_before_hash,
                    "budget_exceeded",
                )
                .await;
                return self
                    .finish_agent_failure(
                        run_id,
                        iteration + 1,
                        tokens,
                        cost_micros,
                        &state_before_hash,
                        "budget_exceeded",
                        "AGENT_BUDGET_EXCEEDED",
                    )
                    .await;
            }
            let tool_call = model_output.get("toolCall").cloned();
            state = if let Some(tool_input) = tool_call.clone() {
                let Some(tool) = tool else {
                    self.finish_iteration(
                        iteration_id,
                        "failed",
                        &state_before_hash,
                        "tool_binding_missing",
                    )
                    .await;
                    return self
                        .finish_agent_failure(
                            run_id,
                            iteration + 1,
                            tokens,
                            cost_micros,
                            &state_before_hash,
                            "tool_binding_missing",
                            "AGENT_TOOL_BINDING_MISSING",
                        )
                        .await;
                };
                let tool_result = self
                    .execute_provider_call(claim, tool, tool_input, iteration * 2 + 1)
                    .await;
                let Some(tool_output) = successful_value(&tool_result) else {
                    self.finish_iteration(
                        iteration_id,
                        "failed",
                        &state_before_hash,
                        "tool_failed",
                    )
                    .await;
                    self.finish_agent(
                        run_id,
                        "failed",
                        iteration + 1,
                        tokens,
                        cost_micros,
                        tool_calls,
                        &state_before_hash,
                        "tool_failed",
                    )
                    .await;
                    return tool_result;
                };
                tool_calls += 1;
                json!({"model":model_output,"tool":tool_output})
            } else {
                model_output.clone()
            };
            let state_after_hash = raw_hash(&state);
            let terminal = tool_call.is_none()
                || model_output.get("done").and_then(Value::as_bool) == Some(true);
            if terminal {
                self.finish_iteration(iteration_id, "completed", &state_after_hash, "completed")
                    .await;
                self.finish_agent(
                    run_id,
                    "succeeded",
                    iteration + 1,
                    tokens,
                    cost_micros,
                    tool_calls,
                    &state_after_hash,
                    "completed",
                )
                .await;
                return WorkerExecution::succeeded(state);
            }
            if !seen.insert(state_after_hash.clone()) {
                self.finish_iteration(iteration_id, "failed", &state_after_hash, "loop_detected")
                    .await;
                return self
                    .finish_agent_failure(
                        run_id,
                        iteration + 1,
                        tokens,
                        cost_micros,
                        &state_after_hash,
                        "loop_detected",
                        "AGENT_LOOP_DETECTED",
                    )
                    .await;
            }
            self.finish_iteration(iteration_id, "completed", &state_after_hash, "continued")
                .await;
        }
        let final_hash = raw_hash(&state);
        self.finish_agent_failure(
            run_id,
            maximum_iterations,
            tokens,
            cost_micros,
            &final_hash,
            "iteration_budget_exhausted",
            "AGENT_ITERATION_BUDGET_EXCEEDED",
        )
        .await
    }

    async fn finish_iteration(
        &self,
        iteration_id: Uuid,
        status: &str,
        state_hash: &str,
        stop_reason: &str,
    ) {
        let _ = sqlx::query(
            "UPDATE agent_iterations SET status=?,state_after_hash=?,stop_reason=?,ended_at=UTC_TIMESTAMP(6) WHERE id=?",
        )
        .bind(status)
        .bind(state_hash)
        .bind(stop_reason)
        .bind(iteration_id)
        .execute(&self.pool)
        .await;
        self.emit_agent_iteration_finish(iteration_id, status, stop_reason)
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_agent(
        &self,
        run_id: Uuid,
        status: &str,
        iterations: u32,
        tokens: u64,
        cost_micros: u64,
        tool_calls: u32,
        state_hash: &str,
        stop_reason: &str,
    ) {
        let _ = sqlx::query(
            "UPDATE agent_runs SET status=?,iteration_count=?,model_call_count=?,tool_call_count=?,input_tokens=?,cost_micros=?,state_hash=?,stop_reason=?,ended_at=UTC_TIMESTAMP(6) WHERE id=?",
        )
        .bind(status)
        .bind(iterations)
        .bind(iterations)
        .bind(tool_calls)
        .bind(tokens)
        .bind(cost_micros)
        .bind(state_hash)
        .bind(stop_reason)
        .bind(run_id)
        .execute(&self.pool)
        .await;
        self.emit_agent_span(
            run_id,
            None,
            agentx_runtime_contracts::TraceEventKindV1::Finished,
            status,
            (status == "failed").then_some(stop_reason),
            None,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_agent_failure(
        &self,
        run_id: Uuid,
        iterations: u32,
        tokens: u64,
        cost_micros: u64,
        state_hash: &str,
        stop_reason: &str,
        error_code: &str,
    ) -> WorkerExecution {
        self.finish_agent(
            run_id,
            "failed",
            iterations,
            tokens,
            cost_micros,
            0,
            state_hash,
            stop_reason,
        )
        .await;
        WorkerExecution::failed(error_code, stop_reason, false)
    }

    async fn execute_resource(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        self.execute_binding(claim, capability_resource_kind(&claim.task.capability))
            .await
    }

    async fn execute_binding(
        &self,
        claim: &ClaimedWorkerAttempt,
        expected_kind: RuntimeResourceKindV1,
    ) -> WorkerExecution {
        let Some(binding) = select_binding(claim, expected_kind) else {
            return WorkerExecution::failed(
                "RUNTIME_RESOURCE_BINDING_MISSING",
                "Node has no exact immutable Runtime Resource Binding",
                false,
            );
        };
        match &binding.configuration {
            RuntimeResourceConfigurationV1::Skill {
                entrypoint_object_id,
                dependency_object_ids,
            } => {
                self.execute_skill(claim, binding, *entrypoint_object_id, dependency_object_ids)
                    .await
            }
            RuntimeResourceConfigurationV1::Composite { .. } => WorkerExecution::failed(
                "COMPOSITE_REQUIRES_COORDINATOR",
                "Composite nodes are executed as child Executions by Workflow Runtime",
                false,
            ),
            RuntimeResourceConfigurationV1::Credential { .. } => WorkerExecution::failed(
                "CREDENTIAL_IS_NOT_EXECUTABLE",
                "Credential bindings may only authorize another Runtime call",
                false,
            ),
            _ => {
                let input = if expected_kind == RuntimeResourceKindV1::Model {
                    match self.model_input(claim).await {
                        Ok(input) => input,
                        Err(error) => {
                            return WorkerExecution::failed(
                                "MODEL_PROMPT_OBJECT_INVALID",
                                error.to_string(),
                                false,
                            );
                        }
                    }
                } else {
                    first_input(claim).unwrap_or(Value::Null)
                };
                self.execute_provider_call(claim, binding, input, 0).await
            }
        }
    }

    async fn model_input(&self, claim: &ClaimedWorkerAttempt) -> anyhow::Result<Value> {
        let target = first_input(claim).unwrap_or(Value::Null);
        let Some(prompt_object_id) = claim
            .node_parameters
            .get("promptObjectId")
            .or_else(|| target.get("promptObjectId"))
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return Ok(target);
        };
        let bytes = self
            .load_runtime_object(claim.task.tenant_id, prompt_object_id)
            .await?;
        let prompt = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Ok(json!({
            "prompt": prompt,
            "target": target,
            "promptObjectId": prompt_object_id,
        }))
    }

    async fn execute_skill(
        &self,
        claim: &ClaimedWorkerAttempt,
        binding: &RuntimeResourceBindingV1,
        entrypoint_object_id: Uuid,
        dependency_object_ids: &[Uuid],
    ) -> WorkerExecution {
        if !binding.object_ids.contains(&entrypoint_object_id)
            || dependency_object_ids
                .iter()
                .any(|object_id| !binding.object_ids.contains(object_id))
        {
            return WorkerExecution::failed(
                "SKILL_OBJECT_CLOSURE_INVALID",
                "Skill entrypoint or dependency is outside the immutable Binding closure",
                false,
            );
        }
        for object_id in binding.object_ids.iter().copied() {
            if let Err(error) = self
                .load_runtime_object(claim.task.tenant_id, object_id)
                .await
            {
                return WorkerExecution::failed("SKILL_OBJECT_INVALID", error.to_string(), false);
            }
        }
        let entrypoint = match self
            .load_runtime_object(claim.task.tenant_id, entrypoint_object_id)
            .await
            .and_then(|bytes| {
                serde_json::from_slice::<RuntimeSkillProgramV1>(&bytes).map_err(anyhow::Error::from)
            }) {
            Ok(entrypoint) => entrypoint,
            Err(error) => {
                return WorkerExecution::failed(
                    "SKILL_ENTRYPOINT_INVALID",
                    error.to_string(),
                    false,
                );
            }
        };
        let mut declared = entrypoint.dependency_object_ids.clone();
        declared.sort_unstable();
        let mut expected = dependency_object_ids.to_vec();
        expected.sort_unstable();
        if declared != expected || entrypoint.instructions.trim().is_empty() {
            return WorkerExecution::failed(
                "SKILL_OBJECT_CLOSURE_INVALID",
                "Skill program dependencies do not match the signed Binding closure",
                false,
            );
        }
        WorkerExecution::succeeded(json!({
            "resourceId": binding.resource_id,
            "resourceVersion": binding.resource_version,
            "instructions": entrypoint.instructions,
            "input": first_input(claim),
        }))
    }

    async fn load_runtime_object(
        &self,
        tenant_id: Uuid,
        object_id: Uuid,
    ) -> anyhow::Result<bytes::Bytes> {
        let row = sqlx::query(
            "SELECT CAST(object_key AS CHAR CHARACTER SET utf8mb4) AS object_key,content_hash,size_bytes FROM runtime_objects WHERE tenant_id=? AND object_id=? AND status='ready'",
        )
        .bind(tenant_id)
        .bind(object_id)
        .fetch_one(&self.pool)
        .await?;
        let key: String = row.try_get("object_key")?;
        let expected_hash: String = row.try_get("content_hash")?;
        let expected_size: u64 = row.try_get("size_bytes")?;
        let bytes = self
            .objects
            .get(&ObjectPath::from(key))
            .await?
            .bytes()
            .await?;
        anyhow::ensure!(
            bytes.len() as u64 == expected_size,
            "Runtime object size does not match metadata"
        );
        let actual_hash = format!("sha256:{:x}", Sha256::digest(&bytes));
        anyhow::ensure!(
            actual_hash == expected_hash,
            "Runtime object hash does not match metadata"
        );
        Ok(bytes)
    }

    async fn execute_declarative_http(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        let Some(endpoint) = claim
            .node_parameters
            .get("url")
            .or_else(|| claim.node_parameters.get("endpoint"))
            .and_then(Value::as_str)
        else {
            return WorkerExecution::failed(
                "HTTP_ENDPOINT_MISSING",
                "Declarative HTTP node requires a frozen endpoint",
                false,
            );
        };
        let request = json!({"input":first_input(claim),"parameters":claim.node_parameters});
        self.call_http(
            claim,
            "http",
            endpoint,
            request,
            0,
            None,
            "authorization",
            None,
        )
        .await
    }

    async fn execute_remote_action(&self, claim: &ClaimedWorkerAttempt) -> WorkerExecution {
        let Some(endpoint) = claim
            .node_parameters
            .get("endpoint")
            .or_else(|| claim.node_parameters.get("url"))
            .and_then(Value::as_str)
        else {
            return WorkerExecution::failed(
                "REMOTE_NODE_ENDPOINT_MISSING",
                "Remote Action requires a frozen Node Protocol endpoint",
                false,
            );
        };
        let inputs = claim
            .inputs
            .iter()
            .enumerate()
            .map(|(index, (port, items))| GroupedInput {
                port: port.clone(),
                branch_index: index as u32,
                items: items.clone(),
            })
            .collect::<Vec<_>>();
        let item_count = inputs.iter().map(|input| input.items.len()).sum::<usize>();
        let request = NodeActionRequest {
            protocol_version: NODE_PROTOCOL_VERSION.into(),
            node_type: claim.node_type.clone(),
            node_version: claim.node_version,
            tenant_id: TenantId::from_uuid(claim.task.tenant_id),
            workflow_version_id: None,
            execution_id: ExecutionId::from_uuid(claim.task.execution_id),
            node_execution_id: NodeExecutionId::from_uuid(claim.task.node_execution_id),
            attempt_id: claim.task.attempt_id,
            run_index: claim.run_index,
            iteration_index: claim.iteration_index,
            mode: ExecutionMode::Production,
            inputs,
            parameters: ResolvedParameters {
                common: claim.node_parameters.clone(),
                per_item: vec![claim.node_parameters.clone(); item_count],
            },
            artifact_handles: Vec::new(),
            credential_handles: Vec::new(),
            idempotency_key: claim.task.attempt_id.to_string(),
            deadline: claim.task.deadline_at,
            cancellation_url: None,
            trace_context: TraceContext {
                trace_id: claim.task.execution_id.to_string(),
                span_id: claim.task.attempt_id.to_string(),
                trace_flags: None,
            },
        };
        let request = match serde_json::to_value(request) {
            Ok(request) => request,
            Err(error) => {
                return WorkerExecution::failed(
                    "REMOTE_NODE_REQUEST_INVALID",
                    error.to_string(),
                    false,
                );
            }
        };
        let response = self
            .call_http(
                claim,
                "remote_action",
                &format!(
                    "{}/agentx/node/v1/actions/execute",
                    endpoint.trim_end_matches('/')
                ),
                request,
                0,
                None,
                "authorization",
                None,
            )
            .await;
        if response.status != WorkerResultStatusV1::Succeeded {
            return response;
        }
        let Some(payload) = response
            .outputs
            .get("main")
            .and_then(|items| items.first())
            .map(|item| item.json.clone())
        else {
            return WorkerExecution::failed(
                "REMOTE_NODE_RESPONSE_INVALID",
                "Remote Node returned no protocol response",
                false,
            );
        };
        match serde_json::from_value::<NodeActionResult>(payload) {
            Ok(NodeActionResult::Completed { outputs, .. }) => WorkerExecution {
                status: WorkerResultStatusV1::Succeeded,
                outputs: outputs
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
                error_code: None,
                error_message: None,
            },
            Ok(NodeActionResult::Failed { error }) => {
                WorkerExecution::failed(&error.code, error.message, false)
            }
            Ok(NodeActionResult::Suspended { resume, checkpoint }) => {
                WorkerExecution::suspended(json!({"resume": resume, "checkpoint": checkpoint}))
            }
            Err(error) => {
                WorkerExecution::failed("REMOTE_NODE_RESPONSE_INVALID", error.to_string(), false)
            }
        }
    }

    async fn execute_provider_call(
        &self,
        claim: &ClaimedWorkerAttempt,
        binding: &RuntimeResourceBindingV1,
        input: Value,
        call_index: u32,
    ) -> WorkerExecution {
        if let RuntimeResourceConfigurationV1::Model {
            provider,
            endpoint,
            model,
            price_version,
            credential,
        } = &binding.configuration
            && provider == "openai_compatible"
        {
            let endpoint = openai_chat_completions_endpoint(endpoint);
            let request = openai_chat_request(claim, model, price_version, &input);
            let execution = self
                .call_http(
                    claim,
                    "model",
                    &endpoint,
                    request,
                    call_index,
                    credential.as_ref(),
                    "authorization",
                    Some(binding),
                )
                .await;
            return openai_execution_output(execution);
        }
        let (kind, endpoint, request, secret, secret_header) = match &binding.configuration {
            RuntimeResourceConfigurationV1::Model {
                endpoint,
                model,
                price_version,
                credential,
                ..
            } => (
                "model",
                endpoint.clone(),
                json!({"model":model,"input":input,"priceVersion":price_version,"parameters":claim.node_parameters}),
                credential.as_ref(),
                "authorization",
            ),
            RuntimeResourceConfigurationV1::Mcp {
                endpoint,
                tool_name,
                credential,
                ..
            } => {
                return self
                    .call_mcp_tool(
                        claim,
                        endpoint,
                        tool_name,
                        input,
                        call_index,
                        credential.as_ref(),
                        binding,
                    )
                    .await;
            }
            RuntimeResourceConfigurationV1::Rag {
                endpoint,
                namespace,
                index_version,
                credential,
            } => {
                let operation = claim
                    .node_parameters
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("query");
                let mut payload = claim.node_parameters.get("input").cloned().unwrap_or(input);
                if !payload.is_object() {
                    payload = json!({"query":payload,"mode":"naive"});
                }
                if let Some(object) = payload.as_object_mut() {
                    object
                        .entry("workspace".to_owned())
                        .or_insert_with(|| json!(namespace));
                    object
                        .entry("indexVersion".to_owned())
                        .or_insert_with(|| json!(index_version));
                }
                let path = if operation == "insert" {
                    "documents/text"
                } else {
                    "query"
                };
                (
                    "rag",
                    format!("{}/{}", endpoint.trim_end_matches('/'), path),
                    payload,
                    credential.as_ref(),
                    "x-api-key",
                )
            }
            RuntimeResourceConfigurationV1::Memory {
                endpoint,
                namespace,
                memory_version,
                credential,
            } => {
                let operation = claim
                    .node_parameters
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("search");
                let input = claim.node_parameters.get("input").cloned().unwrap_or(input);
                let (path, request) = if operation == "add" || operation == "write" {
                    let content = input.get("text").cloned().unwrap_or_else(|| input.clone());
                    (
                        "memories",
                        json!({
                            "messages":[{"role":"user","content":content}],
                            "user_id":namespace,
                            "version":memory_version
                        }),
                    )
                } else {
                    let query = input.get("query").cloned().unwrap_or_else(|| input.clone());
                    let top_k = input.get("top_k").and_then(Value::as_u64).unwrap_or(5);
                    (
                        "search",
                        json!({"query":query,"filters":{"user_id":namespace},"top_k":top_k}),
                    )
                };
                (
                    "memory",
                    format!("{}/{}", endpoint.trim_end_matches('/'), path),
                    request,
                    credential.as_ref(),
                    "authorization",
                )
            }
            RuntimeResourceConfigurationV1::SandboxProfile { .. } => {
                let Ok(manager) = std::env::var("AGENTX_SANDBOX_MANAGER_ENDPOINT") else {
                    return WorkerExecution::failed(
                        "SANDBOX_MANAGER_UNAVAILABLE",
                        "AGENTX_SANDBOX_MANAGER_ENDPOINT is not configured",
                        false,
                    );
                };
                let endpoint = format!(
                    "{}/internal/runtime/v1/sandboxes:execute",
                    manager.trim_end_matches('/')
                );
                let execution = self
                    .call_sandbox_manager_http(
                        claim,
                        &endpoint,
                        serde_json::to_value(crate::sandbox::SandboxExecuteRequestV1 {
                            api_version: 1,
                            tenant_id: claim.task.tenant_id,
                            execution_id: claim.task.execution_id,
                            node_execution_id: claim.task.node_execution_id,
                            attempt_id: claim.task.attempt_id,
                            worker_id: claim.lease.worker_id,
                            fencing_token: claim.lease.fencing_token,
                            idempotency_key: format!("sandbox:execute:{}", claim.task.attempt_id),
                            profile: binding.clone(),
                            input,
                            parameters: claim.node_parameters.clone(),
                        })
                        .unwrap_or(Value::Null),
                        call_index,
                        Some(binding),
                    )
                    .await;
                return sandbox_execution_output(execution);
            }
            _ => {
                return WorkerExecution::failed(
                    "RUNTIME_RESOURCE_NOT_EXECUTABLE",
                    "Runtime Resource configuration is not executable",
                    false,
                );
            }
        };
        self.call_http(
            claim,
            kind,
            &endpoint,
            request,
            call_index,
            secret,
            secret_header,
            Some(binding),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_http(
        &self,
        claim: &ClaimedWorkerAttempt,
        kind: &str,
        endpoint: &str,
        request: Value,
        call_index: u32,
        secret: Option<&agentx_runtime_contracts::VaultSecretReferenceV1>,
        secret_header: &'static str,
        binding: Option<&RuntimeResourceBindingV1>,
    ) -> WorkerExecution {
        self.call_http_with_transport(
            claim,
            kind,
            endpoint,
            request,
            call_index,
            secret,
            secret_header,
            binding,
            RuntimeHttpTransport::Provider,
        )
        .await
    }

    async fn call_sandbox_manager_http(
        &self,
        claim: &ClaimedWorkerAttempt,
        endpoint: &str,
        request: Value,
        call_index: u32,
        binding: Option<&RuntimeResourceBindingV1>,
    ) -> WorkerExecution {
        self.call_http_with_transport(
            claim,
            "sandbox",
            endpoint,
            request,
            call_index,
            None,
            "authorization",
            binding,
            RuntimeHttpTransport::SandboxManager,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_http_with_transport(
        &self,
        claim: &ClaimedWorkerAttempt,
        kind: &str,
        endpoint: &str,
        request: Value,
        call_index: u32,
        secret: Option<&agentx_runtime_contracts::VaultSecretReferenceV1>,
        secret_header: &'static str,
        binding: Option<&RuntimeResourceBindingV1>,
        transport: RuntimeHttpTransport,
    ) -> WorkerExecution {
        let fingerprint = runtime_call_fingerprint(kind, &request);
        let call_id = stable_id(
            claim.task.attempt_id,
            format!("{kind}:{call_index}").as_bytes(),
        );
        let idempotency_key = format!("{}:{kind}:{call_index}", claim.task.attempt_id);
        match self
            .reserve_call(
                claim,
                call_id,
                kind,
                &idempotency_key,
                &fingerprint,
                &request,
                call_index,
                binding,
            )
            .await
        {
            Ok(Some(value)) => return WorkerExecution::succeeded(value),
            Ok(None) => {}
            Err(result) => return result,
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "Idempotency-Key",
            reqwest::header::HeaderValue::from_str(&idempotency_key)
                .expect("UUID-based idempotency key is a valid header"),
        );
        if let Some(reference) = secret {
            let Some(vault) = &self.vault else {
                return self
                    .fail_call(
                        call_id,
                        "VAULT_UNAVAILABLE",
                        "Runtime Vault is not configured",
                        false,
                    )
                    .await;
            };
            match vault.read(reference).await {
                Ok(value) => match reqwest::header::HeaderValue::from_bytes(
                    &provider_secret_header(&value, secret_header),
                ) {
                    Ok(value) => {
                        headers.insert(
                            reqwest::header::HeaderName::from_static(secret_header),
                            value,
                        );
                    }
                    Err(_) => {
                        return self
                            .fail_call(
                                call_id,
                                "VAULT_SECRET_INVALID",
                                "Vault value is not a valid authorization header",
                                false,
                            )
                            .await;
                    }
                },
                Err(error) => {
                    return self
                        .fail_call(call_id, "VAULT_UNAVAILABLE", error.to_string(), false)
                        .await;
                }
            }
        }
        if let Err(error) =
            sqlx::query("UPDATE runtime_calls SET status='sent' WHERE id=? AND status='reserved'")
                .bind(call_id)
                .execute(&self.pool)
                .await
        {
            return WorkerExecution::failed(
                "RUNTIME_CALL_STATE_UNAVAILABLE",
                error.to_string(),
                false,
            );
        }
        let context =
            EgressRequestContext::execution(claim.task.tenant_id, claim.task.execution_id);
        let response = match match transport {
            RuntimeHttpTransport::Provider => self.provider.post_json(
                endpoint,
                context,
                std::time::Duration::from_secs(300),
                headers,
                &request,
            ),
            RuntimeHttpTransport::SandboxManager => self.provider.post_sandbox_manager_json(
                endpoint,
                context,
                std::time::Duration::from_secs(300),
                headers,
                &request,
            ),
        }
        .await
        {
            Ok(response) => response,
            Err(WorkerProviderError::Denied(message)) => {
                return self
                    .fail_call(call_id, "PROVIDER_ENDPOINT_DENIED", message, false)
                    .await;
            }
            Err(WorkerProviderError::Request {
                message,
                is_connect,
            }) => {
                let unknown = !is_connect;
                return self
                    .fail_call(
                        call_id,
                        if unknown {
                            "PROVIDER_OUTCOME_UNKNOWN"
                        } else {
                            "PROVIDER_UNAVAILABLE"
                        },
                        message,
                        unknown,
                    )
                    .await;
            }
        };
        let status = response.status;
        let provider_request_id = response
            .headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let payload = match serde_json::from_slice::<Value>(&response.body) {
            Ok(value) => value,
            Err(error) => {
                return self
                    .fail_call(
                        call_id,
                        "PROVIDER_RESPONSE_INVALID",
                        error.to_string(),
                        false,
                    )
                    .await;
            }
        };
        if !status.is_success() {
            return self
                .fail_call(
                    call_id,
                    "PROVIDER_REJECTED",
                    format!("HTTP {status}: {payload}"),
                    false,
                )
                .await;
        }
        if let Err(error) = sqlx::query(
            "UPDATE runtime_calls SET status='succeeded',provider_request_id=?,response_json=?,input_tokens=?,output_tokens=?,cost_micros=?,ended_at=UTC_TIMESTAMP(6) WHERE id=? AND status='sent'",
        )
        .bind(provider_request_id)
        .bind(&payload)
        .bind(provider_usage_detail(&payload).0)
        .bind(provider_usage_detail(&payload).1)
        .bind(provider_usage_detail(&payload).2)
        .bind(call_id)
        .execute(&self.pool)
        .await
        {
            return WorkerExecution::failed("RUNTIME_CALL_COMMIT_FAILED", error.to_string(), true);
        }
        self.emit_runtime_call_trace(
            call_id,
            agentx_runtime_contracts::TraceEventKindV1::Finished,
            "succeeded",
            None,
            Some(&payload),
        )
        .await;
        WorkerExecution::succeeded(payload)
    }

    #[allow(clippy::too_many_arguments)]
    async fn reserve_call(
        &self,
        claim: &ClaimedWorkerAttempt,
        call_id: Uuid,
        kind: &str,
        idempotency_key: &str,
        fingerprint: &str,
        request: &Value,
        call_index: u32,
        binding: Option<&RuntimeResourceBindingV1>,
    ) -> Result<Option<Value>, WorkerExecution> {
        let mut tx = self.pool.begin().await.map_err(|error| {
            WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false)
        })?;
        if let Some(row) = sqlx::query(
            "SELECT id,status,side_effect,request_fingerprint,response_json,error_code,error_message FROM runtime_calls WHERE tenant_id=? AND idempotency_key=? FOR UPDATE",
        )
        .bind(claim.task.tenant_id)
        .bind(idempotency_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false))?
        {
            if row.try_get::<String, _>("request_fingerprint").map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_INVALID", error.to_string(), false))? != fingerprint {
                return Err(WorkerExecution::failed("RUNTIME_CALL_IDEMPOTENCY_CONFLICT", "Runtime Call key was reused with different input", false));
            }
            let status: String = row.try_get("status").map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_INVALID", error.to_string(), false))?;
            if status == "succeeded" {
                let response = row.try_get::<Option<Value>, _>("response_json").map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_INVALID", error.to_string(), false))?.unwrap_or(Value::Null);
                tx.commit().await.map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false))?;
                return Ok(Some(response));
            }
            let side_effect: String = row.try_get("side_effect").map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_INVALID", error.to_string(), false))?;
            if runtime_call_is_replayable(&status, &side_effect) {
                sqlx::query(
                    "UPDATE runtime_calls SET status='reserved',error_code=NULL,error_message=NULL,ended_at=NULL WHERE id=? AND status=?",
                )
                .bind(row.try_get::<Uuid, _>("id").map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_INVALID", error.to_string(), false))?)
                .bind(&status)
                .execute(&mut *tx)
                .await
                .map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false))?;
                tx.commit().await.map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false))?;
                return Ok(None);
            }
            return Err(WorkerExecution::failed(
                row.try_get::<Option<String>, _>("error_code").ok().flatten().as_deref().unwrap_or("PROVIDER_OUTCOME_UNKNOWN"),
                row.try_get::<Option<String>, _>("error_message").ok().flatten().unwrap_or_else(|| format!("Runtime Call remains {status}")),
                status == "sent" || status == "outcome_unknown",
            ));
        }
        sqlx::query(
            "INSERT INTO runtime_calls(id,tenant_id,execution_id,node_execution_id,attempt_id,agent_run_id,iteration_index,call_index,call_kind,idempotency_key,request_fingerprint,resource_type,resource_id,resource_version_id,side_effect,status,request_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'reserved',?)",
        )
        .bind(call_id)
        .bind(claim.task.tenant_id)
        .bind(claim.task.execution_id)
        .bind(claim.task.node_execution_id)
        .bind(claim.task.attempt_id)
        .bind((claim.node_type == "agent").then(|| stable_id(claim.task.attempt_id, b"agent-run")))
        .bind(if claim.node_type == "agent" { call_index / 2 } else { 0 })
        .bind(call_index)
        .bind(kind)
        .bind(idempotency_key)
        .bind(fingerprint)
        .bind(binding.map(|value| resource_kind_name(value.resource_kind)))
        .bind(binding.map(|value| value.resource_id))
        .bind(binding.map(|value| value.resource_version.as_str()))
        .bind(if kind == "mcp_tool" || kind == "sandbox" { "idempotent" } else { "none" })
        .bind(request)
        .execute(&mut *tx)
        .await
        .map_err(|error| WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false))?;
        tx.commit().await.map_err(|error| {
            WorkerExecution::failed("RUNTIME_CALL_STATE_UNAVAILABLE", error.to_string(), false)
        })?;
        self.emit_runtime_call_trace(
            call_id,
            agentx_runtime_contracts::TraceEventKindV1::Started,
            "reserved",
            None,
            Some(request),
        )
        .await;
        Ok(None)
    }

    async fn fail_call(
        &self,
        call_id: Uuid,
        code: &str,
        message: impl Into<String>,
        outcome_unknown: bool,
    ) -> WorkerExecution {
        let message = message.into();
        let _ = sqlx::query(
            "UPDATE runtime_calls SET status=?,error_code=?,error_message=?,ended_at=UTC_TIMESTAMP(6) WHERE id=? AND status IN ('reserved','sent')",
        )
        .bind(if outcome_unknown { "outcome_unknown" } else { "failed" })
        .bind(code)
        .bind(&message)
        .bind(call_id)
        .execute(&self.pool)
        .await;
        self.emit_runtime_call_trace(
            call_id,
            agentx_runtime_contracts::TraceEventKindV1::Finished,
            if outcome_unknown {
                "outcome_unknown"
            } else {
                "failed"
            },
            Some(code),
            None,
        )
        .await;
        WorkerExecution::failed(code, message, outcome_unknown)
    }

    async fn emit_runtime_call_trace(
        &self,
        call_id: Uuid,
        event_kind: agentx_runtime_contracts::TraceEventKindV1,
        status: &str,
        error_code: Option<&str>,
        content: Option<&Value>,
    ) {
        let row = match sqlx::query("SELECT tenant_id,execution_id,node_execution_id,attempt_id,agent_run_id,iteration_index,call_kind,resource_type,resource_id,resource_version_id,input_tokens,output_tokens,cost_micros,error_message FROM runtime_calls WHERE id=?")
            .bind(call_id)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(row)) => row,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%error, %call_id, "Runtime Call Trace lookup failed");
                return;
            }
        };
        let tenant_id = match row.try_get::<Uuid, _>("tenant_id") {
            Ok(value) => value,
            Err(_) => return,
        };
        let execution_id = match row.try_get::<Uuid, _>("execution_id") {
            Ok(value) => value,
            Err(_) => return,
        };
        let attempt_id = match row.try_get::<Uuid, _>("attempt_id") {
            Ok(value) => value,
            Err(_) => return,
        };
        let kind = row
            .try_get::<String, _>("call_kind")
            .unwrap_or_else(|_| "runtime".into());
        let agent_run_id = row
            .try_get::<Option<Uuid>, _>("agent_run_id")
            .ok()
            .flatten();
        let agent_iteration_id = agent_run_id.map(|run_id| {
            let index = row.try_get::<u32, _>("iteration_index").unwrap_or_default();
            stable_id(run_id, format!("iteration-{index}").as_bytes())
        });
        let parent = agent_iteration_id
            .map(|id| {
                (
                    id,
                    agentx_runtime_contracts::TraceSpanKindV1::AgentIteration,
                )
            })
            .unwrap_or((
                attempt_id,
                agentx_runtime_contracts::TraceSpanKindV1::Attempt,
            ));
        let mut trace = crate::trace_delivery::TraceDraft::span(
            tenant_id,
            execution_id,
            call_id,
            Some(parent),
            agentx_runtime_contracts::TraceSpanKindV1::RuntimeCall,
            runtime_call_span_name(&kind),
            event_kind,
            format!(
                "runtime_call.{}",
                if event_kind == agentx_runtime_contracts::TraceEventKindV1::Finished {
                    "finished"
                } else {
                    "started"
                }
            ),
            status,
        );
        trace.node_execution_id = row.try_get("node_execution_id").ok();
        trace.attempt_id = Some(attempt_id);
        trace.agent_run_id = agent_run_id;
        trace.agent_iteration_id = agent_iteration_id;
        trace.runtime_call_id = Some(call_id);
        trace.resource_type = row.try_get("resource_type").ok();
        trace.resource_id = row.try_get("resource_id").ok();
        trace.resource_version = row
            .try_get::<Option<Uuid>, _>("resource_version_id")
            .ok()
            .flatten()
            .map(|value| value.to_string());
        trace.input_tokens = row.try_get("input_tokens").ok();
        trace.output_tokens = row.try_get("output_tokens").ok();
        trace.cost_micros = row.try_get("cost_micros").unwrap_or_default();
        trace.error_code = error_code.map(str::to_owned);
        trace.error_message = row.try_get("error_message").ok();
        trace.content_role = Some(
            if event_kind == agentx_runtime_contracts::TraceEventKindV1::Started {
                "input"
            } else {
                "output"
            }
            .into(),
        );
        trace.content_preview = content.and_then(crate::trace_delivery::bounded_preview);
        let Ok(mut tx) = self.pool.begin().await else {
            return;
        };
        if let Err(error) = crate::trace_delivery::enqueue(&mut tx, trace).await {
            tracing::warn!(%error, %call_id, "Runtime Call Trace enqueue failed");
            return;
        }
        if let Err(error) = tx.commit().await {
            tracing::warn!(%error, %call_id, "Runtime Call Trace commit failed");
        }
    }

    async fn emit_agent_iteration_finish(
        &self,
        iteration_id: Uuid,
        status: &str,
        stop_reason: &str,
    ) {
        let run_id =
            sqlx::query_scalar::<_, Uuid>("SELECT agent_run_id FROM agent_iterations WHERE id=?")
                .bind(iteration_id)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        if let Some(run_id) = run_id {
            self.emit_agent_span(
                run_id,
                Some(iteration_id),
                agentx_runtime_contracts::TraceEventKindV1::Finished,
                status,
                (status == "failed").then_some(stop_reason),
                None,
            )
            .await;
        }
    }

    async fn emit_agent_span(
        &self,
        run_id: Uuid,
        iteration_id: Option<Uuid>,
        event_kind: agentx_runtime_contracts::TraceEventKindV1,
        status: &str,
        error_code: Option<&str>,
        content: Option<&Value>,
    ) {
        let row = match sqlx::query("SELECT tenant_id,execution_id,node_execution_id,attempt_id,input_tokens,output_tokens,cost_micros,stop_reason FROM agent_runs WHERE id=?")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(row)) => row,
            _ => return,
        };
        let Ok(tenant_id) = row.try_get::<Uuid, _>("tenant_id") else {
            return;
        };
        let Ok(execution_id) = row.try_get::<Uuid, _>("execution_id") else {
            return;
        };
        let Ok(attempt_id) = row.try_get::<Uuid, _>("attempt_id") else {
            return;
        };
        let (entity_id, parent, kind, name, event_type) = if let Some(iteration_id) = iteration_id {
            let index = sqlx::query_scalar::<_, u32>(
                "SELECT iteration_index FROM agent_iterations WHERE id=?",
            )
            .bind(iteration_id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            (
                iteration_id,
                (run_id, agentx_runtime_contracts::TraceSpanKindV1::AgentRun),
                agentx_runtime_contracts::TraceSpanKindV1::AgentIteration,
                format!("Iteration {}", index + 1),
                "agent_iteration",
            )
        } else {
            (
                run_id,
                (
                    attempt_id,
                    agentx_runtime_contracts::TraceSpanKindV1::Attempt,
                ),
                agentx_runtime_contracts::TraceSpanKindV1::AgentRun,
                "Agent run".into(),
                "agent_run",
            )
        };
        let mut trace = crate::trace_delivery::TraceDraft::span(
            tenant_id,
            execution_id,
            entity_id,
            Some(parent),
            kind,
            name,
            event_kind,
            format!(
                "{event_type}.{}",
                if event_kind == agentx_runtime_contracts::TraceEventKindV1::Finished {
                    "finished"
                } else {
                    "started"
                }
            ),
            status,
        );
        trace.node_execution_id = row.try_get("node_execution_id").ok();
        trace.attempt_id = Some(attempt_id);
        trace.agent_run_id = Some(run_id);
        trace.agent_iteration_id = iteration_id;
        trace.input_tokens = row.try_get("input_tokens").ok();
        trace.output_tokens = row.try_get("output_tokens").ok();
        trace.cost_micros = row.try_get("cost_micros").unwrap_or_default();
        trace.error_code = error_code.map(str::to_owned);
        trace.error_message = (status == "failed")
            .then(|| {
                row.try_get::<Option<String>, _>("stop_reason")
                    .ok()
                    .flatten()
            })
            .flatten();
        trace.attributes =
            json!({"stopReason":row.try_get::<Option<String>, _>("stop_reason").ok().flatten()});
        trace.content_role = Some(
            if event_kind == agentx_runtime_contracts::TraceEventKindV1::Started {
                "input"
            } else {
                "output"
            }
            .into(),
        );
        trace.content_preview = content.and_then(crate::trace_delivery::bounded_preview);
        let Ok(mut tx) = self.pool.begin().await else {
            return;
        };
        if crate::trace_delivery::enqueue(&mut tx, trace).await.is_ok() {
            let _ = tx.commit().await;
        }
    }
}

fn execute_builtin_node(node_type: &str, parameters: &Value, value: Value) -> WorkerExecution {
    if node_type == "stop_and_error" {
        return WorkerExecution::failed(
            parameters
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("WORKFLOW_STOPPED"),
            parameters
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Workflow stopped"),
            false,
        );
    }
    if node_type == "set" {
        let mut output = if parameters
            .get("keepOnlySet")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            serde_json::Map::new()
        } else {
            value.as_object().cloned().unwrap_or_default()
        };
        if let Some(values) = parameters.get("values").and_then(Value::as_object) {
            output.extend(values.clone());
        }
        return WorkerExecution::succeeded(Value::Object(output));
    }
    WorkerExecution::succeeded(value)
}

fn select_binding(
    claim: &ClaimedWorkerAttempt,
    kind: RuntimeResourceKindV1,
) -> Option<&RuntimeResourceBindingV1> {
    let requested = claim
        .node_parameters
        .get("resourceId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    claim.resources.iter().find(|binding| {
        binding.resource_kind == kind
            && requested.is_none_or(|resource_id| resource_id == binding.resource_id)
            && (kind != RuntimeResourceKindV1::Mcp || is_mcp_tool_binding(binding))
    })
}

fn mcp_tool_binding(resources: &[RuntimeResourceBindingV1]) -> Option<&RuntimeResourceBindingV1> {
    resources
        .iter()
        .find(|binding| is_mcp_tool_binding(binding))
}

fn is_mcp_tool_binding(binding: &RuntimeResourceBindingV1) -> bool {
    binding.resource_kind == RuntimeResourceKindV1::Mcp
        && matches!(
            &binding.configuration,
            RuntimeResourceConfigurationV1::Mcp { tool_name, .. }
                if !tool_name.trim().is_empty() && tool_name != "__server__"
        )
}

fn capability_resource_kind(capability: &NodeCapability) -> RuntimeResourceKindV1 {
    match capability {
        NodeCapability::Model | NodeCapability::Agent => RuntimeResourceKindV1::Model,
        NodeCapability::McpTool => RuntimeResourceKindV1::Mcp,
        NodeCapability::Skill => RuntimeResourceKindV1::Skill,
        NodeCapability::Rag => RuntimeResourceKindV1::Rag,
        NodeCapability::Memory => RuntimeResourceKindV1::Memory,
        NodeCapability::Sandbox => RuntimeResourceKindV1::SandboxProfile,
        NodeCapability::Builtin
        | NodeCapability::DeclarativeHttp
        | NodeCapability::RemoteAction => RuntimeResourceKindV1::Credential,
    }
}

fn resource_kind_name(kind: RuntimeResourceKindV1) -> &'static str {
    match kind {
        RuntimeResourceKindV1::Model => "model",
        RuntimeResourceKindV1::Mcp => "mcp",
        RuntimeResourceKindV1::Rag => "rag",
        RuntimeResourceKindV1::Memory => "memory",
        RuntimeResourceKindV1::Skill => "skill",
        RuntimeResourceKindV1::Credential => "credential",
        RuntimeResourceKindV1::SandboxProfile => "sandbox_profile",
        RuntimeResourceKindV1::Composite => "composite",
    }
}

fn first_input(claim: &ClaimedWorkerAttempt) -> Option<Value> {
    claim
        .inputs
        .get("main")
        .or_else(|| claim.inputs.values().next())
        .and_then(|items| items.first())
        .map(|item| item.json.clone())
}

fn successful_value(execution: &WorkerExecution) -> Option<Value> {
    if execution.status != WorkerResultStatusV1::Succeeded {
        return None;
    }
    execution
        .outputs
        .get("main")
        .and_then(|items| items.first())
        .map(|item| item.json.clone())
}

#[cfg(test)]
#[path = "worker_runtime_tests.rs"]
mod tests;
