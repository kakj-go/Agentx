use agentx_domain::NodeExecutionId;
use agentx_runtime::{ExecutionMachine, ExpressionContext, ExpressionEngine, NodeActivation};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    engine_persistence::{load_output_namespace, upsert_failed_activation},
    engine_protocol::machine_error,
    error::{RuntimeError, RuntimeResult},
};

pub(crate) struct SuspendRequest<'a> {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub bundle_id: Uuid,
    pub work_package_id: Option<Uuid>,
    pub node_execution_id: NodeExecutionId,
    pub attempt_id: Uuid,
    pub state_version: u64,
    pub node: &'a agentx_runtime::CompiledNode,
    pub activation: &'a NodeActivation,
    pub machine: &'a mut ExecutionMachine,
    pub context: &'a Value,
}

pub(crate) async fn resolve_and_create(
    tx: &mut Transaction<'_, MySql>,
    mut request: SuspendRequest<'_>,
) -> RuntimeResult<()> {
    let current = request
        .activation
        .inputs
        .values()
        .flat_map(|items| items.iter())
        .next()
        .map(|item| item.json.clone())
        .unwrap_or(Value::Null);
    let execution_input: Value =
        sqlx::query_scalar("SELECT input_json FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(request.tenant_id)
            .bind(request.execution_id)
            .fetch_one(&mut **tx)
            .await?;
    let resolved_parameters = ExpressionEngine.resolve_parameters(
        &request.node.parameters,
        &ExpressionContext {
            json: current.clone(),
            input: current.clone(),
            inputs: execution_input,
            outputs: load_output_namespace(tx, request.tenant_id, request.execution_id).await?,
            contexts: request.context.clone(),
            execution: crate::execution_context::with_node(
                crate::execution_context::load(tx, request.tenant_id, request.execution_id).await?,
                &request.node.id,
                request.node_execution_id.as_uuid(),
                request.activation.run_index,
                None,
                None,
            ),
            run_index: request.activation.run_index,
            output_node_keys: request
                .machine
                .workflow()
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node.key.clone()))
                .collect(),
            ..ExpressionContext::default()
        },
    );
    let resolved_parameters = match resolved_parameters {
        Ok(parameters) => parameters,
        Err(error) => {
            fail_parameter_resolution(tx, &mut request, error.to_string()).await?;
            return Ok(());
        }
    };
    create(
        tx,
        request.tenant_id,
        request.execution_id,
        request.bundle_id,
        request.work_package_id,
        request.node_execution_id,
        request.state_version,
        request.node,
        &resolved_parameters,
        &current,
        request.machine,
        request.context,
    )
    .await
}

