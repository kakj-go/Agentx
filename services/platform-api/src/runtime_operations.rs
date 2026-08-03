use agentx_application::{
    ExecutionRuntime, ForkExecutionCommand, RequestExecution, SideEffectConfirmationCommand,
};
use agentx_domain::{
    CheckpointId, ExecutionId, NodeExecutionId, TenantId, UserId, WorkflowVersionId,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::require_workflow_access,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartExecutionRequest {
    #[serde(default)]
    pub input: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionCommandResponse {
    pub execution_id: Uuid,
    pub status: String,
    pub replayed: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeAttemptResponse {
    pub id: Uuid,
    pub attempt_number: u32,
    pub status: String,
    pub worker_instance_id: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub deadline_at: Option<OffsetDateTime>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageResponse {
    pub delivery_id: Uuid,
    pub target_item_index: u32,
    pub source_node_execution_id: Uuid,
    pub source_run_index: u32,
    pub source_output_index: u32,
    pub source_item_index: u32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    pub node_version: u32,
    pub generation: u32,
    pub activation_slot: u32,
    pub run_index: u32,
    pub iteration_index: u32,
    pub status: String,
    pub capability: String,
    pub side_effect_level: String,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub attempts: Vec<NodeAttemptResponse>,
    pub lineage: Vec<LineageResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionListResponse {
    pub items: Vec<NodeExecutionResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Option<Uuid>,
    pub sequence_number: u64,
    pub checkpoint_type: String,
    pub state_hash: String,
    pub activation_count: u64,
    pub delivery_count: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointListResponse {
    pub items: Vec<CheckpointResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub wait_kind: String,
    pub status: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub wake_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub timeout_at: Option<OffsetDateTime>,
    pub authentication_mode: String,
    pub resume_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitListResponse {
    pub items: Vec<WaitResponse>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForkRequest {
    pub checkpoint_id: Uuid,
    pub mode: String,
    pub node_id: Option<String>,
    #[serde(default)]
    pub input_overrides: Value,
    #[serde(default)]
    pub side_effect_decisions: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SideEffectConfirmationRequest {
    pub node_execution_id: Uuid,
    pub checkpoint_id: Option<Uuid>,
    pub decision: String,
    pub idempotency_key: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdempotentCommandResponse {
    pub accepted: bool,
    pub replayed: bool,
}

#[utoipa::path(post,path="/api/v1/workflow-versions/{version_id}/executions",request_body=StartExecutionRequest,responses((status=202,body=ExecutionCommandResponse)))]
pub async fn start_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(version_id): Path<Uuid>,
    Json(input): Json<StartExecutionRequest>,
) -> AppResult<(StatusCode, Json<ExecutionCommandResponse>)> {
    actor.require("execution:run")?;
    let workflow_id: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM workflow_versions WHERE tenant_id=? AND id=?")
            .bind(actor.tenant_id)
            .bind(version_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("Workflow Version"))?;
    require_workflow_access(&state.pool, &actor, workflow_id, true).await?;
    let runtime = runtime(&state)?;
    let accepted = runtime
        .request_execution(RequestExecution {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            invocation_id: None,
            session_id: None,
            workflow_version_id: WorkflowVersionId::from_uuid(version_id),
            requested_by: Some(UserId::from_uuid(actor.user_id)),
            trigger_type: "manual".into(),
            input: input.input,
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExecutionCommandResponse {
            execution_id: accepted.execution_id.as_uuid(),
            status: status_name(accepted.status).into(),
            replayed: false,
        }),
    ))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/cancel",responses((status=204)))]
pub async fn cancel_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("execution:cancel")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    runtime(&state)?
        .cancel_execution(
            TenantId::from_uuid(actor.tenant_id),
            ExecutionId::from_uuid(id),
        )
        .await
        .map_err(runtime_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get,path="/api/v1/executions/{id}/nodes",responses((status=200,body=NodeExecutionListResponse)))]
pub async fn list_nodes(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<NodeExecutionListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows = sqlx::query(&format!("{NODE_SELECT} ORDER BY n.id"))
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    let mut items = Vec::new();
    for row in rows {
        items.push(node_from_row(&state, actor.tenant_id, row, false).await?);
    }
    Ok(Json(NodeExecutionListResponse { items }))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/nodes/{node_execution_id}",responses((status=200,body=NodeExecutionResponse)))]
pub async fn get_node(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, node_execution_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<NodeExecutionResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let row = sqlx::query(&format!("{NODE_SELECT} AND n.id=?"))
        .bind(actor.tenant_id)
        .bind(id)
        .bind(node_execution_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Node Execution"))?;
    Ok(Json(
        node_from_row(&state, actor.tenant_id, row, true).await?,
    ))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/checkpoints",responses((status=200,body=CheckpointListResponse)))]
pub async fn list_checkpoints(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CheckpointListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows=sqlx::query("SELECT id,execution_id,node_execution_id,sequence_number,checkpoint_type,state_hash,payload_json,created_at FROM checkpoints WHERE tenant_id=? AND execution_id=? ORDER BY sequence_number").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let payload: Value = row.try_get("payload_json")?;
            Ok(CheckpointResponse {
                id: row.try_get("id")?,
                execution_id: row.try_get("execution_id")?,
                node_execution_id: row.try_get("node_execution_id")?,
                sequence_number: row.try_get("sequence_number")?,
                checkpoint_type: row.try_get("checkpoint_type")?,
                state_hash: row.try_get("state_hash")?,
                activation_count: payload
                    .get("activations")
                    .and_then(Value::as_object)
                    .map_or(0, |v| v.len() as u64),
                delivery_count: payload
                    .get("deliveries")
                    .and_then(Value::as_array)
                    .map_or(0, |v| v.len() as u64),
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(CheckpointListResponse { items }))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/waits",responses((status=200,body=WaitListResponse)))]
pub async fn list_waits(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<WaitListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows=sqlx::query("SELECT id,execution_id,node_execution_id,wait_kind,status,wake_at,timeout_at,authentication_mode FROM wait_subscriptions WHERE tenant_id=? AND execution_id=? ORDER BY created_at").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let wait_id: Uuid = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            Ok(WaitResponse {
                id: wait_id,
                execution_id: row.try_get("execution_id")?,
                node_execution_id: row.try_get("node_execution_id")?,
                wait_kind: row.try_get("wait_kind")?,
                resume_url: (status == "waiting")
                    .then(|| format!("/gateway/v1/waits/{wait_id}/resume")),
                status,
                wake_at: row.try_get("wake_at")?,
                timeout_at: row.try_get("timeout_at")?,
                authentication_mode: row.try_get("authentication_mode")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(WaitListResponse { items }))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/fork",request_body=ForkRequest,responses((status=202,body=ExecutionCommandResponse)))]
pub async fn fork_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ForkRequest>,
) -> AppResult<(StatusCode, Json<ExecutionCommandResponse>)> {
    actor.require("execution:fork")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, true).await?;
    let accepted = runtime(&state)?
        .fork_execution(ForkExecutionCommand {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            source_execution_id: ExecutionId::from_uuid(id),
            checkpoint_id: CheckpointId::from_uuid(input.checkpoint_id),
            mode: input.mode,
            node_id: input.node_id,
            input_overrides: input.input_overrides,
            side_effect_decisions: input.side_effect_decisions,
            actor_user_id: UserId::from_uuid(actor.user_id),
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExecutionCommandResponse {
            execution_id: accepted.execution_id.as_uuid(),
            status: status_name(accepted.status).into(),
            replayed: false,
        }),
    ))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/side-effect-confirmations",request_body=SideEffectConfirmationRequest,responses((status=200,body=IdempotentCommandResponse)))]
pub async fn confirm_side_effect(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<SideEffectConfirmationRequest>,
) -> AppResult<Json<IdempotentCommandResponse>> {
    actor.require("execution:fork")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, true).await?;
    let replayed = runtime(&state)?
        .confirm_side_effect(SideEffectConfirmationCommand {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            execution_id: ExecutionId::from_uuid(id),
            node_execution_id: NodeExecutionId::from_uuid(input.node_execution_id),
            checkpoint_id: input.checkpoint_id.map(CheckpointId::from_uuid),
            decision: input.decision,
            actor_user_id: UserId::from_uuid(actor.user_id),
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok(Json(IdempotentCommandResponse {
        accepted: true,
        replayed,
    }))
}

const NODE_SELECT: &str = "SELECT n.id,n.execution_id,n.node_id,n.node_name,n.node_type,n.node_version,n.generation,n.activation_slot,n.run_index,n.iteration_index,n.status,n.capability,n.side_effect_level,n.input_json,n.output_json,n.error_code,n.error_message,n.started_at,n.ended_at FROM node_executions n WHERE n.tenant_id=? AND n.execution_id=?";
async fn node_from_row(
    state: &AppState,
    tenant: Uuid,
    row: sqlx::mysql::MySqlRow,
    detail: bool,
) -> AppResult<NodeExecutionResponse> {
    let id: Uuid = row.try_get("id")?;
    let attempts=sqlx::query("SELECT id,attempt_number,status,worker_instance_id,deadline_at,error_code,error_message,started_at,ended_at FROM node_attempts WHERE tenant_id=? AND node_execution_id=? ORDER BY attempt_number").bind(tenant).bind(id).fetch_all(&state.pool).await?.into_iter().map(|r|Ok(NodeAttemptResponse{id:r.try_get("id")?,attempt_number:r.try_get("attempt_number")?,status:r.try_get("status")?,worker_instance_id:r.try_get("worker_instance_id")?,deadline_at:r.try_get("deadline_at")?,error_code:r.try_get("error_code")?,error_message:r.try_get("error_message")?,started_at:r.try_get("started_at")?,ended_at:r.try_get("ended_at")?})).collect::<Result<Vec<_>,sqlx::Error>>()?;
    let lineage = if detail {
        sqlx::query("SELECT delivery_id,target_item_index,source_node_execution_id,source_run_index,source_output_index,source_item_index FROM item_lineage WHERE tenant_id=? AND execution_id=? AND delivery_id IN (SELECT id FROM execution_edge_deliveries WHERE source_node_execution_id=? OR target_node_id=?) ORDER BY delivery_id,target_item_index").bind(tenant).bind(row.try_get::<Uuid,_>("execution_id")?).bind(id).bind(row.try_get::<String,_>("node_id")?).fetch_all(&state.pool).await?.into_iter().map(|r|Ok(LineageResponse{delivery_id:r.try_get("delivery_id")?,target_item_index:r.try_get("target_item_index")?,source_node_execution_id:r.try_get("source_node_execution_id")?,source_run_index:r.try_get("source_run_index")?,source_output_index:r.try_get("source_output_index")?,source_item_index:r.try_get("source_item_index")?})).collect::<Result<Vec<_>,sqlx::Error>>()?
    } else {
        Vec::new()
    };
    Ok(NodeExecutionResponse {
        id,
        execution_id: row.try_get("execution_id")?,
        node_id: row.try_get("node_id")?,
        node_name: row.try_get("node_name")?,
        node_type: row.try_get("node_type")?,
        node_version: row.try_get("node_version")?,
        generation: row.try_get("generation")?,
        activation_slot: row.try_get("activation_slot")?,
        run_index: row.try_get("run_index")?,
        iteration_index: row.try_get("iteration_index")?,
        status: row.try_get("status")?,
        capability: row.try_get("capability")?,
        side_effect_level: row.try_get("side_effect_level")?,
        input: row.try_get("input_json")?,
        output: row.try_get("output_json")?,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        attempts,
        lineage,
    })
}
async fn execution_workflow(state: &AppState, actor: &AuthActor, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT workflow_id FROM workflow_executions WHERE tenant_id=? AND id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Execution"))
}
fn runtime(state: &AppState) -> AppResult<&dyn ExecutionRuntime> {
    state.runtime.as_deref().ok_or_else(|| {
        AppError::service_unavailable("RUNTIME_UNAVAILABLE", "Workflow runtime is unavailable")
    })
}
fn runtime_error(error: anyhow::Error) -> AppError {
    let message = error.to_string();
    if message.contains("not found") {
        AppError::not_found("Runtime resource")
    } else if message.contains("INVALID")
        || message.contains("IDEMPOTENCY")
        || message.contains("SIDE_EFFECT")
    {
        AppError::conflict("RUNTIME_COMMAND_REJECTED", message)
    } else {
        AppError::service_unavailable("RUNTIME_UNAVAILABLE", "Workflow runtime is unavailable")
    }
}
fn status_name(status: agentx_domain::ExecutionStatus) -> &'static str {
    match status {
        agentx_domain::ExecutionStatus::Created => "created",
        agentx_domain::ExecutionStatus::Queued => "queued",
        agentx_domain::ExecutionStatus::Running => "running",
        agentx_domain::ExecutionStatus::Waiting => "waiting",
        agentx_domain::ExecutionStatus::WaitingApproval => "waiting_approval",
        agentx_domain::ExecutionStatus::Suspended => "suspended",
        agentx_domain::ExecutionStatus::Succeeded => "succeeded",
        agentx_domain::ExecutionStatus::Failed => "failed",
        agentx_domain::ExecutionStatus::Cancelled => "cancelled",
        agentx_domain::ExecutionStatus::TimedOut => "timed_out",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_status_names_match_database() {
        assert_eq!(
            status_name(agentx_domain::ExecutionStatus::WaitingApproval),
            "waiting_approval"
        );
    }
}
