use std::{collections::BTreeMap, str::FromStr};

use agentx_application::{
    ArtifactWrite, McpToolRequest, ModelRequest, ModelResponse, RuntimeContext, RuntimeError,
    RuntimeResult, TraceSink,
};
use agentx_domain::{ArtifactId, ResourceReference, ResourceType, TraceEvent};
use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use futures::{StreamExt, stream};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::resources::{ResourceRuntimes, first_input, reference};

#[derive(Clone)]
pub struct AgentRunner {
    pool: MySqlPool,
    runtimes: ResourceRuntimes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Budget {
    max_iterations: u32,
    max_model_calls: u32,
    max_tool_calls: u32,
    max_total_tokens: u64,
    max_cost_micros: u64,
    max_duration_ms: u64,
    limit_action: String,
    max_output_tokens: u64,
}

struct RunState {
    id: Uuid,
    messages: Vec<Value>,
    iteration: u32,
    model_calls: u32,
    tool_calls: u32,
    tokens: u64,
    cost: u64,
    started_at: OffsetDateTime,
}
struct ToolInvocation {
    index: usize,
    call_id: String,
    resource: ResourceReference,
    arguments: Value,
    fingerprint: String,
    side_effect: String,
}
struct ToolOutcome {
    index: usize,
    call_id: String,
    tool_name: String,
    fingerprint: String,
    error_code: Option<String>,
    value: Value,
    newly_counted: bool,
}
#[derive(Debug)]
enum Reservation {
    New(Uuid),
    Reused(Value),
}

impl AgentRunner {
    pub fn new(pool: MySqlPool, runtimes: ResourceRuntimes) -> Self {
        Self { pool, runtimes }
    }

