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
    error::{AppError, AppResult, UniqueConstraint, map_unique},
    grants::require_resource_visible,
    security::AuthActor,
    state::AppState,
};

pub(crate) const MODEL_NAME: UniqueConstraint = UniqueConstraint {
    index: "uq_model_alias",
    code: "MODEL_NAME_EXISTS",
    field: "alias",
    message: "A model with this name already exists",
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
    pub connection_name: String,
    pub provider_type: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
    pub model_name: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub default_parameters: Value,
    pub revision_number: u64,
    pub connection_status: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connection_checked_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub type ModelPage = PageResponse<ModelResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelRequest {
    pub connection_name: String,
    pub provider_type: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
    #[serde(default = "default_model_name")]
    pub alias: String,
    #[serde(default = "default_model_name")]
    pub model_name: String,
    #[serde(default = "default_max_input_tokens")]
    pub max_input_tokens: u64,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u64,
    #[serde(default = "default_parameters")]
    pub default_parameters: Value,
    pub price: Option<CreateModelPriceRequest>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelRequest {
    pub connection_name: String,
    pub provider_type: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
    pub alias: String,
    pub status: String,
    pub model_name: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
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
    pub connection_name: String,
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

const MODEL_COLUMNS: &str = "a.id,a.alias,a.status,a.version alias_version,a.updated_at,d.id deployment_id,d.connection_name,d.provider_type,d.endpoint,d.credential_id,d.owner_department_id,d.model_name,d.max_input_tokens,d.max_output_tokens,d.default_parameters,d.revision_number,COALESCE((SELECT h.status FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at) ORDER BY h.check_sequence DESC LIMIT 1),'untested') connection_status,(SELECT h.checked_at FROM resource_health_checks h WHERE h.tenant_id=a.tenant_id AND h.resource_type='model' AND h.resource_id=a.id AND h.checked_at>=GREATEST(a.updated_at,d.updated_at) ORDER BY h.check_sequence DESC LIMIT 1) connection_checked_at";

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
        let sql = format!(
            "SELECT {MODEL_COLUMNS},COUNT(*) OVER() total_count FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND (?='' OR a.status=?) AND (?='%%' OR a.alias LIKE ? OR d.connection_name LIKE ? OR d.model_name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?"
        );
        sqlx::query(&sql)
            .bind(actor.tenant_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&state.pool)
            .await?
    } else {
        let sql = format!(
            "SELECT {MODEL_COLUMNS},COUNT(*) OVER() total_count FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=a.tenant_id AND dc.descendant_id=d.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=a.tenant_id AND rg.subject_type='department' AND rg.resource_type='model' AND rg.resource_id=a.id AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)) AND (?='' OR a.status=?) AND (?='%%' OR a.alias LIKE ? OR d.connection_name LIKE ? OR d.model_name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?"
        );
        sqlx::query(&sql)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
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

#[utoipa::path(post,path="/api/v1/models/aliases",request_body=CreateModelRequest)]
pub async fn create_model(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateModelRequest>,
) -> AppResult<(StatusCode, Json<ModelResponse>)> {
    actor.require("model:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    validate_provider_type(&input.provider_type)?;
    validate_endpoint(&input.endpoint)?;
    validate_token_limits(input.max_input_tokens, input.max_output_tokens)?;
    validate_price(input.price.as_ref())?;
    let connection_name = validate_name(&input.connection_name, 160)?;
    let model_name = validate_name(&input.model_name, 255)?;
    let alias = validate_alias(&input.alias)?;
    ensure_model_name_available(&state, actor.tenant_id, &alias, None).await?;
    let model_id = Uuid::now_v7();
    let deployment_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO model_deployments(id,tenant_id,connection_name,provider_type,endpoint,credential_id,owner_department_id,model_name,max_input_tokens,max_output_tokens,default_parameters) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(deployment_id).bind(actor.tenant_id).bind(connection_name).bind(&input.provider_type).bind(&input.endpoint).bind(input.credential_id).bind(input.owner_department_id).bind(model_name).bind(input.max_input_tokens).bind(input.max_output_tokens).bind(input.default_parameters).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(?,?,?,?)")
        .bind(model_id)
        .bind(actor.tenant_id)
        .bind(alias)
        .bind(deployment_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| map_unique(error, &[MODEL_NAME]))?;
    sqlx::query("INSERT INTO model_alias_deployment_history(id,tenant_id,alias_id,previous_deployment_id,deployment_id,changed_by) VALUES(?,?,?,NULL,?,?)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(model_id).bind(deployment_id).bind(actor.user_id).execute(&mut *tx).await?;
    if let Some(price) = input.price {
        insert_price(&mut tx, &actor, deployment_id, 1, &price).await?;
    }
    audit(
        &mut tx,
        &actor,
        "model.created",
        "model",
        model_id,
        json!({"deploymentId":deployment_id,"providerType":input.provider_type}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_model(&state, actor.tenant_id, model_id).await?),
    ))
}

#[utoipa::path(patch,path="/api/v1/models/aliases/{id}",request_body=UpdateModelRequest,params(("id"=Uuid,Path)))]
pub async fn update_model(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    actor.require("model:manage")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    validate_provider_type(&input.provider_type)?;
    validate_endpoint(&input.endpoint)?;
    validate_token_limits(input.max_input_tokens, input.max_output_tokens)?;
    validate_price(input.price.as_ref())?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Model status is invalid",
        ));
    }
    let connection_name = validate_name(&input.connection_name, 160)?;
    let model_name = validate_name(&input.model_name, 255)?;
    let alias = validate_alias(&input.alias)?;
    ensure_model_name_available(&state, actor.tenant_id, &alias, Some(id)).await?;
    let mut tx = state.pool.begin().await?;
    let current = sqlx::query(
        "SELECT a.deployment_id,a.version,d.revision_number FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.id=? FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("Model"))?;
    let previous: Uuid = current.try_get("deployment_id")?;
    let current_version: u64 = current.try_get("version")?;
    if current_version != input.expected_alias_version {
        return Err(AppError::conflict(
            "MODEL_VERSION_CONFLICT",
            "Model changed",
        ));
    }
    let next = current.try_get::<u64, _>("revision_number")? + 1;
    let deployment_id = Uuid::now_v7();
    sqlx::query("INSERT INTO model_deployments(id,tenant_id,connection_name,provider_type,endpoint,credential_id,owner_department_id,model_name,max_input_tokens,max_output_tokens,default_parameters,revision_number,supersedes_deployment_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(deployment_id).bind(actor.tenant_id).bind(connection_name).bind(&input.provider_type).bind(&input.endpoint).bind(input.credential_id).bind(input.owner_department_id).bind(model_name).bind(input.max_input_tokens).bind(input.max_output_tokens).bind(input.default_parameters).bind(next).bind(previous).execute(&mut *tx).await?;
    if let Some(price) = input.price {
        insert_price(&mut tx, &actor, deployment_id, 1, &price).await?;
    } else if let Some(price) = sqlx::query("SELECT currency,CAST(input_per_million AS CHAR) input_per_million,CAST(output_per_million AS CHAR) output_per_million FROM model_price_versions WHERE tenant_id=? AND deployment_id=? ORDER BY version_number DESC LIMIT 1")
        .bind(actor.tenant_id).bind(previous).fetch_optional(&mut *tx).await?
    {
        let copied = CreateModelPriceRequest {
            currency: price.try_get("currency")?,
            input_per_million: price.try_get("input_per_million")?,
            output_per_million: price.try_get("output_per_million")?,
        };
        insert_price(&mut tx, &actor, deployment_id, 1, &copied).await?;
    }
    let updated = sqlx::query("UPDATE model_aliases SET alias=?,status=?,deployment_id=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?")
        .bind(alias).bind(&input.status).bind(deployment_id).bind(actor.tenant_id).bind(id).bind(input.expected_alias_version).execute(&mut *tx).await.map_err(|error| map_unique(error, &[MODEL_NAME]))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::conflict(
            "MODEL_VERSION_CONFLICT",
            "Model changed",
        ));
    }
    sqlx::query("INSERT INTO model_alias_deployment_history(id,tenant_id,alias_id,previous_deployment_id,deployment_id,changed_by) VALUES(?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(previous).bind(deployment_id).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "model.updated",
        "model",
        id,
        json!({"previousDeploymentId":previous,"deploymentId":deployment_id,"revisionNumber":next,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_model(&state, actor.tenant_id, id).await?))
}

async fn ensure_model_name_available(
    state: &AppState,
    tenant: Uuid,
    alias: &str,
    exclude: Option<Uuid>,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM model_aliases WHERE tenant_id=? AND alias=? AND (? IS NULL OR id<>?))",
    )
    .bind(tenant)
    .bind(alias)
    .bind(exclude)
    .bind(exclude)
    .fetch_one(&state.pool)
    .await?;
    if exists {
        Err(AppError::unique(MODEL_NAME))
    } else {
        Ok(())
    }
}

