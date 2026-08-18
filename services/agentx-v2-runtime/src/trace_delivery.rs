use agentx_runtime_contracts::{TraceEventEnvelopeV1, content_hash};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{RuntimeError, RuntimeResult};

pub const TRACE_STREAM: &str = "agentx:v2:trace:v1";

pub struct TraceDraft {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub event_type: String,
    pub status: String,
    pub node_execution_id: Option<Uuid>,
    pub attempt_id: Option<Uuid>,
    pub runtime_call_id: Option<Uuid>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_version: Option<String>,
    pub duration_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_micros: u64,
    pub error_code: Option<String>,
    pub attributes: Value,
    pub content_ref: Option<Uuid>,
}

impl TraceDraft {
    pub fn execution(
        tenant_id: Uuid,
        execution_id: Uuid,
        event_type: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self {
            tenant_id,
            execution_id,
            event_type: event_type.into(),
            status: status.into(),
            node_execution_id: None,
            attempt_id: None,
            runtime_call_id: None,
            resource_type: None,
            resource_id: None,
            resource_version: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            cost_micros: 0,
            error_code: None,
            attributes: json!({}),
            content_ref: None,
        }
    }
}

pub async fn enqueue(tx: &mut Transaction<'_, MySql>, draft: TraceDraft) -> RuntimeResult<u64> {
    if !draft.attributes.is_object() || draft.attributes.to_string().len() > 16 * 1024 {
        return Err(RuntimeError::BadRequest(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::UnsupportedCapability,
            "Trace attributes exceed the reviewed object budget".into(),
        ));
    }
    let execution = sqlx::query("SELECT trace_id,trace_watermark FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(draft.tenant_id).bind(draft.execution_id).fetch_one(&mut **tx).await?;
    let sequence = execution.try_get::<u64, _>("trace_watermark")? + 1;
    let event_id = Uuid::now_v7();
    let occurred_at = OffsetDateTime::now_utc();
    let unsigned = json!({
        "schemaVersion":1,"eventId":event_id,"tenantId":draft.tenant_id,
        "executionId":draft.execution_id,"executionSequence":sequence,
        "traceId":execution.try_get::<Uuid,_>("trace_id")?,"spanId":event_id,
        "parentSpanId":Value::Null,"nodeExecutionId":draft.node_execution_id,
        "attemptId":draft.attempt_id,"runtimeCallId":draft.runtime_call_id,
        "resourceType":draft.resource_type,"resourceId":draft.resource_id,
        "resourceVersion":draft.resource_version,"eventType":draft.event_type,
        "status":draft.status,"durationMs":draft.duration_ms,"inputTokens":draft.input_tokens,
        "outputTokens":draft.output_tokens,"costMicros":draft.cost_micros,
        "errorCode":draft.error_code,"attributes":draft.attributes,
        "contentRef":draft.content_ref,"occurredAt":occurred_at
    });
    let hash = content_hash(&unsigned).map_err(|error| RuntimeError::Internal(error.into()))?;
    let event_summary = json!({
        "nodeExecutionId":draft.node_execution_id,"attemptId":draft.attempt_id,
        "runtimeCallId":draft.runtime_call_id,"errorCode":draft.error_code,
        "attributes":draft.attributes
    });
    let envelope = TraceEventEnvelopeV1 {
        schema_version: 1,
        event_id,
        tenant_id: draft.tenant_id,
        execution_id: draft.execution_id,
        execution_sequence: sequence,
        trace_id: execution.try_get("trace_id")?,
        span_id: event_id,
        parent_span_id: None,
        node_execution_id: draft.node_execution_id,
        attempt_id: draft.attempt_id,
        runtime_call_id: draft.runtime_call_id,
        resource_type: draft.resource_type,
        resource_id: draft.resource_id,
        resource_version: draft.resource_version,
        event_type: draft.event_type,
        status: draft.status,
        duration_ms: draft.duration_ms,
        input_tokens: draft.input_tokens,
        output_tokens: draft.output_tokens,
        cost_micros: draft.cost_micros,
        error_code: draft.error_code,
        attributes: draft.attributes,
        content_ref: draft.content_ref,
        occurred_at,
        content_hash: hash.clone(),
    };
    let bytes =
        serde_json::to_vec(&envelope).map_err(|error| RuntimeError::Internal(error.into()))?;
    if bytes.len() > 64 * 1024 {
        return Err(RuntimeError::BadRequest(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::UnsupportedCapability,
            "Trace Envelope exceeds 64 KiB".into(),
        ));
    }
    sqlx::query("UPDATE workflow_executions SET trace_watermark=? WHERE tenant_id=? AND id=?")
        .bind(sequence)
        .bind(draft.tenant_id)
        .bind(draft.execution_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO trace_outbox(event_id,tenant_id,execution_id,execution_sequence,payload_json,content_hash,status) VALUES(?,?,?,?,?,?,'pending')")
        .bind(event_id).bind(draft.tenant_id).bind(draft.execution_id).bind(sequence)
        .bind(serde_json::to_value(&envelope).map_err(|error|RuntimeError::Internal(error.into()))?)
        .bind(hash.as_str()).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO execution_events(tenant_id,execution_id,sequence_number,event_type,status,summary_json,occurred_at) VALUES(?,?,?,?,?,?,?)")
        .bind(draft.tenant_id).bind(draft.execution_id).bind(sequence).bind(&envelope.event_type)
        .bind(&envelope.status).bind(event_summary)
        .bind(occurred_at).execute(&mut **tx).await?;
    Ok(sequence)
}

#[derive(Clone, Debug)]
pub struct TraceClaim {
    pub event_id: Uuid,
    pub owner_id: Uuid,
    pub fencing_token: u64,
    pub envelope: TraceEventEnvelopeV1,
}

pub async fn claim(pool: &sqlx::MySqlPool, owner_id: Uuid) -> RuntimeResult<Option<TraceClaim>> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT event_id,payload_json FROM trace_outbox WHERE status IN ('pending','failed') AND available_at<=UTC_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) ORDER BY created_at,event_id LIMIT 1 FOR UPDATE SKIP LOCKED")
        .fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let event_id: Uuid = row.try_get("event_id")?;
    sqlx::query("UPDATE trace_outbox SET status='pending',locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),heartbeat_at=UTC_TIMESTAMP(6),fencing_token=fencing_token+1,attempt_count=attempt_count+1 WHERE event_id=? AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6))")
        .bind(owner_id).bind(event_id).execute(&mut *tx).await?;
    let fencing_token: u64 =
        sqlx::query_scalar("SELECT fencing_token FROM trace_outbox WHERE event_id=?")
            .bind(event_id)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    let envelope = serde_json::from_value(row.try_get("payload_json")?)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    Ok(Some(TraceClaim {
        event_id,
        owner_id,
        fencing_token,
        envelope,
    }))
}

