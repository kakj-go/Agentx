//! Durable wakeups for Agent Session inputs that arrived while another
//! Application Session operation was open.
//!
//! The pending row is authoritative.  This module only creates an idempotent
//! Runtime `resume_execution` command after the Session Register is free; it
//! never reconstructs an execution from process memory or Control Plane data.

use agentx_runtime_contracts::deterministic_uuid;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use crate::error::RuntimeResult;

const PENDING_SESSION_QUERY: &str = r#"
SELECT p.entry_id,p.execution_id,p.node_execution_id,p.attempt_id
FROM agent_session_pending_entries p
JOIN agent_session_registers r ON r.tenant_id=p.tenant_id
  AND r.session_key=p.session_key
  AND r.stable_agent_node_key=p.stable_agent_node_key
  AND (r.open_operation_id IS NULL OR r.lease_expires_at<=UTC_TIMESTAMP(6))
WHERE p.status='pending' AND p.queue_kind='retry'
  AND p.wake_command_id IS NULL AND p.execution_id IS NOT NULL
ORDER BY p.sequence_number,p.entry_id LIMIT ? FOR UPDATE SKIP LOCKED
"#;

const INSERT_WAKE_COMMAND: &str = r#"
INSERT IGNORE INTO runtime_commands(
  id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status
)
SELECT ?,tenant_id,'resume_execution','execution',CAST(BIN_TO_UUID(execution_id) AS CHAR),?,?, 'pending'
FROM agent_session_pending_entries WHERE entry_id=?
"#;

const RESET_FAILED_WAKE_COMMAND: &str = r#"
UPDATE runtime_commands
SET status='pending',available_at=UTC_TIMESTAMP(6),locked_by=NULL,locked_until=NULL
WHERE id=? AND status='failed'
"#;

const CLAIM_PENDING_ENTRY: &str = r#"
UPDATE agent_session_pending_entries
SET wake_command_id=?
WHERE entry_id=? AND status='pending' AND wake_command_id IS NULL
"#;

/// Create at most `limit` durable wakeup commands and bind each command to
/// exactly one pending input.  Register and pending rows are locked together,
/// so a terminal Session commit and this wakeup cannot race into two resumes.
pub async fn wake_pending_sessions(pool: &MySqlPool, limit: u32) -> RuntimeResult<u32> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(PENDING_SESSION_QUERY)
        .bind(limit.clamp(1, 100))
        .fetch_all(&mut *tx)
        .await?;

    let mut woken = 0_u32;
    for row in rows {
        let entry_id: String = row.try_get("entry_id")?;
        let _execution_id: Uuid = row.try_get("execution_id")?;
        let node_execution_id: Uuid = row.try_get("node_execution_id")?;
        let attempt_id: Uuid = row.try_get("attempt_id")?;
        let command_id = wakeup_command_id(attempt_id, &entry_id);
        let idempotency_key = format!("agent-session-wakeup:{entry_id}");
        let payload = serde_json::json!({
            "nodeExecutionId": node_execution_id,
            "outputPort": "main",
            "payload": null,
            "agentSessionPendingEntryId": entry_id,
        });
        sqlx::query(INSERT_WAKE_COMMAND)
            .bind(command_id)
            .bind(idempotency_key)
            .bind(payload)
            .bind(&entry_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(RESET_FAILED_WAKE_COMMAND)
            .bind(command_id)
            .execute(&mut *tx)
            .await?;
        let updated = sqlx::query(CLAIM_PENDING_ENTRY)
            .bind(command_id)
            .bind(&entry_id)
            .execute(&mut *tx)
            .await?;
        if updated.rows_affected() == 1 {
            woken = woken.saturating_add(1);
        }
    }
    tx.commit().await?;
    Ok(woken)
}

fn wakeup_command_id(attempt_id: Uuid, entry_id: &str) -> Uuid {
    deterministic_uuid(
        attempt_id,
        format!("agent-session-wakeup:{entry_id}").as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CLAIM_PENDING_ENTRY, INSERT_WAKE_COMMAND, PENDING_SESSION_QUERY, RESET_FAILED_WAKE_COMMAND,
        wakeup_command_id,
    };
    use uuid::Uuid;

    #[test]
    fn wakeup_command_identity_is_stable_and_entry_scoped() {
        let attempt = Uuid::from_u128(7);
        let first = wakeup_command_id(attempt, "pending:a");
        assert_eq!(first, wakeup_command_id(attempt, "pending:a"));
        assert_ne!(first, wakeup_command_id(attempt, "pending:b"));
        assert_ne!(first, wakeup_command_id(Uuid::from_u128(8), "pending:a"));
    }

    #[test]
    fn wakeup_queries_preserve_sql_token_boundaries() {
        for query in [
            PENDING_SESSION_QUERY,
            INSERT_WAKE_COMMAND,
            RESET_FAILED_WAKE_COMMAND,
            CLAIM_PENDING_ENTRY,
        ] {
            assert!(!query.contains("pJOIN"));
            assert!(!query.contains("tenant_idAND"));
            assert!(!query.contains("session_keyAND"));
        }
        assert!(PENDING_SESSION_QUERY.contains("FROM agent_session_pending_entries p\nJOIN"));
        assert!(PENDING_SESSION_QUERY.contains("r.tenant_id=p.tenant_id\n  AND"));
    }
}
