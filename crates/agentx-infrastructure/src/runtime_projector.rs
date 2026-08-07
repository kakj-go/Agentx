use std::sync::Arc;

use agentx_application::{ArtifactStore, RuntimeEventEnvelope};
use agentx_domain::{ArtifactId, TenantId};
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

const PROJECTOR_NAME: &str = "m7-business-v1";

#[derive(Clone)]
pub struct RuntimeEventProjector {
    pool: MySqlPool,
    artifacts: Option<Arc<dyn ArtifactStore>>,
}

impl RuntimeEventProjector {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self {
            pool,
            artifacts: None,
        }
    }

    #[must_use]
    pub fn with_artifacts(mut self, artifacts: Arc<dyn ArtifactStore>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }

    pub async fn project_batch(&self, limit: u32) -> Result<u64> {
        let rows = sqlx::query("SELECT o.id,o.payload_json FROM outbox_events o LEFT JOIN projection_receipts r ON r.projector_name=? AND r.event_id=o.id WHERE r.event_id IS NULL AND (o.event_type LIKE 'runtime.%' OR o.event_type LIKE 'execution.%' OR o.event_type LIKE 'node.%') ORDER BY o.occurred_at,o.id LIMIT ?")
            .bind(PROJECTOR_NAME)
            .bind(limit.clamp(1, 500))
            .fetch_all(&self.pool)
            .await?;
        let mut projected = 0;
        for row in rows {
            let event_id: Uuid = row.try_get("id")?;
            let payload: Value = row.try_get("payload_json")?;
            let event: RuntimeEventEnvelope = serde_json::from_value(payload)
                .with_context(|| format!("Runtime Event {event_id} has an invalid envelope"))?;
            if self.project(&event).await? {
                projected += 1;
            }
        }
        Ok(projected)
    }

    async fn project(&self, event: &RuntimeEventEnvelope) -> Result<bool> {
        let execution_result = self.load_execution_result(event).await?;
        let mut transaction = self.pool.begin().await?;
        let receipt = sqlx::query("INSERT IGNORE INTO projection_receipts(projector_name,event_id,tenant_id) VALUES(?,?,?)")
            .bind(PROJECTOR_NAME)
            .bind(event.event_id)
            .bind(event.tenant_id.as_uuid())
            .execute(&mut *transaction)
            .await?;
        if receipt.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        if event.event_type == "runtime.command.completed" {
            project_command_completed(&mut transaction, event).await?;
        } else if event.event_type == "runtime.command.failed" {
            project_command_failed(&mut transaction, event).await?;
        } else if event.execution_id.is_some() {
            project_execution_event(&mut transaction, event, execution_result.as_ref()).await?;
        }
        transaction.commit().await?;
        Ok(true)
    }

    async fn load_execution_result(&self, event: &RuntimeEventEnvelope) -> Result<Option<Value>> {
        if event.event_type != "execution.succeeded" {
            return Ok(None);
        }
        let Some(execution_id) = event.execution_id else {
            return Ok(None);
        };
        let row = sqlx::query("SELECT result_json,result_artifact_id FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(event.tenant_id.as_uuid())
            .bind(execution_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        if let Some(result) = row.try_get::<Option<Value>, _>("result_json")? {
            return Ok(Some(result));
        }
        let Some(artifact_id) = row.try_get::<Option<Uuid>, _>("result_artifact_id")? else {
            return Ok(None);
        };
        let artifacts = self.artifacts.as_ref().context(
            "Execution result is externalized but the business Projector has no Artifact Store",
        )?;
        let artifact = artifacts
            .get(
                TenantId::from_uuid(event.tenant_id.as_uuid()),
                ArtifactId::from_uuid(artifact_id),
            )
            .await?
            .context("Externalized Execution result Artifact is missing")?;
        Ok(Some(serde_json::from_slice(&artifact.content).context(
            "Externalized Execution result Artifact is invalid JSON",
        )?))
    }
}

async fn project_command_failed(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
) -> Result<()> {
    match event.aggregate_type.as_str() {
        "application_invocation" => {
            let invocation_id = Uuid::parse_str(&event.aggregate_id)?;
            sqlx::query(
                "SELECT id FROM application_invocations WHERE tenant_id=? AND id=? FOR UPDATE",
            )
            .bind(event.tenant_id.as_uuid())
            .bind(invocation_id)
            .fetch_optional(&mut **transaction)
            .await?;
            sqlx::query("UPDATE application_invocations SET status=IF(status IN ('completed','cancelled'),status,'failed'),completed_at=COALESCE(completed_at,CURRENT_TIMESTAMP(6)) WHERE tenant_id=? AND id=?")
                .bind(event.tenant_id.as_uuid())
                .bind(invocation_id)
                .execute(&mut **transaction)
                .await?;
            append_invocation_event(
                transaction,
                event.tenant_id.as_uuid(),
                invocation_id,
                "invocation.failed",
                json!({"code":event.payload.get("code"),"message":event.payload.get("message"),"runtimeEventId":event.event_id}),
            )
            .await?;
        }
        "approval_task" => {
            sqlx::query("UPDATE approval_tasks SET resume_status='failed' WHERE tenant_id=? AND id=? AND resume_status='pending'")
                .bind(event.tenant_id.as_uuid())
                .bind(Uuid::parse_str(&event.aggregate_id)?)
                .execute(&mut **transaction)
                .await?;
        }
        "evaluation_run_case" => {
            let case_id = Uuid::parse_str(&event.aggregate_id)?;
            let run_id: Option<Uuid> = sqlx::query_scalar("SELECT evaluation_run_id FROM evaluation_run_cases WHERE tenant_id=? AND id=? FOR UPDATE")
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .fetch_optional(&mut **transaction)
                .await?;
            sqlx::query("UPDATE evaluation_run_cases SET status='failed',error_code=?,error_message=?,completed_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status NOT IN ('completed','cancelled')")
                .bind(event.payload.get("code").and_then(Value::as_str).unwrap_or("RUNTIME_COMMAND_FAILED"))
                .bind(event.payload.get("message").and_then(Value::as_str).unwrap_or("Runtime command failed"))
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .execute(&mut **transaction)
                .await?;
            if let Some(run_id) = run_id {
                sqlx::query("UPDATE evaluation_runs SET status=IF(EXISTS(SELECT 1 FROM evaluation_run_cases c WHERE c.evaluation_run_id=? AND c.status IN ('queued','running','scoring')),'running','failed'),completed_at=IF(EXISTS(SELECT 1 FROM evaluation_run_cases c WHERE c.evaluation_run_id=? AND c.status IN ('queued','running','scoring')),NULL,CURRENT_TIMESTAMP(6)) WHERE tenant_id=? AND id=? AND status<>'cancelled'")
                    .bind(run_id)
                    .bind(run_id)
                    .bind(event.tenant_id.as_uuid())
                    .bind(run_id)
                    .execute(&mut **transaction)
                    .await?;
            }
        }
        "evaluation_rule_result" => {
            let result_id = Uuid::parse_str(&event.aggregate_id)?;
            let case_id: Option<Uuid> = sqlx::query_scalar("SELECT evaluation_run_case_id FROM evaluation_rule_results WHERE tenant_id=? AND id=? FOR UPDATE")
                .bind(event.tenant_id.as_uuid())
                .bind(result_id)
                .fetch_optional(&mut **transaction)
                .await?;
            sqlx::query("UPDATE evaluation_rule_results SET status='error',detail_json=?,completed_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status IN ('queued','running')")
                .bind(json!({"code":event.payload.get("code"),"message":event.payload.get("message")}))
                .bind(event.tenant_id.as_uuid())
                .bind(result_id)
                .execute(&mut **transaction)
                .await?;
            if let Some(case_id) = case_id {
                crate::evaluation_projector::finalize_evaluation_case(
                    transaction,
                    event.tenant_id.as_uuid(),
                    case_id,
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn project_command_completed(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
) -> Result<()> {
    match event.aggregate_type.as_str() {
        "application_invocation" => {
            let invocation_id = Uuid::parse_str(&event.aggregate_id)?;
            let execution_id = event.execution_id.map(|id| id.as_uuid());
            sqlx::query(
                "SELECT id FROM application_invocations WHERE tenant_id=? AND id=? FOR UPDATE",
            )
            .bind(event.tenant_id.as_uuid())
            .bind(invocation_id)
            .fetch_optional(&mut **transaction)
            .await?;
            let command_type = event
                .payload
                .get("commandType")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let status = if command_type == "cancel_execution" {
                "cancelled"
            } else {
                "running"
            };
            sqlx::query("UPDATE application_invocations SET execution_id=COALESCE(execution_id,?),status=IF(status IN ('completed','failed','cancelled'),status,?),completed_at=IF(?='cancelled',COALESCE(completed_at,CURRENT_TIMESTAMP(6)),completed_at) WHERE tenant_id=? AND id=?")
                .bind(execution_id)
                .bind(status)
                .bind(status)
                .bind(event.tenant_id.as_uuid())
                .bind(invocation_id)
                .execute(&mut **transaction)
                .await?;
            append_invocation_event(
                transaction,
                event.tenant_id.as_uuid(),
                invocation_id,
                if command_type == "cancel_execution" {
                    "invocation.cancelled"
                } else {
                    "invocation.accepted"
                },
                json!({"executionId":execution_id,"status":status,"runtimeEventId":event.event_id}),
            )
            .await?;
        }
        "approval_task" => {
            let task_id = Uuid::parse_str(&event.aggregate_id)?;
            sqlx::query("UPDATE approval_tasks SET resume_status='succeeded' WHERE tenant_id=? AND id=? AND resume_status='pending'")
                .bind(event.tenant_id.as_uuid())
                .bind(task_id)
                .execute(&mut **transaction)
                .await?;
        }
        "evaluation_run_case" => {
            let case_id = Uuid::parse_str(&event.aggregate_id)?;
            sqlx::query("UPDATE evaluation_run_cases SET target_execution_id=COALESCE(target_execution_id,?),status=IF(status='queued','running',status) WHERE tenant_id=? AND id=?")
                .bind(event.execution_id.map(|id| id.as_uuid()))
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .execute(&mut **transaction)
                .await?;
            sqlx::query("UPDATE evaluation_runs er JOIN evaluation_run_cases c ON c.evaluation_run_id=er.id AND c.tenant_id=er.tenant_id SET er.status='running' WHERE c.tenant_id=? AND c.id=? AND er.status='queued'")
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .execute(&mut **transaction)
                .await?;
        }
        "evaluation_rule_result" => {
            let result_id = Uuid::parse_str(&event.aggregate_id)?;
            sqlx::query("UPDATE evaluation_rule_results SET evaluator_execution_id=COALESCE(evaluator_execution_id,?),status=IF(status='queued','running',status) WHERE tenant_id=? AND id=?")
                .bind(event.execution_id.map(|id| id.as_uuid()))
                .bind(event.tenant_id.as_uuid())
                .bind(result_id)
                .execute(&mut **transaction)
                .await?;
        }
        _ => {}
    }
    Ok(())
}

async fn project_execution_event(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
    execution_result: Option<&Value>,
) -> Result<()> {
    let Some(execution_id) = event.execution_id.map(|id| id.as_uuid()) else {
        return Ok(());
    };
    let Some(row) = sqlx::query(
        "SELECT invocation_id,session_id,input_json FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(event.tenant_id.as_uuid())
    .bind(execution_id)
    .fetch_optional(&mut **transaction)
    .await?
    else {
        return Ok(());
    };
    crate::evaluation_projector::project_evaluation_execution(
        transaction,
        event,
        execution_id,
        execution_result,
    )
    .await?;
    let Some(invocation_id) = row.try_get::<Option<Uuid>, _>("invocation_id")? else {
        return Ok(());
    };
    let invocation = sqlx::query("SELECT i.id,ad.output_expression,ad.output_schema_json FROM application_invocations i LEFT JOIN application_deployments ad ON ad.id=i.application_deployment_id AND ad.tenant_id=i.tenant_id WHERE i.tenant_id=? AND i.id=? FOR UPDATE")
        .bind(event.tenant_id.as_uuid())
        .bind(invocation_id)
        .fetch_one(&mut **transaction)
        .await?;
    if let Some(sequence) = event.sequence {
        let last_sequence: u64 = sqlx::query_scalar(
            "SELECT last_event_sequence FROM application_invocations WHERE tenant_id=? AND id=?",
        )
        .bind(event.tenant_id.as_uuid())
        .bind(invocation_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !advances_event_sequence(last_sequence, sequence) {
            return Ok(());
        }
        sqlx::query(
            "UPDATE application_invocations SET last_event_sequence=? WHERE tenant_id=? AND id=?",
        )
        .bind(sequence)
        .bind(event.tenant_id.as_uuid())
        .bind(invocation_id)
        .execute(&mut **transaction)
        .await?;
    }
    let application_output = if event.event_type == "execution.succeeded" {
        execution_result
            .map(|result| {
                project_application_output(
                    result,
                    &row.try_get::<Value, _>("input_json")?,
                    invocation
                        .try_get::<Option<String>, _>("output_expression")?
                        .as_deref(),
                    &invocation
                        .try_get::<Option<Value>, _>("output_schema_json")?
                        .unwrap_or_else(|| json!({})),
                )
            })
            .transpose()
    } else {
        Ok(None)
    };
    let output_error = application_output.as_ref().err().map(ToString::to_string);
    let projected_status = if output_error.is_some() {
        Some("failed")
    } else {
        invocation_status(&event.event_type, &event.payload)
    };
    if let Some(status) = projected_status {
        sqlx::query("UPDATE application_invocations SET status=IF(status IN ('completed','failed','cancelled'),status,?),completed_at=IF(? IN ('completed','failed','cancelled'),COALESCE(completed_at,CURRENT_TIMESTAMP(6)),completed_at) WHERE tenant_id=? AND id=?")
            .bind(status)
            .bind(status)
            .bind(event.tenant_id.as_uuid())
            .bind(invocation_id)
            .execute(&mut **transaction)
            .await?;
    }
    append_invocation_event(
        transaction,
        event.tenant_id.as_uuid(),
        invocation_id,
        &event.event_type,
        json!({
            "executionId": execution_id,
            "executionSequence": event.sequence,
            "status": projected_status,
            "summary": event.payload,
            "runtimeEventId": event.event_id,
        }),
    )
    .await?;
    if let Some(error) = output_error {
        let error_code = application_output_error_code(&error);
        append_invocation_event(
            transaction,
            event.tenant_id.as_uuid(),
            invocation_id,
            "application.output_invalid",
            json!({"executionId":execution_id,"errorCode":error_code,"errorMessage":error,"runtimeEventId":event.event_id}),
        )
        .await?;
    }
    if event.event_type == "execution.succeeded"
        && let Some(session_id) = row.try_get::<Option<Uuid>, _>("session_id")?
        && let Ok(Some(output)) = application_output
    {
        append_assistant_message(
            transaction,
            event.tenant_id.as_uuid(),
            session_id,
            invocation_id,
            output,
        )
        .await?;
    }
    Ok(())
}

fn application_output_error_code(message: &str) -> &'static str {
    if message.contains("APPLICATION_PRIMARY_OUTPUT_NOT_REACHED") {
        "APPLICATION_PRIMARY_OUTPUT_NOT_REACHED"
    } else {
        "APPLICATION_OUTPUT_INVALID"
    }
}

fn advances_event_sequence(last_sequence: u64, incoming_sequence: u64) -> bool {
    incoming_sequence > last_sequence
}

fn project_application_output(
    result: &Value,
    input: &Value,
    expression: Option<&str>,
    schema: &Value,
) -> Result<Value> {
    let outputs = result
        .get("primaryOutput")
        .filter(|value| !value.is_null())
        .context("APPLICATION_PRIMARY_OUTPUT_NOT_REACHED: primary output node did not complete")?
        .get("outputs")
        .cloned()
        .context("APPLICATION_PRIMARY_OUTPUT_NOT_REACHED: primary output has no outputs")?;
    let output = if let Some(expression) = expression.filter(|value| !value.trim().is_empty()) {
        ExpressionEngine.evaluate(
            expression.strip_prefix('=').unwrap_or(expression),
            &ExpressionContext {
                json: outputs.clone(),
                input: input.clone(),
                ..ExpressionContext::default()
            },
        )?
    } else {
        infer_application_output(&outputs)
    };
    let validator =
        jsonschema::validator_for(schema).context("Application Output Schema is invalid")?;
    validator.validate(&output).map_err(|error| {
        anyhow::anyhow!("Application output does not match Output Schema: {error}")
    })?;
    Ok(output)
}

fn infer_application_output(outputs: &Value) -> Value {
    let Some(item) = outputs
        .get("main")
        .and_then(Value::as_array)
        .filter(|items| items.len() == 1)
        .and_then(|items| items.first())
        .and_then(|item| item.get("json"))
    else {
        return outputs.clone();
    };
    item.pointer("/message/content")
        .or_else(|| item.get("output"))
        .or_else(|| item.get("text"))
        .filter(|value| value.is_string())
        .cloned()
        .unwrap_or_else(|| outputs.clone())
}

fn invocation_status(event_type: &str, payload: &Value) -> Option<&'static str> {
    match event_type {
        "execution.succeeded" => Some("completed"),
        "execution.failed" | "execution.timed_out" | "execution.activation_budget_exceeded" => {
            Some("failed")
        }
        "execution.cancelled" => Some("cancelled"),
        "execution.started" | "execution.resumed" => Some("running"),
        _ if event_type.starts_with("execution.") => {
            match payload.get("status").and_then(Value::as_str) {
                Some("succeeded") => Some("completed"),
                Some("failed" | "timed_out") => Some("failed"),
                Some("cancelled") => Some("cancelled"),
                Some("queued" | "running" | "waiting" | "waiting_approval" | "suspended") => {
                    Some("running")
                }
                _ => None,
            }
        }
        _ => None,
    }
}

async fn append_invocation_event(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    invocation_id: Uuid,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    let sequence: u64 = sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=?")
        .bind(tenant_id)
        .bind(invocation_id)
        .fetch_one(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,?,?,?)")
        .bind(tenant_id)
        .bind(invocation_id)
        .bind(sequence)
        .bind(event_type)
        .bind(payload)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn append_assistant_message(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    session_id: Uuid,
    invocation_id: Uuid,
    result: Value,
) -> Result<()> {
    let already_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM application_messages WHERE tenant_id=? AND session_id=? AND invocation_id=? AND role='assistant')")
        .bind(tenant_id)
        .bind(session_id)
        .bind(invocation_id)
        .fetch_one(&mut **transaction)
        .await?;
    if already_exists {
        return Ok(());
    }
    let output = result;
    let sequence: u64 = sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM application_messages WHERE tenant_id=? AND session_id=?")
        .bind(tenant_id)
        .bind(session_id)
        .fetch_one(&mut **transaction)
        .await?;
    let message_id = Uuid::now_v7();
    sqlx::query("INSERT INTO application_messages(id,tenant_id,session_id,invocation_id,sequence_number,role) VALUES(?,?,?,?,?,'assistant')")
        .bind(message_id)
        .bind(tenant_id)
        .bind(session_id)
        .bind(invocation_id)
        .bind(sequence)
        .execute(&mut **transaction)
        .await?;
    let (part_type, content) = match output {
        Value::String(text) => ("text", Value::String(text)),
        value => ("json", value),
    };
    sqlx::query("INSERT INTO application_message_parts(id,tenant_id,message_id,part_index,part_type,content_json) VALUES(?,?,?,?,?,?)")
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(message_id)
        .bind(0_u32)
        .bind(part_type)
        .bind(content)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        advances_event_sequence, application_output_error_code, invocation_status,
        project_application_output,
    };
    use serde_json::json;

    #[test]
    fn only_execution_lifecycle_events_change_invocation_status() {
        assert_eq!(
            invocation_status("node.completed", &json!({"status":"succeeded"})),
            None
        );
        assert_eq!(
            invocation_status("execution.started", &json!({})),
            Some("running")
        );
        assert_eq!(
            invocation_status("execution.succeeded", &json!({})),
            Some("completed")
        );
        assert_eq!(
            invocation_status("execution.cancelled", &json!({})),
            Some("cancelled")
        );
    }

    #[test]
    fn duplicate_and_out_of_order_sequences_cannot_regress_projection() {
        assert!(!advances_event_sequence(7, 7));
        assert!(!advances_event_sequence(7, 6));
        assert!(advances_event_sequence(7, 8));
    }

    #[test]
    fn application_output_expression_and_schema_are_enforced() {
        let result = json!({"primaryOutput":{"outputs":{"main":[{"json":{"answer":42}}]}}});
        let output = project_application_output(
            &result,
            &json!({}),
            Some("$json.main[0].json.answer"),
            &json!({"type":"integer"}),
        )
        .unwrap();
        assert_eq!(output, json!(42));
        assert!(
            project_application_output(&result, &json!({}), None, &json!({"type":"string"}),)
                .is_err()
        );
    }

    #[test]
    fn application_output_errors_have_stable_public_codes() {
        assert_eq!(
            application_output_error_code(
                "APPLICATION_PRIMARY_OUTPUT_NOT_REACHED: primary output has no outputs"
            ),
            "APPLICATION_PRIMARY_OUTPUT_NOT_REACHED"
        );
        assert_eq!(
            application_output_error_code("Application output does not match Output Schema"),
            "APPLICATION_OUTPUT_INVALID"
        );
    }

    #[test]
    fn application_output_infers_agent_text_and_preserves_ambiguous_items() {
        let result = json!({"primaryOutput":{"outputs":{"main":[{"json":{"message":{"content":"hello"}}}]}}});
        assert_eq!(
            project_application_output(&result, &json!({}), None, &json!({})).unwrap(),
            json!("hello")
        );
        let ambiguous = json!({"primaryOutput":{"outputs":{"main":[{"json":{"text":"one"}},{"json":{"text":"two"}}]}}});
        assert_eq!(
            project_application_output(&ambiguous, &json!({}), None, &json!({})).unwrap(),
            json!({"main":[{"json":{"text":"one"}},{"json":{"text":"two"}}]})
        );
    }

    #[test]
    fn application_output_requires_reached_primary_node() {
        let error =
            project_application_output(&json!({"terminalNodes":[]}), &json!({}), None, &json!({}))
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("APPLICATION_PRIMARY_OUTPUT_NOT_REACHED")
        );
    }
}
