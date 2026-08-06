use agentx_application::RuntimeEventEnvelope;
use anyhow::{Context, Result};
use sqlx::{MySql, Transaction};

pub async fn append_runtime_event(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
) -> Result<()> {
    let payload = serde_json::to_value(event).context("serialize runtime event")?;
    let sequence = event.sequence.unwrap_or(0);
    sqlx::query("INSERT INTO outbox_events(id,tenant_id,event_type,schema_version,aggregate_type,aggregate_id,execution_id,sequence_number,payload_json) VALUES(?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id")
        .bind(event.event_id)
        .bind(event.tenant_id.as_uuid())
        .bind(&event.event_type)
        .bind(&event.schema_version)
        .bind(&event.aggregate_type)
        .bind(&event.aggregate_id)
        .bind(event.execution_id.map(|id| id.as_uuid()))
        .bind((event.sequence.is_some()).then_some(sequence))
        .bind(payload)
        .execute(&mut **transaction)
        .await
        .context("append runtime event outbox")?;
    if let (Some(execution_id), Some(sequence)) = (event.execution_id, event.sequence) {
        sqlx::query("INSERT INTO execution_events(tenant_id,execution_id,event_id,sequence_number,event_type,schema_version,status,summary_json,occurred_at) VALUES(?,?,?,?,?,?,?, ?,?) ON DUPLICATE KEY UPDATE event_id=VALUES(event_id)")
            .bind(event.tenant_id.as_uuid())
            .bind(execution_id.as_uuid())
            .bind(event.event_id)
            .bind(sequence)
            .bind(&event.event_type)
            .bind(&event.schema_version)
            .bind(event.payload.get("status").and_then(|v| v.as_str()).unwrap_or("running"))
            .bind(&event.payload)
            .bind(event.occurred_at)
            .execute(&mut **transaction)
            .await
            .context("append execution runtime event")?;
    }
    Ok(())
}
