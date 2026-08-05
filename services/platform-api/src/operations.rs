use agentx_api_types::PageResponse;
use agentx_application::{ArtifactStore, ResumeExecutionCommand};
use agentx_domain::{ArtifactId, ExecutionId, NodeExecutionId, TenantId, UserId};
use agentx_infrastructure::{
    artifact::MySqlObjectArtifactStore, operations_projection::redact_trace_attributes,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, outbox, require_workflow_access},
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub node_id: String,
    pub title: String,
    pub description: Option<String>,
    pub request_payload: Option<Value>,
    pub status: String,
    pub claimed_by: Option<Uuid>,
    pub claimed_by_name: Option<String>,
    pub resume_status: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub deadline_at: Option<OffsetDateTime>,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
pub type ApprovalPage = PageResponse<ApprovalResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionActionRequest {
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReassignApprovalRequest {
    pub target_user_id: Uuid,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideApprovalRequest {
    pub version: u64,
    pub input: Option<Value>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalActionResponse {
    pub id: Uuid,
    pub action_type: String,
    pub actor_user_id: Uuid,
    pub actor_name: String,
    pub from_status: String,
    pub to_status: String,
    pub input: Option<Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalCandidateResponse {
    pub user_id: Uuid,
    pub display_name: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResponse {
    pub id: Uuid,
    pub notification_type: String,
    pub title_key: String,
    pub body_key: String,
    pub arguments: Value,
    pub target_type: String,
    pub target_id: Uuid,
    pub target_path: String,
    pub tone: String,
    pub read: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationInboxResponse {
    pub items: Vec<NotificationResponse>,
    pub unread_count: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub workflow_version_id: Uuid,
    pub workflow_version_number: u64,
    pub invocation_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub trace_id: Uuid,
    pub trigger_type: String,
    pub execution_type: String,
    pub parent_execution_id: Option<Uuid>,
    pub caller_execution_id: Option<Uuid>,
    pub fork_checkpoint_id: Option<Uuid>,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub duration_ms: Option<u64>,
    pub cost_micros: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}
pub type ExecutionPage = PageResponse<ExecutionResponse>;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TraceEventResponse {
    pub event_id: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub execution_id: String,
    pub node_execution_id: Option<String>,
    pub attempt_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub runtime_call_id: Option<String>,
    pub sandbox_id: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub resource_version_id: Option<String>,
    pub node_id: Option<String>,
    pub event_type: String,
    pub status: String,
    pub event_time: String,
    pub duration_ms: Option<u64>,
    pub run_index: u32,
    pub iteration_index: u32,
    pub model_name: Option<String>,
    pub provider_name: Option<String>,
    pub mcp_tool_name: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_micros: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub stop_reason: Option<String>,
    pub partial: bool,
    pub attributes: Value,
    pub content_ref: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TraceResponse {
    pub execution_id: Uuid,
    pub trace_id: Uuid,
    pub events: Vec<TraceEventResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeComponentStatus {
    pub component: String,
    pub status: String,
    pub instances: Option<u64>,
    pub queue_depth: Option<u64>,
    pub last_heartbeat: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusResponse {
    pub components: Vec<RuntimeComponentStatus>,
    pub running: u64,
    pub waiting: u64,
    pub failed_today: u64,
    pub active_sandboxes: u64,
    pub sandbox_compatibility: Option<Value>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDetailsResponse {
    pub execution_id: Uuid,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micros: u64,
    pub agent_runs: Vec<AgentRunDetail>,
    pub iterations: Vec<AgentIterationDetail>,
    pub calls: Vec<RuntimeCallDetail>,
    pub sandboxes: Vec<SandboxLeaseDetail>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunDetail {
    pub id: Uuid,
    pub node_execution_id: Uuid,
    pub status: String,
    pub budget: Value,
    pub iteration_count: u32,
    pub model_call_count: u32,
    pub tool_call_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micros: u64,
    pub state_artifact_id: Option<Uuid>,
    pub state_hash: Option<String>,
    pub stop_reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentIterationDetail {
    pub id: Uuid,
    pub agent_run_id: Uuid,
    pub iteration_index: u32,
    pub status: String,
    pub state_before_hash: String,
    pub state_after_hash: Option<String>,
    pub state_artifact_id: Option<Uuid>,
    pub stop_reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallDetail {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub agent_run_id: Option<Uuid>,
    pub iteration_index: u32,
    pub call_index: u32,
    pub call_kind: String,
    pub request_fingerprint: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_version_id: Option<Uuid>,
    pub side_effect: String,
    pub status: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micros: u64,
    pub usage_estimated: bool,
    pub response_artifact_id: Option<Uuid>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SandboxLeaseDetail {
    pub id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub sandbox_id: Option<String>,
    pub profile_version_id: Uuid,
    pub status: String,
    pub expires_at: String,
    pub heartbeat_at: String,
    pub termination_attempts: u32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub terminated_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummaryResponse {
    pub workflow_count: u64,
    pub running_executions: u64,
    pub executions_today: u64,
    pub succeeded_today: u64,
    pub failed_today: u64,
    pub cost_micros_today: u64,
    pub pending_approvals: u64,
}

#[utoipa::path(get, path = "/api/v1/approvals")]
pub async fn list_approvals(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ApprovalPage>> {
    actor.require("approval:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let visible = approval_visibility_sql(&actor);
    let sql = format!(
        "{APPROVAL_SELECT} WHERE t.tenant_id=? AND {visible} AND (?='' OR t.status=?) AND (?='%%' OR t.title LIKE ? OR w.name LIKE ?) ORDER BY t.created_at DESC LIMIT ? OFFSET ?"
    );
    let mut query_rows = sqlx::query(&sql).bind(actor.tenant_id);
    if !actor.company_admin {
        query_rows = query_rows
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let rows = query_rows
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(&search)
        .bind(page_size)
        .bind(u64::from((page - 1) * page_size))
        .fetch_all(&state.pool)
        .await?;
    let count_sql = format!(
        "SELECT COUNT(*) FROM approval_tasks t JOIN workflows w ON w.id=t.workflow_id WHERE t.tenant_id=? AND {visible} AND (?='' OR t.status=?) AND (?='%%' OR t.title LIKE ? OR w.name LIKE ?)"
    );
    let mut count = sqlx::query_scalar(&count_sql).bind(actor.tenant_id);
    if !actor.company_admin {
        count = count
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let total: i64 = count
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(&search)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(approval_from_row)
            .collect::<AppResult<_>>()?,
        page,
        page_size,
        total: total as u64,
    }))
}

#[utoipa::path(get, path = "/api/v1/approvals/{id}")]
pub async fn get_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:view")?;
    require_approval_visible(&state, &actor, id).await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get, path = "/api/v1/approvals/{id}/actions")]
pub async fn list_approval_actions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ApprovalActionResponse>>> {
    actor.require("approval:view")?;
    require_approval_visible(&state, &actor, id).await?;
    let rows=sqlx::query("SELECT a.id,a.action_type,a.actor_user_id,u.display_name actor_name,a.from_status,a.to_status,a.input_json,a.created_at FROM approval_actions a JOIN users u ON u.id=a.actor_user_id WHERE a.tenant_id=? AND a.approval_task_id=? ORDER BY a.created_at").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(approval_action_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(get, path = "/api/v1/approvals/{id}/candidates")]
pub async fn list_approval_candidates(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ApprovalCandidateResponse>>> {
    actor.require("approval:view")?;
    require_approval_visible(&state, &actor, id).await?;
    let rows = sqlx::query("SELECT DISTINCT u.id,u.display_name FROM users u JOIN user_departments ud ON ud.tenant_id=u.tenant_id AND ud.user_id=u.id JOIN approval_tasks t ON t.tenant_id=u.tenant_id AND t.id=? JOIN workflows w ON w.tenant_id=t.tenant_id AND w.id=t.workflow_id WHERE u.tenant_id=? AND u.status='active' AND EXISTS(SELECT 1 FROM approval_candidates c WHERE c.tenant_id=u.tenant_id AND c.approval_task_id=t.id AND ((c.candidate_type='user' AND c.candidate_id=u.id) OR (c.candidate_type='role' AND EXISTS(SELECT 1 FROM user_roles ur WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND ur.role_id=c.candidate_id)) OR (c.candidate_type='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=u.tenant_id AND dc.ancestor_id=c.candidate_id AND dc.descendant_id=ud.department_id)))) AND (EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND r.code='company_admin') OR w.visibility='company' OR w.owner_user_id=u.id OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=u.id) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND dc.descendant_id=w.owner_department_id))) ORDER BY u.display_name")
        .bind(id)
        .bind(actor.tenant_id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                Ok(ApprovalCandidateResponse {
                    user_id: row.try_get("id")?,
                    display_name: row.try_get("display_name")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/claim",request_body=VersionActionRequest)]
pub async fn claim_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<VersionActionRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:act")?;
    if !is_candidate(&state, &actor, id, actor.user_id).await? && !actor.company_admin {
        return Err(AppError::forbidden("You are not an Approval candidate"));
    }
    transition_claim(&state, &actor, id, input.version).await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/release",request_body=VersionActionRequest)]
pub async fn release_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<VersionActionRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:act")?;
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE approval_tasks SET status='pending',claimed_by=NULL,claimed_at=NULL,version=version+1 WHERE id=? AND tenant_id=? AND status='claimed' AND claimed_by=? AND version=?").bind(id).bind(actor.tenant_id).bind(actor.user_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval is not claimed by the current user or changed on the server",
        ));
    }
    append_action(&mut tx, &actor, id, "release", None, "claimed", "pending").await?;
    tx.commit().await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/reassign",request_body=ReassignApprovalRequest)]
pub async fn reassign_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ReassignApprovalRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:manage")?;
    if !is_candidate(&state, &actor, id, input.target_user_id).await? {
        return Err(AppError::unprocessable(
            "INVALID_APPROVAL_ASSIGNEE",
            "Target user is not an eligible candidate",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE approval_tasks SET status='claimed',claimed_by=?,claimed_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE id=? AND tenant_id=? AND status IN ('pending','claimed') AND version=?").bind(input.target_user_id).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval changed on the server",
        ));
    }
    let source_event_id = append_action(
        &mut tx,
        &actor,
        id,
        "reassign",
        Some(json!({"targetUserId":input.target_user_id})),
        "claimed",
        "claimed",
    )
    .await?;
    create_notification(
        &mut tx,
        NotificationSpec {
            tenant: actor.tenant_id,
            source_event_id,
            user: input.target_user_id,
            kind: "approval_reassigned",
            title: "notifications.approvalReassigned.title",
            body: "notifications.approvalReassigned.body",
            args: json!({}),
            target_type: "approval",
            target_id: id,
            path: format!("/approvals/{id}"),
            tone: "warning",
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/approve",request_body=DecideApprovalRequest)]
pub async fn approve(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<DecideApprovalRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    decide(&state, &actor, id, input, "approved").await
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/reject",request_body=DecideApprovalRequest)]
pub async fn reject(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<DecideApprovalRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    decide(&state, &actor, id, input, "rejected").await
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/cancel",request_body=VersionActionRequest)]
pub async fn cancel_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<VersionActionRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:manage")?;
    terminal_admin_transition(&state, &actor, id, input.version, "cancelled", "cancel").await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/approvals/{id}/timeout",request_body=VersionActionRequest)]
pub async fn timeout_approval(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<VersionActionRequest>,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:manage")?;
    let due:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM approval_tasks WHERE id=? AND tenant_id=? AND deadline_at<=CURRENT_TIMESTAMP(6))").bind(id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    if !due {
        return Err(AppError::conflict(
            "APPROVAL_NOT_DUE",
            "Approval deadline has not elapsed",
        ));
    }
    terminal_admin_transition(&state, &actor, id, input.version, "timed_out", "timeout").await?;
    Ok(Json(load_approval(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get, path = "/api/v1/notifications")]
pub async fn list_notifications(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<NotificationInboxResponse>> {
    actor.require("notification:view")?;
    let rows=sqlx::query("SELECT n.id,n.notification_type,n.title_key,n.body_key,n.arguments_json,n.target_type,n.target_id,n.target_path,n.tone,(r.read_at IS NOT NULL) is_read,n.created_at FROM notification_receipts r JOIN notifications n ON n.id=r.notification_id AND n.tenant_id=r.tenant_id WHERE r.tenant_id=? AND r.user_id=? ORDER BY n.created_at DESC LIMIT 100").bind(actor.tenant_id).bind(actor.user_id).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(notification_from_row)
        .collect::<AppResult<Vec<_>>>()?;
    let unread_count = items.iter().filter(|item| !item.read).count() as u64;
    Ok(Json(NotificationInboxResponse {
        items,
        unread_count,
    }))
}

#[utoipa::path(post, path = "/api/v1/notifications/{id}/read")]
pub async fn read_notification(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("notification:view")?;
    let changed=sqlx::query("UPDATE notification_receipts SET read_at=COALESCE(read_at,CURRENT_TIMESTAMP(6)) WHERE tenant_id=? AND notification_id=? AND user_id=?").bind(actor.tenant_id).bind(id).bind(actor.user_id).execute(&state.pool).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::not_found("Notification"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/notifications/read-all")]
pub async fn read_all_notifications(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<StatusCode> {
    actor.require("notification:view")?;
    sqlx::query("UPDATE notification_receipts SET read_at=COALESCE(read_at,CURRENT_TIMESTAMP(6)) WHERE tenant_id=? AND user_id=?").bind(actor.tenant_id).bind(actor.user_id).execute(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/executions")]
pub async fn list_executions(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ExecutionPage>> {
    actor.require("execution:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let visible = workflow_visibility_sql(&actor);
    let sql = format!(
        "{EXECUTION_SELECT} WHERE e.tenant_id=? AND {visible} AND (?='' OR e.status=?) AND (?='%%' OR w.name LIKE ?) ORDER BY e.started_at DESC LIMIT ? OFFSET ?"
    );
    let mut rows_query = sqlx::query(&sql).bind(actor.tenant_id);
    if !actor.company_admin {
        rows_query = rows_query
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let rows = rows_query
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(page_size)
        .bind(u64::from((page - 1) * page_size))
        .fetch_all(&state.pool)
        .await?;
    let count_sql = format!(
        "SELECT COUNT(*) FROM workflow_executions e JOIN workflows w ON w.id=e.workflow_id WHERE e.tenant_id=? AND {visible} AND (?='' OR e.status=?) AND (?='%%' OR w.name LIKE ?)"
    );
    let mut count_query = sqlx::query_scalar(&count_sql).bind(actor.tenant_id);
    if !actor.company_admin {
        count_query = count_query
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let total: i64 = count_query
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(execution_from_row)
            .collect::<AppResult<_>>()?,
        page,
        page_size,
        total: total as u64,
    }))
}

#[utoipa::path(get, path = "/api/v1/executions/{id}")]
pub async fn get_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ExecutionResponse>> {
    actor.require("execution:view")?;
    let execution = load_execution(&state, actor.tenant_id, id).await?;
    require_workflow_access(&state.pool, &actor, execution.workflow_id, false).await?;
    Ok(Json(execution))
}

#[utoipa::path(get, path = "/api/v1/executions/{id}/runtime-details")]
pub async fn execution_runtime_details(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<RuntimeDetailsResponse>> {
    actor.require("trace:view")?;
    let execution = load_execution(&state, actor.tenant_id, id).await?;
    require_workflow_access(&state.pool, &actor, execution.workflow_id, false).await?;
    let runs=sqlx::query("SELECT id,node_execution_id,status,budget_json,iteration_count,model_call_count,tool_call_count,input_tokens,output_tokens,cost_micros,state_artifact_id,state_hash,stop_reason,started_at,ended_at FROM agent_runs WHERE tenant_id=? AND execution_id=? ORDER BY started_at,id").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?.into_iter().map(agent_run_detail).collect::<AppResult<Vec<_>>>()?;
    let iterations=sqlx::query("SELECT i.id,i.agent_run_id,i.iteration_index,i.status,i.state_before_hash,i.state_after_hash,i.state_artifact_id,i.stop_reason,i.started_at,i.ended_at FROM agent_iterations i JOIN agent_runs r ON r.id=i.agent_run_id WHERE i.tenant_id=? AND r.execution_id=? ORDER BY i.started_at,i.iteration_index").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?.into_iter().map(agent_iteration_detail).collect::<AppResult<Vec<_>>>()?;
    let calls=sqlx::query("SELECT id,attempt_id,agent_run_id,iteration_index,call_index,call_kind,request_fingerprint,resource_type,resource_id,resource_version_id,side_effect,status,input_tokens,output_tokens,cost_micros,usage_estimated,response_artifact_id,error_code,error_message,started_at,ended_at FROM runtime_calls WHERE tenant_id=? AND execution_id=? ORDER BY started_at,iteration_index,call_index").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?.into_iter().map(runtime_call_detail).collect::<AppResult<Vec<_>>>()?;
    let sandboxes=sqlx::query("SELECT id,node_execution_id,attempt_id,sandbox_id,profile_version_id,status,expires_at,heartbeat_at,termination_attempts,last_error,created_at,terminated_at FROM sandbox_leases WHERE tenant_id=? AND execution_id=? ORDER BY created_at,id").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?.into_iter().map(sandbox_lease_detail).collect::<AppResult<Vec<_>>>()?;
    Ok(Json(RuntimeDetailsResponse {
        execution_id: id,
        input_tokens: execution.input_tokens,
        output_tokens: execution.output_tokens,
        cost_micros: execution.cost_micros,
        agent_runs: runs,
        iterations,
        calls,
        sandboxes,
    }))
}

#[utoipa::path(get, path = "/api/v1/executions/{id}/trace")]
pub async fn execution_trace(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Query(query): Query<TraceQuery>,
) -> AppResult<Json<TraceResponse>> {
    actor.require("trace:view")?;
    let execution = load_execution(&state, actor.tenant_id, id).await?;
    require_workflow_access(&state.pool, &actor, execution.workflow_id, false).await?;
    let clickhouse = state.clickhouse.as_ref().ok_or_else(|| {
        AppError::service_unavailable("TRACE_UNAVAILABLE", "Trace storage is unavailable")
    })?;
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let cursor = query.cursor.unwrap_or_default();
    let sql = "SELECT toString(e.event_id) event_id,toString(e.trace_id) trace_id,toString(e.span_id) span_id,toString(e.parent_span_id) parent_span_id,toString(e.execution_id) execution_id,toString(e.node_execution_id) node_execution_id,toString(e.attempt_id) attempt_id,toString(e.agent_run_id) agent_run_id,toString(e.runtime_call_id) runtime_call_id,e.sandbox_id,e.resource_type,toString(e.resource_id) resource_id,toString(e.resource_version_id) resource_version_id,e.node_id,e.event_type,e.status,toString(e.event_time) event_time,e.duration_ms,e.run_index,e.iteration_index,e.model_name,e.provider_name,e.mcp_tool_name,e.input_tokens,e.output_tokens,e.cost_micros,e.error_code,e.error_message,e.stop_reason,toUInt8(e.partial) partial,e.attributes_json,toString(e.content_ref) content_ref FROM workflow_trace_events AS e FINAL WHERE e.tenant_id=toUUID(?) AND e.execution_id=toUUID(?) AND (?='' OR concat(toString(e.event_time),'|',toString(e.event_id))>?) ORDER BY e.event_time,e.event_id LIMIT ?";
    let rows = clickhouse
        .query(sql)
        .bind(actor.tenant_id.to_string())
        .bind(id.to_string())
        .bind(&cursor)
        .bind(&cursor)
        .bind(limit + 1)
        .fetch_all::<TraceRow>()
        .await
        .map_err(|e| AppError::service_unavailable("TRACE_UNAVAILABLE", e.to_string()))?;
    let has_more = rows.len() > limit as usize;
    let events = rows
        .into_iter()
        .take(limit as usize)
        .map(TraceEventResponse::from)
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        events
            .last()
            .map(|e| format!("{}|{}", e.event_time, e.event_id))
    } else {
        None
    };
    Ok(Json(TraceResponse {
        execution_id: id,
        trace_id: execution.trace_id,
        events,
        next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/executions/{id}/artifacts/{artifact_id}")]
pub async fn execution_artifact(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, artifact_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    actor.require("trace:view")?;
    let execution = load_execution(&state, actor.tenant_id, id).await?;
    require_workflow_access(&state.pool, &actor, execution.workflow_id, false).await?;
    let clickhouse = state.clickhouse.as_ref().ok_or_else(|| {
        AppError::service_unavailable("TRACE_UNAVAILABLE", "Trace storage is unavailable")
    })?;
    let linked = clickhouse
        .query("SELECT count() count FROM workflow_trace_events FINAL WHERE tenant_id=toUUID(?) AND execution_id=toUUID(?) AND content_ref=toUUID(?)")
        .bind(actor.tenant_id.to_string())
        .bind(id.to_string())
        .bind(artifact_id.to_string())
        .fetch_one::<CountRow>()
        .await
        .map_err(|error| AppError::service_unavailable("TRACE_UNAVAILABLE", error.to_string()))?
        .count
        > 0;
    if !linked {
        return Err(AppError::not_found("Execution Artifact"));
    }
    let objects = state.object_store.clone().ok_or_else(|| {
        AppError::service_unavailable(
            "ARTIFACT_STORE_UNAVAILABLE",
            "Artifact storage is unavailable",
        )
    })?;
    let artifact = MySqlObjectArtifactStore::new(state.pool.clone(), objects)
        .get(
            TenantId::from_uuid(actor.tenant_id),
            ArtifactId::from_uuid(artifact_id),
        )
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::not_found("Execution Artifact"))?;
    let mut response = Response::new(Body::from(artifact.content));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&artifact.content_type).map_err(AppError::internal)?,
    );
    response.headers_mut().insert(
        "x-content-sha256",
        HeaderValue::from_str(&artifact.sha256).map_err(AppError::internal)?,
    );
    Ok(response)
}

#[utoipa::path(get, path = "/api/v1/runtime/status")]
pub async fn runtime_status(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<RuntimeStatusResponse>> {
    actor.require("runtime:view")?;
    let counts = execution_counts(&state, actor.tenant_id).await?;
    let rows=sqlx::query("SELECT service_type,COUNT(*) instances,MAX(heartbeat_at) last_heartbeat,MAX(CASE WHEN status='ready' THEN 1 ELSE 0 END) ready FROM runtime_service_heartbeats WHERE tenant_id=? AND heartbeat_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 30 SECOND) GROUP BY service_type").bind(actor.tenant_id).fetch_all(&state.pool).await?;
    let mut components = vec!["coordinator", "worker", "sandbox", "trace_writer"]
        .into_iter()
        .map(|name| RuntimeComponentStatus {
            component: name.into(),
            status: "unknown".into(),
            instances: None,
            queue_depth: None,
            last_heartbeat: None,
        })
        .collect::<Vec<_>>();
    for row in rows {
        let name: String = row.try_get("service_type")?;
        if let Some(component) = components.iter_mut().find(|v| v.component == name) {
            let instances: i64 = row.try_get("instances")?;
            let ready: i64 = row.try_get("ready")?;
            let heartbeat: OffsetDateTime = row.try_get("last_heartbeat")?;
            component.instances = Some(instances as u64);
            component.status = if ready > 0 { "ready" } else { "degraded" }.into();
            component.last_heartbeat = Some(heartbeat.to_string());
        }
    }
    let mut queue = RuntimeComponentStatus {
        component: "trace_queue".into(),
        status: "unknown".into(),
        instances: None,
        queue_depth: None,
        last_heartbeat: None,
    };
    if let Some(settings) = state.redis.as_ref() {
        if let Ok(mut connection) = agentx_infrastructure::clients::connect_redis(settings).await {
            let depth: std::result::Result<u64, _> = connection.xlen("agentx:trace:v1").await;
            if let Ok(depth) = depth {
                queue.status = "ready".into();
                queue.queue_depth = Some(depth);
            }
        }
    }
    components.push(queue);
    let active_sandboxes: i64=sqlx::query_scalar("SELECT COUNT(*) FROM sandbox_leases WHERE tenant_id=? AND status IN ('creating','ready','running','interrupting','terminating','orphaned')").bind(actor.tenant_id).fetch_one(&state.pool).await?;
    let sandbox_compatibility:Option<Value>=sqlx::query_scalar("SELECT detail_json FROM runtime_service_heartbeats WHERE tenant_id=? AND service_type='sandbox' ORDER BY heartbeat_at DESC LIMIT 1").bind(actor.tenant_id).fetch_optional(&state.pool).await?;
    Ok(Json(RuntimeStatusResponse {
        components,
        running: counts.0,
        waiting: counts.1,
        failed_today: counts.2,
        active_sandboxes: active_sandboxes as u64,
        sandbox_compatibility,
    }))
}

#[utoipa::path(get, path = "/api/v1/dashboard/summary")]
pub async fn dashboard_summary(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<DashboardSummaryResponse>> {
    let visible = workflow_visibility_sql(&actor);
    let workflow_sql = format!(
        "SELECT COUNT(*) FROM workflows w WHERE w.tenant_id=? AND {visible} AND w.status='active'"
    );
    let mut workflow_query = sqlx::query_scalar(&workflow_sql).bind(actor.tenant_id);
    if !actor.company_admin {
        workflow_query = workflow_query
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let workflow_count: i64 = workflow_query.fetch_one(&state.pool).await?;
    let execution_sql = format!(
        "SELECT SUM(e.status='running') running,SUM(e.started_at>=CURRENT_DATE) today,SUM(e.status='succeeded' AND e.started_at>=CURRENT_DATE) succeeded,SUM(e.status='failed' AND e.started_at>=CURRENT_DATE) failed,COALESCE(SUM(IF(e.started_at>=CURRENT_DATE,e.cost_micros,0)),0) cost FROM workflow_executions e JOIN workflows w ON w.id=e.workflow_id WHERE e.tenant_id=? AND {visible}"
    );
    let mut execution_query = sqlx::query(&execution_sql).bind(actor.tenant_id);
    if !actor.company_admin {
        execution_query = execution_query
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let row = execution_query.fetch_one(&state.pool).await?;
    let approval_visible = approval_visibility_sql(&actor);
    let pending_sql = format!(
        "SELECT COUNT(*) FROM approval_tasks t JOIN workflows w ON w.id=t.workflow_id WHERE t.tenant_id=? AND {approval_visible} AND t.status IN ('pending','claimed')"
    );
    let mut pending_query = sqlx::query_scalar(&pending_sql).bind(actor.tenant_id);
    if !actor.company_admin {
        pending_query = pending_query
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id);
    }
    let pending: i64 = pending_query.fetch_one(&state.pool).await?;
    Ok(Json(DashboardSummaryResponse {
        workflow_count: workflow_count as u64,
        running_executions: optional_count(&row, "running"),
        executions_today: optional_count(&row, "today"),
        succeeded_today: optional_count(&row, "succeeded"),
        failed_today: optional_count(&row, "failed"),
        cost_micros_today: row.try_get::<u64, _>("cost").unwrap_or(0),
        pending_approvals: pending as u64,
    }))
}

const APPROVAL_SELECT: &str = "SELECT t.id,t.execution_id,t.workflow_id,w.name workflow_name,t.node_id,t.title,t.description,t.request_payload_json,t.status,t.claimed_by,u.display_name claimed_by_name,t.resume_status,t.deadline_at,t.version,t.created_at FROM approval_tasks t JOIN workflows w ON w.id=t.workflow_id LEFT JOIN users u ON u.id=t.claimed_by";
const EXECUTION_SELECT: &str = "SELECT e.id,e.workflow_id,w.name workflow_name,e.workflow_version_id,wv.version_number workflow_version_number,e.invocation_id,e.session_id,e.trace_id,e.trigger_type,e.execution_type,e.parent_execution_id,e.caller_execution_id,e.fork_checkpoint_id,e.status,e.started_at,e.ended_at,e.duration_ms,e.cost_micros,e.input_tokens,e.output_tokens,e.error_code,e.error_message FROM workflow_executions e JOIN workflows w ON w.id=e.workflow_id JOIN workflow_versions wv ON wv.id=e.workflow_version_id";
const WORKFLOW_VISIBILITY: &str = "(w.owner_user_id=? OR w.visibility='company' OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=?) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id)))";

fn workflow_visibility_sql(actor: &AuthActor) -> &'static str {
    if actor.company_admin {
        "TRUE"
    } else {
        WORKFLOW_VISIBILITY
    }
}

fn approval_visibility_sql(actor: &AuthActor) -> &'static str {
    if actor.company_admin {
        "TRUE"
    } else {
        "((t.claimed_by=? OR EXISTS(SELECT 1 FROM approval_candidates c WHERE c.approval_task_id=t.id AND c.tenant_id=t.tenant_id AND ((c.candidate_type='user' AND c.candidate_id=?) OR (c.candidate_type='role' AND EXISTS(SELECT 1 FROM user_roles ur WHERE ur.tenant_id=t.tenant_id AND ur.user_id=? AND ur.role_id=c.candidate_id)) OR (c.candidate_type='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=t.tenant_id AND dc.ancestor_id=c.candidate_id AND dc.descendant_id=?))))) AND (w.owner_user_id=? OR w.visibility='company' OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=?) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id))))"
    }
}
async fn require_approval_visible(state: &AppState, actor: &AuthActor, id: Uuid) -> AppResult<()> {
    if actor.company_admin {
        return Ok(());
    }
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM approval_tasks t JOIN workflows w ON w.id=t.workflow_id WHERE t.id=? AND t.tenant_id=? AND {})",
        approval_visibility_sql(actor)
    );
    let allowed: bool = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(actor.tenant_id)
        .bind(actor.user_id)
        .bind(actor.user_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(actor.user_id)
        .bind(actor.user_id)
        .bind(actor.user_id)
        .fetch_one(&state.pool)
        .await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::not_found("Approval"))
    }
}
async fn is_candidate(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    user: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN user_departments ud ON ud.tenant_id=u.tenant_id AND ud.user_id=u.id JOIN approval_tasks t ON t.tenant_id=u.tenant_id AND t.id=? JOIN workflows w ON w.tenant_id=t.tenant_id AND w.id=t.workflow_id WHERE u.tenant_id=? AND u.id=? AND u.status='active' AND EXISTS(SELECT 1 FROM approval_candidates c WHERE c.tenant_id=t.tenant_id AND c.approval_task_id=t.id AND ((c.candidate_type='user' AND c.candidate_id=u.id) OR (c.candidate_type='role' AND EXISTS(SELECT 1 FROM user_roles ur WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND ur.role_id=c.candidate_id)) OR (c.candidate_type='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=u.tenant_id AND dc.ancestor_id=c.candidate_id AND dc.descendant_id=ud.department_id)))) AND (EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND r.code='company_admin') OR w.visibility='company' OR w.owner_user_id=u.id OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=u.id) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=u.tenant_id AND ur.user_id=u.id AND dc.descendant_id=w.owner_department_id))))")
        .bind(id)
        .bind(actor.tenant_id)
        .bind(user)
        .fetch_one(&state.pool)
        .await?)
}
async fn transition_claim(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    version: u64,
) -> AppResult<()> {
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE approval_tasks SET status='claimed',claimed_by=?,claimed_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE id=? AND tenant_id=? AND status='pending' AND version=?").bind(actor.user_id).bind(id).bind(actor.tenant_id).bind(version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval was already claimed or changed on the server",
        ));
    }
    append_action(&mut tx, actor, id, "claim", None, "pending", "claimed").await?;
    tx.commit().await?;
    Ok(())
}
async fn decide(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    input: DecideApprovalRequest,
    target: &str,
) -> AppResult<Json<ApprovalResponse>> {
    actor.require("approval:act")?;
    let action = if target == "approved" {
        "approve"
    } else {
        "reject"
    };
    let mut tx = state.pool.begin().await?;
    let runtime_row=sqlx::query("SELECT t.execution_id,t.node_execution_id,w.id wait_id FROM approval_tasks t LEFT JOIN wait_subscriptions w ON w.resume_token_id=t.resume_token_id AND w.tenant_id=t.tenant_id WHERE t.id=? AND t.tenant_id=? FOR UPDATE")
        .bind(id).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    let changed=sqlx::query("UPDATE approval_tasks SET status=?,resume_status=IF(node_execution_id IS NULL,'blocked_runtime','pending'),version=version+1 WHERE id=? AND tenant_id=? AND status='claimed' AND claimed_by=? AND version=?").bind(target).bind(id).bind(actor.tenant_id).bind(actor.user_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval must be claimed by the current user before deciding",
        ));
    }
    append_action(
        &mut tx,
        actor,
        id,
        action,
        input.input.clone(),
        "claimed",
        target,
    )
    .await?;
    audit(
        &mut tx,
        actor,
        &format!("approval.{action}"),
        "approval",
        id,
        json!({"resumeStatus":if runtime_row.try_get::<Option<Uuid>,_>("node_execution_id")?.is_some(){"pending"}else{"blocked_runtime"}}),
    )
    .await?;
    outbox(
        &mut tx,
        actor,
        "ApprovalResolved",
        "approval",
        id,
        json!({"decision":target,"resumeStatus":"pending"}),
    )
    .await?;
    tx.commit().await?;
    if let (Some(node_execution_id), Some(wait_id)) = (
        runtime_row.try_get::<Option<Uuid>, _>("node_execution_id")?,
        runtime_row.try_get::<Option<Uuid>, _>("wait_id")?,
    ) {
        let runtime = state.runtime.as_deref().ok_or_else(|| {
            AppError::service_unavailable("RUNTIME_UNAVAILABLE", "Workflow runtime is unavailable")
        })?;
        let result = runtime
            .resume_execution(ResumeExecutionCommand {
                tenant_id: TenantId::from_uuid(actor.tenant_id),
                execution_id: ExecutionId::from_uuid(runtime_row.try_get("execution_id")?),
                node_execution_id: NodeExecutionId::from_uuid(node_execution_id),
                resume_token: wait_id.to_string(),
                output_port: if target == "approved" {
                    "approved".into()
                } else {
                    "rejected".into()
                },
                payload: json!({"decision":target,"input":input.input,"actorUserId":actor.user_id}),
                idempotency_key: format!("approval:{id}:{}:{target}", input.version),
                actor_user_id: Some(UserId::from_uuid(actor.user_id)),
            })
            .await;
        let (resume_status, error) = match result {
            Ok(_) => ("succeeded", None),
            Err(error) => ("failed", Some(error.to_string())),
        };
        sqlx::query("UPDATE approval_tasks SET resume_status=? WHERE id=? AND tenant_id=?")
            .bind(resume_status)
            .bind(id)
            .bind(actor.tenant_id)
            .execute(&state.pool)
            .await?;
        if let Some(error) = error {
            return Err(AppError::service_unavailable(
                "RUNTIME_RESUME_FAILED",
                error,
            ));
        }
    }
    Ok(Json(load_approval(state, actor.tenant_id, id).await?))
}
async fn terminal_admin_transition(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    version: u64,
    target: &str,
    action: &str,
) -> AppResult<()> {
    let mut tx = state.pool.begin().await?;
    let current: Option<String> = sqlx::query_scalar("SELECT status FROM approval_tasks WHERE id=? AND tenant_id=? AND version=? AND status IN ('pending','claimed') FOR UPDATE")
        .bind(id).bind(actor.tenant_id).bind(version).fetch_optional(&mut *tx).await?;
    let Some(current) = current else {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval is terminal or changed on the server",
        ));
    };
    let changed=sqlx::query("UPDATE approval_tasks SET status=?,resume_status='blocked_runtime',version=version+1 WHERE id=? AND tenant_id=? AND status=? AND version=?").bind(target).bind(id).bind(actor.tenant_id).bind(&current).bind(version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPROVAL_STATE_CONFLICT",
            "Approval is terminal or changed on the server",
        ));
    }
    append_action(&mut tx, actor, id, action, None, &current, target).await?;
    tx.commit().await?;
    Ok(())
}
async fn append_action(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    id: Uuid,
    action: &str,
    input: Option<Value>,
    from: &str,
    to: &str,
) -> AppResult<Uuid> {
    let action_id = Uuid::now_v7();
    sqlx::query("INSERT INTO approval_actions(id,tenant_id,approval_task_id,actor_user_id,action_type,input_json,from_status,to_status) VALUES(?,?,?,?,?,?,?,?)").bind(action_id).bind(actor.tenant_id).bind(id).bind(actor.user_id).bind(action).bind(input).bind(from).bind(to).execute(&mut **tx).await?;
    audit(
        tx,
        actor,
        &format!("approval.{action}"),
        "approval",
        id,
        json!({"fromStatus":from,"toStatus":to,"actionId":action_id}),
    )
    .await?;
    Ok(action_id)
}
struct NotificationSpec<'a> {
    tenant: Uuid,
    source_event_id: Uuid,
    user: Uuid,
    kind: &'a str,
    title: &'a str,
    body: &'a str,
    args: Value,
    target_type: &'a str,
    target_id: Uuid,
    path: String,
    tone: &'a str,
}

async fn create_notification(
    tx: &mut Transaction<'_, MySql>,
    notification: NotificationSpec<'_>,
) -> AppResult<()> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO notifications(id,tenant_id,source_event_id,notification_type,title_key,body_key,arguments_json,target_type,target_id,target_path,tone) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id").bind(id).bind(notification.tenant).bind(notification.source_event_id).bind(notification.kind).bind(notification.title).bind(notification.body).bind(notification.args).bind(notification.target_type).bind(notification.target_id).bind(notification.path).bind(notification.tone).execute(&mut **tx).await?;
    let notification_id: Uuid = sqlx::query_scalar("SELECT id FROM notifications WHERE tenant_id=? AND source_event_id=? AND notification_type=?")
        .bind(notification.tenant).bind(notification.source_event_id).bind(notification.kind).fetch_one(&mut **tx).await?;
    sqlx::query(
        "INSERT IGNORE INTO notification_receipts(tenant_id,notification_id,user_id) VALUES(?,?,?)",
    )
    .bind(notification.tenant)
    .bind(notification_id)
    .bind(notification.user)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
fn approval_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<ApprovalResponse> {
    Ok(ApprovalResponse {
        id: r.try_get("id")?,
        execution_id: r.try_get("execution_id")?,
        workflow_id: r.try_get("workflow_id")?,
        workflow_name: r.try_get("workflow_name")?,
        node_id: r.try_get("node_id")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        request_payload: r.try_get("request_payload_json")?,
        status: r.try_get("status")?,
        claimed_by: r.try_get("claimed_by")?,
        claimed_by_name: r.try_get("claimed_by_name")?,
        resume_status: r.try_get("resume_status")?,
        deadline_at: r.try_get("deadline_at")?,
        version: r.try_get("version")?,
        created_at: r.try_get("created_at")?,
    })
}
async fn load_approval(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ApprovalResponse> {
    let sql = format!("{APPROVAL_SELECT} WHERE t.tenant_id=? AND t.id=?");
    let r = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Approval"))?;
    approval_from_row(r)
}
fn approval_action_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<ApprovalActionResponse> {
    Ok(ApprovalActionResponse {
        id: r.try_get("id")?,
        action_type: r.try_get("action_type")?,
        actor_user_id: r.try_get("actor_user_id")?,
        actor_name: r.try_get("actor_name")?,
        from_status: r.try_get("from_status")?,
        to_status: r.try_get("to_status")?,
        input: r.try_get("input_json")?,
        created_at: r.try_get("created_at")?,
    })
}
fn notification_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<NotificationResponse> {
    Ok(NotificationResponse {
        id: r.try_get("id")?,
        notification_type: r.try_get("notification_type")?,
        title_key: r.try_get("title_key")?,
        body_key: r.try_get("body_key")?,
        arguments: r.try_get("arguments_json")?,
        target_type: r.try_get("target_type")?,
        target_id: r.try_get("target_id")?,
        target_path: r.try_get("target_path")?,
        tone: r.try_get("tone")?,
        read: r.try_get::<i8, _>("is_read")? != 0,
        created_at: r.try_get("created_at")?,
    })
}
fn execution_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<ExecutionResponse> {
    Ok(ExecutionResponse {
        id: r.try_get("id")?,
        workflow_id: r.try_get("workflow_id")?,
        workflow_name: r.try_get("workflow_name")?,
        workflow_version_id: r.try_get("workflow_version_id")?,
        workflow_version_number: r.try_get("workflow_version_number")?,
        invocation_id: r.try_get("invocation_id")?,
        session_id: r.try_get("session_id")?,
        trace_id: r.try_get("trace_id")?,
        trigger_type: r.try_get("trigger_type")?,
        execution_type: r.try_get("execution_type")?,
        parent_execution_id: r.try_get("parent_execution_id")?,
        caller_execution_id: r.try_get("caller_execution_id")?,
        fork_checkpoint_id: r.try_get("fork_checkpoint_id")?,
        status: r.try_get("status")?,
        started_at: r.try_get("started_at")?,
        ended_at: r.try_get("ended_at")?,
        duration_ms: r.try_get("duration_ms")?,
        cost_micros: r.try_get("cost_micros")?,
        input_tokens: r.try_get("input_tokens")?,
        output_tokens: r.try_get("output_tokens")?,
        error_code: r.try_get("error_code")?,
        error_message: r.try_get("error_message")?,
    })
}
fn agent_run_detail(r: sqlx::mysql::MySqlRow) -> AppResult<AgentRunDetail> {
    Ok(AgentRunDetail {
        id: r.try_get("id")?,
        node_execution_id: r.try_get("node_execution_id")?,
        status: r.try_get("status")?,
        budget: r.try_get("budget_json")?,
        iteration_count: r.try_get("iteration_count")?,
        model_call_count: r.try_get("model_call_count")?,
        tool_call_count: r.try_get("tool_call_count")?,
        input_tokens: r.try_get("input_tokens")?,
        output_tokens: r.try_get("output_tokens")?,
        cost_micros: r.try_get("cost_micros")?,
        state_artifact_id: r.try_get("state_artifact_id")?,
        state_hash: r.try_get("state_hash")?,
        stop_reason: r.try_get("stop_reason")?,
        started_at: format_timestamp(r.try_get("started_at")?)?,
        ended_at: r
            .try_get::<Option<OffsetDateTime>, _>("ended_at")?
            .map(format_timestamp)
            .transpose()?,
    })
}
fn agent_iteration_detail(r: sqlx::mysql::MySqlRow) -> AppResult<AgentIterationDetail> {
    Ok(AgentIterationDetail {
        id: r.try_get("id")?,
        agent_run_id: r.try_get("agent_run_id")?,
        iteration_index: r.try_get("iteration_index")?,
        status: r.try_get("status")?,
        state_before_hash: r.try_get("state_before_hash")?,
        state_after_hash: r.try_get("state_after_hash")?,
        state_artifact_id: r.try_get("state_artifact_id")?,
        stop_reason: r.try_get("stop_reason")?,
        started_at: format_timestamp(r.try_get("started_at")?)?,
        ended_at: r
            .try_get::<Option<OffsetDateTime>, _>("ended_at")?
            .map(format_timestamp)
            .transpose()?,
    })
}
fn runtime_call_detail(r: sqlx::mysql::MySqlRow) -> AppResult<RuntimeCallDetail> {
    Ok(RuntimeCallDetail {
        id: r.try_get("id")?,
        attempt_id: r.try_get("attempt_id")?,
        agent_run_id: r.try_get("agent_run_id")?,
        iteration_index: r.try_get("iteration_index")?,
        call_index: r.try_get("call_index")?,
        call_kind: r.try_get("call_kind")?,
        request_fingerprint: r.try_get("request_fingerprint")?,
        resource_type: r.try_get("resource_type")?,
        resource_id: r.try_get("resource_id")?,
        resource_version_id: r.try_get("resource_version_id")?,
        side_effect: r.try_get("side_effect")?,
        status: r.try_get("status")?,
        input_tokens: r.try_get("input_tokens")?,
        output_tokens: r.try_get("output_tokens")?,
        cost_micros: r.try_get("cost_micros")?,
        usage_estimated: r.try_get::<i8, _>("usage_estimated")? != 0,
        response_artifact_id: r.try_get("response_artifact_id")?,
        error_code: r.try_get("error_code")?,
        error_message: r.try_get("error_message")?,
        started_at: format_timestamp(r.try_get("started_at")?)?,
        ended_at: r
            .try_get::<Option<OffsetDateTime>, _>("ended_at")?
            .map(format_timestamp)
            .transpose()?,
    })
}
fn sandbox_lease_detail(r: sqlx::mysql::MySqlRow) -> AppResult<SandboxLeaseDetail> {
    Ok(SandboxLeaseDetail {
        id: r.try_get("id")?,
        node_execution_id: r.try_get("node_execution_id")?,
        attempt_id: r.try_get("attempt_id")?,
        sandbox_id: r.try_get("sandbox_id")?,
        profile_version_id: r.try_get("profile_version_id")?,
        status: r.try_get("status")?,
        expires_at: format_timestamp(r.try_get("expires_at")?)?,
        heartbeat_at: format_timestamp(r.try_get("heartbeat_at")?)?,
        termination_attempts: r.try_get("termination_attempts")?,
        last_error: r.try_get("last_error")?,
        created_at: format_timestamp(r.try_get("created_at")?)?,
        terminated_at: r
            .try_get::<Option<OffsetDateTime>, _>("terminated_at")?
            .map(format_timestamp)
            .transpose()?,
    })
}

fn format_timestamp(value: OffsetDateTime) -> AppResult<String> {
    value.format(&Rfc3339).map_err(AppError::internal)
}
async fn load_execution(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ExecutionResponse> {
    let sql = "SELECT e.id,e.workflow_id,w.name workflow_name,e.workflow_version_id,wv.version_number workflow_version_number,e.invocation_id,e.session_id,e.trace_id,e.trigger_type,e.execution_type,e.parent_execution_id,e.caller_execution_id,e.fork_checkpoint_id,e.status,e.started_at,e.ended_at,e.duration_ms,e.cost_micros,e.input_tokens,e.output_tokens,e.error_code,e.error_message FROM workflow_executions e JOIN workflows w ON w.id=e.workflow_id JOIN workflow_versions wv ON wv.id=e.workflow_version_id WHERE e.tenant_id=? AND e.id=?";
    let r = sqlx::query(sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Execution"))?;
    execution_from_row(r)
}
async fn execution_counts(state: &AppState, tenant: Uuid) -> AppResult<(u64, u64, u64)> {
    let r=sqlx::query("SELECT SUM(status='running') running,SUM(status IN ('waiting','waiting_approval')) waiting,SUM(status='failed' AND started_at>=CURRENT_DATE) failed FROM workflow_executions WHERE tenant_id=?").bind(tenant).fetch_one(&state.pool).await?;
    Ok((
        optional_count(&r, "running"),
        optional_count(&r, "waiting"),
        optional_count(&r, "failed"),
    ))
}
fn optional_count(r: &sqlx::mysql::MySqlRow, name: &str) -> u64 {
    r.try_get::<Option<i64>, _>(name)
        .ok()
        .flatten()
        .unwrap_or(0) as u64
}

#[derive(clickhouse::Row, Deserialize)]
struct CountRow {
    count: u64,
}

#[derive(clickhouse::Row, Deserialize)]
struct TraceRow {
    event_id: String,
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    execution_id: String,
    node_execution_id: Option<String>,
    attempt_id: Option<String>,
    agent_run_id: Option<String>,
    runtime_call_id: Option<String>,
    sandbox_id: Option<String>,
    resource_type: Option<String>,
    resource_id: Option<String>,
    resource_version_id: Option<String>,
    node_id: Option<String>,
    event_type: String,
    status: String,
    event_time: String,
    duration_ms: Option<u64>,
    run_index: u32,
    iteration_index: u32,
    model_name: Option<String>,
    provider_name: Option<String>,
    mcp_tool_name: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cost_micros: u64,
    error_code: Option<String>,
    error_message: Option<String>,
    stop_reason: Option<String>,
    partial: u8,
    attributes_json: String,
    content_ref: Option<String>,
}
impl From<TraceRow> for TraceEventResponse {
    fn from(r: TraceRow) -> Self {
        Self {
            event_id: r.event_id,
            trace_id: r.trace_id,
            span_id: r.span_id,
            parent_span_id: r.parent_span_id,
            execution_id: r.execution_id,
            node_execution_id: r.node_execution_id,
            attempt_id: r.attempt_id,
            agent_run_id: r.agent_run_id,
            runtime_call_id: r.runtime_call_id,
            sandbox_id: r.sandbox_id,
            resource_type: r.resource_type,
            resource_id: r.resource_id,
            resource_version_id: r.resource_version_id,
            node_id: r.node_id,
            event_type: r.event_type,
            status: r.status,
            event_time: r.event_time,
            duration_ms: r.duration_ms,
            run_index: r.run_index,
            iteration_index: r.iteration_index,
            model_name: r.model_name,
            provider_name: r.provider_name,
            mcp_tool_name: r.mcp_tool_name,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            cost_micros: r.cost_micros,
            error_code: r.error_code,
            error_message: r.error_message,
            stop_reason: r.stop_reason,
            partial: r.partial != 0,
            attributes: trace_attributes(&r.attributes_json),
            content_ref: r.content_ref,
        }
    }
}

fn trace_attributes(value: &str) -> Value {
    redact_trace_attributes(serde_json::from_str(value).unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::{approval_visibility_sql, format_timestamp, trace_attributes};
    use crate::security::AuthActor;
    use serde_json::json;
    use uuid::Uuid;
    #[test]
    fn company_admin_visibility_does_not_require_candidate_bindings() {
        let actor = AuthActor {
            tenant_id: Uuid::nil(),
            user_id: Uuid::nil(),
            username: String::new(),
            display_name: String::new(),
            department_id: Uuid::nil(),
            permissions: vec![],
            roles: vec![],
            company_admin: true,
        };
        assert_eq!(approval_visibility_sql(&actor), "TRUE");
    }

    #[test]
    fn trace_query_redacts_secrets_even_for_legacy_rows() {
        let value = trace_attributes(
            &json!({
                "authorization":"Bearer legacy-secret",
                "nested":{"apiToken":"legacy-token","safe":true}
            })
            .to_string(),
        );
        assert_eq!(value["authorization"], "[REDACTED]");
        assert_eq!(value["nested"]["apiToken"], "[REDACTED]");
        assert_eq!(value["nested"]["safe"], true);
    }

    #[test]
    fn runtime_timestamps_use_rfc3339() {
        let value = time::OffsetDateTime::from_unix_timestamp(1_775_260_800).unwrap();
        assert_eq!(format_timestamp(value).unwrap(), "2026-04-04T00:00:00Z");
    }
}
