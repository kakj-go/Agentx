use agentx_application::{
    ApprovalTaskPort, ExecutionProjectionPort, ExecutionQuery, InvocationEventPublisher,
    NotificationPublisher, RuntimeStatusProvider, TraceSink,
};
use agentx_domain::{
    ApprovalTaskId, ExecutionId, ExecutionStatus, ExecutionSummary, InvocationId, NotificationId,
    TenantId, TraceEvent,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{MySqlPool, Row};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

#[derive(Clone)]
pub struct MySqlOperationsProjection {
    pool: MySqlPool,
}

impl MySqlOperationsProjection {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ExecutionProjectionPort for MySqlOperationsProjection {
    async fn create(&self, value: &ExecutionSummary) -> Result<()> {
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,invocation_id,session_id,trace_id,trigger_type,status,started_at,ended_at,duration_ms,cost_micros,error_code,error_message) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(value.id.as_uuid()).bind(value.tenant_id.as_uuid()).bind(value.workflow_id.as_uuid()).bind(value.workflow_version_id.as_uuid())
            .bind(value.invocation_id.map(|id| id.as_uuid())).bind(value.session_id.map(|id| id.as_uuid())).bind(value.trace_id.as_uuid())
            .bind(&value.trigger_type).bind(status_name(&value.status)).bind(value.started_at).bind(value.ended_at).bind(value.duration_ms)
            .bind(value.cost_micros).bind(&value.error_code).bind(&value.error_message).execute(&self.pool).await?;
        sqlx::query("INSERT INTO execution_events(tenant_id,execution_id,sequence_number,event_type,status,summary_json,occurred_at) VALUES(?,?,1,'execution.projected',?,?,?)")
            .bind(value.tenant_id.as_uuid()).bind(value.id.as_uuid()).bind(status_name(&value.status)).bind(json!({"triggerType":value.trigger_type,"costMicros":value.cost_micros})).bind(value.started_at).execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl TraceSink for MySqlOperationsProjection {
    async fn append(&self, value: TraceEvent) -> Result<()> {
        let payload = json!({
            "eventId":value.event_id,"tenantId":value.tenant_id.as_uuid(),"traceId":value.trace_id.as_uuid(),"spanId":value.span_id,
            "parentSpanId":value.parent_span_id,"executionId":value.execution_id.as_uuid(),"workflowId":value.workflow_id.as_uuid(),
            "workflowVersionId":value.workflow_version_id.as_uuid(),"nodeExecutionId":value.node_execution_id.map(|id|id.as_uuid()),"attemptId":value.attempt_id.map(|id|id.as_uuid()),
            "agentRunId":value.agent_run_id,"runtimeCallId":value.runtime_call_id,"sandboxId":value.sandbox_id,"resourceType":value.resource_type,"resourceId":value.resource_id,"resourceVersionId":value.resource_version_id,"nodeId":value.attributes.get("nodeId"),
            "eventType":value.event_type,"status":value.status,"eventTime":value.event_time,"durationMs":value.duration_ms,"runIndex":value.run_index,
            "iterationIndex":value.iteration_index,"modelName":value.model_name,"providerName":value.provider_name,"mcpToolName":value.mcp_tool_name,
            "inputTokens":value.input_tokens,"outputTokens":value.output_tokens,"costMicros":value.cost_micros,"errorCode":value.error_code,
            "errorMessage":value.error_message,"stopReason":value.stop_reason,"partial":value.partial,"attributes":redact_trace_attributes(value.attributes),"contentRef":value.content_ref.map(|id|id.as_uuid())
        });
        sqlx::query("INSERT INTO trace_delivery_outbox(event_id,tenant_id,execution_id,payload_json) VALUES(?,?,?,?) ON DUPLICATE KEY UPDATE event_id=event_id")
            .bind(value.event_id).bind(value.tenant_id.as_uuid()).bind(value.execution_id.as_uuid()).bind(payload).execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl ApprovalTaskPort for MySqlOperationsProjection {
    async fn create_task(
        &self,
        tenant_id: TenantId,
        task_id: ApprovalTaskId,
        payload: Value,
    ) -> Result<()> {
        let execution = uuid(&payload, "executionId")?;
        let workflow = uuid(&payload, "workflowId")?;
        let candidate = uuid(&payload, "candidateUserId")?;
        let node = text(&payload, "nodeId")?;
        let title = text(&payload, "title")?;
        let description = payload.get("description").and_then(Value::as_str);
        let deadline = payload
            .get("deadlineAt")
            .and_then(Value::as_str)
            .map(|value| OffsetDateTime::parse(value, &Rfc3339))
            .transpose()
            .context("deadlineAt must be an RFC3339 timestamp")?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO approval_tasks(id,tenant_id,execution_id,workflow_id,node_id,title,description,request_payload_json,deadline_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(task_id.as_uuid()).bind(tenant_id.as_uuid()).bind(execution).bind(workflow).bind(node).bind(title).bind(description)
            .bind(payload.get("requestPayload")).bind(deadline).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO approval_candidates(tenant_id,approval_task_id,candidate_type,candidate_id) VALUES(?,?,'user',?)")
            .bind(tenant_id.as_uuid()).bind(task_id.as_uuid()).bind(candidate).execute(&mut *tx).await?;
        let notification_id = Uuid::now_v7();
        sqlx::query("INSERT INTO notifications(id,tenant_id,source_event_id,notification_type,title_key,body_key,arguments_json,target_type,target_id,target_path,tone) VALUES(?,?,?,'approval_created','notifications.approvalReassigned.title','notifications.approvalReassigned.body',JSON_OBJECT(),'approval',?,?,'warning')")
            .bind(notification_id).bind(tenant_id.as_uuid()).bind(task_id.as_uuid()).bind(task_id.as_uuid()).bind(format!("/approvals/{}",task_id.as_uuid())).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO notification_receipts(tenant_id,notification_id,user_id) VALUES(?,?,?)",
        )
        .bind(tenant_id.as_uuid())
        .bind(notification_id)
        .bind(candidate)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl NotificationPublisher for MySqlOperationsProjection {
    async fn publish(
        &self,
        tenant_id: TenantId,
        notification_id: NotificationId,
        payload: Value,
    ) -> Result<()> {
        let recipient = uuid(&payload, "recipientUserId")?;
        let source_event = payload
            .get("sourceEventId")
            .and_then(Value::as_str)
            .map(Uuid::parse_str)
            .transpose()?
            .unwrap_or_else(|| notification_id.as_uuid());
        let target_id = uuid(&payload, "targetId")?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO notifications(id,tenant_id,source_event_id,notification_type,title_key,body_key,arguments_json,target_type,target_id,target_path,tone) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id")
            .bind(notification_id.as_uuid()).bind(tenant_id.as_uuid()).bind(source_event)
            .bind(text(&payload,"notificationType")?).bind(text(&payload,"titleKey")?).bind(text(&payload,"bodyKey")?)
            .bind(payload.get("arguments").cloned().unwrap_or_else(|| json!({}))).bind(text(&payload,"targetType")?)
            .bind(target_id).bind(text(&payload,"targetPath")?).bind(text(&payload,"tone")?).execute(&mut *tx).await?;
        let stored_id: Uuid = sqlx::query_scalar("SELECT id FROM notifications WHERE tenant_id=? AND source_event_id=? AND notification_type=?")
            .bind(tenant_id.as_uuid()).bind(source_event).bind(text(&payload,"notificationType")?).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT IGNORE INTO notification_receipts(tenant_id,notification_id,user_id) VALUES(?,?,?)")
            .bind(tenant_id.as_uuid()).bind(stored_id).bind(recipient).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl InvocationEventPublisher for MySqlOperationsProjection {
    async fn publish(
        &self,
        tenant_id: TenantId,
        invocation_id: InvocationId,
        event: Value,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM application_invocations WHERE tenant_id=? AND id=? FOR UPDATE")
            .bind(tenant_id.as_uuid())
            .bind(invocation_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
        let sequence: u64 = sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=?")
            .bind(tenant_id.as_uuid()).bind(invocation_id.as_uuid()).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,?,?,?)")
            .bind(tenant_id.as_uuid()).bind(invocation_id.as_uuid()).bind(sequence)
            .bind(text(&event,"eventType")?).bind(event.get("payload").cloned().unwrap_or_else(||json!({}))).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl ExecutionQuery for MySqlOperationsProjection {
    async fn status(
        &self,
        tenant_id: TenantId,
        execution_id: ExecutionId,
    ) -> Result<Option<ExecutionStatus>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT status FROM workflow_executions WHERE tenant_id=? AND id=?")
                .bind(tenant_id.as_uuid())
                .bind(execution_id.as_uuid())
                .fetch_optional(&self.pool)
                .await?;
        value.map(|value| parse_status(&value)).transpose()
    }
}

#[async_trait]
impl RuntimeStatusProvider for MySqlOperationsProjection {
    async fn snapshot(&self, tenant_id: TenantId) -> Result<Value> {
        let row = sqlx::query("SELECT COALESCE(SUM(status='running'),0) running,COALESCE(SUM(status IN ('waiting','waiting_approval')),0) waiting,COALESCE(SUM(status='failed' AND started_at>=CURRENT_DATE),0) failed_today FROM workflow_executions WHERE tenant_id=?")
            .bind(tenant_id.as_uuid()).fetch_one(&self.pool).await?;
        Ok(json!({
            "running": row.try_get::<i64,_>("running")?,
            "waiting": row.try_get::<i64,_>("waiting")?,
            "failedToday": row.try_get::<i64,_>("failed_today")?,
        }))
    }
}

fn status_name(value: &ExecutionStatus) -> &'static str {
    match value {
        ExecutionStatus::Created => "created",
        ExecutionStatus::Queued => "queued",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Waiting => "waiting",
        ExecutionStatus::WaitingApproval => "waiting_approval",
        ExecutionStatus::Suspended => "suspended",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::TimedOut => "timed_out",
    }
}
fn parse_status(value: &str) -> Result<ExecutionStatus> {
    Ok(match value {
        "created" => ExecutionStatus::Created,
        "queued" => ExecutionStatus::Queued,
        "running" => ExecutionStatus::Running,
        "waiting" => ExecutionStatus::Waiting,
        "waiting_approval" => ExecutionStatus::WaitingApproval,
        "suspended" => ExecutionStatus::Suspended,
        "succeeded" => ExecutionStatus::Succeeded,
        "failed" => ExecutionStatus::Failed,
        "cancelled" => ExecutionStatus::Cancelled,
        "timed_out" => ExecutionStatus::TimedOut,
        other => anyhow::bail!("unknown execution status {other}"),
    })
}
fn uuid(value: &Value, key: &str) -> Result<Uuid> {
    Uuid::parse_str(text(value, key)?).with_context(|| format!("{key} must be a UUID"))
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("{key} is required"))
}
pub fn redact_trace_attributes(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if ["authorization", "cookie", "secret", "password", "token"]
                        .iter()
                        .any(|part| lower.contains(part))
                    {
                        (key, Value::String("[REDACTED]".into()))
                    } else {
                        (key, redact_trace_attributes(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(redact_trace_attributes).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_status, redact_trace_attributes, status_name};
    use agentx_domain::ExecutionStatus;
    use serde_json::json;
    #[test]
    fn trace_attributes_are_redacted() {
        let value = redact_trace_attributes(
            json!({"authorization":"Bearer secret","nested":{"apiToken":"secret","safe":true}}),
        );
        assert_eq!(value["authorization"], "[REDACTED]");
        assert_eq!(value["nested"]["apiToken"], "[REDACTED]");
        assert_eq!(value["nested"]["safe"], true);
    }

    #[test]
    fn execution_status_projection_round_trips_every_state() {
        let states = [
            ExecutionStatus::Created,
            ExecutionStatus::Queued,
            ExecutionStatus::Running,
            ExecutionStatus::Waiting,
            ExecutionStatus::WaitingApproval,
            ExecutionStatus::Suspended,
            ExecutionStatus::Succeeded,
            ExecutionStatus::Failed,
            ExecutionStatus::Cancelled,
            ExecutionStatus::TimedOut,
        ];
        for state in states {
            assert_eq!(parse_status(status_name(&state)).unwrap(), state);
        }
        assert!(parse_status("unknown").is_err());
    }
}