pub async fn publish(redis: &mut ConnectionManager, claim: &TraceClaim) -> RuntimeResult<String> {
    let payload = serde_json::to_string(&claim.envelope)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let stream_id = redis::cmd("XADD")
        .arg(TRACE_STREAM)
        .arg("*")
        .arg("event_id")
        .arg(claim.event_id.to_string())
        .arg("content_hash")
        .arg(claim.envelope.content_hash.as_str())
        .arg("payload")
        .arg(payload)
        .query_async::<String>(redis)
        .await
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    Ok(stream_id)
}

pub async fn complete(
    pool: &sqlx::MySqlPool,
    claim: &TraceClaim,
    stream_id: &str,
) -> RuntimeResult<()> {
    let changed = sqlx::query("UPDATE trace_outbox SET status='streamed',stream_id=?,streamed_at=UTC_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,heartbeat_at=NULL,last_error=NULL WHERE event_id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6) AND status='pending'")
        .bind(stream_id).bind(claim.event_id).bind(claim.owner_id).bind(claim.fencing_token).execute(pool).await?;
    if changed.rows_affected() != 1 {
        return Err(RuntimeError::Conflict(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::IdempotencyConflict,
            "Trace Outbox Lease was lost".into(),
        ));
    }
    Ok(())
}

pub async fn fail(pool: &sqlx::MySqlPool, claim: &TraceClaim, error: &str) -> RuntimeResult<()> {
    let message = error.chars().take(1000).collect::<String>();
    let delay = (1_u64 << claim.fencing_token.min(6)).min(60);
    let changed = sqlx::query("UPDATE trace_outbox SET status='failed',available_at=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),locked_by=NULL,locked_until=NULL,heartbeat_at=NULL,last_error=? WHERE event_id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)")
        .bind(delay).bind(message).bind(claim.event_id).bind(claim.owner_id).bind(claim.fencing_token).execute(pool).await?;
    if changed.rows_affected() != 1 {
        return Err(RuntimeError::Conflict(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::IdempotencyConflict,
            "Trace Outbox Lease was lost".into(),
        ));
    }
    Ok(())
}

pub async fn ensure_stream(redis: &mut ConnectionManager) -> RuntimeResult<bool> {
    let exists: bool = redis
        .exists(TRACE_STREAM)
        .await
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    if !exists {
        let _: String = redis::cmd("XADD")
            .arg(TRACE_STREAM)
            .arg("MAXLEN")
            .arg("~")
            .arg(1_000_000)
            .arg("*")
            .arg("bootstrap")
            .arg("1")
            .query_async(redis)
            .await
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    }
    Ok(!exists)
}

pub async fn requeue_after_stream_loss(pool: &sqlx::MySqlPool) -> RuntimeResult<u64> {
    let changed = sqlx::query(
        "UPDATE trace_outbox SET status='pending',available_at=UTC_TIMESTAMP(6),stream_id=NULL,streamed_at=NULL,locked_by=NULL,locked_until=NULL,heartbeat_at=NULL,last_error='runtime_redis_stream_rebuilt' WHERE status='streamed'",
    )
    .execute(pool)
    .await?;
    Ok(changed.rows_affected())
}
