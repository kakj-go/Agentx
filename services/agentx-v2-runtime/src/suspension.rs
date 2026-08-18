use agentx_domain::NodeExecutionId;
use agentx_runtime::ExecutionMachine;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::error::RuntimeResult;

pub async fn enqueue_due(pool: &sqlx::MySqlPool, owner: Uuid, limit: u32) -> RuntimeResult<u64> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "SELECT id,tenant_id,execution_id,node_execution_id,resume_token_id,CASE WHEN wake_at IS NOT NULL AND wake_at<=UTC_TIMESTAMP(6) THEN 'resumed' ELSE 'timed_out' END wait_status FROM wait_subscriptions WHERE status='waiting' AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) AND ((wake_at IS NOT NULL AND wake_at<=UTC_TIMESTAMP(6)) OR (timeout_at IS NOT NULL AND timeout_at<=UTC_TIMESTAMP(6))) ORDER BY COALESCE(wake_at,timeout_at),id LIMIT ? FOR UPDATE SKIP LOCKED",
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(&mut *tx)
    .await?;
    for row in &rows {
        use sqlx::Row;
        let wait_id: Uuid = row.try_get("id")?;
        let tenant_id: Uuid = row.try_get("tenant_id")?;
        let execution_id: Uuid = row.try_get("execution_id")?;
        let node_execution_id: Uuid = row.try_get("node_execution_id")?;
        let wait_status: String = row.try_get("wait_status")?;
        let output_port = if wait_status == "resumed" {
            "resumed"
        } else {
            "timed_out"
        };
        sqlx::query("UPDATE wait_subscriptions SET locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),fencing_token=fencing_token+1,heartbeat_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status='waiting'")
            .bind(owner).bind(tenant_id).bind(wait_id).execute(&mut *tx).await?;
        sqlx::query("INSERT IGNORE INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status) VALUES(?,?,'resume_wait','execution',?,?,?,'pending')")
            .bind(crate::engine_names::deterministic_uuid(wait_id, b"due-wait-command"))
            .bind(tenant_id)
            .bind(execution_id.to_string())
            .bind(format!("wait-due:{wait_id}"))
            .bind(json!({
                "waitId":wait_id,
                "nodeExecutionId":node_execution_id,
                "outputPort":output_port,
                "waitStatus":wait_status,
                "payload":{}
            }))
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE execution_resume_tokens SET status=IF(?='resumed','used','expired'),used_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status='active'")
            .bind(&wait_status).bind(tenant_id).bind(row.try_get::<Uuid,_>("resume_token_id")?).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(rows.len() as u64)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    bundle_id: Uuid,
    work_package_id: Option<Uuid>,
    node_execution_id: NodeExecutionId,
    state_version: u64,
    node: &agentx_runtime::CompiledNode,
    machine: &ExecutionMachine,
    context: &Value,
) -> RuntimeResult<()> {
    let checkpoint_id = crate::engine::persist_checkpoint(
        tx,
        tenant_id,
        execution_id,
        bundle_id,
        work_package_id,
        state_version,
        Some(node_execution_id),
        machine,
        context,
        "node_suspended",
    )
    .await?;
    if node.node_type == "approval" {
        let task_id = Uuid::now_v7();
        let workflow_id: Uuid = sqlx::query_scalar(
            "SELECT workflow_id FROM workflow_executions WHERE tenant_id=? AND id=?",
        )
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_one(&mut **tx)
        .await?;
        let title = node
            .parameters
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(&node.name)
            .to_owned();
        let description = node
            .parameters
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let request = node.parameters.get("request").cloned();
        let timeout_seconds = node
            .parameters
            .get("timeoutSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(86_400);
        sqlx::query(
            "INSERT INTO approval_tasks(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,workflow_id,node_id,title,description,request_payload_json,status,resume_status,deadline_at,version) VALUES(?,?,?,?,?,?,?,?,?,?,?,'pending','not_requested',DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),1)",
        )
        .bind(task_id)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(node_execution_id.as_uuid())
        .bind(bundle_id)
        .bind(checkpoint_id)
        .bind(workflow_id)
        .bind(&node.id)
        .bind(&title)
        .bind(&description)
        .bind(&request)
        .bind(timeout_seconds)
        .execute(&mut **tx)
        .await?;
        if let Some(candidate_id) = node
            .parameters
            .get("candidateUserId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        {
            sqlx::query("INSERT INTO approval_candidates(tenant_id,approval_task_id,candidate_type,candidate_id) VALUES(?,?,'user',?)")
                .bind(tenant_id)
                .bind(task_id)
                .bind(candidate_id)
                .execute(&mut **tx)
                .await?;
        }
        crate::event_export::enqueue_approval_event_from_task(tx, tenant_id, task_id).await?;
    } else {
        let mut raw = [0_u8; 32];
        OsRng.fill_bytes(&mut raw);
        let token = URL_SAFE_NO_PAD.encode(raw);
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let token_id = Uuid::now_v7();
        let wait_id = Uuid::now_v7();
        let kind = node
            .parameters
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("duration");
        let resume_kind = if matches!(kind, "duration" | "datetime") {
            "time"
        } else if kind == "form" {
            "form"
        } else {
            "webhook"
        };
        let duration_micros = node
            .parameters
            .get("durationMs")
            .and_then(Value::as_u64)
            .unwrap_or(1_000)
            .saturating_mul(1_000);
        let resume_at = node
            .parameters
            .get("resumeAt")
            .and_then(Value::as_str)
            .map(|value| OffsetDateTime::parse(value, &Rfc3339))
            .transpose()
            .map_err(|error| {
                crate::error::RuntimeError::InvalidRequest("INVALID_WAIT_TIME", error.to_string())
            })?;
        let timeout_at = node
            .parameters
            .get("timeoutAt")
            .and_then(Value::as_str)
            .map(|value| OffsetDateTime::parse(value, &Rfc3339))
            .transpose()
            .map_err(|error| {
                crate::error::RuntimeError::InvalidRequest(
                    "INVALID_WAIT_TIMEOUT",
                    error.to_string(),
                )
            })?;
        sqlx::query(
            "INSERT INTO execution_resume_tokens(id,tenant_id,execution_id,node_execution_id,token_hash,resume_kind,status,response_json,expires_at) VALUES(?,?,?,?,?,?,'active',?,COALESCE(?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 1 DAY)))",
        )
        .bind(token_id)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(node_execution_id.as_uuid())
        .bind(token_hash)
        .bind(resume_kind)
        .bind(json!({"resumeToken":token,"waitId":wait_id}))
        .bind(timeout_at.or(resume_at))
        .execute(&mut **tx)
        .await?;
        let query = if kind == "duration" {
            sqlx::query("INSERT INTO wait_subscriptions(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,state_version,resume_token_id,wait_kind,status,wake_at,timeout_at,authentication_mode,response_mode) VALUES(?,?,?,?,?,?,?,?,'duration','waiting',DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? MICROSECOND),?,'signed','accepted')")
                .bind(wait_id).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
                .bind(bundle_id).bind(checkpoint_id).bind(state_version).bind(token_id)
                .bind(duration_micros).bind(timeout_at)
        } else {
            sqlx::query("INSERT INTO wait_subscriptions(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,state_version,resume_token_id,wait_kind,status,wake_at,timeout_at,authentication_mode,response_mode) VALUES(?,?,?,?,?,?,?,?,?,'waiting',?,?,?,'accepted')")
                .bind(wait_id).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
                .bind(bundle_id).bind(checkpoint_id).bind(state_version).bind(token_id).bind(kind)
                .bind(resume_at).bind(timeout_at).bind(node.parameters.get("authenticationMode").and_then(Value::as_str).unwrap_or("signed"))
        };
        query.execute(&mut **tx).await?;
        sqlx::query(
            "INSERT INTO bundle_references(id,tenant_id,bundle_id,reference_kind,owner_id) VALUES(?,?,?,'pending_wait',?)",
        )
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(bundle_id)
        .bind(wait_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
