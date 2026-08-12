use agentx_application::RuntimeEventEnvelope;
use agentx_domain::TenantId;
use agentx_runtime::RuntimeExecutionStatus;
use anyhow::Result;
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::quota::QuotaAdmission;
use crate::runtime_events::append_runtime_event;

pub(crate) async fn sync_execution_status(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    status: RuntimeExecutionStatus,
    quota_admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let status = super::runtime_repository_support::execution_status_name(status);
    let terminal = matches!(status, "succeeded" | "failed" | "cancelled" | "timed_out");
    sqlx::query("UPDATE workflow_executions SET status=?,ended_at=IF(?,COALESCE(ended_at,CURRENT_TIMESTAMP(6)),NULL),duration_ms=IF(?,TIMESTAMPDIFF(MICROSECOND,started_at,COALESCE(ended_at,CURRENT_TIMESTAMP(6)))/1000,NULL),state_version=state_version+1 WHERE tenant_id=? AND id=?")
        .bind(status).bind(terminal).bind(terminal).bind(tenant_id).bind(execution_id).execute(&mut **transaction).await?;
    if !terminal {
        return Ok(());
    }
    crate::quota::release_scope_with_admission(
        transaction,
        tenant_id,
        "execution",
        &execution_id.to_string(),
        quota_admission,
    )
    .await?;
    let emitted: bool = sqlx::query_scalar(
        "SELECT terminal_event_emitted FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_one(&mut **transaction)
    .await?;
    if emitted {
        return Ok(());
    }
    let result = if matches!(status, "succeeded" | "failed") {
        let (result, hash) = crate::runtime_results::materialize_execution_result(
            transaction,
            tenant_id,
            execution_id,
        )
        .await?;
        sqlx::query("UPDATE workflow_executions SET result_json=?,result_hash=?,terminal_event_emitted=TRUE WHERE tenant_id=? AND id=?")
            .bind(&result)
            .bind(&hash)
            .bind(tenant_id)
            .bind(execution_id)
            .execute(&mut **transaction)
            .await?;
        let parsed: Value = result.clone();
        Some(
            json!({"resultHash":hash,"outputs":parsed.get("outputs").cloned().unwrap_or(Value::Object(Default::default())),"error":parsed.get("error").cloned()}),
        )
    } else {
        sqlx::query(
            "UPDATE workflow_executions SET terminal_event_emitted=TRUE WHERE tenant_id=? AND id=?",
        )
        .bind(tenant_id)
        .bind(execution_id)
        .execute(&mut **transaction)
        .await?;
        None
    };
    let mut payload = result.unwrap_or_else(|| json!({}));
    if let Value::Object(ref mut object) = payload {
        object.insert("status".into(), Value::String(status.into()));
        if status != "succeeded" && !object.contains_key("error") {
            let row = sqlx::query("SELECT error_code,error_message FROM workflow_executions WHERE tenant_id=? AND id=?")
                .bind(tenant_id)
                .bind(execution_id)
                .fetch_one(&mut **transaction)
                .await?;
            if let Some(code) = row.try_get::<Option<String>, _>("error_code")? {
                object.insert("errorCode".into(), Value::String(code));
            }
            if let Some(message) = row.try_get::<Option<String>, _>("error_message")? {
                object.insert("errorMessage".into(), Value::String(message));
            }
        }
    }
    insert_execution_event(
        transaction,
        tenant_id,
        execution_id,
        &format!("execution.{status}"),
        status,
        payload,
    )
    .await?;
    Ok(())
}

pub(crate) async fn insert_execution_event(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    event_type: &str,
    status: &str,
    summary: Value,
) -> Result<()> {
    sqlx::query("SELECT id FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_one(&mut **transaction)
        .await?;
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM execution_events WHERE tenant_id=? AND execution_id=?")
        .bind(tenant_id).bind(execution_id).fetch_one(&mut **transaction).await?;
    let mut payload = match summary {
        Value::Object(object) => Value::Object(object),
        value => json!({"summary": value}),
    };
    if let Value::Object(ref mut object) = payload {
        object.insert("status".into(), Value::String(status.into()));
    }
    append_runtime_event(
        transaction,
        &RuntimeEventEnvelope::new(
            TenantId::from_uuid(tenant_id),
            event_type,
            "execution",
            execution_id.to_string(),
            Some(agentx_domain::ExecutionId::from_uuid(execution_id)),
            Some(sequence),
            payload,
        ),
    )
    .await
}

pub(crate) struct TraceInsert<'a> {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Option<Uuid>,
    pub trace_id: Uuid,
    pub node_execution_id: Option<Uuid>,
    pub node_id: Option<&'a str>,
    pub event_type: &'a str,
    pub status: &'a str,
    pub run_index: u32,
    pub attributes: Value,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub(crate) async fn insert_trace(
    transaction: &mut Transaction<'_, MySql>,
    value: TraceInsert<'_>,
) -> Result<()> {
    let event_id = Uuid::now_v7();
    let payload = json!({"eventId":event_id,"tenantId":value.tenant_id,"traceId":value.trace_id,"spanId":Uuid::now_v7(),"parentSpanId":null,"executionId":value.execution_id,"workflowId":value.workflow_id,"workflowVersionId":value.workflow_version_id,"nodeExecutionId":value.node_execution_id,"nodeId":value.node_id,"eventType":value.event_type,"status":value.status,"eventTime":OffsetDateTime::now_utc(),"durationMs":null,"runIndex":value.run_index,"iterationIndex":0,"modelName":null,"providerName":null,"mcpToolName":null,"inputTokens":null,"outputTokens":null,"costMicros":0,"errorCode":value.error_code,"errorMessage":value.error_message,"attributes":value.attributes,"contentRef":null});
    sqlx::query("INSERT INTO trace_delivery_outbox(event_id,tenant_id,execution_id,payload_json) VALUES(?,?,?,?)")
        .bind(event_id).bind(value.tenant_id).bind(value.execution_id).bind(payload).execute(&mut **transaction).await?;
    Ok(())
}
