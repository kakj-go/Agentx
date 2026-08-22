use super::*;

impl RuntimeWorker {
    pub(super) async fn emit_resolved_parameters(&self, claim: &ClaimedWorkerAttempt) {
        let mut trace = crate::trace_delivery::TraceDraft::span(
            claim.task.tenant_id,
            claim.task.execution_id,
            claim.task.attempt_id,
            Some((
                claim.task.node_execution_id,
                agentx_runtime_contracts::TraceSpanKindV1::Node,
            )),
            agentx_runtime_contracts::TraceSpanKindV1::Attempt,
            "Resolved node parameters",
            agentx_runtime_contracts::TraceEventKindV1::Updated,
            "node.parameters.resolved",
            "running",
        );
        trace.node_execution_id = Some(claim.task.node_execution_id);
        trace.attempt_id = Some(claim.task.attempt_id);
        trace.attributes = json!({"diagnostic":"resolved_parameters","nodeType":claim.node_type,"stringConversions":claim.string_conversions});
        trace.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::ResolvedParameters);
        trace.content_preview = crate::trace_delivery::bounded_preview(&json!({
            "common": claim.node_parameters,
            "perItem": claim.per_item_parameters,
        }));
        let Ok(mut tx) = self.pool.begin().await else {
            return;
        };
        if crate::trace_delivery::enqueue(&mut tx, trace).await.is_ok() {
            let _ = tx.commit().await;
        }
    }

    pub(super) async fn emit_agent_iteration_finish(
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

    pub(super) async fn emit_agent_span(
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
        trace.content_kind = Some(match (iteration_id.is_some(), event_kind) {
            (true, agentx_runtime_contracts::TraceEventKindV1::Started) => {
                agentx_runtime_contracts::TraceContentKindV1::IterationInput
            }
            (true, _) => agentx_runtime_contracts::TraceContentKindV1::IterationOutput,
            (false, agentx_runtime_contracts::TraceEventKindV1::Started) => {
                agentx_runtime_contracts::TraceContentKindV1::AgentInput
            }
            (false, _) => agentx_runtime_contracts::TraceContentKindV1::AgentOutput,
        });
        trace.content_preview = content.and_then(crate::trace_delivery::bounded_preview);
        let Ok(mut tx) = self.pool.begin().await else {
            return;
        };
        if crate::trace_delivery::enqueue(&mut tx, trace).await.is_ok() {
            let _ = tx.commit().await;
        }
    }
}
