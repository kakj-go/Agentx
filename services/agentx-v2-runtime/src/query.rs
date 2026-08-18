use agentx_runtime_contracts::{
    SessionDetailV1, SessionSearchPageV1, SessionSearchRequestV1, SessionSummaryV1,
    SessionUpgradeCommandV1, SessionUpgradeReceiptV1, SessionVersionPolicyV1,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    RuntimeState,
    error::{RuntimeError, RuntimeResult},
};

pub use crate::query_authority::{
    get_execution, get_execution_artifact, get_execution_checkpoints, get_execution_events,
    get_execution_node, get_execution_nodes, get_execution_runtime_details, get_execution_waits,
    get_invocation, search_executions, search_invocations,
};

pub async fn search_sessions(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<SessionSearchRequestV1>,
) -> RuntimeResult<Json<SessionSearchPageV1>> {
    let claims = state
        .trust
        .delegation(&headers, request.tenant_id, "runtime.query.sessions")?;
    let application_id = request.application_id.ok_or(RuntimeError::Unauthorized)?;
    if !claims.application_ids.contains(&application_id) {
        return Err(RuntimeError::Unauthorized);
    }
    let limit = request.limit.clamp(1, 100);
    let statuses =
        serde_json::to_string(&request.statuses).map_err(|e| RuntimeError::Internal(e.into()))?;
    let rows = sqlx::query("SELECT id,application_id,application_deployment_id,workflow_version_id,bundle_id,version_policy,external_user_id,title,status,version,updated_at FROM application_sessions WHERE tenant_id=? AND application_id=? AND (JSON_LENGTH(?)=0 OR JSON_CONTAINS(?,JSON_QUOTE(status))) AND (? IS NULL OR CONCAT(DATE_FORMAT(updated_at,'%Y-%m-%dT%H:%i:%s.%fZ'),'|',LOWER(HEX(id)))>?) ORDER BY updated_at,id LIMIT ?")
        .bind(request.tenant_id).bind(application_id).bind(&statuses).bind(&statuses).bind(&request.after).bind(&request.after).bind(limit+1).fetch_all(&state.pool).await?;
    let mut items = rows
        .into_iter()
        .map(session_summary)
        .collect::<RuntimeResult<Vec<_>>>()?;
    let next = if items.len() > limit as usize {
        items.truncate(limit as usize);
        items.last().map(|item| {
            format!(
                "{}|{}",
                item.updated_at
                    .format(&time::format_description::well_known::Iso8601::DEFAULT)
                    .unwrap_or_default(),
                item.session_id.simple()
            )
        })
    } else {
        None
    };
    Ok(Json(SessionSearchPageV1 {
        api_version: 1,
        items,
        next,
    }))
}

