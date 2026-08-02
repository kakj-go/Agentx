use std::time::Duration;

use agentx_application::{Outbox, OutboxDelivery, OutboxDispatcher, OutboxMessage};
use agentx_domain::TenantId;
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

pub struct MySqlOutbox {
    pool: MySqlPool,
}

impl MySqlOutbox {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Outbox for MySqlOutbox {
    async fn append(&self, message: OutboxMessage) -> Result<()> {
        sqlx::query(
            "INSERT INTO outbox_events (id, tenant_id, event_type, aggregate_type, aggregate_id, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(message.event_id)
        .bind(message.tenant_id.as_uuid())
        .bind(message.event_type)
        .bind(message.aggregate_type)
        .bind(message.aggregate_id)
        .bind(message.payload)
        .execute(&self.pool)
        .await
        .context("failed to append outbox event")?;
        Ok(())
    }
}

pub struct MySqlOutboxDispatcher {
    pool: MySqlPool,
}

impl MySqlOutboxDispatcher {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OutboxDispatcher for MySqlOutboxDispatcher {
    async fn claim(&self, batch_size: u32, lease: Duration) -> Result<Vec<OutboxDelivery>> {
        let batch_size = batch_size.clamp(1, 500);
        let lease_id = Uuid::now_v7();
        let lease_seconds = lease.as_secs().clamp(1, 3600);
        let mut transaction = self.pool.begin().await.context("begin outbox claim")?;
        let rows = sqlx::query(
            "SELECT id FROM outbox_events WHERE published_at IS NULL AND available_at<=CURRENT_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6)) ORDER BY occurred_at,id LIMIT ? FOR UPDATE SKIP LOCKED",
        )
        .bind(batch_size)
        .fetch_all(&mut *transaction)
        .await
        .context("select pending outbox events")?;
        for row in rows {
            let event_id: Uuid = row.try_get("id")?;
            sqlx::query("UPDATE outbox_events SET locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),attempt_count=attempt_count+1 WHERE id=? AND published_at IS NULL")
                .bind(lease_id)
                .bind(lease_seconds)
                .bind(event_id)
                .execute(&mut *transaction)
                .await
                .context("lease outbox event")?;
        }
        transaction.commit().await.context("commit outbox claim")?;

        let rows = sqlx::query("SELECT id,tenant_id,event_type,aggregate_type,aggregate_id,payload_json,attempt_count FROM outbox_events WHERE locked_by=? AND published_at IS NULL ORDER BY occurred_at,id")
            .bind(lease_id)
            .fetch_all(&self.pool)
            .await
            .context("load leased outbox events")?;
        rows.into_iter()
            .map(|row| {
                Ok(OutboxDelivery {
                    event_id: row.try_get("id")?,
                    tenant_id: TenantId::from_uuid(row.try_get("tenant_id")?),
                    event_type: row.try_get("event_type")?,
                    aggregate_type: row.try_get("aggregate_type")?,
                    aggregate_id: row.try_get("aggregate_id")?,
                    payload: row.try_get("payload_json")?,
                    attempt_count: row.try_get("attempt_count")?,
                    lease_id,
                })
            })
            .collect()
    }

    async fn mark_published(&self, delivery: &OutboxDelivery) -> Result<bool> {
        let result = sqlx::query("UPDATE outbox_events SET published_at=CURRENT_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,last_error=NULL WHERE id=? AND locked_by=? AND published_at IS NULL")
            .bind(delivery.event_id)
            .bind(delivery.lease_id)
            .execute(&self.pool)
            .await
            .context("mark outbox event published")?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_failed(
        &self,
        delivery: &OutboxDelivery,
        error: &str,
        retry_after: Duration,
    ) -> Result<bool> {
        let retry_seconds = retry_after.as_secs().clamp(1, 86_400);
        let result = sqlx::query("UPDATE outbox_events SET available_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),locked_by=NULL,locked_until=NULL,last_error=? WHERE id=? AND locked_by=? AND published_at IS NULL")
            .bind(retry_seconds)
            .bind(error.chars().take(1024).collect::<String>())
            .bind(delivery.event_id)
            .bind(delivery.lease_id)
            .execute(&self.pool)
            .await
            .context("reschedule failed outbox event")?;
        Ok(result.rows_affected() == 1)
    }
}
