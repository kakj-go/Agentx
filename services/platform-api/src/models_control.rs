use agentx_api_types::PageResponse;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    connection_test::{self, HealthCheckResponse},
    control_common::{audit, require_department_scope, validate_name},
    credentials,
    error::{AppError, AppResult},
    grants::require_resource_visible,
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    pub id: Uuid,
    pub alias: String,
    pub status: String,
    pub alias_version: u64,
    pub deployment_id: Uuid,
    pub deployment_name: String,
    pub model_name: String,
    pub provider_id: Uuid,
    pub provider_name: String,
    pub provider_type: String,
    pub owner_department_id: Uuid,
    pub connection_status: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connection_checked_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type ModelPage = PageResponse<ModelResponse>;
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderResponse {
    pub id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub status: String,
    pub owner_department_id: Uuid,
    pub version: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelProviderRequest {
    pub name: String,
    pub provider_type: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelProviderRequest {
    pub name: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub status: String,
    pub version: u64,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeploymentResponse {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub name: String,
    pub model_name: String,
    pub endpoint_override: Option<String>,
    pub credential_id: Option<Uuid>,
    pub default_parameters: Value,
    pub status: String,
    pub revision_number: u64,
    pub supersedes_deployment_id: Option<Uuid>,
    pub version: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelDeploymentRequest {
    pub provider_id: Uuid,
    pub name: String,
    pub model_name: String,
    pub endpoint_override: Option<String>,
    pub credential_id: Option<Uuid>,
    pub default_parameters: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeploymentQuery {
    pub provider_id: Option<Uuid>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelAliasRequest {
    pub alias: String,
    pub deployment_id: Uuid,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelAliasRequest {
    pub alias: String,
    pub status: String,
    pub version: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeploymentRevisionRequest {
    pub provider_id: Uuid,
    pub name: String,
    pub model_name: String,
    pub endpoint_override: Option<String>,
    pub credential_id: Option<Uuid>,
    pub default_parameters: Value,
    pub expected_alias_version: u64,
    pub price: Option<CreateModelPriceRequest>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelDeploymentHistoryResponse {
    pub id: Uuid,
    pub previous_deployment_id: Option<Uuid>,
    pub deployment_id: Uuid,
    pub revision_number: u64,
    pub model_name: String,
    pub changed_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub changed_at: OffsetDateTime,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceResponse {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub version_number: u64,
    pub currency: String,
    pub input_per_million: String,
    pub output_per_million: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelPriceRequest {
    pub currency: String,
    pub input_per_million: String,
    pub output_per_million: String,
}

#[utoipa::path(get, path = "/api/v1/models/aliases")]
pub async fn list_models(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ModelListQuery>,
) -> AppResult<Json<ModelPage>> {
    actor.require("model:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let rows = if actor.company_admin {
        sqlx::query(MODEL_SELECT_ADMIN)
            .bind(actor.tenant_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query(MODEL_SELECT_SCOPED)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&state.pool)
            .await?
    };
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(model_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total,
    }))
}
const MODEL_COLUMNS: &str = "a.id,a.alias,a.status,a.version alias_version,a.updated_at,d.id deployment_id,d.name deployment_name,d.model_name,p.id provider_id,p.name provider_name,p.provider_type,p.owner_department_id,COALESCE((SELECT h.status FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1),'untested') connection_status,(SELECT h.checked_at FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1) connection_checked_at,COUNT(*) OVER() total_count";
const MODEL_SELECT_ADMIN: &str = "SELECT a.id,a.alias,a.status,a.version alias_version,a.updated_at,d.id deployment_id,d.name deployment_name,d.model_name,p.id provider_id,p.name provider_name,p.provider_type,p.owner_department_id,COALESCE((SELECT h.status FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1),'untested') connection_status,(SELECT h.checked_at FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1) connection_checked_at,COUNT(*) OVER() total_count FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND (?='' OR a.status=?) AND (?='%%' OR a.alias LIKE ? OR d.model_name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?";
const MODEL_SELECT_SCOPED: &str = "SELECT a.id,a.alias,a.status,a.version alias_version,a.updated_at,d.id deployment_id,d.name deployment_name,d.model_name,p.id provider_id,p.name provider_name,p.provider_type,p.owner_department_id,COALESCE((SELECT h.status FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1),'untested') connection_status,(SELECT h.checked_at FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at,p.updated_at) ORDER BY h.check_sequence DESC LIMIT 1) connection_checked_at,COUNT(*) OVER() total_count FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=a.tenant_id AND dc.descendant_id=p.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=a.tenant_id AND rg.subject_type='department' AND rg.resource_type='model' AND rg.resource_id=a.id AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)) AND (?='' OR a.status=?) AND (?='%%' OR a.alias LIKE ? OR d.model_name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?";

#[utoipa::path(get, path = "/api/v1/models/providers")]
pub async fn list_providers(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<ModelProviderResponse>>> {
    actor.require("model:view")?;
    let rows = if actor.company_admin {
        sqlx::query("SELECT id,name,provider_type,endpoint,credential_id,status,owner_department_id,version FROM model_providers WHERE tenant_id=? ORDER BY name").bind(actor.tenant_id).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT p.id,p.name,p.provider_type,p.endpoint,p.credential_id,p.status,p.owner_department_id,p.version FROM model_providers p WHERE p.tenant_id=? AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=p.tenant_id AND dc.descendant_id=p.owner_department_id) ORDER BY p.name").bind(actor.tenant_id).bind(actor.user_id).fetch_all(&state.pool).await?
    };
    Ok(Json(
        rows.into_iter()
            .map(provider_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(get,path="/api/v1/models/aliases/{id}",params(("id"=Uuid,Path)))]
pub async fn get_model(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ModelResponse>> {
    actor.require("model:view")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    Ok(Json(load_model(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(
    operation_id = "list_model_deployments",
    get,
    path = "/api/v1/models/deployments"
)]
pub async fn list_deployments(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ModelDeploymentQuery>,
) -> AppResult<Json<Vec<ModelDeploymentResponse>>> {
    actor.require("model:view")?;
    let rows = if actor.company_admin {
        sqlx::query("SELECT d.id,d.provider_id,d.name,d.model_name,d.endpoint_override,d.credential_id,d.default_parameters,d.status,d.revision_number,d.supersedes_deployment_id,d.version FROM model_deployments d WHERE d.tenant_id=? AND (? IS NULL OR d.provider_id=?) ORDER BY d.name,d.revision_number DESC")
            .bind(actor.tenant_id).bind(query.provider_id).bind(query.provider_id).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT d.id,d.provider_id,d.name,d.model_name,d.endpoint_override,d.credential_id,d.default_parameters,d.status,d.revision_number,d.supersedes_deployment_id,d.version FROM model_deployments d JOIN model_providers p ON p.id=d.provider_id WHERE d.tenant_id=? AND (? IS NULL OR d.provider_id=?) AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=d.tenant_id AND dc.descendant_id=p.owner_department_id) ORDER BY d.name,d.revision_number DESC")
            .bind(actor.tenant_id).bind(query.provider_id).bind(query.provider_id).bind(actor.user_id).fetch_all(&state.pool).await?
    };
    Ok(Json(
        rows.into_iter()
            .map(deployment_from_row)
            .collect::<Result<_, _>>()?,
    ))
}
#[utoipa::path(post,path="/api/v1/models/providers",request_body=CreateModelProviderRequest)]
pub async fn create_provider(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateModelProviderRequest>,
) -> AppResult<(StatusCode, Json<ModelProviderResponse>)> {
    actor.require("model:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    let name = validate_name(&input.name, 160)?;
    if !matches!(
        input.provider_type.as_str(),
        "openai_compatible" | "custom_http"
    ) {
        return Err(AppError::bad_request(
            "INVALID_MODEL_PROVIDER",
            "Provider type is invalid",
        ));
    }
    validate_endpoint(&input.endpoint)?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO model_providers(id,tenant_id,name,provider_type,endpoint,credential_id,owner_department_id) VALUES(?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(name).bind(&input.provider_type).bind(&input.endpoint).bind(input.credential_id).bind(input.owner_department_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "model.provider_created",
        "model_provider",
        id,
        json!({"providerType":input.provider_type}),
    )
    .await?;
    tx.commit().await?;
    let row=sqlx::query("SELECT id,name,provider_type,endpoint,credential_id,status,owner_department_id,version FROM model_providers WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    Ok((StatusCode::CREATED, Json(provider_from_row(row)?)))
}

#[utoipa::path(patch,path="/api/v1/models/providers/{id}",request_body=UpdateModelProviderRequest,params(("id"=Uuid,Path)))]
pub async fn update_provider(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateModelProviderRequest>,
) -> AppResult<Json<ModelProviderResponse>> {
    actor.require("model:manage")?;
    let department: Option<Uuid> = sqlx::query_scalar(
        "SELECT owner_department_id FROM model_providers WHERE tenant_id=? AND id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    require_department_scope(
        &state.pool,
        &actor,
        department.ok_or_else(|| AppError::not_found("Model provider"))?,
    )
    .await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    let name = validate_name(&input.name, 160)?;
    validate_endpoint(&input.endpoint)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Provider status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE model_providers SET name=?,endpoint=?,credential_id=?,status=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?")
        .bind(name).bind(&input.endpoint).bind(input.credential_id).bind(&input.status)
        .bind(actor.tenant_id).bind(id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "MODEL_PROVIDER_VERSION_CONFLICT",
            "Model provider changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "model.provider_updated",
        "model_provider",
        id,
        json!({"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    let row=sqlx::query("SELECT id,name,provider_type,endpoint,credential_id,status,owner_department_id,version FROM model_providers WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    Ok(Json(provider_from_row(row)?))
}

#[utoipa::path(post,path="/api/v1/models/deployments",request_body=CreateModelDeploymentRequest)]
pub async fn create_deployment(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateModelDeploymentRequest>,
) -> AppResult<(StatusCode, Json<ModelDeploymentResponse>)> {
    actor.require("model:manage")?;
    let provider:Option<Uuid>=sqlx::query_scalar("SELECT owner_department_id FROM model_providers WHERE id=? AND tenant_id=? AND status='active'").bind(input.provider_id).bind(actor.tenant_id).fetch_optional(&state.pool).await?;
    let department = provider.ok_or_else(|| AppError::not_found("Model provider"))?;
    require_department_scope(&state.pool, &actor, department).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    let name = validate_name(&input.name, 160)?;
    let model_name = validate_name(&input.model_name, 255)?;
    if let Some(endpoint) = &input.endpoint_override {
        validate_endpoint(endpoint)?;
    }
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO model_deployments(id,tenant_id,provider_id,name,model_name,endpoint_override,credential_id,default_parameters) VALUES(?,?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(input.provider_id).bind(name).bind(model_name).bind(input.endpoint_override).bind(input.credential_id).bind(input.default_parameters).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "model.deployment_created",
        "model_deployment",
        id,
        json!({"providerId":input.provider_id}),
    )
    .await?;
    let row=sqlx::query("SELECT id,provider_id,name,model_name,endpoint_override,credential_id,default_parameters,status,revision_number,supersedes_deployment_id,version FROM model_deployments WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(deployment_from_row(row)?)))
}

#[utoipa::path(post,path="/api/v1/models/aliases",request_body=CreateModelAliasRequest)]
pub async fn create_alias(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateModelAliasRequest>,
) -> AppResult<(StatusCode, Json<ModelResponse>)> {
    actor.require("model:manage")?;
    let alias = input.alias.trim().to_ascii_lowercase();
    if alias.is_empty() || alias.len() > 128 {
        return Err(AppError::bad_request(
            "INVALID_MODEL_ALIAS",
            "Alias is invalid",
        ));
    }
    let department:Option<Uuid>=sqlx::query_scalar("SELECT p.owner_department_id FROM model_deployments d JOIN model_providers p ON p.id=d.provider_id WHERE d.id=? AND d.tenant_id=? AND d.status='active'").bind(input.deployment_id).bind(actor.tenant_id).fetch_optional(&state.pool).await?;
    require_department_scope(
        &state.pool,
        &actor,
        department.ok_or_else(|| AppError::not_found("Model deployment"))?,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(?,?,?,?)")
        .bind(id)
        .bind(actor.tenant_id)
        .bind(alias)
        .bind(input.deployment_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO model_alias_deployment_history(id,tenant_id,alias_id,previous_deployment_id,deployment_id,changed_by) VALUES(?,?,?,NULL,?,?)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(input.deployment_id).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "model.alias_created",
        "model",
        id,
        json!({"deploymentId":input.deployment_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_model(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(patch,path="/api/v1/models/aliases/{id}",request_body=UpdateModelAliasRequest,params(("id"=Uuid,Path)))]
pub async fn update_alias(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateModelAliasRequest>,
) -> AppResult<Json<ModelResponse>> {
    actor.require("model:manage")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    let alias = input.alias.trim().to_ascii_lowercase();
    if alias.is_empty() || alias.len() > 128 {
        return Err(AppError::bad_request(
            "INVALID_MODEL_ALIAS",
            "Alias is invalid",
        ));
    }
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Model alias status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE model_aliases SET alias=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(&alias).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "MODEL_ALIAS_VERSION_CONFLICT",
            "Model alias changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "model.alias_updated",
        "model",
        id,
        json!({"alias":alias,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_model(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/models/aliases/{id}/deployment-revisions",request_body=CreateDeploymentRevisionRequest,params(("id"=Uuid,Path)))]
pub async fn create_deployment_revision(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateDeploymentRevisionRequest>,
) -> AppResult<(StatusCode, Json<ModelResponse>)> {
    actor.require("model:manage")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    let provider_department: Option<Uuid> = sqlx::query_scalar("SELECT owner_department_id FROM model_providers WHERE tenant_id=? AND id=? AND status='active'")
        .bind(actor.tenant_id).bind(input.provider_id).fetch_optional(&state.pool).await?;
    require_department_scope(
        &state.pool,
        &actor,
        provider_department.ok_or_else(|| AppError::not_found("Model provider"))?,
    )
    .await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    let name = validate_name(&input.name, 160)?;
    let model_name = validate_name(&input.model_name, 255)?;
    if let Some(endpoint) = &input.endpoint_override {
        validate_endpoint(endpoint)?;
    }
    validate_price(input.price.as_ref())?;
    let mut tx = state.pool.begin().await?;
    let current = sqlx::query(
        "SELECT deployment_id,version FROM model_aliases WHERE tenant_id=? AND id=? FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("Model alias"))?;
    let previous: Uuid = current.try_get("deployment_id")?;
    let current_version: u64 = current.try_get("version")?;
    if current_version != input.expected_alias_version {
        return Err(AppError::conflict(
            "MODEL_ALIAS_VERSION_CONFLICT",
            "Model alias changed",
        ));
    }
    let next:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(revision_number),0)+1 AS UNSIGNED) FROM model_deployments WHERE tenant_id=? AND (id=? OR supersedes_deployment_id=?) FOR UPDATE").bind(actor.tenant_id).bind(previous).bind(previous).fetch_one(&mut *tx).await?;
    let deployment_id = Uuid::now_v7();
    sqlx::query("INSERT INTO model_deployments(id,tenant_id,provider_id,name,model_name,endpoint_override,credential_id,default_parameters,revision_number,supersedes_deployment_id) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(deployment_id).bind(actor.tenant_id).bind(input.provider_id).bind(name).bind(model_name).bind(input.endpoint_override).bind(input.credential_id).bind(input.default_parameters).bind(next).bind(previous).execute(&mut *tx).await?;
    if let Some(price)=input.price {
        insert_price(&mut tx,&actor,deployment_id,1,&price).await?;
    } else if let Some(price)=sqlx::query("SELECT currency,CAST(input_per_million AS CHAR) input_per_million,CAST(output_per_million AS CHAR) output_per_million FROM model_price_versions WHERE tenant_id=? AND deployment_id=? ORDER BY version_number DESC LIMIT 1").bind(actor.tenant_id).bind(previous).fetch_optional(&mut *tx).await? {
        let copied=CreateModelPriceRequest{currency:price.try_get("currency")?,input_per_million:price.try_get("input_per_million")?,output_per_million:price.try_get("output_per_million")?};
        insert_price(&mut tx,&actor,deployment_id,1,&copied).await?;
    }
    sqlx::query("UPDATE model_aliases SET deployment_id=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?").bind(deployment_id).bind(actor.tenant_id).bind(id).bind(input.expected_alias_version).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO model_alias_deployment_history(id,tenant_id,alias_id,previous_deployment_id,deployment_id,changed_by) VALUES(?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(previous).bind(deployment_id).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "model.deployment_revised",
        "model",
        id,
        json!({"previousDeploymentId":previous,"deploymentId":deployment_id,"revisionNumber":next}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_model(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get,path="/api/v1/models/aliases/{id}/deployment-history",params(("id"=Uuid,Path)))]
pub async fn list_deployment_history(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ModelDeploymentHistoryResponse>>> {
    actor.require("model:view")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    let rows=sqlx::query("SELECT h.id,h.previous_deployment_id,h.deployment_id,d.revision_number,d.model_name,h.changed_by,h.changed_at FROM model_alias_deployment_history h JOIN model_deployments d ON d.id=h.deployment_id WHERE h.tenant_id=? AND h.alias_id=? ORDER BY h.changed_at DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                Ok(ModelDeploymentHistoryResponse {
                    id: r.try_get("id")?,
                    previous_deployment_id: r.try_get("previous_deployment_id")?,
                    deployment_id: r.try_get("deployment_id")?,
                    revision_number: r.try_get("revision_number")?,
                    model_name: r.try_get("model_name")?,
                    changed_by: r.try_get("changed_by")?,
                    changed_at: r.try_get("changed_at")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/models/deployments/{id}/prices",request_body=CreateModelPriceRequest,params(("id"=Uuid,Path)))]
pub async fn create_price(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateModelPriceRequest>,
) -> AppResult<(StatusCode, Json<ModelPriceResponse>)> {
    actor.require("model:manage")?;
    require_deployment_visible(&state, &actor, id).await?;
    let input_price = input.input_per_million.parse::<f64>();
    let output_price = input.output_per_million.parse::<f64>();
    if input.currency.len() != 3
        || !input_price.is_ok_and(|value| value.is_finite() && value >= 0.0)
        || !output_price.is_ok_and(|value| value.is_finite() && value >= 0.0)
    {
        return Err(AppError::bad_request(
            "INVALID_MODEL_PRICE",
            "Currency or price is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let next:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(version_number),0)+1 AS UNSIGNED) FROM model_price_versions WHERE tenant_id=? AND deployment_id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    let price_id = Uuid::now_v7();
    let inserted=sqlx::query("INSERT INTO model_price_versions(id,tenant_id,deployment_id,version_number,currency,input_per_million,output_per_million,created_by) SELECT ?,?,?,?,?,?,?,? FROM model_deployments WHERE id=? AND tenant_id=? AND status='active'").bind(price_id).bind(actor.tenant_id).bind(id).bind(next).bind(input.currency.to_ascii_uppercase()).bind(&input.input_per_million).bind(&input.output_per_million).bind(actor.user_id).bind(id).bind(actor.tenant_id).execute(&mut *tx).await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::not_found("Model deployment"));
    }
    audit(
        &mut tx,
        &actor,
        "model.price_created",
        "model_deployment",
        id,
        json!({"priceVersionId":price_id,"versionNumber":next}),
    )
    .await?;
    tx.commit().await?;
    let row=sqlx::query("SELECT id,deployment_id,version_number,currency,CAST(input_per_million AS CHAR) input_per_million,CAST(output_per_million AS CHAR) output_per_million,created_at FROM model_price_versions WHERE id=?").bind(price_id).fetch_one(&state.pool).await?;
    Ok((StatusCode::CREATED, Json(price_from_row(row)?)))
}

#[utoipa::path(get,path="/api/v1/models/deployments/{id}/prices",params(("id"=Uuid,Path)))]
pub async fn list_prices(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ModelPriceResponse>>> {
    actor.require("model:view")?;
    require_deployment_visible(&state, &actor, id).await?;
    let rows=sqlx::query("SELECT id,deployment_id,version_number,currency,CAST(input_per_million AS CHAR) input_per_million,CAST(output_per_million AS CHAR) output_per_million,created_at FROM model_price_versions WHERE tenant_id=? AND deployment_id=? ORDER BY version_number DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(price_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/models/aliases/{id}/test-connection",params(("id"=Uuid,Path)))]
pub async fn test_model(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<HealthCheckResponse>> {
    actor.require("model:manage")?;
    let r = sqlx::query(
        "SELECT COALESCE(d.endpoint_override,p.endpoint) endpoint,COALESCE(d.credential_id,p.credential_id) credential_id,p.owner_department_id FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.id=? AND a.tenant_id=? AND a.status='active' AND d.status='active' AND p.status='active'",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Model alias"))?;
    require_department_scope(&state.pool, &actor, r.try_get("owner_department_id")?).await?;
    let endpoint: String = r.try_get("endpoint")?;
    connection_test::run_http_check(
        &state,
        &actor,
        "model",
        id,
        &format!("{}/models", endpoint.trim_end_matches('/')),
        r.try_get("credential_id")?,
    )
    .await
}

async fn load_model(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ModelResponse> {
    let sql = format!(
        "SELECT {MODEL_COLUMNS} FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND a.id=?"
    );
    let row = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Model alias"))?;
    model_from_row(row).map_err(Into::into)
}
async fn require_deployment_visible(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<()> {
    let department: Option<Uuid> = sqlx::query_scalar("SELECT p.owner_department_id FROM model_deployments d JOIN model_providers p ON p.id=d.provider_id WHERE d.id=? AND d.tenant_id=?")
        .bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?;
    let department = department.ok_or_else(|| AppError::not_found("Model deployment"))?;
    require_department_scope(&state.pool, actor, department).await
}
fn model_from_row(r: sqlx::mysql::MySqlRow) -> Result<ModelResponse, sqlx::Error> {
    Ok(ModelResponse {
        id: r.try_get("id")?,
        alias: r.try_get("alias")?,
        status: r.try_get("status")?,
        alias_version: r.try_get("alias_version")?,
        deployment_id: r.try_get("deployment_id")?,
        deployment_name: r.try_get("deployment_name")?,
        model_name: r.try_get("model_name")?,
        provider_id: r.try_get("provider_id")?,
        provider_name: r.try_get("provider_name")?,
        provider_type: r.try_get("provider_type")?,
        owner_department_id: r.try_get("owner_department_id")?,
        connection_status: r.try_get("connection_status")?,
        connection_checked_at: r.try_get("connection_checked_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn provider_from_row(r: sqlx::mysql::MySqlRow) -> Result<ModelProviderResponse, sqlx::Error> {
    Ok(ModelProviderResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        provider_type: r.try_get("provider_type")?,
        endpoint: r.try_get("endpoint")?,
        credential_id: r.try_get("credential_id")?,
        status: r.try_get("status")?,
        owner_department_id: r.try_get("owner_department_id")?,
        version: r.try_get("version")?,
    })
}
fn deployment_from_row(r: sqlx::mysql::MySqlRow) -> Result<ModelDeploymentResponse, sqlx::Error> {
    Ok(ModelDeploymentResponse {
        id: r.try_get("id")?,
        provider_id: r.try_get("provider_id")?,
        name: r.try_get("name")?,
        model_name: r.try_get("model_name")?,
        endpoint_override: r.try_get("endpoint_override")?,
        credential_id: r.try_get("credential_id")?,
        default_parameters: r.try_get("default_parameters")?,
        status: r.try_get("status")?,
        revision_number: r.try_get("revision_number")?,
        supersedes_deployment_id: r.try_get("supersedes_deployment_id")?,
        version: r.try_get("version")?,
    })
}
fn validate_price(input: Option<&CreateModelPriceRequest>) -> AppResult<()> {
    let Some(input) = input else {
        return Ok(());
    };
    let input_price = input.input_per_million.parse::<f64>();
    let output_price = input.output_per_million.parse::<f64>();
    if input.currency.len() != 3
        || !input_price.is_ok_and(|value| value.is_finite() && value >= 0.0)
        || !output_price.is_ok_and(|value| value.is_finite() && value >= 0.0)
    {
        return Err(AppError::bad_request(
            "INVALID_MODEL_PRICE",
            "Currency or price is invalid",
        ));
    }
    Ok(())
}
async fn insert_price(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    deployment_id: Uuid,
    version_number: u64,
    input: &CreateModelPriceRequest,
) -> AppResult<Uuid> {
    let price_id = Uuid::now_v7();
    sqlx::query("INSERT INTO model_price_versions(id,tenant_id,deployment_id,version_number,currency,input_per_million,output_per_million,created_by) VALUES(?,?,?,?,?,?,?,?)")
        .bind(price_id).bind(actor.tenant_id).bind(deployment_id).bind(version_number).bind(input.currency.to_ascii_uppercase()).bind(&input.input_per_million).bind(&input.output_per_million).bind(actor.user_id).execute(&mut **tx).await?;
    Ok(price_id)
}
fn price_from_row(r: sqlx::mysql::MySqlRow) -> Result<ModelPriceResponse, sqlx::Error> {
    Ok(ModelPriceResponse {
        id: r.try_get("id")?,
        deployment_id: r.try_get("deployment_id")?,
        version_number: r.try_get("version_number")?,
        currency: r.try_get("currency")?,
        input_per_million: r.try_get("input_per_million")?,
        output_per_million: r.try_get("output_per_million")?,
        created_at: r.try_get("created_at")?,
    })
}
fn validate_endpoint(value: &str) -> AppResult<()> {
    let endpoint = url::Url::parse(value)
        .map_err(|_| AppError::bad_request("INVALID_ENDPOINT", "Endpoint is invalid"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(AppError::bad_request(
            "INVALID_ENDPOINT",
            "Endpoint must use HTTP or HTTPS",
        ));
    }
    Ok(())
}