#[utoipa::path(get,path="/api/v1/models/aliases/{id}/deployment-history",params(("id"=Uuid,Path)))]
pub async fn list_deployment_history(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ModelDeploymentHistoryResponse>>> {
    actor.require("model:view")?;
    require_resource_visible(&state, &actor, "model", id).await?;
    let rows=sqlx::query("SELECT h.id,h.previous_deployment_id,h.deployment_id,d.revision_number,d.connection_name,d.model_name,h.changed_by,h.changed_at FROM model_alias_deployment_history h JOIN model_deployments d ON d.id=h.deployment_id WHERE h.tenant_id=? AND h.alias_id=? ORDER BY h.changed_at DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                Ok(ModelDeploymentHistoryResponse {
                    id: r.try_get("id")?,
                    previous_deployment_id: r.try_get("previous_deployment_id")?,
                    deployment_id: r.try_get("deployment_id")?,
                    revision_number: r.try_get("revision_number")?,
                    connection_name: r.try_get("connection_name")?,
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
    validate_price(Some(&input))?;
    let mut tx = state.pool.begin().await?;
    let next:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(version_number),0)+1 AS UNSIGNED) FROM model_price_versions WHERE tenant_id=? AND deployment_id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    let price_id = insert_price(&mut tx, &actor, id, next, &input).await?;
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
    let row = sqlx::query(
        "SELECT d.provider_type,d.endpoint,d.credential_id,d.owner_department_id,d.model_name FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.id=? AND a.tenant_id=? AND a.status='active' AND d.status='active'",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Model"))?;
    require_department_scope(&state.pool, &actor, row.try_get("owner_department_id")?).await?;
    let provider_type: String = row.try_get("provider_type")?;
    validate_provider_type(&provider_type)?;
    let endpoint: String = row.try_get("endpoint")?;
    let model_name: String = row.try_get("model_name")?;
    connection_test::run_openai_chat_completions_check(
        &state,
        &actor,
        "model",
        id,
        &format!("{}/chat/completions", endpoint.trim_end_matches('/')),
        row.try_get("credential_id")?,
        &model_name,
    )
    .await
}

async fn load_model(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ModelResponse> {
    let sql = format!(
        "SELECT {MODEL_COLUMNS} FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.id=?"
    );
    let row = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Model"))?;
    model_from_row(row).map_err(Into::into)
}

async fn require_deployment_visible(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<()> {
    let department: Option<Uuid> = sqlx::query_scalar(
        "SELECT owner_department_id FROM model_deployments WHERE id=? AND tenant_id=?",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?;
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
        connection_name: r.try_get("connection_name")?,
        provider_type: r.try_get("provider_type")?,
        endpoint: r.try_get("endpoint")?,
        credential_id: r.try_get("credential_id")?,
        owner_department_id: r.try_get("owner_department_id")?,
        model_name: r.try_get("model_name")?,
        max_input_tokens: r.try_get("max_input_tokens")?,
        max_output_tokens: r.try_get("max_output_tokens")?,
        default_parameters: r.try_get("default_parameters")?,
        revision_number: r.try_get("revision_number")?,
        connection_status: r.try_get("connection_status")?,
        connection_checked_at: r.try_get("connection_checked_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

fn validate_alias(value: &str) -> AppResult<String> {
    let alias = value.trim().to_ascii_lowercase();
    if alias.is_empty() || alias.len() > 128 {
        return Err(AppError::bad_request(
            "INVALID_MODEL_ALIAS",
            "Model name is invalid",
        ));
    }
    Ok(alias)
}

fn validate_provider_type(value: &str) -> AppResult<()> {
    if value != "openai_compatible" {
        return Err(AppError::bad_request(
            "INVALID_MODEL_PROVIDER",
            "API format is unsupported",
        ));
    }
    Ok(())
}

fn validate_token_limits(max_input_tokens: u64, max_output_tokens: u64) -> AppResult<()> {
    if max_input_tokens == 0 || max_output_tokens == 0 {
        return Err(AppError::bad_request(
            "INVALID_MODEL_TOKEN_LIMITS",
            "Model token limits must be positive",
        ));
    }
    Ok(())
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

fn default_model_name() -> String {
    "gpt-5.6-sol".into()
}

const fn default_max_input_tokens() -> u64 {
    1_050_000
}

const fn default_max_output_tokens() -> u64 {
    128_000
}

fn default_parameters() -> Value {
    json!({})
}