    pub async fn execute(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        parameters: Value,
    ) -> RuntimeResult<TaskResult> {
        let budget = Budget::from_parameters(&parameters);
        let model_reference = reference(task, ResourceType::Model)?;
        let tool_catalog = tool_catalog(task)?;
        let tools = tool_catalog
            .values()
            .map(tool_schema)
            .collect::<RuntimeResult<Vec<_>>>()?;
        let mut run = self.load_or_create_run(task, &parameters, &budget).await?;
        if run.messages.is_empty() {
            run.messages = initial_messages(task, context, &parameters, &self.runtimes).await?;
        }
        let mut fingerprints = self.load_fingerprints(run.id, run.iteration).await?;
        let mut state_hashes = self.load_state_hashes(run.id).await?;
        let mut error_counts = self.load_error_counts(run.id).await?;

        loop {
            if let Some(reason) = limits(&run, &budget) {
                return self.stop(task, &mut run, &budget, &reason, None).await;
            }
            let state_before = state_hash(&run.messages, &parameters);
            let iteration_id = self
                .begin_iteration(task, &run, state_before.as_str())
                .await?;
            let model_request = ModelRequest {
                resource: model_reference.clone(),
                messages: run.messages.clone(),
                tools: tools.clone(),
                parameters: parameters
                    .get("modelParameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"max_tokens":budget.max_output_tokens})),
            };
            let model_key = format!(
                "{}:{}:agent:{}:model",
                task.execution_id, task.node_execution_id, run.iteration
            );
            let model_fingerprint =
                hash_json(&serde_json::to_value(&model_request).map_err(protocol_error)?);
            let input_reserve = serde_json::to_vec(&model_request.messages)
                .map_err(protocol_error)?
                .len() as u64;
            let cost_reserve = estimated_cost(task, input_reserve, budget.max_output_tokens)?;
            let reservation = match self
                .reserve_call(
                    task,
                    &run,
                    &budget,
                    &model_key,
                    &model_fingerprint,
                    "model",
                    &model_reference,
                    "none",
                    input_reserve + budget.max_output_tokens,
                    cost_reserve,
                    0,
                )
                .await
            {
                Ok(value) => value,
                Err(error) if limit_reason(&error).is_some() => {
                    return self
                        .stop(
                            task,
                            &mut run,
                            &budget,
                            limit_reason(&error).expect("limit reason was checked"),
                            Some(iteration_id),
                        )
                        .await;
                }
                Err(error) => return Err(error),
            };
            let (response, runtime_call_id, newly_counted) = match reservation {
                Reservation::Reused(value) => (
                    serde_json::from_value(value).map_err(protocol_error)?,
                    None,
                    false,
                ),
                Reservation::New(call_id) => {
                    self.mark_sent(call_id).await?;
                    match self.runtimes.model.complete(context, model_request).await {
                        Ok(response) => {
                            self.complete_model_call(task, run.id, call_id, &response)
                                .await?;
                            (response, Some(call_id), true)
                        }
                        Err(error) => {
                            self.fail_call(call_id, &error).await?;
                            return Err(error);
                        }
                    }
                }
            };
            self.emit(
                context,
                task,
                run.id,
                runtime_call_id,
                Some(&model_reference),
                "model.call",
                "succeeded",
                Some(&response),
                None,
                None,
                json!({"estimated":response.usage_estimated}),
            )
            .await;
            if newly_counted {
                run.model_calls += 1;
                run.tokens = run
                    .tokens
                    .saturating_add(response.input_tokens + response.output_tokens);
                run.cost = run.cost.saturating_add(response.cost_micros);
            }
            run.messages.push(response.message.clone());
            if response.tool_calls.is_empty() {
                let after = state_hash(&run.messages, &parameters);
                self.finish_iteration(iteration_id, run.id, &mut run, &after, None)
                    .await?;
                self.emit(
                    context,
                    task,
                    run.id,
                    None,
                    None,
                    "agent.iteration",
                    "succeeded",
                    None,
                    None,
                    Some(&response.stop_reason),
                    json!({"stateHash":after}),
                )
                .await;
                return self.succeed(task, &mut run, response).await;
            }
            if run
                .tool_calls
                .saturating_add(response.tool_calls.len() as u32)
                > budget.max_tool_calls
            {
                return self
                    .stop(
                        task,
                        &mut run,
                        &budget,
                        "tool_call_limit",
                        Some(iteration_id),
                    )
                    .await;
            }
            let invocations = parse_tool_calls(&response.tool_calls, &tool_catalog)?;
            for invocation in &invocations {
                fingerprints.push(invocation.fingerprint.clone());
                if repeated_fingerprint(&fingerprints, 3) {
                    return self
                        .stop(
                            task,
                            &mut run,
                            &budget,
                            "repeated_tool_call",
                            Some(iteration_id),
                        )
                        .await;
                }
                if abab(&fingerprints) {
                    return self
                        .stop(
                            task,
                            &mut run,
                            &budget,
                            "tool_call_abab",
                            Some(iteration_id),
                        )
                        .await;
                }
            }
            let outcomes = match self
                .execute_tools(task, context, &run, &budget, invocations)
                .await
            {
                Ok(value) => value,
                Err(error) if limit_reason(&error).is_some() => {
                    return self
                        .stop(
                            task,
                            &mut run,
                            &budget,
                            limit_reason(&error).expect("limit reason was checked"),
                            Some(iteration_id),
                        )
                        .await;
                }
                Err(error) => return Err(error),
            };
            run.tool_calls = run.tool_calls.saturating_add(
                outcomes
                    .iter()
                    .filter(|outcome| outcome.newly_counted)
                    .count() as u32,
            );
            for outcome in outcomes {
                if outcome.newly_counted
                    && let Some(code) = &outcome.error_code
                {
                    let key = format!("{}:{code}", outcome.fingerprint);
                    let count = error_counts.entry(key).or_default();
                    *count += 1;
                    if *count >= 3 {
                        return self
                            .stop(
                                task,
                                &mut run,
                                &budget,
                                "repeated_tool_error",
                                Some(iteration_id),
                            )
                            .await;
                    }
                }
                run.messages.push(json!({"role":"tool","tool_call_id":outcome.call_id,"name":outcome.tool_name,"content":outcome.value.to_string()}));
            }
            let after = state_hash(&run.messages, &parameters);
            state_hashes.push(after.clone());
            self.finish_iteration(iteration_id, run.id, &mut run, &after, None)
                .await?;
            self.emit(
                context,
                task,
                run.id,
                None,
                None,
                "agent.iteration",
                "succeeded",
                None,
                None,
                None,
                json!({"stateHash":after,"toolCalls":response.tool_calls.len()}),
            )
            .await;
            if state_stalled(&state_hashes) {
                return self
                    .stop(task, &mut run, &budget, "state_stall", None)
                    .await;
            }
            run.iteration += 1;
        }
    }

    async fn execute_tools(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        run: &RunState,
        budget: &Budget,
        invocations: Vec<ToolInvocation>,
    ) -> RuntimeResult<Vec<ToolOutcome>> {
        let mut result = Vec::with_capacity(invocations.len());
        let mut group = Vec::new();
        for invocation in invocations {
            if matches!(invocation.side_effect.as_str(), "none" | "read_only") {
                group.push(invocation);
                continue;
            }
            if !group.is_empty() {
                result.extend(
                    self.execute_read_only_group(
                        task,
                        context,
                        run,
                        budget,
                        std::mem::take(&mut group),
                    )
                    .await?,
                );
            }
            result.push(
                self.execute_tool(task, context, run, budget, invocation)
                    .await?,
            );
        }
        if !group.is_empty() {
            result.extend(
                self.execute_read_only_group(task, context, run, budget, group)
                    .await?,
            );
        }
        result.sort_by_key(|value| value.index);
        Ok(result)
    }

    async fn execute_read_only_group(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        run: &RunState,
        budget: &Budget,
        group: Vec<ToolInvocation>,
    ) -> RuntimeResult<Vec<ToolOutcome>> {
        let futures = group
            .into_iter()
            .map(|invocation| self.execute_tool(task, context, run, budget, invocation));
        stream::iter(futures)
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    async fn execute_tool(
        &self,
        task: &RuntimeTask,
        context: &RuntimeContext,
        run: &RunState,
        budget: &Budget,
        invocation: ToolInvocation,
    ) -> RuntimeResult<ToolOutcome> {
        let key = format!(
            "{}:{}:agent:{}:tool:{}",
            task.execution_id, task.node_execution_id, run.iteration, invocation.index
        );
        let call = match self
            .reserve_call(
                task,
                run,
                budget,
                &key,
                &invocation.fingerprint,
                "mcp_tool",
                &invocation.resource,
                &invocation.side_effect,
                0,
                0,
                invocation.index as u32 + 1,
            )
            .await?
        {
            Reservation::Reused(value) => {
                return Ok(ToolOutcome {
                    index: invocation.index,
                    call_id: invocation.call_id,
                    tool_name: tool_name(task, &invocation.resource),
                    fingerprint: invocation.fingerprint,
                    error_code: value
                        .get("errorCode")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    value,
                    newly_counted: false,
                });
            }
            Reservation::New(id) => id,
        };
        self.mark_sent(call).await?;
        match self
            .runtimes
            .mcp
            .call(
                context,
                McpToolRequest {
                    resource: invocation.resource.clone(),
                    arguments: invocation.arguments,
                },
            )
            .await
        {
            Ok(response) => {
                let value = json!({"content":response.content,"structuredContent":response.structured_content,"isError":response.is_error});
                let error_code = response.is_error.then_some("MCP_TOOL_ERROR");
                self.complete_tool_call(call, run.id, &value, error_code)
                    .await?;
                self.emit(
                    context,
                    task,
                    run.id,
                    Some(call),
                    Some(&invocation.resource),
                    "mcp.tool",
                    if response.is_error {
                        "failed"
                    } else {
                        "succeeded"
                    },
                    None,
                    response.is_error.then_some("MCP_TOOL_ERROR"),
                    None,
                    value.clone(),
                )
                .await;
                Ok(ToolOutcome {
                    index: invocation.index,
                    call_id: invocation.call_id,
                    tool_name: tool_name(task, &invocation.resource),
                    fingerprint: invocation.fingerprint,
                    error_code: response.is_error.then(|| "MCP_TOOL_ERROR".into()),
                    value,
                    newly_counted: true,
                })
            }
            Err(error) => {
                self.fail_call(call, &error).await?;
                self.emit(
                    context,
                    task,
                    run.id,
                    Some(call),
                    Some(&invocation.resource),
                    "mcp.tool",
                    "failed",
                    None,
                    Some(&error.code),
                    None,
                    error.attributes.clone(),
                )
                .await;
                if error.outcome_unknown && !unknown_outcome_can_retry(&invocation.side_effect) {
                    return Err(RuntimeError::new(
                        "EXTERNAL_CALL_OUTCOME_UNKNOWN",
                        "Tool call result is unknown and cannot be replayed safely",
                    )
                    .outcome_unknown(true));
                }
                Ok(ToolOutcome {
                    index: invocation.index,
                    call_id: invocation.call_id,
                    tool_name: tool_name(task, &invocation.resource),
                    fingerprint: invocation.fingerprint,
                    error_code: Some(error.code.clone()),
                    value: json!({"isError":true,"errorCode":error.code,"message":error.message}),
                    newly_counted: true,
                })
            }
        }
    }

    async fn load_or_create_run(
        &self,
        task: &RuntimeTask,
        parameters: &Value,
        budget: &Budget,
    ) -> RuntimeResult<RunState> {
        let budget_json = serde_json::to_value(budget).map_err(protocol_error)?;
        sqlx::query("INSERT INTO agent_runs(id,tenant_id,execution_id,node_execution_id,status,budget_json) VALUES(?,?,?,?, 'running',?) ON DUPLICATE KEY UPDATE status='running',budget_json=VALUES(budget_json),ended_at=NULL")
            .bind(Uuid::now_v7()).bind(task.tenant_id).bind(task.execution_id).bind(task.node_execution_id).bind(budget_json).execute(&self.pool).await.map_err(storage_error)?;
        let row=sqlx::query("SELECT id,iteration_count,model_call_count,tool_call_count,input_tokens+output_tokens tokens,cost_micros,state_artifact_id,started_at FROM agent_runs WHERE tenant_id=? AND node_execution_id=?")
            .bind(task.tenant_id).bind(task.node_execution_id).fetch_one(&self.pool).await.map_err(storage_error)?;
        let artifact: Option<Uuid> = row.try_get("state_artifact_id").map_err(storage_error)?;
        let messages = if let Some(id) = artifact {
            self.runtimes
                .artifacts
                .get(
                    agentx_domain::TenantId::from_uuid(task.tenant_id),
                    ArtifactId::from_uuid(id),
                )
                .await
                .map_err(storage_error)?
                .and_then(|value| serde_json::from_slice(&value.content).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let _ = parameters;
        Ok(RunState {
            id: row.try_get("id").map_err(storage_error)?,
            messages,
            iteration: row.try_get("iteration_count").map_err(storage_error)?,
            model_calls: row.try_get("model_call_count").map_err(storage_error)?,
            tool_calls: row.try_get("tool_call_count").map_err(storage_error)?,
            tokens: row.try_get("tokens").map_err(storage_error)?,
            cost: row.try_get("cost_micros").map_err(storage_error)?,
            started_at: row.try_get("started_at").map_err(storage_error)?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn reserve_call(
        &self,
        task: &RuntimeTask,
        run: &RunState,
        budget: &Budget,
        key: &str,
        fingerprint: &str,
        kind: &str,
        resource: &ResourceReference,
        side_effect: &str,
        reserved_tokens: u64,
        reserved_cost: u64,
        call_index: u32,
    ) -> RuntimeResult<Reservation> {
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        if let Some(row)=sqlx::query("SELECT id,status,response_artifact_id,request_fingerprint,call_kind,side_effect,agent_run_id,reserved_input_tokens+reserved_output_tokens reserved_tokens,reserved_cost_micros FROM runtime_calls WHERE tenant_id=? AND idempotency_key=? FOR UPDATE").bind(task.tenant_id).bind(key).fetch_optional(&mut *tx).await.map_err(storage_error)?{
            let id:Uuid=row.try_get("id").map_err(storage_error)?;let status:String=row.try_get("status").map_err(storage_error)?;
            let stored_fingerprint:String=row.try_get("request_fingerprint").map_err(storage_error)?;
            let stored_kind:String=row.try_get("call_kind").map_err(storage_error)?;
            let stored_side_effect:String=row.try_get("side_effect").map_err(storage_error)?;
            if stored_fingerprint != fingerprint || stored_kind != kind || stored_side_effect != side_effect {
                return Err(RuntimeError::new("RUNTIME_CALL_IDEMPOTENCY_CONFLICT","The runtime call idempotency key was reused with a different request"));
            }
            if status=="succeeded"{let artifact:Option<Uuid>=row.try_get("response_artifact_id").map_err(storage_error)?;tx.commit().await.map_err(storage_error)?;let id=artifact.ok_or_else(||RuntimeError::new("RUNTIME_CALL_LEDGER_INVALID","Completed runtime call has no response Artifact"))?;let artifact=self.runtimes.artifacts.get(agentx_domain::TenantId::from_uuid(task.tenant_id),ArtifactId::from_uuid(id)).await.map_err(storage_error)?.ok_or_else(||RuntimeError::new("RUNTIME_CALL_LEDGER_INVALID","Runtime call response Artifact is missing"))?;return Ok(Reservation::Reused(serde_json::from_slice(&artifact.content).map_err(protocol_error)?));}
            if matches!(status.as_str(),"sent"|"unknown")&&!unknown_outcome_can_retry(side_effect){return Err(RuntimeError::new("EXTERNAL_CALL_OUTCOME_UNKNOWN","A previous side-effecting call has an unknown outcome").outcome_unknown(true));}
            let agent_run_id:Uuid=row.try_get("agent_run_id").map_err(storage_error)?;
            if agent_run_id != run.id {
                return Err(RuntimeError::new("RUNTIME_CALL_LEDGER_INVALID","The runtime call belongs to a different Agent run"));
            }
            let old_reserved_tokens:u64=row.try_get("reserved_tokens").map_err(storage_error)?;
            let old_reserved_cost:u64=row.try_get("reserved_cost_micros").map_err(storage_error)?;
            let budget_row=sqlx::query("SELECT input_tokens+output_tokens+reserved_tokens tokens,cost_micros+reserved_cost_micros cost FROM agent_runs WHERE id=? FOR UPDATE").bind(run.id).fetch_one(&mut *tx).await.map_err(storage_error)?;
            let tokens:u64=budget_row.try_get("tokens").map_err(storage_error)?;
            let cost:u64=budget_row.try_get("cost").map_err(storage_error)?;
            if tokens.saturating_sub(old_reserved_tokens).saturating_add(reserved_tokens)>budget.max_total_tokens{return Err(limit_error("token_limit"));}
            if cost.saturating_sub(old_reserved_cost).saturating_add(reserved_cost)>budget.max_cost_micros{return Err(limit_error("cost_limit"));}
            sqlx::query("UPDATE runtime_calls SET attempt_id=?,status='reserved',reserved_input_tokens=?,reserved_output_tokens=0,reserved_cost_micros=?,error_code=NULL,error_message=NULL,started_at=CURRENT_TIMESTAMP(6),ended_at=NULL WHERE id=?").bind(task.attempt_id).bind(reserved_tokens).bind(reserved_cost).bind(id).execute(&mut *tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE agent_runs SET reserved_tokens=GREATEST(reserved_tokens-?,0)+?,reserved_cost_micros=GREATEST(reserved_cost_micros-?,0)+? WHERE id=?").bind(old_reserved_tokens).bind(reserved_tokens).bind(old_reserved_cost).bind(reserved_cost).bind(run.id).execute(&mut *tx).await.map_err(storage_error)?;
            tx.commit().await.map_err(storage_error)?;return Ok(Reservation::New(id));
        }
        let row=sqlx::query("SELECT model_call_count,tool_call_count,input_tokens+output_tokens+reserved_tokens tokens,cost_micros+reserved_cost_micros cost FROM agent_runs WHERE id=? FOR UPDATE").bind(run.id).fetch_one(&mut *tx).await.map_err(storage_error)?;
        let model_calls: u32 = row.try_get("model_call_count").map_err(storage_error)?;
        let tool_calls: u32 = row.try_get("tool_call_count").map_err(storage_error)?;
        let tokens: u64 = row.try_get("tokens").map_err(storage_error)?;
        let cost: u64 = row.try_get("cost").map_err(storage_error)?;
        if kind == "model" && model_calls >= budget.max_model_calls {
            return Err(limit_error("model_call_limit"));
        }
        if kind == "mcp_tool" && tool_calls >= budget.max_tool_calls {
            return Err(limit_error("tool_call_limit"));
        }
        if tokens.saturating_add(reserved_tokens) > budget.max_total_tokens {
            return Err(limit_error("token_limit"));
        }
        if cost.saturating_add(reserved_cost) > budget.max_cost_micros {
            return Err(limit_error("cost_limit"));
        }
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO runtime_calls(id,tenant_id,execution_id,node_execution_id,attempt_id,agent_run_id,iteration_index,call_index,call_kind,idempotency_key,request_fingerprint,resource_type,resource_id,resource_version_id,side_effect,status,reserved_input_tokens,reserved_cost_micros) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(id).bind(task.tenant_id).bind(task.execution_id).bind(task.node_execution_id).bind(task.attempt_id).bind(run.id).bind(run.iteration).bind(call_index).bind(kind).bind(key).bind(fingerprint).bind(resource.resource_type.as_str()).bind(resource.resource_id).bind(resource.resource_version_id).bind(side_effect).bind("reserved").bind(reserved_tokens).bind(reserved_cost).execute(&mut *tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_runs SET reserved_tokens=reserved_tokens+?,reserved_cost_micros=reserved_cost_micros+?,model_call_count=model_call_count+?,tool_call_count=tool_call_count+? WHERE id=?")
            .bind(reserved_tokens).bind(reserved_cost).bind(u8::from(kind=="model")).bind(u8::from(kind=="mcp_tool")).bind(run.id).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(Reservation::New(id))
    }

    async fn mark_sent(&self, id: Uuid) -> RuntimeResult<()> {
        sqlx::query("UPDATE runtime_calls SET status='sent' WHERE id=? AND status='reserved'")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        Ok(())
    }
    async fn complete_model_call(
        &self,
        task: &RuntimeTask,
        run_id: Uuid,
        id: Uuid,
        response: &ModelResponse,
    ) -> RuntimeResult<()> {
        let already_completed: bool =
            sqlx::query_scalar("SELECT status='succeeded' FROM runtime_calls WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?;
        if already_completed {
            return Ok(());
        }
        let value = serde_json::to_value(response).map_err(protocol_error)?;
        let artifact = self.write_json(task.tenant_id, &value).await?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let row=sqlx::query("SELECT status,reserved_input_tokens+reserved_output_tokens reserved_tokens,reserved_cost_micros FROM runtime_calls WHERE id=? FOR UPDATE").bind(id).fetch_one(&mut *tx).await.map_err(storage_error)?;
        let status: String = row.try_get("status").map_err(storage_error)?;
        if status == "succeeded" {
            tx.commit().await.map_err(storage_error)?;
            return Ok(());
        }
        let reserved_tokens: u64 = row.try_get("reserved_tokens").map_err(storage_error)?;
        let reserved_cost: u64 = row.try_get("reserved_cost_micros").map_err(storage_error)?;
        let changed=sqlx::query("UPDATE runtime_calls SET status='succeeded',reserved_input_tokens=0,reserved_output_tokens=0,reserved_cost_micros=0,input_tokens=?,output_tokens=?,cost_micros=?,usage_estimated=?,response_artifact_id=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=? AND status<>'succeeded'").bind(response.input_tokens).bind(response.output_tokens).bind(response.cost_micros).bind(response.usage_estimated).bind(artifact).bind(id).execute(&mut *tx).await.map_err(storage_error)?.rows_affected();
        if changed > 0 {
            sqlx::query("UPDATE agent_runs SET reserved_tokens=GREATEST(reserved_tokens-?,0),reserved_cost_micros=GREATEST(reserved_cost_micros-?,0),input_tokens=input_tokens+?,output_tokens=output_tokens+?,cost_micros=cost_micros+? WHERE id=?").bind(reserved_tokens).bind(reserved_cost).bind(response.input_tokens).bind(response.output_tokens).bind(response.cost_micros).bind(run_id).execute(&mut *tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE workflow_executions SET input_tokens=input_tokens+?,output_tokens=output_tokens+?,cost_micros=cost_micros+? WHERE tenant_id=? AND id=?").bind(response.input_tokens).bind(response.output_tokens).bind(response.cost_micros).bind(task.tenant_id).bind(task.execution_id).execute(&mut *tx).await.map_err(storage_error)?;
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(())
    }
    async fn complete_tool_call(
        &self,
        id: Uuid,
        run_id: Uuid,
        value: &Value,
        error_code: Option<&str>,
    ) -> RuntimeResult<()> {
        let already_completed: bool =
            sqlx::query_scalar("SELECT status='succeeded' FROM runtime_calls WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?;
        if already_completed {
            return Ok(());
        }
        let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM runtime_calls WHERE id=?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage_error)?;
        let artifact = self.write_json(tenant, value).await?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let row=sqlx::query("SELECT status,reserved_input_tokens+reserved_output_tokens reserved_tokens,reserved_cost_micros FROM runtime_calls WHERE id=? FOR UPDATE").bind(id).fetch_one(&mut *tx).await.map_err(storage_error)?;
        let status: String = row.try_get("status").map_err(storage_error)?;
        if status == "succeeded" {
            tx.commit().await.map_err(storage_error)?;
            return Ok(());
        }
        let reserved_tokens: u64 = row.try_get("reserved_tokens").map_err(storage_error)?;
        let reserved_cost: u64 = row.try_get("reserved_cost_micros").map_err(storage_error)?;
        sqlx::query("UPDATE runtime_calls SET status='succeeded',reserved_input_tokens=0,reserved_output_tokens=0,reserved_cost_micros=0,response_artifact_id=?,error_code=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(artifact).bind(error_code).bind(id).execute(&mut *tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_runs SET reserved_tokens=GREATEST(reserved_tokens-?,0),reserved_cost_micros=GREATEST(reserved_cost_micros-?,0) WHERE id=?").bind(reserved_tokens).bind(reserved_cost).bind(run_id).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(())
    }
    async fn fail_call(&self, id: Uuid, error: &RuntimeError) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let row=sqlx::query("SELECT status,agent_run_id,reserved_input_tokens+reserved_output_tokens reserved_tokens,reserved_cost_micros FROM runtime_calls WHERE id=? FOR UPDATE").bind(id).fetch_one(&mut *tx).await.map_err(storage_error)?;
        let status: String = row.try_get("status").map_err(storage_error)?;
        if matches!(
            status.as_str(),
            "succeeded" | "failed" | "unknown" | "cancelled"
        ) {
            tx.commit().await.map_err(storage_error)?;
            return Ok(());
        }
        let run_id: Uuid = row.try_get("agent_run_id").map_err(storage_error)?;
        let reserved_tokens: u64 = row.try_get("reserved_tokens").map_err(storage_error)?;
        let reserved_cost: u64 = row.try_get("reserved_cost_micros").map_err(storage_error)?;
        sqlx::query("UPDATE runtime_calls SET status=?,reserved_input_tokens=0,reserved_output_tokens=0,reserved_cost_micros=0,error_code=?,error_message=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(if error.outcome_unknown{"unknown"}else{"failed"}).bind(&error.code).bind(&error.message).bind(id).execute(&mut *tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_runs SET reserved_tokens=GREATEST(reserved_tokens-?,0),reserved_cost_micros=GREATEST(reserved_cost_micros-?,0) WHERE id=?").bind(reserved_tokens).bind(reserved_cost).bind(run_id).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(())
    }
    async fn begin_iteration(
        &self,
        task: &RuntimeTask,
        run: &RunState,
        state: &str,
    ) -> RuntimeResult<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO agent_iterations(id,tenant_id,agent_run_id,iteration_index,status,state_before_hash) VALUES(?,?,?,?,'running',?) ON DUPLICATE KEY UPDATE status='running',state_before_hash=VALUES(state_before_hash),ended_at=NULL").bind(id).bind(task.tenant_id).bind(run.id).bind(run.iteration).bind(state).execute(&self.pool).await.map_err(storage_error)?;
        sqlx::query_scalar(
            "SELECT id FROM agent_iterations WHERE agent_run_id=? AND iteration_index=?",
        )
        .bind(run.id)
        .bind(run.iteration)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)
    }
    async fn finish_iteration(
        &self,
        iteration_id: Uuid,
        run_id: Uuid,
        run: &mut RunState,
        state: &str,
        stop: Option<&str>,
    ) -> RuntimeResult<()> {
        let artifact = self.write_json_from_run(run_id, &run.messages).await?;
        sqlx::query("UPDATE agent_iterations SET status='completed',state_after_hash=?,state_artifact_id=?,stop_reason=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(state).bind(artifact).bind(stop).bind(iteration_id).execute(&self.pool).await.map_err(storage_error)?;
        sqlx::query("UPDATE agent_runs SET iteration_count=GREATEST(iteration_count,?),state_hash=?,state_artifact_id=? WHERE id=?").bind(run.iteration+1).bind(state).bind(artifact).bind(run_id).execute(&self.pool).await.map_err(storage_error)?;
        Ok(())
    }
    async fn succeed(
        &self,
        task: &RuntimeTask,
        run: &mut RunState,
        response: ModelResponse,
    ) -> RuntimeResult<TaskResult> {
        sqlx::query("UPDATE agent_runs SET status='succeeded',stop_reason=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(&response.stop_reason).bind(run.id).execute(&self.pool).await.map_err(storage_error)?;
        Ok(completed(
            task,
            json!({"message":response.message,"stopReason":response.stop_reason,"iterations":run.iteration+1,"modelCalls":run.model_calls,"toolCalls":run.tool_calls,"inputOutputTokens":run.tokens,"costMicros":run.cost}),
        ))
    }
    async fn stop(
        &self,
        task: &RuntimeTask,
        run: &mut RunState,
        budget: &Budget,
        reason: &str,
        iteration: Option<Uuid>,
    ) -> RuntimeResult<TaskResult> {
        if let Some(id) = iteration {
            let hash = state_hash(
                &run.messages,
                &serde_json::to_value(budget).map_err(protocol_error)?,
            );
            self.finish_iteration(id, run.id, run, &hash, Some(reason))
                .await?;
        }
        sqlx::query("UPDATE agent_runs SET status='failed',stop_reason=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(reason).bind(run.id).execute(&self.pool).await.map_err(storage_error)?;
        if budget.limit_action == "fail" {
            Err(limit_error(reason))
        } else {
            Ok(limit_output(task, run, budget, reason))
        }
    }
    async fn write_json(&self, tenant: Uuid, value: &Value) -> RuntimeResult<Uuid> {
        let artifact = self
            .runtimes
            .artifacts
            .put(ArtifactWrite {
                tenant_id: agentx_domain::TenantId::from_uuid(tenant),
                content_type: "application/json".into(),
                content: serde_json::to_vec(value).map_err(protocol_error)?,
            })
            .await
            .map_err(storage_error)?;
        Ok(artifact.id.as_uuid())
    }
    async fn write_json_from_run(&self, run_id: Uuid, value: &[Value]) -> RuntimeResult<Uuid> {
        let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM agent_runs WHERE id=?")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await
            .map_err(storage_error)?;
        self.write_json(tenant, &Value::Array(value.to_vec())).await
    }
    async fn load_fingerprints(&self, run: Uuid, iteration: u32) -> RuntimeResult<Vec<String>> {
        sqlx::query_scalar("SELECT request_fingerprint FROM runtime_calls WHERE agent_run_id=? AND call_kind='mcp_tool' AND iteration_index<? ORDER BY iteration_index,call_index").bind(run).bind(iteration).fetch_all(&self.pool).await.map_err(storage_error)
    }
    async fn load_state_hashes(&self, run: Uuid) -> RuntimeResult<Vec<String>> {
        sqlx::query_scalar("SELECT state_after_hash FROM agent_iterations WHERE agent_run_id=? AND state_after_hash IS NOT NULL ORDER BY iteration_index").bind(run).fetch_all(&self.pool).await.map_err(storage_error)
    }
    async fn load_error_counts(&self, run: Uuid) -> RuntimeResult<BTreeMap<String, u32>> {
        let rows=sqlx::query("SELECT request_fingerprint,error_code,COUNT(*) count FROM runtime_calls WHERE agent_run_id=? AND call_kind='mcp_tool' AND error_code IS NOT NULL GROUP BY request_fingerprint,error_code").bind(run).fetch_all(&self.pool).await.map_err(storage_error)?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    format!(
                        "{}:{}",
                        row.try_get::<String, _>("request_fingerprint")
                            .map_err(storage_error)?,
                        row.try_get::<String, _>("error_code")
                            .map_err(storage_error)?
                    ),
                    row.try_get::<u32, _>("count").map_err(storage_error)?,
                ))
            })
            .collect()
    }
    #[allow(clippy::too_many_arguments)]
    async fn emit(
        &self,
        context: &RuntimeContext,
        task: &RuntimeTask,
        run_id: Uuid,
        call_id: Option<Uuid>,
        resource: Option<&ResourceReference>,
        event_type: &str,
        status: &str,
        model: Option<&ModelResponse>,
        error_code: Option<&str>,
        stop_reason: Option<&str>,
        attributes: Value,
    ) {
        let snapshot = resource.and_then(|reference| context.resource(reference));
        let event = TraceEvent {
            event_id: Uuid::now_v7(),
            tenant_id: context.tenant_id,
            trace_id: context.trace_id,
            span_id: Uuid::now_v7(),
            parent_span_id: Some(context.span_id),
            execution_id: context.execution_id,
            workflow_id: context.workflow_id,
            workflow_version_id: context.workflow_version_id,
            node_execution_id: Some(context.node_execution_id),
            attempt_id: Some(context.attempt_id),
            agent_run_id: Some(run_id),
            runtime_call_id: call_id,
            sandbox_id: None,
            resource_type: resource.map(|value| value.resource_type.as_str().into()),
            resource_id: resource.map(|value| value.resource_id),
            resource_version_id: resource.and_then(|value| value.resource_version_id),
            event_type: event_type.into(),
            status: status.into(),
            event_time: OffsetDateTime::now_utc(),
            duration_ms: None,
            run_index: task.run_index,
            iteration_index: task.iteration_index,
            model_name: snapshot
                .and_then(|value| value.snapshot.get("modelName"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            provider_name: snapshot
                .and_then(|value| value.snapshot.get("providerType"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            mcp_tool_name: snapshot
                .and_then(|value| value.snapshot.get("toolName"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            input_tokens: model.map(|value| value.input_tokens),
            output_tokens: model.map(|value| value.output_tokens),
            cost_micros: model.map(|value| value.cost_micros).unwrap_or(0),
            error_code: error_code.map(str::to_owned),
            error_message: None,
            stop_reason: stop_reason.map(str::to_owned),
            partial: model.is_some_and(|value| value.partial),
            content_ref: None,
            attributes,
        };
        let _ = self.runtimes.trace.append(event).await;
    }
}

impl Budget {
    fn from_parameters(value: &Value) -> Self {
        Self {
            max_iterations: number(value, "maxIterations", 12).clamp(1, 12) as u32,
            max_model_calls: number(value, "maxModelCalls", 12).clamp(1, 12) as u32,
            max_tool_calls: number(value, "maxToolCalls", 32).min(32) as u32,
            max_total_tokens: number(value, "maxTotalTokens", 64_000).clamp(1, 64_000),
            max_cost_micros: number(value, "maxCostMicros", 1_000_000).min(1_000_000),
            max_duration_ms: number(value, "maxDurationMs", 300_000).clamp(1_000, 300_000),
            limit_action: value
                .get("limitAction")
                .and_then(Value::as_str)
                .filter(|value| matches!(*value, "fail" | "error_output" | "partial"))
                .unwrap_or("error_output")
                .into(),
            max_output_tokens: value
                .get("maxOutputTokens")
                .and_then(Value::as_u64)
                .or_else(|| {
                    value
                        .get("modelParameters")
                        .and_then(|v| v.get("max_tokens"))
                        .and_then(Value::as_u64)
                })
                .unwrap_or(4096)
                .min(64_000),
        }
    }
}
fn number(value: &Value, key: &str, default: u64) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(default)
}
async fn initial_messages(
    task: &RuntimeTask,
    context: &RuntimeContext,
    parameters: &Value,
    runtimes: &ResourceRuntimes,
) -> RuntimeResult<Vec<Value>> {
    let mut system = parameters
        .get("systemPrompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    for reference in task
        .resource_references
        .iter()
        .filter(|value| value.resource_type == ResourceType::Skill)
    {
        let bundle = runtimes.skill.load(context, reference.clone()).await?;
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(&bundle.instructions);
    }
    let mut result = Vec::new();
    if !system.is_empty() {
        result.push(json!({"role":"system","content":system}));
    }
    if let Some(messages) = parameters.get("messages").and_then(Value::as_array) {
        result.extend(messages.clone())
    } else {
        result.push(json!({"role":"user","content":first_input(task)}));
    }
    Ok(result)
}
fn tool_catalog(task: &RuntimeTask) -> RuntimeResult<BTreeMap<String, (ResourceReference, Value)>> {
    let mut result = BTreeMap::new();
    for snapshot in task
        .resource_snapshots
        .iter()
        .filter(|value| value.reference.resource_type == ResourceType::McpTool)
    {
        let name = snapshot
            .snapshot
            .get("toolName")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::new("MCP_TOOL_SNAPSHOT_INVALID", "Tool name is missing"))?
            .to_owned();
        if result
            .insert(
                name.clone(),
                (snapshot.reference.clone(), snapshot.snapshot.clone()),
            )
            .is_some()
        {
            return Err(RuntimeError::new(
                "MCP_TOOL_NAME_CONFLICT",
                format!("Agent has duplicate tool name {name}"),
            ));
        }
    }
    Ok(result)
}
fn tool_schema(value: &(ResourceReference, Value)) -> RuntimeResult<Value> {
    Ok(
        json!({"type":"function","function":{"name":value.1.get("toolName").and_then(Value::as_str).ok_or_else(||RuntimeError::new("MCP_TOOL_SNAPSHOT_INVALID","Tool name is missing"))?,"description":value.1.get("title").cloned().unwrap_or(Value::Null),"parameters":value.1.get("inputSchema").cloned().unwrap_or_else(||json!({"type":"object"}))}}),
    )
}
fn parse_tool_calls(
    calls: &[Value],
    catalog: &BTreeMap<String, (ResourceReference, Value)>,
) -> RuntimeResult<Vec<ToolInvocation>> {
    calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let function = call.get("function").ok_or_else(|| {
                RuntimeError::new("MODEL_TOOL_CALL_INVALID", "Tool call has no function")
            })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RuntimeError::new("MODEL_TOOL_CALL_INVALID", "Tool call has no name")
                })?;
            let (resource, snapshot) = catalog.get(name).ok_or_else(|| {
                RuntimeError::new(
                    "MODEL_TOOL_NOT_AUTHORIZED",
                    format!("Model requested unavailable tool {name}"),
                )
            })?;
            let arguments = match function.get("arguments") {
                Some(Value::String(value)) => serde_json::from_str(value).map_err(|_| {
                    RuntimeError::new(
                        "MODEL_TOOL_ARGUMENTS_INVALID",
                        "Tool arguments are not valid JSON",
                    )
                })?,
                Some(value) => value.clone(),
                None => json!({}),
            };
            let version = snapshot
                .get("toolVersionId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(format!("{version}{}", canonical_json(&arguments)).as_bytes())
            );
            Ok(ToolInvocation {
                index,
                call_id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("tool-call")
                    .into(),
                resource: resource.clone(),
                arguments,
                fingerprint,
                side_effect: snapshot
                    .get("sideEffect")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .into(),
            })
        })
        .collect()
}
fn tool_name(task: &RuntimeTask, resource: &ResourceReference) -> String {
    task.resource_snapshots
        .iter()
        .find(|value| value.reference == *resource)
        .and_then(|value| value.snapshot.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .into()
}
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(values) => {
            let ordered = values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            format!(
                "{{{}}}",
                ordered
                    .iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}
fn hash_json(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical_json(value).as_bytes()))
}
fn state_hash(messages: &[Value], settings: &Value) -> String {
    hash_json(&json!({"messages":messages,"settings":settings}))
}
fn repeated_fingerprint(values: &[String], limit: usize) -> bool {
    values.len() >= limit
        && values[values.len() - limit..]
            .iter()
            .all(|value| value == values.last().unwrap())
}
fn abab(values: &[String]) -> bool {
    values.len() >= 4
        && values[values.len() - 4] == values[values.len() - 2]
        && values[values.len() - 3] == values[values.len() - 1]
        && values[values.len() - 4] != values[values.len() - 3]
}
fn state_stalled(values: &[String]) -> bool {
    values.len() >= 3
        && values[values.len() - 3..]
            .iter()
            .all(|value| value == values.last().unwrap())
}
fn limits(run: &RunState, budget: &Budget) -> Option<String> {
    if run.iteration >= budget.max_iterations {
        Some("iteration_limit".into())
    } else if run.model_calls >= budget.max_model_calls {
        Some("model_call_limit".into())
    } else if run.tool_calls >= budget.max_tool_calls {
        Some("tool_call_limit".into())
    } else if run.tokens >= budget.max_total_tokens {
        Some("token_limit".into())
    } else if run.cost >= budget.max_cost_micros {
        Some("cost_limit".into())
    } else if (OffsetDateTime::now_utc() - run.started_at).whole_milliseconds() as u64
        >= budget.max_duration_ms
    {
        Some("duration_limit".into())
    } else {
        None
    }
}
fn estimated_cost(task: &RuntimeTask, input: u64, output: u64) -> RuntimeResult<u64> {
    let snapshot = task
        .resource_snapshots
        .iter()
        .find(|value| value.reference.resource_type == ResourceType::Model)
        .ok_or_else(|| {
            RuntimeError::new("MODEL_SNAPSHOT_MISSING", "Agent Model snapshot is missing")
        })?;
    let price = snapshot
        .snapshot
        .get("price")
        .ok_or_else(|| RuntimeError::new("MODEL_PRICE_MISSING", "Model price is missing"))?;
    if price.get("currency").and_then(Value::as_str) != Some("USD") {
        return Err(RuntimeError::new(
            "MODEL_PRICE_CURRENCY_UNSUPPORTED",
            "M5 supports USD prices only",
        ));
    }
    let parse = |key| {
        price
            .get(key)
            .and_then(Value::as_str)
            .and_then(|value| Decimal::from_str(value).ok())
            .ok_or_else(|| {
                RuntimeError::new(
                    "MODEL_PRICE_INVALID",
                    format!("Model price {key} is invalid"),
                )
            })
    };
    (Decimal::from(input) * parse("inputPerMillion")?
        + Decimal::from(output) * parse("outputPerMillion")?)
    .ceil()
    .to_u64()
    .ok_or_else(|| RuntimeError::new("MODEL_PRICE_INVALID", "Model price exceeds u64"))
}
fn completed(task: &RuntimeTask, json: Value) -> TaskResult {
    let mut item = task
        .inputs
        .values()
        .flatten()
        .next()
        .cloned()
        .unwrap_or_default();
    item.json = json;
    TaskResult::Completed(BTreeMap::from([("main".into(), vec![item])]))
}
fn limit_output(task: &RuntimeTask, run: &RunState, budget: &Budget, reason: &str) -> TaskResult {
    completed(
        task,
        json!({"error":{"code":"AGENT_LIMIT_REACHED","stopReason":reason},"partial":budget.limit_action=="partial","iterations":run.iteration,"modelCalls":run.model_calls,"toolCalls":run.tool_calls,"tokens":run.tokens,"costMicros":run.cost}),
    )
}
fn unknown_outcome_can_retry(side_effect: &str) -> bool {
    matches!(side_effect, "none" | "read_only" | "idempotent")
}
fn limit_error(reason: &str) -> RuntimeError {
    let mut error = RuntimeError::new(
        "AGENT_LIMIT_REACHED",
        format!("Agent stopped because {reason}"),
    );
    error.attributes = json!({"stopReason":reason});
    error
}
fn limit_reason(error: &RuntimeError) -> Option<&str> {
    (error.code == "AGENT_LIMIT_REACHED")
        .then(|| error.attributes.get("stopReason").and_then(Value::as_str))
        .flatten()
}
fn storage_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new("AGENT_LEDGER_UNAVAILABLE", error.to_string()).retryable(true)
}
fn protocol_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new("AGENT_STATE_INVALID", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentx_application::RuntimeResourceSnapshot;
    use agentx_domain::{ResourceOperation, ResourceType};

    fn task_with_model_price(price: Value) -> RuntimeTask {
        let reference = ResourceReference {
            binding_id: None,
            binding_role: None,
            resource_type: ResourceType::Model,
            resource_id: Uuid::now_v7(),
            resource_version_id: Some(Uuid::now_v7()),
            operation: ResourceOperation::Use,
        };
        RuntimeTask {
            tenant_id: Uuid::now_v7(),
            workflow_id: Uuid::now_v7(),
            workflow_service_identity_id: Uuid::now_v7(),
            workflow_version_id: Some(Uuid::now_v7()),
            execution_id: Uuid::now_v7(),
            node_execution_id: Uuid::now_v7(),
            attempt_id: Uuid::now_v7(),
            attempt_number: 1,
            node_type: "agent".into(),
            node_version: 1,
            node_parameters: json!({}),
            inputs: BTreeMap::new(),
            run_index: 0,
            iteration_index: 0,
            capability: "agent".into(),
            idempotency_key: "agent-test".into(),
            deadline: OffsetDateTime::now_utc() + time::Duration::minutes(1),
            mode: "normal".into(),
            trace_id: Uuid::now_v7(),
            linked_nodes: json!({}),
            resource_references: vec![reference.clone()],
            resource_snapshots: vec![RuntimeResourceSnapshot {
                node_id: "agent".into(),
                reference,
                snapshot_hash: "test".into(),
                snapshot: json!({"price":price}),
            }],
        }
    }

    fn run_state() -> RunState {
        RunState {
            id: Uuid::now_v7(),
            messages: Vec::new(),
            iteration: 0,
            model_calls: 0,
            tool_calls: 0,
            tokens: 0,
            cost: 0,
            started_at: OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn detects_loops_without_fuzzy_matching() {
        assert!(repeated_fingerprint(
            &["a".into(), "a".into(), "a".into()],
            3
        ));
        assert!(abab(&["a".into(), "b".into(), "a".into(), "b".into()]));
        assert!(!abab(&["a".into(), "b".into(), "a".into(), "c".into()]));
        assert!(!repeated_fingerprint(&["a".into(), "a".into()], 3));
        assert!(!repeated_fingerprint(
            &["a".into(), "a".into(), "b".into()],
            3
        ));
        assert!(state_stalled(&["x".into(), "x".into(), "x".into()]));
        assert!(!state_stalled(&["x".into(), "x".into(), "y".into()]));
    }
    #[test]
    fn canonicalizes_object_keys() {
        assert_eq!(canonical_json(&json!({"b":2,"a":1})), "{\"a\":1,\"b\":2}");
    }
    #[test]
    fn output_token_budget_can_be_lowered_independently() {
        let budget = Budget::from_parameters(&json!({
            "maxTotalTokens": 4096,
            "maxOutputTokens": 512
        }));
        assert_eq!(budget.max_total_tokens, 4096);
        assert_eq!(budget.max_output_tokens, 512);
    }

    #[test]
    fn limit_actions_and_platform_caps_are_stable() {
        for (configured, expected) in [
            ("fail", "fail"),
            ("error_output", "error_output"),
            ("partial", "partial"),
            ("invalid", "error_output"),
        ] {
            assert_eq!(
                Budget::from_parameters(&json!({"limitAction":configured})).limit_action,
                expected
            );
        }
        let capped = Budget::from_parameters(&json!({
            "maxIterations":999,
            "maxModelCalls":999,
            "maxToolCalls":999,
            "maxTotalTokens":999999,
            "maxCostMicros":999999999,
            "maxDurationMs":999999999
        }));
        assert_eq!(capped.max_iterations, 12);
        assert_eq!(capped.max_model_calls, 12);
        assert_eq!(capped.max_tool_calls, 32);
        assert_eq!(capped.max_total_tokens, 64_000);
        assert_eq!(capped.max_cost_micros, 1_000_000);
        assert_eq!(capped.max_duration_ms, 300_000);
    }

    #[test]
    fn error_output_and_partial_have_distinct_item_semantics() {
        let task = task_with_model_price(json!({
            "currency":"USD",
            "inputPerMillion":"1",
            "outputPerMillion":"1"
        }));
        let run = run_state();
        for (action, partial) in [("error_output", false), ("partial", true)] {
            let budget = Budget::from_parameters(&json!({"limitAction":action}));
            let TaskResult::Completed(outputs) = limit_output(&task, &run, &budget, "token_limit")
            else {
                panic!("limit action must return an output item");
            };
            let value = &outputs["main"][0].json;
            assert_eq!(value["partial"], partial);
            assert_eq!(value["error"]["code"], "AGENT_LIMIT_REACHED");
            assert_eq!(value["error"]["stopReason"], "token_limit");
        }
        assert_eq!(limit_error("token_limit").code, "AGENT_LIMIT_REACHED");
    }

    #[test]
    fn unknown_outcome_retry_policy_matches_side_effect_contract() {
        for value in ["none", "read_only", "idempotent"] {
            assert!(unknown_outcome_can_retry(value));
        }
        for value in ["reversible", "irreversible", "unknown"] {
            assert!(!unknown_outcome_can_retry(value));
        }
    }

    #[test]
    fn estimated_cost_uses_decimal_arithmetic_and_rejects_non_usd() {
        let task = task_with_model_price(json!({
            "currency":"USD",
            "inputPerMillion":"0.1",
            "outputPerMillion":"0.2"
        }));
        assert_eq!(estimated_cost(&task, 3, 2).unwrap(), 1);

        let non_usd = task_with_model_price(json!({
            "currency":"EUR",
            "inputPerMillion":"1",
            "outputPerMillion":"1"
        }));
        assert_eq!(
            estimated_cost(&non_usd, 1, 1).unwrap_err().code,
            "MODEL_PRICE_CURRENCY_UNSUPPORTED"
        );
    }

    #[derive(Default)]
    struct UnusedRuntime;

    #[async_trait::async_trait]
    impl agentx_application::ModelRuntime for UnusedRuntime {
        async fn complete(
            &self,
            _context: &RuntimeContext,
            _request: ModelRequest,
        ) -> RuntimeResult<ModelResponse> {
            Err(RuntimeError::new("TEST_UNUSED", "unused model runtime"))
        }

        async fn stream(
            &self,
            _context: &RuntimeContext,
            _request: ModelRequest,
        ) -> RuntimeResult<agentx_application::RuntimeStream<agentx_application::ModelEvent>>
        {
            Err(RuntimeError::new("TEST_UNUSED", "unused model runtime"))
        }
    }

    #[async_trait::async_trait]
    impl agentx_application::McpToolRuntime for UnusedRuntime {
        async fn call(
            &self,
            _context: &RuntimeContext,
            _request: McpToolRequest,
        ) -> RuntimeResult<agentx_application::McpToolResponse> {
            Err(RuntimeError::new("TEST_UNUSED", "unused MCP runtime"))
        }
    }

    #[async_trait::async_trait]
    impl agentx_application::SkillRuntime for UnusedRuntime {
        async fn load(
            &self,
            _context: &RuntimeContext,
            _resource: ResourceReference,
        ) -> RuntimeResult<agentx_application::SkillBundle> {
            Err(RuntimeError::new("TEST_UNUSED", "unused Skill runtime"))
        }
    }

    #[async_trait::async_trait]
    impl agentx_application::RagRuntime for UnusedRuntime {
        async fn execute(
            &self,
            _context: &RuntimeContext,
            _request: agentx_application::RagRequest,
        ) -> RuntimeResult<Value> {
            Err(RuntimeError::new("TEST_UNUSED", "unused RAG runtime"))
        }
    }

    #[async_trait::async_trait]
    impl agentx_application::MemoryRuntime for UnusedRuntime {
        async fn execute(
            &self,
            _context: &RuntimeContext,
            _request: agentx_application::MemoryRequest,
        ) -> RuntimeResult<Value> {
            Err(RuntimeError::new("TEST_UNUSED", "unused Memory runtime"))
        }
    }

    #[tokio::test]
    async fn ledger_serializes_budget_and_recovers_calls_across_attempts() {
        use std::{sync::Arc, time::Duration as StdDuration};

        use agentx_infrastructure::{
            artifact::MySqlObjectArtifactStore, config::MySqlSettings, mysql,
            operations_projection::MySqlOperationsProjection,
        };
        use object_store::memory::InMemory;
        use secrecy::SecretString;
        use testcontainers::{
            GenericImage, ImageExt,
            core::{IntoContainerPort, WaitFor},
            runners::AsyncRunner,
        };

        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .expect("start MySQL");
        let settings = MySqlSettings {
            host: "127.0.0.1".into(),
            port: container.get_host_port_ipv4(3306.tcp()).await.unwrap(),
            database: "agentx".into(),
            username: "agentx".into(),
            password: SecretString::from("agentx-test-password"),
            max_connections: 8,
            tls_mode: agentx_infrastructure::config::MySqlTlsMode::Disabled,
            tls_ca_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
        };
        let pool = loop {
            match mysql::connect(&settings).await {
                Ok(pool) => break pool,
                Err(_) => tokio::time::sleep(StdDuration::from_millis(250)).await,
            }
        };
        mysql::run_migrations(&pool).await.expect("run migrations");
        let (task, run) = seed_agent_ledger(&pool).await;
        let unused = Arc::new(UnusedRuntime);
        let artifacts = Arc::new(MySqlObjectArtifactStore::new(
            pool.clone(),
            Arc::new(InMemory::new()),
        ));
        let runner = AgentRunner::new(
            pool.clone(),
            ResourceRuntimes {
                model: unused.clone(),
                mcp: unused.clone(),
                skill: unused.clone(),
                rag: unused.clone(),
                memory: unused,
                sandbox: None,
                artifacts,
                trace: MySqlOperationsProjection::new(pool.clone()),
            },
        );
        let budget = Budget::from_parameters(&json!({
            "maxTotalTokens":100,
            "maxCostMicros":1000
        }));
        let resource = task.resource_references[0].clone();
        let (left, right) = tokio::join!(
            runner.reserve_call(
                &task,
                &run,
                &budget,
                "budget-a",
                "fingerprint-a",
                "model",
                &resource,
                "none",
                60,
                0,
                0,
            ),
            runner.reserve_call(
                &task,
                &run,
                &budget,
                "budget-b",
                "fingerprint-b",
                "model",
                &resource,
                "none",
                60,
                0,
                0,
            )
        );
        let (model_call, model_key, model_fingerprint) = match (left, right) {
            (Ok(Reservation::New(id)), Err(error)) => {
                assert_eq!(limit_reason(&error), Some("token_limit"));
                (id, "budget-a", "fingerprint-a")
            }
            (Err(error), Ok(Reservation::New(id))) => {
                assert_eq!(limit_reason(&error), Some("token_limit"));
                (id, "budget-b", "fingerprint-b")
            }
            _ => panic!("exactly one concurrent reservation must succeed"),
        };
        assert_eq!(
            sqlx::query_scalar::<_, u64>("SELECT reserved_tokens FROM agent_runs WHERE id=?")
                .bind(run.id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            60
        );

        runner.mark_sent(model_call).await.unwrap();
        let response = ModelResponse {
            message: json!({"role":"assistant","content":"done"}),
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            cost_micros: 7,
            usage_estimated: false,
            stop_reason: "stop".into(),
            partial: false,
        };
        runner
            .complete_model_call(&task, run.id, model_call, &response)
            .await
            .unwrap();
        runner
            .complete_model_call(&task, run.id, model_call, &response)
            .await
            .unwrap();

        let mut retry_task = task.clone();
        retry_task.attempt_id = Uuid::now_v7();
        sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,lease_token,deadline_at) VALUES(?,?,?,?,2,'running','agent-ledger-attempt-2',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
            .bind(retry_task.attempt_id).bind(task.tenant_id).bind(task.execution_id).bind(task.node_execution_id).bind(Uuid::now_v7()).execute(&pool).await.unwrap();
        assert!(matches!(
            runner
                .reserve_call(
                    &retry_task,
                    &run,
                    &budget,
                    model_key,
                    model_fingerprint,
                    "model",
                    &resource,
                    "none",
                    60,
                    0,
                    0,
                )
                .await
                .unwrap(),
            Reservation::Reused(_)
        ));

        let irreversible = runner
            .reserve_call(
                &task,
                &run,
                &budget,
                "irreversible",
                "irreversible-fingerprint",
                "mcp_tool",
                &resource,
                "irreversible",
                0,
                0,
                1,
            )
            .await
            .unwrap();
        let Reservation::New(irreversible) = irreversible else {
            panic!("first irreversible call must reserve");
        };
        runner.mark_sent(irreversible).await.unwrap();
        let error = runner
            .reserve_call(
                &retry_task,
                &run,
                &budget,
                "irreversible",
                "irreversible-fingerprint",
                "mcp_tool",
                &resource,
                "irreversible",
                0,
                0,
                1,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "EXTERNAL_CALL_OUTCOME_UNKNOWN");
        assert!(error.outcome_unknown);

        let idempotent = runner
            .reserve_call(
                &task,
                &run,
                &budget,
                "idempotent",
                "idempotent-fingerprint",
                "mcp_tool",
                &resource,
                "idempotent",
                0,
                0,
                2,
            )
            .await
            .unwrap();
        let Reservation::New(idempotent_id) = idempotent else {
            panic!("first idempotent call must reserve");
        };
        runner.mark_sent(idempotent_id).await.unwrap();
        assert!(matches!(
            runner
                .reserve_call(
                    &retry_task,
                    &run,
                    &budget,
                    "idempotent",
                    "idempotent-fingerprint",
                    "mcp_tool",
                    &resource,
                    "idempotent",
                    0,
                    0,
                    2,
                )
                .await
                .unwrap(),
            Reservation::New(id) if id == idempotent_id
        ));

        let ledger = sqlx::query("SELECT reserved_tokens,input_tokens+output_tokens tokens,cost_micros,model_call_count,tool_call_count FROM agent_runs WHERE id=?")
            .bind(run.id).fetch_one(&pool).await.unwrap();
        assert_eq!(ledger.try_get::<u64, _>("reserved_tokens").unwrap(), 0);
        assert_eq!(ledger.try_get::<u64, _>("tokens").unwrap(), 15);
        assert_eq!(ledger.try_get::<u64, _>("cost_micros").unwrap(), 7);
        assert_eq!(ledger.try_get::<u32, _>("model_call_count").unwrap(), 1);
        assert_eq!(ledger.try_get::<u32, _>("tool_call_count").unwrap(), 2);
        assert_eq!(
            sqlx::query_scalar::<_, u64>(
                "SELECT input_tokens+output_tokens FROM workflow_executions WHERE id=?"
            )
            .bind(task.execution_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            15
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM artifacts WHERE tenant_id=?")
                .bind(task.tenant_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    async fn seed_agent_ledger(pool: &MySqlPool) -> (RuntimeTask, RunState) {
        let task = task_with_model_price(json!({
            "currency":"USD",
            "inputPerMillion":"1",
            "outputPerMillion":"1"
        }));
        let user = Uuid::now_v7();
        let department = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Agent Ledger','agent ledger')",
        )
        .bind(task.tenant_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
            .bind(department).bind(task.tenant_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,'agent-ledger','agent-ledger','Agent Ledger')")
            .bind(user).bind(task.tenant_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Agent Ledger',?,?)")
            .bind(task.workflow_id).bind(task.tenant_id).bind(user).bind(department).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'2.0',JSON_OBJECT(),'agent-ledger',?)")
            .bind(task.workflow_version_id).bind(task.tenant_id).bind(task.workflow_id).bind(user).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(?,?,?,?,?,'manual','running',CURRENT_TIMESTAMP(6))")
            .bind(task.execution_id).bind(task.tenant_id).bind(task.workflow_id).bind(task.workflow_version_id).bind(task.trace_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES(?,?,?,'agent','Agent','agent',1,0,0,0,'running','agent')")
            .bind(task.node_execution_id).bind(task.tenant_id).bind(task.execution_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,lease_token,deadline_at) VALUES(?,?,?,?,1,'running','agent-ledger-attempt-1',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
            .bind(task.attempt_id).bind(task.tenant_id).bind(task.execution_id).bind(task.node_execution_id).bind(Uuid::now_v7()).execute(pool).await.unwrap();
        let run = run_state();
        sqlx::query("INSERT INTO agent_runs(id,tenant_id,execution_id,node_execution_id,status,budget_json) VALUES(?,?,?,?,'running',JSON_OBJECT())")
            .bind(run.id).bind(task.tenant_id).bind(task.execution_id).bind(task.node_execution_id).execute(pool).await.unwrap();
        (task, run)
    }
}
