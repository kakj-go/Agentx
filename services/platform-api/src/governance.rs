use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::audit,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPolicyInput {
    pub dimension: String,
    pub hard_limit: String,
    pub period_seconds: Option<u64>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateQuotaPoliciesRequest {
    pub policies: Vec<QuotaPolicyInput>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPolicyResponse {
    pub dimension: String,
    pub hard_limit: String,
    pub period_seconds: Option<u64>,
    pub active_reserved: String,
    pub period_usage: String,
    pub version: u64,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkerCapabilityResponse {
    pub instance_id: String,
    pub capability: String,
    pub node_protocol_version: String,
    pub ir_schema_versions: Value,
    pub compiler_version_min: String,
    pub compiler_version_max: String,
    pub manifest_hashes: Value,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub heartbeat_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRetentionRunRequest {
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default = "default_artifact_retention_days")]
    pub artifact_retention_days: u32,
    #[serde(default = "default_trace_retention_days")]
    pub trace_retention_days: u32,
    #[serde(default = "default_message_retention_days")]
    pub message_retention_days: u32,
    #[serde(default = "default_evaluation_retention_days")]
    pub evaluation_retention_days: u32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetentionRunResponse {
    pub id: Uuid,
    pub dry_run: bool,
    pub status: String,
    pub candidate_count: u64,
    pub deleted_count: u64,
    pub error_message: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetentionItemResponse {
    pub id: Uuid,
    pub data_type: String,
    pub target_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub attempt_count: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

fn default_true() -> bool {
    true
}

fn default_artifact_retention_days() -> u32 {
    30
}

fn default_trace_retention_days() -> u32 {
    180
}

fn default_message_retention_days() -> u32 {
    180
}

fn default_evaluation_retention_days() -> u32 {
    365
}

#[utoipa::path(get, path = "/api/v1/runtime/quotas")]
pub async fn get_quotas(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<QuotaPolicyResponse>>> {
    actor.require("runtime:view")?;
    let rows = sqlx::query("SELECT p.dimension_key,CAST(p.hard_limit AS CHAR) hard_limit,p.period_seconds,p.version,CAST(COALESCE((SELECT SUM(r.amount) FROM quota_reservations r WHERE r.tenant_id=p.tenant_id AND r.dimension_key=p.dimension_key AND r.status='active' AND r.expires_at>CURRENT_TIMESTAMP(6)),0) AS CHAR) active_reserved,CAST(COALESCE((SELECT SUM(l.amount) FROM quota_usage_ledger l WHERE l.tenant_id=p.tenant_id AND l.dimension_key=p.dimension_key AND (p.period_seconds IS NULL OR l.occurred_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL p.period_seconds SECOND))),0) AS CHAR) period_usage FROM quota_policies p WHERE p.tenant_id=? ORDER BY p.dimension_key")
        .bind(actor.tenant_id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                Ok(QuotaPolicyResponse {
                    dimension: row.try_get("dimension_key")?,
                    hard_limit: row.try_get("hard_limit")?,
                    period_seconds: row.try_get("period_seconds")?,
                    active_reserved: row.try_get("active_reserved")?,
                    period_usage: row.try_get("period_usage")?,
                    version: row.try_get("version")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?,
    ))
}

#[utoipa::path(put, path = "/api/v1/runtime/quotas", request_body = UpdateQuotaPoliciesRequest)]
pub async fn update_quotas(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<UpdateQuotaPoliciesRequest>,
) -> AppResult<Json<Vec<QuotaPolicyResponse>>> {
    actor.require("runtime:view")?;
    if !actor.company_admin {
        return Err(AppError::forbidden(
            "Only Company Admin can update Runtime quotas",
        ));
    }
    if input.policies.is_empty()
        || input.policies.len() > agentx_infrastructure::quota::SUPPORTED_DIMENSIONS.len()
    {
        return Err(AppError::bad_request(
            "INVALID_QUOTA_POLICIES",
            "At least one supported quota policy is required",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut tx = state.pool.begin().await?;
    for policy in &input.policies {
        if !agentx_infrastructure::quota::SUPPORTED_DIMENSIONS.contains(&policy.dimension.as_str())
            || !seen.insert(policy.dimension.as_str())
        {
            return Err(AppError::bad_request(
                "INVALID_QUOTA_DIMENSION",
                "Quota dimensions must be supported and unique",
            ));
        }
        let limit = Decimal::from_str(&policy.hard_limit).map_err(|_| {
            AppError::bad_request("INVALID_QUOTA_LIMIT", "Quota hardLimit must be a decimal")
        })?;
        if limit <= Decimal::ZERO {
            return Err(AppError::bad_request(
                "INVALID_QUOTA_LIMIT",
                "Quota hardLimit must be greater than zero",
            ));
        }
        sqlx::query("INSERT INTO quota_policies(tenant_id,dimension_key,hard_limit,period_seconds,updated_by) VALUES(?,?,?,?,?) ON DUPLICATE KEY UPDATE hard_limit=VALUES(hard_limit),period_seconds=VALUES(period_seconds),updated_by=VALUES(updated_by),version=version+1")
            .bind(actor.tenant_id)
            .bind(&policy.dimension)
            .bind(limit)
            .bind(policy.period_seconds)
            .bind(actor.user_id)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        &actor,
        "runtime.quotas.updated",
        "tenant",
        actor.tenant_id,
        json!({"dimensions":seen}),
    )
    .await?;
    tx.commit().await?;
    get_quotas(State(state), actor).await
}

#[utoipa::path(get, path = "/api/v1/runtime/capabilities")]
pub async fn get_capabilities(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<WorkerCapabilityResponse>>> {
    actor.require("runtime:view")?;
    let rows = sqlx::query("SELECT instance_id,capability,node_protocol_version,ir_schema_versions_json,compiler_version_min,compiler_version_max,manifest_hashes_json,status,heartbeat_at FROM worker_capabilities WHERE heartbeat_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 60 SECOND) ORDER BY instance_id,capability")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                Ok(WorkerCapabilityResponse {
                    instance_id: row.try_get("instance_id")?,
                    capability: row.try_get("capability")?,
                    node_protocol_version: row.try_get("node_protocol_version")?,
                    ir_schema_versions: row.try_get("ir_schema_versions_json")?,
                    compiler_version_min: row.try_get("compiler_version_min")?,
                    compiler_version_max: row.try_get("compiler_version_max")?,
                    manifest_hashes: row.try_get("manifest_hashes_json")?,
                    status: row.try_get("status")?,
                    heartbeat_at: row.try_get("heartbeat_at")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/retention-runs", request_body = CreateRetentionRunRequest)]
pub async fn create_retention_run(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateRetentionRunRequest>,
) -> AppResult<(StatusCode, Json<RetentionRunResponse>)> {
    actor.require("runtime:view")?;
    if !actor.company_admin {
        return Err(AppError::forbidden("Only Company Admin can run retention"));
    }
    if [
        input.artifact_retention_days,
        input.trace_retention_days,
        input.message_retention_days,
        input.evaluation_retention_days,
    ]
    .iter()
    .any(|days| !(1..=3650).contains(days))
    {
        return Err(AppError::bad_request(
            "INVALID_RETENTION_DAYS",
            "Retention days must be between 1 and 3650",
        ));
    }
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    for (data_type, retention_days) in [
        ("artifact", input.artifact_retention_days),
        ("trace", input.trace_retention_days),
        ("application_message", input.message_retention_days),
        ("evaluation_report", input.evaluation_retention_days),
    ] {
        sqlx::query("INSERT INTO retention_policies(tenant_id,data_type,retention_days,enabled,updated_by) VALUES(?,?,?,TRUE,?) ON DUPLICATE KEY UPDATE retention_days=VALUES(retention_days),enabled=TRUE,updated_by=VALUES(updated_by),version=version+1")
            .bind(actor.tenant_id)
            .bind(data_type)
            .bind(retention_days)
            .bind(actor.user_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO retention_runs(id,tenant_id,dry_run,requested_by) VALUES(?,?,?,?)")
        .bind(id)
        .bind(actor.tenant_id)
        .bind(input.dry_run)
        .bind(actor.user_id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        "retention.run.created",
        "retention_run",
        id,
        json!({"dryRun":input.dry_run,"artifactRetentionDays":input.artifact_retention_days,"traceRetentionDays":input.trace_retention_days,"messageRetentionDays":input.message_retention_days,"evaluationRetentionDays":input.evaluation_retention_days}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(load_retention_run(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/retention-runs")]
pub async fn list_retention_runs(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<RetentionRunResponse>>> {
    actor.require("runtime:view")?;
    let rows = sqlx::query("SELECT id,dry_run,status,candidate_count,deleted_count,error_message,created_at,completed_at FROM retention_runs WHERE tenant_id=? ORDER BY created_at DESC LIMIT 100")
        .bind(actor.tenant_id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(retention_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(get, path = "/api/v1/retention-runs/{id}/items", params(("id" = Uuid, Path)))]
pub async fn list_retention_items(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<RetentionItemResponse>>> {
    actor.require("runtime:view")?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM retention_runs WHERE tenant_id=? AND id=?)",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if !exists {
        return Err(AppError::not_found("Retention Run"));
    }
    let rows = sqlx::query("SELECT id,data_type,target_id,status,reason,attempt_count,updated_at FROM retention_items WHERE tenant_id=? AND retention_run_id=? ORDER BY data_type,created_at,id LIMIT 10000")
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                Ok(RetentionItemResponse {
                    id: row.try_get("id")?,
                    data_type: row.try_get("data_type")?,
                    target_id: row.try_get("target_id")?,
                    status: row.try_get("status")?,
                    reason: row.try_get("reason")?,
                    attempt_count: row.try_get("attempt_count")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
    ))
}

async fn load_retention_run(
    state: &AppState,
    tenant_id: Uuid,
    id: Uuid,
) -> AppResult<RetentionRunResponse> {
    let row = sqlx::query("SELECT id,dry_run,status,candidate_count,deleted_count,error_message,created_at,completed_at FROM retention_runs WHERE tenant_id=? AND id=?")
        .bind(tenant_id)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    retention_from_row(row)
}

fn retention_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<RetentionRunResponse> {
    Ok(RetentionRunResponse {
        id: row.try_get("id")?,
        dry_run: row.try_get("dry_run")?,
        status: row.try_get("status")?,
        candidate_count: row.try_get("candidate_count")?,
        deleted_count: row.try_get("deleted_count")?,
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}