pub async fn get_session(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> RuntimeResult<Json<SessionDetailV1>> {
    let row = sqlx::query("SELECT tenant_id,id,application_id,application_deployment_id,workflow_version_id,bundle_id,version_policy,status,version,updated_at,external_user_id,title FROM application_sessions WHERE id=?")
        .bind(id).fetch_optional(&state.pool).await?.ok_or(RuntimeError::NotFound)?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let claims = state
        .trust
        .delegation(&headers, tenant_id, "runtime.query.sessions")?;
    let application_id: Uuid = row.try_get("application_id")?;
    if !claims.session_ids.contains(&id) && !claims.application_ids.contains(&application_id) {
        return Err(RuntimeError::Unauthorized);
    }
    let summary = session_summary(&row)?;
    Ok(Json(SessionDetailV1 {
        api_version: 1,
        summary,
        external_user_id: row.try_get("external_user_id")?,
        title: row.try_get("title")?,
    }))
}

pub async fn apply_session_command(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<SessionUpgradeCommandV1>,
) -> RuntimeResult<Json<SessionUpgradeReceiptV1>> {
    let claims = state
        .trust
        .delegation(&headers, request.tenant_id, "runtime.sessions.upgrade")?;
    if !claims.session_ids.contains(&request.session_id)
        && !claims.application_ids.contains(&request.application_id)
    {
        return Err(RuntimeError::Unauthorized);
    }
    if let Some(row) = sqlx::query("SELECT to_bundle_id,to_session_version FROM application_session_version_history WHERE tenant_id=? AND session_id=? AND command_idempotency_key=?")
        .bind(request.tenant_id).bind(request.session_id).bind(&request.idempotency_key).fetch_optional(&state.pool).await? {
        if row.try_get::<Option<Uuid>,_>("to_bundle_id")? != Some(request.target_bundle_id) { return Err(RuntimeError::Conflict(agentx_runtime_contracts::RuntimePublishErrorCodeV1::IdempotencyConflict, "Session command key conflicts".into())); }
        return Ok(Json(SessionUpgradeReceiptV1 { api_version:1, session_id:request.session_id, bundle_id:request.target_bundle_id, session_version:row.try_get("to_session_version")?, replayed:true }));
    }
    let mut tx = state.pool.begin().await?;
    let session = sqlx::query("SELECT bundle_id,workflow_version_id,version,version_policy FROM application_sessions WHERE tenant_id=? AND application_id=? AND id=? AND status='active' FOR UPDATE")
        .bind(request.tenant_id).bind(request.application_id).bind(request.session_id).fetch_optional(&mut *tx).await?.ok_or(RuntimeError::NotFound)?;
    if session.try_get::<String, _>("version_policy")? != "manual_upgrade"
        || session.try_get::<u64, _>("version")? != request.expected_session_version
    {
        return Err(RuntimeError::Conflict(
            agentx_runtime_contracts::RuntimePublishErrorCodeV1::HeadVersionConflict,
            "Session Version does not match".into(),
        ));
    }
    let target = sqlx::query("SELECT workflow_version_id FROM deployment_bundles WHERE tenant_id=? AND application_id=? AND id=? AND status IN ('active','superseded','retained')")
        .bind(request.tenant_id).bind(request.application_id).bind(request.target_bundle_id).fetch_optional(&mut *tx).await?.ok_or(RuntimeError::NotFound)?;
    let old_bundle: Option<Uuid> = session.try_get("bundle_id")?;
    let new_version = request.expected_session_version + 1;
    sqlx::query("UPDATE application_sessions SET bundle_id=?,workflow_version_id=?,version=? WHERE tenant_id=? AND id=? AND version=?")
        .bind(request.target_bundle_id).bind(target.try_get::<Uuid,_>("workflow_version_id")?).bind(new_version).bind(request.tenant_id).bind(request.session_id).bind(request.expected_session_version).execute(&mut *tx).await?;
    if let Some(old) = old_bundle {
        sqlx::query("UPDATE bundle_references SET released_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND bundle_id=? AND reference_kind='pinned_session' AND owner_id=? AND released_at IS NULL").bind(request.tenant_id).bind(old).bind(request.session_id).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO bundle_references(id,tenant_id,bundle_id,reference_kind,owner_id) VALUES(?,?,?,'pinned_session',?)").bind(Uuid::now_v7()).bind(request.tenant_id).bind(request.target_bundle_id).bind(request.session_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO application_session_version_history(id,tenant_id,session_id,from_workflow_version_id,to_workflow_version_id,from_bundle_id,to_bundle_id,from_session_version,to_session_version,changed_by,command_idempotency_key) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(request.tenant_id).bind(request.session_id).bind(session.try_get::<Option<Uuid>,_>("workflow_version_id")?).bind(target.try_get::<Uuid,_>("workflow_version_id")?).bind(old_bundle).bind(request.target_bundle_id).bind(request.expected_session_version).bind(new_version).bind(request.actor_user_id).bind(&request.idempotency_key).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(SessionUpgradeReceiptV1 {
        api_version: 1,
        session_id: request.session_id,
        bundle_id: request.target_bundle_id,
        session_version: new_version,
        replayed: false,
    }))
}

fn session_summary(
    row: impl std::borrow::Borrow<sqlx::mysql::MySqlRow>,
) -> RuntimeResult<SessionSummaryV1> {
    let row = row.borrow();
    let policy = match row.try_get::<String, _>("version_policy")?.as_str() {
        "pinned" => SessionVersionPolicyV1::Pinned,
        "follow_deployment" => SessionVersionPolicyV1::FollowDeployment,
        "manual_upgrade" => SessionVersionPolicyV1::ManualUpgrade,
        _ => {
            return Err(RuntimeError::Internal(anyhow::anyhow!(
                "invalid Session policy"
            )));
        }
    };
    Ok(SessionSummaryV1 {
        session_id: row.try_get("id")?,
        application_id: row.try_get("application_id")?,
        application_deployment_id: row.try_get("application_deployment_id")?,
        workflow_version_id: row.try_get("workflow_version_id")?,
        bundle_id: row.try_get("bundle_id")?,
        version_policy: policy,
        external_user_id: row.try_get("external_user_id")?,
        title: row.try_get("title")?,
        status: row.try_get("status")?,
        version: row.try_get("version")?,
        updated_at: row.try_get("updated_at")?,
    })
}