async fn fail_parameter_resolution(
    tx: &mut Transaction<'_, MySql>,
    request: &mut SuspendRequest<'_>,
    message: String,
) -> RuntimeResult<()> {
    request
        .machine
        .fail(
            request.node_execution_id,
            "DYNAMIC_VALUE_EVALUATION_FAILED",
            &message,
            false,
        )
        .map_err(machine_error)?;
    let failed = request
        .machine
        .activation(request.node_execution_id)
        .cloned()
        .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
    upsert_failed_activation(
        tx,
        request.tenant_id,
        request.execution_id,
        &failed,
        request.node,
        "DYNAMIC_VALUE_EVALUATION_FAILED",
        &message,
    )
    .await?;
    sqlx::query("UPDATE node_attempts SET status='failed',error_code='DYNAMIC_VALUE_EVALUATION_FAILED',error_message=?,ended_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status='suspended'")
        .bind(&message)
        .bind(request.tenant_id)
        .bind(request.attempt_id)
        .execute(&mut **tx)
        .await?;
    let mut trace = crate::trace_delivery::TraceDraft::execution(
        request.tenant_id,
        request.execution_id,
        "node.failed",
        "failed",
    );
    trace.node_execution_id = Some(request.node_execution_id.as_uuid());
    trace.attempt_id = Some(request.attempt_id);
    trace.error_code = Some("DYNAMIC_VALUE_EVALUATION_FAILED".into());
    trace.error_message = Some(message);
    crate::trace_delivery::enqueue_best_effort(tx, trace).await;
    Ok(())
}

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
    let approvals = sqlx::query(
        "SELECT id,tenant_id,execution_id,node_execution_id,request_payload_json FROM approval_tasks WHERE status IN ('pending','claimed') AND deadline_at<=UTC_TIMESTAMP(6) ORDER BY deadline_at,id LIMIT ? FOR UPDATE SKIP LOCKED",
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(&mut *tx)
    .await?;
    for row in &approvals {
        use sqlx::Row;
        let task_id: Uuid = row.try_get("id")?;
        let tenant_id: Uuid = row.try_get("tenant_id")?;
        let execution_id: Uuid = row.try_get("execution_id")?;
        let node_execution_id: Uuid = row.try_get("node_execution_id")?;
        let input: Option<Value> = row.try_get("request_payload_json")?;
        sqlx::query("UPDATE approval_tasks SET status='timed_out',resume_status='pending',version=version+1,locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND status IN ('pending','claimed')")
            .bind(tenant_id).bind(task_id).execute(&mut *tx).await?;
        sqlx::query("INSERT IGNORE INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status) VALUES(?,?,'resume_wait','execution',?,?,?,'pending')")
            .bind(crate::engine_names::deterministic_uuid(task_id, b"approval-timeout-command"))
            .bind(tenant_id).bind(execution_id.to_string()).bind(format!("approval-timeout:{task_id}"))
            .bind(json!({"nodeExecutionId":node_execution_id,"outputPort":"timed_out","waitStatus":"timed_out","payload":approval_timeout_output(task_id,input.unwrap_or(Value::Null))}))
            .execute(&mut *tx).await?;
        crate::event_export::enqueue_approval_event_from_task(&mut tx, tenant_id, task_id).await?;
    }
    tx.commit().await?;
    Ok((rows.len() + approvals.len()) as u64)
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
    parameters: &Value,
    input: &Value,
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
        let title = parameters
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(&node.name)
            .to_owned();
        let description = parameters
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let request = Some(input.clone());
        let (timeout_seconds, timeout_at) = approval_timeout(parameters)?;
        sqlx::query(
            "INSERT INTO approval_tasks(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,workflow_id,node_id,title,description,request_payload_json,status,resume_status,deadline_at,version) VALUES(?,?,?,?,?,?,?,?,?,?,?,'pending','not_requested',COALESCE(?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND)),1)",
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
        .bind(timeout_at)
        .bind(timeout_seconds)
        .execute(&mut **tx)
        .await?;
        if let Some(candidate_id) = parameters
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
        emit_wait_started(
            tx,
            tenant_id,
            execution_id,
            node_execution_id,
            task_id,
            "approval",
            &title,
            request.as_ref(),
        )
        .await;
    } else {
        let mut raw = [0_u8; 32];
        OsRng.fill_bytes(&mut raw);
        let token = URL_SAFE_NO_PAD.encode(raw);
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let token_id = Uuid::now_v7();
        let wait_id = Uuid::now_v7();
        let kind = parameters
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
        let duration_micros = parameters
            .get("durationMs")
            .and_then(Value::as_u64)
            .unwrap_or(1_000)
            .saturating_mul(1_000);
        let resume_at = parameters
            .get("resumeAt")
            .and_then(Value::as_str)
            .map(|value| OffsetDateTime::parse(value, &Rfc3339))
            .transpose()
            .map_err(|error| {
                crate::error::RuntimeError::InvalidRequest("INVALID_WAIT_TIME", error.to_string())
            })?;
        let timeout_at = parameters
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
            sqlx::query("INSERT INTO wait_subscriptions(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,state_version,resume_token_id,wait_kind,status,wake_at,timeout_at,authentication_mode,response_mode,payload_schema_json) VALUES(?,?,?,?,?,?,?,?,'duration','waiting',DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? MICROSECOND),?,'signed','accepted',?)")
                .bind(wait_id).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
                .bind(bundle_id).bind(checkpoint_id).bind(state_version).bind(token_id)
                .bind(duration_micros).bind(timeout_at).bind(parameters.get("payloadSchema"))
        } else {
            sqlx::query("INSERT INTO wait_subscriptions(id,tenant_id,execution_id,node_execution_id,bundle_id,checkpoint_id,state_version,resume_token_id,wait_kind,status,wake_at,timeout_at,authentication_mode,response_mode,payload_schema_json) VALUES(?,?,?,?,?,?,?,?,?,'waiting',?,?,?,'accepted',?)")
                .bind(wait_id).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
                .bind(bundle_id).bind(checkpoint_id).bind(state_version).bind(token_id).bind(kind)
                .bind(resume_at).bind(timeout_at).bind(parameters.get("authenticationMode").and_then(Value::as_str).unwrap_or("signed")).bind(parameters.get("payloadSchema"))
        };
        query.execute(&mut **tx).await?;
        emit_wait_started(
            tx,
            tenant_id,
            execution_id,
            node_execution_id,
            wait_id,
            kind,
            &node.name,
            None,
        )
        .await;
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

fn approval_timeout(parameters: &Value) -> RuntimeResult<(u64, Option<OffsetDateTime>)> {
    let timeout_seconds = parameters
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(86_400_000)
        .div_ceil(1_000);
    let timeout_at = parameters
        .get("timeoutAt")
        .and_then(Value::as_str)
        .map(|value| OffsetDateTime::parse(value, &Rfc3339))
        .transpose()
        .map_err(|error| {
            RuntimeError::InvalidRequest("INVALID_APPROVAL_TIMEOUT", error.to_string())
        })?;
    Ok((timeout_seconds, timeout_at))
}

fn approval_timeout_output(task_id: Uuid, input: Value) -> Value {
    json!({
        "taskId": task_id,
        "decision": "timed_out",
        "reason": null,
        "decidedBy": null,
        "input": input,
    })
}

#[allow(clippy::too_many_arguments)]
async fn emit_wait_started(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    node_execution_id: NodeExecutionId,
    wait_id: Uuid,
    wait_kind: &str,
    span_name: &str,
    content: Option<&Value>,
) {
    let mut trace = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        wait_id,
        Some((
            node_execution_id.as_uuid(),
            agentx_runtime_contracts::TraceSpanKindV1::Node,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Wait,
        if wait_kind == "approval" {
            format!("Approval · {span_name}")
        } else {
            format!("Wait · {span_name}")
        },
        agentx_runtime_contracts::TraceEventKindV1::Started,
        "wait.started",
        "waiting",
    );
    trace.node_execution_id = Some(node_execution_id.as_uuid());
    trace.wait_id = Some(wait_id);
    trace.attributes = json!({"waitKind":wait_kind});
    trace.content_kind = content.map(|_| agentx_runtime_contracts::TraceContentKindV1::WaitRequest);
    trace.content_preview = content.and_then(crate::trace_delivery::bounded_preview);
    crate::trace_delivery::enqueue_best_effort(tx, trace).await;
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{approval_timeout, approval_timeout_output};

    #[test]
    fn approval_timeout_consumes_milliseconds_and_absolute_deadline() {
        let (seconds, deadline) =
            approval_timeout(&json!({"timeoutMs":1501,"timeoutAt":"2026-08-20T10:00:00Z"}))
                .unwrap();
        assert_eq!(seconds, 2);
        assert_eq!(deadline.unwrap().unix_timestamp(), 1_787_220_000);
        assert!(approval_timeout(&json!({"timeoutAt":"not-a-time"})).is_err());
    }

    #[test]
    fn approval_timeout_output_matches_the_frozen_port_contract() {
        let task_id = Uuid::now_v7();
        assert_eq!(
            approval_timeout_output(task_id, json!({"request":"deploy"})),
            json!({"taskId":task_id,"decision":"timed_out","reason":null,"decidedBy":null,"input":{"request":"deploy"}})
        );
    }
}
