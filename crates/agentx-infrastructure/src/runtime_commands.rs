use agentx_application::{RuntimeCommand, RuntimeCommandType, RuntimeEventEnvelope};
use agentx_domain::ExecutionId;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct RuntimeCommandRepository {
    pool: MySqlPool,
}

#[derive(Clone, Debug)]
pub struct ClaimedRuntimeCommand {
    pub command: RuntimeCommand,
    pub lease_id: Uuid,
    pub attempt_count: u32,
}

impl RuntimeCommandRepository {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    pub async fn enqueue(&self, command: &RuntimeCommand) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let inserted = Self::enqueue_in_transaction(&mut transaction, command).await?;
        transaction.commit().await?;
        Ok(inserted)
    }

    pub async fn enqueue_in_transaction(
        transaction: &mut Transaction<'_, MySql>,
        command: &RuntimeCommand,
    ) -> Result<bool> {
        let existing = sqlx::query("SELECT id,aggregate_type,aggregate_id,payload_json FROM runtime_commands WHERE tenant_id=? AND command_type=? AND idempotency_key=?")
            .bind(command.tenant_id.as_uuid())
            .bind(command.command_type.as_str())
            .bind(&command.idempotency_key)
            .fetch_optional(&mut **transaction)
            .await
            .context("read existing runtime command")?;
        if let Some(existing) = existing {
            ensure_same_command(&existing, command)?;
            return Ok(false);
        }

        sqlx::query("INSERT INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json) VALUES(?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id")
            .bind(command.id)
            .bind(command.tenant_id.as_uuid())
            .bind(command.command_type.as_str())
            .bind(&command.aggregate_type)
            .bind(&command.aggregate_id)
            .bind(&command.idempotency_key)
            .bind(&command.payload)
            .execute(&mut **transaction)
            .await
            .context("enqueue runtime command")?;
        let stored = sqlx::query("SELECT id,aggregate_type,aggregate_id,payload_json FROM runtime_commands WHERE tenant_id=? AND command_type=? AND idempotency_key=?")
            .bind(command.tenant_id.as_uuid())
            .bind(command.command_type.as_str())
            .bind(&command.idempotency_key)
            .fetch_one(&mut **transaction)
            .await
            .context("read enqueued runtime command")?;
        ensure_same_command(&stored, command)?;
        Ok(stored.try_get::<Uuid, _>("id")? == command.id)
    }

    pub async fn claim(
        &self,
        batch_size: u32,
        lease_seconds: u64,
    ) -> Result<Vec<ClaimedRuntimeCommand>> {
        let lease_id = Uuid::now_v7();
        let lease_seconds = lease_seconds.clamp(5, 300);
        let mut tx = self
            .pool
            .begin()
            .await
            .context("begin runtime command claim")?;
        let rows = sqlx::query("SELECT id FROM runtime_commands WHERE status IN ('pending','processing') AND available_at<=CURRENT_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6)) ORDER BY created_at,id LIMIT ? FOR UPDATE SKIP LOCKED")
            .bind(batch_size.clamp(1, 200))
            .fetch_all(&mut *tx)
            .await
            .context("select runtime commands")?;
        for row in &rows {
            let id: Uuid = row.try_get("id")?;
            sqlx::query("UPDATE runtime_commands SET status='processing',locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),attempt_count=attempt_count+1 WHERE id=? AND status IN ('pending','processing')")
                .bind(lease_id).bind(lease_seconds).bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await.context("commit runtime command claim")?;
        let rows = sqlx::query("SELECT id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,attempt_count FROM runtime_commands WHERE locked_by=? AND status='processing' ORDER BY created_at,id")
            .bind(lease_id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let command_type: String = row.try_get("command_type")?;
                let command_type = match command_type.as_str() {
                    "start_execution" => RuntimeCommandType::StartExecution,
                    "cancel_execution" => RuntimeCommandType::CancelExecution,
                    "resume_execution" => RuntimeCommandType::ResumeExecution,
                    other => return Err(anyhow!("unsupported runtime command type {other}")),
                };
                Ok(ClaimedRuntimeCommand {
                    command: RuntimeCommand {
                        id: row.try_get("id")?,
                        tenant_id: agentx_domain::TenantId::from_uuid(row.try_get("tenant_id")?),
                        command_type,
                        aggregate_type: row.try_get("aggregate_type")?,
                        aggregate_id: row.try_get("aggregate_id")?,
                        idempotency_key: row.try_get("idempotency_key")?,
                        payload: row.try_get("payload_json")?,
                    },
                    lease_id,
                    attempt_count: row.try_get("attempt_count")?,
                })
            })
            .collect()
    }

    pub async fn complete(&self, claimed: &ClaimedRuntimeCommand, result: Value) -> Result<bool> {
        let changed = sqlx::query("UPDATE runtime_commands SET status='completed',result_json=?,completed_at=CURRENT_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,error_code=NULL,error_message=NULL WHERE id=? AND locked_by=? AND status='processing'")
            .bind(result).bind(claimed.command.id).bind(claimed.lease_id).execute(&self.pool).await?;
        Ok(changed.rows_affected() == 1)
    }

    pub async fn complete_with_event(
        &self,
        claimed: &ClaimedRuntimeCommand,
        execution_id: Option<Uuid>,
        result: Value,
    ) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE runtime_commands SET status='completed',result_json=?,completed_at=CURRENT_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,error_code=NULL,error_message=NULL WHERE id=? AND locked_by=? AND status='processing'")
            .bind(&result)
            .bind(claimed.command.id)
            .bind(claimed.lease_id)
            .execute(&mut *transaction)
            .await?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }
        crate::runtime_events::append_runtime_event(
            &mut transaction,
            &RuntimeEventEnvelope::new(
                claimed.command.tenant_id,
                "runtime.command.completed",
                &claimed.command.aggregate_type,
                &claimed.command.aggregate_id,
                execution_id.map(ExecutionId::from_uuid),
                None,
                serde_json::json!({
                    "commandId": claimed.command.id,
                    "commandType": claimed.command.command_type.as_str(),
                    "executionId": execution_id,
                    "result": result,
                    "status": "completed",
                }),
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn fail(
        &self,
        claimed: &ClaimedRuntimeCommand,
        code: &str,
        message: &str,
        retry_after: u64,
    ) -> Result<bool> {
        self.fail_with_policy(claimed, code, message, retry_after, false)
            .await
    }

    pub async fn fail_terminal(
        &self,
        claimed: &ClaimedRuntimeCommand,
        code: &str,
        message: &str,
    ) -> Result<bool> {
        self.fail_with_policy(claimed, code, message, 1, true).await
    }

    async fn fail_with_policy(
        &self,
        claimed: &ClaimedRuntimeCommand,
        code: &str,
        message: &str,
        retry_after: u64,
        force_terminal: bool,
    ) -> Result<bool> {
        let retry_after = retry_after.clamp(1, 86_400);
        let terminal = force_terminal || claimed.attempt_count >= 20;
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE runtime_commands SET status=IF(?,'failed',IF(attempt_count>=20,'failed','pending')),available_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),locked_by=NULL,locked_until=NULL,error_code=?,error_message=? WHERE id=? AND locked_by=? AND status='processing'")
            .bind(terminal).bind(retry_after).bind(code).bind(message.chars().take(1000).collect::<String>()).bind(claimed.command.id).bind(claimed.lease_id).execute(&mut *transaction).await?;
        if changed.rows_affected() == 1 && terminal {
            crate::runtime_events::append_runtime_event(
                &mut transaction,
                &RuntimeEventEnvelope::new(
                    claimed.command.tenant_id,
                    "runtime.command.failed",
                    &claimed.command.aggregate_type,
                    &claimed.command.aggregate_id,
                    None,
                    None,
                    serde_json::json!({
                        "commandId": claimed.command.id,
                        "commandType": claimed.command.command_type.as_str(),
                        "code": code,
                        "message": message.chars().take(1000).collect::<String>(),
                        "status": "failed",
                    }),
                ),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(changed.rows_affected() == 1)
    }

    pub async fn reconcile_expired(&self) -> Result<u64> {
        let result = sqlx::query("UPDATE runtime_commands SET status='pending',locked_by=NULL,locked_until=NULL,available_at=CURRENT_TIMESTAMP(6) WHERE status='processing' AND locked_until<CURRENT_TIMESTAMP(6)")
            .execute(&self.pool).await?;
        Ok(result.rows_affected())
    }
}

fn ensure_same_command(row: &sqlx::mysql::MySqlRow, command: &RuntimeCommand) -> Result<()> {
    let aggregate_type: String = row.try_get("aggregate_type")?;
    let aggregate_id: String = row.try_get("aggregate_id")?;
    let payload: Value = row.try_get("payload_json")?;
    if aggregate_type != command.aggregate_type
        || aggregate_id != command.aggregate_id
        || payload != command.payload
    {
        return Err(anyhow!(
            "runtime command idempotency key was reused with different content"
        ));
    }
    Ok(())
}
