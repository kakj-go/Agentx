use agentx_api_types::PageResponse;
use agentx_infrastructure::credential::PlainSecret;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, outbox, require_department_scope, validate_name},
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
pub struct ApplicationResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub status: String,
    pub owner_department_id: Uuid,
    pub active_deployment_id: Option<Uuid>,
    pub active_version_number: Option<u64>,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub type ApplicationPage = PageResponse<ApplicationResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    pub workflow_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub status: String,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeploymentResponse {
    pub id: Uuid,
    pub application_id: Uuid,
    pub workflow_version_id: Uuid,
    pub workflow_version_number: u64,
    pub environment_id: Uuid,
    pub environment_name: String,
    pub sequence_number: u64,
    pub input_schema: Value,
    pub output_schema: Value,
    pub output_expression: Option<String>,
    pub session_version_policy: String,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationDeploymentRequest {
    pub workflow_version_id: Uuid,
    pub environment_id: Uuid,
    pub input_schema: Value,
    pub output_schema: Value,
    pub output_expression: Option<String>,
    pub session_version_policy: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub family_id: Uuid,
    pub name: String,
    pub prefix: String,
    pub status: String,
    pub secret: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookResponse {
    pub id: Uuid,
    pub name: String,
    pub public_id: String,
    pub path: String,
    pub status: String,
    pub secret: Option<String>,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookRequest {
    pub name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebhookRequest {
    pub name: String,
    pub status: String,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResponse {
    pub id: Uuid,
    pub name: String,
    pub cron_expression: String,
    pub timezone: String,
    pub input: Value,
    pub misfire_policy: String,
    pub status: String,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduleRequest {
    pub name: String,
    pub cron_expression: String,
    pub timezone: String,
    pub input: Value,
    #[serde(default = "default_misfire_policy")]
    pub misfire_policy: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduleRequest {
    pub name: String,
    pub cron_expression: String,
    pub timezone: String,
    pub input: Value,
    #[serde(default = "default_misfire_policy")]
    pub misfire_policy: String,
    pub status: String,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub id: Uuid,
    pub application_id: Uuid,
    pub application_deployment_id: Uuid,
    pub workflow_version_id: Option<Uuid>,
    pub version_policy: String,
    pub external_user_id: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeSessionRequest {
    pub workflow_version_id: Uuid,
    pub version: u64,
}

#[utoipa::path(get, path = "/api/v1/applications")]
pub async fn list_applications(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ApplicationPage>> {
    actor.require("application:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let rows = if actor.company_admin {
        sqlx::query(APPLICATION_SELECT_LIST)
            .bind(actor.tenant_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(u64::from((page - 1) * page_size))
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query(APPLICATION_SELECT_VISIBLE)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(u64::from((page - 1) * page_size))
            .fetch_all(&state.pool)
            .await?
    };
    let total: i64 = if actor.company_admin {
        sqlx::query_scalar("SELECT COUNT(*) FROM applications WHERE tenant_id=? AND (?='' OR status=?) AND (?='%%' OR name LIKE ?)")
            .bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).fetch_one(&state.pool).await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM applications a WHERE a.tenant_id=? AND (a.visibility='company' OR a.owner_user_id=? OR (a.visibility='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=a.tenant_id AND ((dc.ancestor_id=a.owner_department_id AND dc.descendant_id=?) OR (dc.ancestor_id=? AND dc.descendant_id=a.owner_department_id)))) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=a.tenant_id AND ur.user_id=? AND dc.descendant_id=a.owner_department_id)) AND (?='' OR a.status=?) AND (?='%%' OR a.name LIKE ?)")
            .bind(actor.tenant_id).bind(actor.user_id).bind(actor.department_id).bind(actor.department_id).bind(actor.user_id).bind(&status).bind(&status).bind(&search).bind(&search).fetch_one(&state.pool).await?
    };
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(application_from_row)
            .collect::<AppResult<_>>()?,
        page,
        page_size,
        total: total as u64,
    }))
}

#[utoipa::path(post, path = "/api/v1/applications", request_body = CreateApplicationRequest)]
pub async fn create_application(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateApplicationRequest>,
) -> AppResult<(StatusCode, Json<ApplicationResponse>)> {
    actor.require("application:manage")?;
    validate_visibility(&input.visibility)?;
    let name = validate_name(&input.name, 160)?;
    let slug = validate_slug(&input.slug)?;
    let workflow_department: Uuid = sqlx::query_scalar(
        "SELECT owner_department_id FROM workflows WHERE id=? AND tenant_id=? AND status='active'",
    )
    .bind(input.workflow_id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Workflow"))?;
    require_department_scope(&state.pool, &actor, workflow_department).await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO applications(id,tenant_id,workflow_id,name,slug,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(id).bind(actor.tenant_id).bind(input.workflow_id).bind(&name).bind(&slug).bind(&input.description)
        .bind(&input.visibility).bind(actor.user_id).bind(actor.department_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "application.created",
        "application",
        id,
        json!({"workflowId":input.workflow_id}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "ApplicationCreated",
        "application",
        id,
        json!({"applicationId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_application(&state, &actor, id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/applications/{id}", params(("id" = Uuid, Path)))]
pub async fn get_application(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ApplicationResponse>> {
    actor.require("application:view")?;
    Ok(Json(load_application(&state, &actor, id).await?))
}

#[utoipa::path(patch, path = "/api/v1/applications/{id}", request_body = UpdateApplicationRequest, params(("id" = Uuid, Path)))]
pub async fn update_application(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateApplicationRequest>,
) -> AppResult<Json<ApplicationResponse>> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    validate_visibility(&input.visibility)?;
    if !matches!(input.status.as_str(), "draft" | "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_APPLICATION_STATUS",
            "Application status is invalid",
        ));
    }
    let name = validate_name(&input.name, 160)?;
    let mut tx = state.pool.begin().await?;
    let changed = sqlx::query("UPDATE applications SET name=?,description=?,visibility=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?")
        .bind(name).bind(&input.description).bind(&input.visibility).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "APPLICATION_VERSION_CONFLICT",
            "Application changed on the server",
        ));
    }
    if input.status == "active" {
        sqlx::query("UPDATE trigger_bindings b JOIN application_deployment_heads h ON h.tenant_id=b.tenant_id AND h.application_id=b.application_id AND h.deployment_id=b.application_deployment_id SET b.status=IF(b.trigger_kind='lifecycle','activating','active'),b.next_poll_at=IF(b.trigger_kind IN ('poll','lifecycle'),CURRENT_TIMESTAMP(6),NULL),b.last_error=NULL WHERE b.tenant_id=? AND b.application_id=?")
            .bind(actor.tenant_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE trigger_bindings SET status=IF(trigger_kind='lifecycle','deactivating','disabled'),next_poll_at=IF(trigger_kind='lifecycle',CURRENT_TIMESTAMP(6),NULL),locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND application_id=? AND status<>'disabled'")
            .bind(actor.tenant_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        &actor,
        "application.updated",
        "application",
        id,
        json!({"visibility":input.visibility,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_application(&state, &actor, id).await?))
}

#[utoipa::path(get, path = "/api/v1/applications/{id}/deployments", operation_id = "list_application_deployments", params(("id" = Uuid, Path)))]
pub async fn list_deployments(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ApplicationDeploymentResponse>>> {
    actor.require("application:view")?;
    require_application_access(&state, &actor, id, false).await?;
    let rows = sqlx::query(DEPLOYMENT_SELECT)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(deployment_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/deployments", operation_id = "create_application_deployment", request_body = CreateApplicationDeploymentRequest, params(("id" = Uuid, Path)))]
pub async fn create_deployment(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateApplicationDeploymentRequest>,
) -> AppResult<(StatusCode, Json<ApplicationDeploymentResponse>)> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    validate_version_policy(&input.session_version_policy)?;
    let valid: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM applications a JOIN workflow_versions wv ON wv.workflow_id=a.workflow_id AND wv.tenant_id=a.tenant_id JOIN workflow_deployment_heads h ON h.workflow_id=a.workflow_id AND h.environment_id=? JOIN workflow_deployments wd ON wd.id=h.active_deployment_id AND wd.workflow_version_id=wv.id WHERE a.id=? AND a.tenant_id=? AND wv.id=?)")
        .bind(input.environment_id).bind(id).bind(actor.tenant_id).bind(input.workflow_version_id).fetch_one(&state.pool).await?;
    if !valid {
        return Err(AppError::unprocessable(
            "WORKFLOW_VERSION_NOT_DEPLOYED",
            "Workflow Version is not active in the selected Environment",
        ));
    }
    validate_schema(&input.input_schema)?;
    validate_schema(&input.output_schema)?;
    if let Some(expression) = input.output_expression.as_deref() {
        let source = expression.strip_prefix('=').unwrap_or(expression);
        agentx_runtime::ExpressionEngine
            .validate(source)
            .map_err(|error| {
                AppError::unprocessable("INVALID_OUTPUT_EXPRESSION", error.to_string())
            })?;
    }
    let deployment_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT id FROM applications WHERE id=? AND tenant_id=? FOR UPDATE")
        .bind(id)
        .bind(actor.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE application_deployments SET status='superseded' WHERE tenant_id=? AND application_id=? AND status='active'").bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
    let sequence: u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM application_deployments WHERE tenant_id=? AND application_id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO application_deployments(id,tenant_id,application_id,workflow_version_id,environment_id,sequence_number,input_schema_json,output_schema_json,output_expression,session_version_policy,created_by) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(deployment_id).bind(actor.tenant_id).bind(id).bind(input.workflow_version_id).bind(input.environment_id).bind(sequence)
        .bind(input.input_schema).bind(input.output_schema).bind(input.output_expression).bind(input.session_version_policy).bind(actor.user_id).execute(&mut *tx).await?;
    reconcile_trigger_bindings(
        &mut tx,
        actor.tenant_id,
        id,
        deployment_id,
        input.workflow_version_id,
    )
    .await?;
    sqlx::query("INSERT INTO application_deployment_heads(tenant_id,application_id,deployment_id) VALUES(?,?,?) ON DUPLICATE KEY UPDATE deployment_id=VALUES(deployment_id),version=version+1")
        .bind(actor.tenant_id).bind(id).bind(deployment_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE applications SET status='active',version=version+1 WHERE id=? AND tenant_id=?",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &actor,
        "application.deployed",
        "application",
        id,
        json!({"deploymentId":deployment_id,"workflowVersionId":input.workflow_version_id}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "ApplicationPublished",
        "application",
        id,
        json!({"deploymentId":deployment_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_deployment(&state, actor.tenant_id, deployment_id).await?),
    ))
}

async fn reconcile_trigger_bindings(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant_id: Uuid,
    application_id: Uuid,
    deployment_id: Uuid,
    workflow_version_id: Uuid,
) -> AppResult<()> {
    sqlx::query("UPDATE trigger_bindings SET status=IF(trigger_kind='lifecycle','deactivating','disabled'),next_poll_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND application_id=? AND status IN ('active','activating','deactivating')")
        .bind(tenant_id)
        .bind(application_id)
        .execute(&mut **tx)
        .await?;
    let definition: Value = sqlx::query_scalar(
        "SELECT definition_json FROM workflow_versions WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(workflow_version_id)
    .fetch_one(&mut **tx)
    .await?;
    let Some(nodes) = definition.get("nodes").and_then(Value::as_array) else {
        return Ok(());
    };
    for node in nodes {
        let node_id = node.get("id").and_then(Value::as_str).unwrap_or_default();
        let node_type = node.get("type").and_then(Value::as_str).unwrap_or_default();
        let node_version = node.get("typeVersion").and_then(Value::as_u64).unwrap_or(1);
        let manifest: Option<Value> = sqlx::query_scalar("SELECT v.manifest_json FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.node_type=? AND v.version_number=? AND d.status='active' AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.tenant_id IS NULL LIMIT 1")
            .bind(node_type)
            .bind(node_version)
            .bind(tenant_id)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(operations) = manifest
            .as_ref()
            .and_then(|value| value.get("lifecycleOperations"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let has = |operation: &str| {
            operations
                .iter()
                .any(|value| value.as_str() == Some(operation))
        };
        let mut trigger_kinds = Vec::new();
        if has("poll") {
            trigger_kinds.push("poll");
        }
        if has("webhook") {
            trigger_kinds.push("webhook");
        }
        if has("activate") || has("deactivate") {
            trigger_kinds.push("lifecycle");
        }
        for trigger_kind in trigger_kinds {
            sqlx::query("INSERT INTO trigger_bindings(id,tenant_id,application_id,application_deployment_id,workflow_version_id,node_id,trigger_kind,configuration_json,status,next_poll_at) VALUES(?,?,?,?,?,?,?,?,IF(?='lifecycle','activating','active'),IF(? IN ('poll','lifecycle'),CURRENT_TIMESTAMP(6),NULL)) ON DUPLICATE KEY UPDATE status=VALUES(status),configuration_json=VALUES(configuration_json),workflow_version_id=VALUES(workflow_version_id),next_poll_at=VALUES(next_poll_at),last_error=NULL")
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(application_id)
            .bind(deployment_id)
            .bind(workflow_version_id)
            .bind(node_id)
            .bind(trigger_kind)
            .bind(json!({
                "nodeType": node_type,
                "nodeVersion": node_version,
                "parameters": node.get("parameters").cloned().unwrap_or_else(|| json!({})),
            }))
            .bind(trigger_kind)
            .bind(trigger_kind)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/applications/{id}/api-keys", params(("id" = Uuid, Path)))]
pub async fn list_api_keys(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ApiKeyResponse>>> {
    actor.require("application:manage_key")?;
    require_application_access(&state, &actor, id, true).await?;
    let rows=sqlx::query("SELECT id,family_id,name,key_prefix,status,created_at,last_used_at FROM application_api_keys WHERE tenant_id=? AND application_id=? ORDER BY created_at DESC")
        .bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(api_key_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/api-keys", request_body = CreateApiKeyRequest, params(("id" = Uuid, Path)))]
pub async fn create_api_key(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateApiKeyRequest>,
) -> AppResult<(StatusCode, Json<ApiKeyResponse>)> {
    actor.require("application:manage_key")?;
    require_application_access(&state, &actor, id, true).await?;
    let name = validate_name(&input.name, 160)?;
    let (key_id, secret, prefix, hash) = generate_api_key();
    let family_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO application_api_keys(id,tenant_id,application_id,family_id,name,key_prefix,secret_hash,created_by) VALUES(?,?,?,?,?,?,?,?)")
        .bind(key_id).bind(actor.tenant_id).bind(id).bind(family_id).bind(name).bind(&prefix).bind(hash.as_slice()).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "application.api_key.created",
        "application_api_key",
        key_id,
        json!({"applicationId":id,"prefix":prefix}),
    )
    .await?;
    tx.commit().await?;
    let mut response = load_api_key(&state, actor.tenant_id, key_id).await?;
    response.secret = Some(secret);
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/api-keys/{key_id}/rotate", params(("id" = Uuid, Path), ("key_id" = Uuid, Path)))]
pub async fn rotate_api_key(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, key_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ApiKeyResponse>> {
    actor.require("application:manage_key")?;
    require_application_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    let old=sqlx::query("SELECT family_id,name FROM application_api_keys WHERE id=? AND application_id=? AND tenant_id=? AND status='active' FOR UPDATE").bind(key_id).bind(id).bind(actor.tenant_id).fetch_optional(&mut *tx).await?.ok_or_else(|| AppError::not_found("API key"))?;
    let family: Uuid = old.try_get("family_id")?;
    let name: String = old.try_get("name")?;
    let (new_id, secret, prefix, hash) = generate_api_key();
    let changed = sqlx::query("UPDATE application_api_keys SET status='revoked',revoked_at=CURRENT_TIMESTAMP(6) WHERE id=? AND status='active'").bind(key_id).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "API_KEY_ROTATION_CONFLICT",
            "API key was already rotated or revoked",
        ));
    }
    sqlx::query("INSERT INTO application_api_keys(id,tenant_id,application_id,family_id,name,key_prefix,secret_hash,created_by) VALUES(?,?,?,?,?,?,?,?)")
        .bind(new_id).bind(actor.tenant_id).bind(id).bind(family).bind(name).bind(&prefix).bind(hash.as_slice()).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "application.api_key.rotated",
        "application_api_key",
        new_id,
        json!({"applicationId":id,"previousKeyId":key_id,"prefix":prefix}),
    )
    .await?;
    tx.commit().await?;
    let mut response = load_api_key(&state, actor.tenant_id, new_id).await?;
    response.secret = Some(secret);
    Ok(Json(response))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/api-keys/{key_id}/revoke", params(("id" = Uuid, Path), ("key_id" = Uuid, Path)))]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, key_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    actor.require("application:manage_key")?;
    require_application_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE application_api_keys SET status='revoked',revoked_at=CURRENT_TIMESTAMP(6) WHERE id=? AND application_id=? AND tenant_id=? AND status='active'").bind(key_id).bind(id).bind(actor.tenant_id).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::not_found("API key"));
    }
    audit(
        &mut tx,
        &actor,
        "application.api_key.revoked",
        "application_api_key",
        key_id,
        json!({"applicationId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/applications/{id}/webhooks", params(("id" = Uuid, Path)))]
pub async fn list_webhooks(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<WebhookResponse>>> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    let rows=sqlx::query("SELECT id,name,public_id,status,version FROM application_webhooks WHERE tenant_id=? AND application_id=? ORDER BY created_at DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(webhook_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/webhooks", request_body = CreateWebhookRequest, params(("id" = Uuid, Path)))]
pub async fn create_webhook(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateWebhookRequest>,
) -> AppResult<(StatusCode, Json<WebhookResponse>)> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    let name = validate_name(&input.name, 160)?;
    let webhook_id = Uuid::now_v7();
    let public_id = random_token(18);
    let secret = random_token(32);
    let stored =
        store_webhook_secret(&state, actor.tenant_id, webhook_id, secret.as_bytes()).await?;
    let mut tx = state.pool.begin().await?;
    let persisted: AppResult<()> = async {
        sqlx::query("INSERT INTO application_webhooks(id,tenant_id,application_id,name,public_id,secret_provider,secret_ref,secret_provider_version,secret_algorithm,secret_key_id,secret_nonce,secret_ciphertext,created_by) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(webhook_id).bind(actor.tenant_id).bind(id).bind(name).bind(&public_id)
            .bind(&stored.provider).bind(&stored.secret_ref).bind(&stored.provider_version)
            .bind(&stored.algorithm).bind(&stored.key_id).bind(&stored.nonce).bind(&stored.ciphertext)
            .bind(actor.user_id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            &actor,
            "application.webhook.created",
            "application_webhook",
            webhook_id,
            json!({"applicationId":id,"publicId":public_id}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(error) = persisted {
        cleanup_webhook_secret(&state, &stored).await;
        return Err(error);
    }
    let mut response = load_webhook(&state, actor.tenant_id, webhook_id).await?;
    response.secret = Some(secret);
    Ok((StatusCode::CREATED, Json(response)))
}

struct StoredWebhookSecret {
    provider: String,
    secret_ref: Option<String>,
    provider_version: Option<String>,
    algorithm: Option<String>,
    key_id: Option<String>,
    nonce: Option<Vec<u8>>,
    ciphertext: Option<Vec<u8>>,
}

async fn store_webhook_secret(
    state: &AppState,
    tenant_id: Uuid,
    webhook_id: Uuid,
    value: &[u8],
) -> AppResult<StoredWebhookSecret> {
    let plaintext = PlainSecret::new(value.to_vec());
    if let Some(provider) = &state.secret_provider {
        let secret_ref = format!("tenants/{tenant_id}/webhooks/{webhook_id}");
        let version = provider.write(&secret_ref, &plaintext).await.map_err(|_| {
            AppError::service_unavailable(
                "WEBHOOK_SECRET_PROVIDER_UNAVAILABLE",
                "Webhook Secret provider cannot store the secret",
            )
        })?;
        return Ok(StoredWebhookSecret {
            provider: "vault_kv_v2".into(),
            secret_ref: Some(secret_ref),
            provider_version: Some(version.to_string()),
            algorithm: None,
            key_id: None,
            nonce: None,
            ciphertext: None,
        });
    }
    let keyring = state.credential_keyring.as_ref().ok_or_else(|| {
        AppError::service_unavailable(
            "CREDENTIAL_STORE_UNAVAILABLE",
            "Secret encryption is unavailable",
        )
    })?;
    let aad = format!("{tenant_id}/{webhook_id}/webhook/1");
    let encrypted = keyring
        .encrypt(&plaintext, aad.as_bytes())
        .map_err(AppError::internal)?;
    Ok(StoredWebhookSecret {
        provider: "local_encrypted".into(),
        secret_ref: None,
        provider_version: None,
        algorithm: Some(encrypted.algorithm.into()),
        key_id: Some(encrypted.key_id),
        nonce: Some(encrypted.nonce.to_vec()),
        ciphertext: Some(encrypted.ciphertext),
    })
}

async fn cleanup_webhook_secret(state: &AppState, stored: &StoredWebhookSecret) {
    let (Some(provider), Some(secret_ref), Some(version)) = (
        state.secret_provider.as_ref(),
        stored.secret_ref.as_deref(),
        stored
            .provider_version
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok()),
    ) else {
        return;
    };
    let _ = provider.destroy(secret_ref, version).await;
}

#[utoipa::path(patch, path = "/api/v1/applications/{id}/webhooks/{webhook_id}", request_body = UpdateWebhookRequest)]
pub async fn update_webhook(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, webhook_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateWebhookRequest>,
) -> AppResult<Json<WebhookResponse>> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_WEBHOOK_STATUS",
            "Webhook status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let changed = sqlx::query("UPDATE application_webhooks SET name=?,status=?,version=version+1 WHERE id=? AND application_id=? AND tenant_id=? AND version=?")
        .bind(validate_name(&input.name,160)?).bind(&input.status).bind(webhook_id).bind(id).bind(actor.tenant_id).bind(input.version)
        .execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "WEBHOOK_VERSION_CONFLICT",
            "Webhook changed on the server",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "application.webhook.updated",
        "application_webhook",
        webhook_id,
        json!({"applicationId":id,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        load_webhook(&state, actor.tenant_id, webhook_id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/applications/{id}/schedules", params(("id" = Uuid, Path)))]
pub async fn list_schedules(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ScheduleResponse>>> {
    actor.require("application:view")?;
    require_application_access(&state, &actor, id, false).await?;
    let rows=sqlx::query("SELECT id,name,cron_expression,timezone,input_json,misfire_policy,status,version FROM application_schedules WHERE tenant_id=? AND application_id=? ORDER BY created_at DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(schedule_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/applications/{id}/schedules", request_body = CreateScheduleRequest, params(("id" = Uuid, Path)))]
pub async fn create_schedule(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateScheduleRequest>,
) -> AppResult<(StatusCode, Json<ScheduleResponse>)> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    validate_schedule(&input.cron_expression, &input.timezone)?;
    validate_misfire_policy(&input.misfire_policy)?;
    let schedule_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    let application_status: Option<String> =
        sqlx::query_scalar("SELECT status FROM applications WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(application_status) = application_status else {
        return Err(AppError::not_found("Application"));
    };
    let schedule_status = if application_status == "active" {
        "active"
    } else {
        "disabled"
    };
    sqlx::query("INSERT INTO application_schedules(id,tenant_id,application_id,name,cron_expression,timezone,input_json,misfire_policy,status,created_by) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(schedule_id).bind(actor.tenant_id).bind(id).bind(validate_name(&input.name,160)?).bind(&input.cron_expression).bind(&input.timezone).bind(&input.input).bind(&input.misfire_policy).bind(schedule_status).bind(actor.user_id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "application.schedule.created", "application_schedule", schedule_id, json!({"applicationId":id,"cronExpression":input.cron_expression,"timezone":input.timezone,"misfirePolicy":input.misfire_policy})).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_schedule(&state, actor.tenant_id, schedule_id).await?),
    ))
}

#[utoipa::path(patch, path = "/api/v1/applications/{id}/schedules/{schedule_id}", request_body = UpdateScheduleRequest, params(("id" = Uuid, Path), ("schedule_id" = Uuid, Path)))]
pub async fn update_schedule(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, schedule_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateScheduleRequest>,
) -> AppResult<Json<ScheduleResponse>> {
    actor.require("application:manage")?;
    require_application_access(&state, &actor, id, true).await?;
    validate_schedule(&input.cron_expression, &input.timezone)?;
    validate_misfire_policy(&input.misfire_policy)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_SCHEDULE_STATUS",
            "Schedule status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE application_schedules SET name=?,cron_expression=?,timezone=?,input_json=?,misfire_policy=?,status=?,version=version+1 WHERE id=? AND application_id=? AND tenant_id=? AND version=?")
        .bind(validate_name(&input.name,160)?).bind(&input.cron_expression).bind(&input.timezone).bind(&input.input).bind(&input.misfire_policy).bind(&input.status).bind(schedule_id).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "SCHEDULE_VERSION_CONFLICT",
            "Schedule changed on the server",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "application.schedule.updated",
        "application_schedule",
        schedule_id,
        json!({"applicationId":id,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        load_schedule(&state, actor.tenant_id, schedule_id).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/applications/{id}/sessions", params(("id" = Uuid, Path)))]
pub async fn list_sessions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<SessionResponse>>> {
    actor.require("application:view")?;
    require_application_access(&state, &actor, id, false).await?;
    let rows = sqlx::query(SESSION_SELECT)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(session_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/sessions/{id}/upgrade", request_body = UpgradeSessionRequest, params(("id" = Uuid, Path)))]
pub async fn upgrade_session(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpgradeSessionRequest>,
) -> AppResult<Json<SessionResponse>> {
    actor.require("application:manage")?;
    let row=sqlx::query("SELECT application_id,workflow_version_id,version_policy FROM application_sessions WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Session"))?;
    let application_id: Uuid = row.try_get("application_id")?;
    require_application_access(&state, &actor, application_id, true).await?;
    let policy: String = row.try_get("version_policy")?;
    if policy != "manual_upgrade" {
        return Err(AppError::conflict(
            "SESSION_POLICY_NOT_UPGRADABLE",
            "Session is not configured for manual upgrade",
        ));
    }
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM applications a JOIN workflow_versions wv ON wv.workflow_id=a.workflow_id AND wv.tenant_id=a.tenant_id WHERE a.id=? AND a.tenant_id=? AND wv.id=?)").bind(application_id).bind(actor.tenant_id).bind(input.workflow_version_id).fetch_one(&state.pool).await?;
    if !valid {
        return Err(AppError::unprocessable(
            "INVALID_WORKFLOW_VERSION",
            "Workflow Version does not belong to the Application",
        ));
    }
    let old: Option<Uuid> = row.try_get("workflow_version_id")?;
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE application_sessions SET workflow_version_id=?,version=version+1 WHERE id=? AND tenant_id=? AND version=? AND status='active'").bind(input.workflow_version_id).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "SESSION_VERSION_CONFLICT",
            "Session changed on the server",
        ));
    }
    sqlx::query("INSERT INTO application_session_version_history(id,tenant_id,session_id,from_workflow_version_id,to_workflow_version_id,changed_by) VALUES(?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(old).bind(input.workflow_version_id).bind(actor.user_id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "application.session.upgraded", "application_session", id, json!({"applicationId":application_id,"fromWorkflowVersionId":old,"toWorkflowVersionId":input.workflow_version_id})).await?;
    tx.commit().await?;
    Ok(Json(load_session(&state, actor.tenant_id, id).await?))
}

const APPLICATION_SELECT_LIST: &str = "SELECT a.id,a.workflow_id,w.name workflow_name,a.name,a.slug,a.description,a.visibility,a.status,a.owner_department_id,h.deployment_id active_deployment_id,wv.version_number active_version_number,a.version,a.updated_at FROM applications a JOIN workflows w ON w.id=a.workflow_id LEFT JOIN application_deployment_heads h ON h.application_id=a.id LEFT JOIN application_deployments ad ON ad.id=h.deployment_id LEFT JOIN workflow_versions wv ON wv.id=ad.workflow_version_id WHERE a.tenant_id=? AND (?='' OR a.status=?) AND (?='%%' OR a.name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?";
const APPLICATION_SELECT_VISIBLE: &str = "SELECT a.id,a.workflow_id,w.name workflow_name,a.name,a.slug,a.description,a.visibility,a.status,a.owner_department_id,h.deployment_id active_deployment_id,wv.version_number active_version_number,a.version,a.updated_at FROM applications a JOIN workflows w ON w.id=a.workflow_id LEFT JOIN application_deployment_heads h ON h.application_id=a.id LEFT JOIN application_deployments ad ON ad.id=h.deployment_id LEFT JOIN workflow_versions wv ON wv.id=ad.workflow_version_id WHERE a.tenant_id=? AND (a.visibility='company' OR a.owner_user_id=? OR (a.visibility='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=a.tenant_id AND ((dc.ancestor_id=a.owner_department_id AND dc.descendant_id=?) OR (dc.ancestor_id=? AND dc.descendant_id=a.owner_department_id)))) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=a.tenant_id AND ur.user_id=? AND dc.descendant_id=a.owner_department_id)) AND (?='' OR a.status=?) AND (?='%%' OR a.name LIKE ?) ORDER BY a.updated_at DESC LIMIT ? OFFSET ?";
const DEPLOYMENT_SELECT: &str = "SELECT ad.id,ad.application_id,ad.workflow_version_id,wv.version_number,ad.environment_id,e.name environment_name,ad.sequence_number,ad.input_schema_json,ad.output_schema_json,ad.output_expression,ad.session_version_policy,ad.status,ad.created_at FROM application_deployments ad JOIN workflow_versions wv ON wv.id=ad.workflow_version_id JOIN workflow_environments e ON e.id=ad.environment_id WHERE ad.tenant_id=? AND ad.application_id=? ORDER BY ad.sequence_number DESC";
const SESSION_SELECT: &str = "SELECT id,application_id,application_deployment_id,workflow_version_id,version_policy,external_user_id,title,status,version,updated_at FROM application_sessions WHERE tenant_id=? AND application_id=? ORDER BY updated_at DESC";

async fn require_application_access(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    write: bool,
) -> AppResult<()> {
    let row=sqlx::query("SELECT owner_user_id,owner_department_id,visibility FROM applications WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Application"))?;
    if actor.company_admin {
        return Ok(());
    }
    let owner: Uuid = row.try_get("owner_user_id")?;
    let department: Uuid = row.try_get("owner_department_id")?;
    let visibility: String = row.try_get("visibility")?;
    if owner == actor.user_id
        || (!write && visibility == "company")
        || (!write && visibility == "department" && department == actor.department_id)
    {
        Ok(())
    } else {
        require_department_scope(&state.pool, actor, department).await
    }
}
async fn load_application(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<ApplicationResponse> {
    require_application_access(state, actor, id, false).await?;
    let row=sqlx::query("SELECT a.id,a.workflow_id,w.name workflow_name,a.name,a.slug,a.description,a.visibility,a.status,a.owner_department_id,h.deployment_id active_deployment_id,wv.version_number active_version_number,a.version,a.updated_at FROM applications a JOIN workflows w ON w.id=a.workflow_id LEFT JOIN application_deployment_heads h ON h.application_id=a.id LEFT JOIN application_deployments ad ON ad.id=h.deployment_id LEFT JOIN workflow_versions wv ON wv.id=ad.workflow_version_id WHERE a.id=? AND a.tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    application_from_row(row)
}
fn application_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<ApplicationResponse> {
    Ok(ApplicationResponse {
        id: row.try_get("id")?,
        workflow_id: row.try_get("workflow_id")?,
        workflow_name: row.try_get("workflow_name")?,
        name: row.try_get("name")?,
        slug: row.try_get("slug")?,
        description: row.try_get("description")?,
        visibility: row.try_get("visibility")?,
        status: row.try_get("status")?,
        owner_department_id: row.try_get("owner_department_id")?,
        active_deployment_id: row.try_get("active_deployment_id")?,
        active_version_number: row.try_get("active_version_number")?,
        version: row.try_get("version")?,
        updated_at: row.try_get("updated_at")?,
    })
}
fn deployment_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<ApplicationDeploymentResponse> {
    Ok(ApplicationDeploymentResponse {
        id: row.try_get("id")?,
        application_id: row.try_get("application_id")?,
        workflow_version_id: row.try_get("workflow_version_id")?,
        workflow_version_number: row.try_get("version_number")?,
        environment_id: row.try_get("environment_id")?,
        environment_name: row.try_get("environment_name")?,
        sequence_number: row.try_get("sequence_number")?,
        input_schema: row.try_get("input_schema_json")?,
        output_schema: row.try_get("output_schema_json")?,
        output_expression: row.try_get("output_expression")?,
        session_version_policy: row.try_get("session_version_policy")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
    })
}
async fn load_deployment(
    state: &AppState,
    tenant_id: Uuid,
    id: Uuid,
) -> AppResult<ApplicationDeploymentResponse> {
    let row = sqlx::query("SELECT ad.id,ad.application_id,ad.workflow_version_id,wv.version_number,ad.environment_id,e.name environment_name,ad.sequence_number,ad.input_schema_json,ad.output_schema_json,ad.output_expression,ad.session_version_policy,ad.status,ad.created_at FROM application_deployments ad JOIN workflow_versions wv ON wv.id=ad.workflow_version_id JOIN workflow_environments e ON e.id=ad.environment_id WHERE ad.tenant_id=? AND ad.id=?")
        .bind(tenant_id)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    deployment_from_row(row)
}
fn api_key_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<ApiKeyResponse> {
    Ok(ApiKeyResponse {
        id: row.try_get("id")?,
        family_id: row.try_get("family_id")?,
        name: row.try_get("name")?,
        prefix: row.try_get("key_prefix")?,
        status: row.try_get("status")?,
        secret: None,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
    })
}
async fn load_api_key(state: &AppState, tenant_id: Uuid, id: Uuid) -> AppResult<ApiKeyResponse> {
    let row=sqlx::query("SELECT id,family_id,name,key_prefix,status,created_at,last_used_at FROM application_api_keys WHERE tenant_id=? AND id=?").bind(tenant_id).bind(id).fetch_one(&state.pool).await?;
    api_key_from_row(row)
}
fn webhook_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<WebhookResponse> {
    let public_id: String = row.try_get("public_id")?;
    Ok(WebhookResponse {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        path: format!("/gateway/v1/webhooks/{public_id}"),
        public_id,
        status: row.try_get("status")?,
        secret: None,
        version: row.try_get("version")?,
    })
}
async fn load_webhook(state: &AppState, tenant_id: Uuid, id: Uuid) -> AppResult<WebhookResponse> {
    let row=sqlx::query("SELECT id,name,public_id,status,version FROM application_webhooks WHERE tenant_id=? AND id=?").bind(tenant_id).bind(id).fetch_one(&state.pool).await?;
    webhook_from_row(row)
}
fn schedule_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<ScheduleResponse> {
    Ok(ScheduleResponse {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        cron_expression: row.try_get("cron_expression")?,
        timezone: row.try_get("timezone")?,
        input: row.try_get("input_json")?,
        misfire_policy: row.try_get("misfire_policy")?,
        status: row.try_get("status")?,
        version: row.try_get("version")?,
    })
}
async fn load_schedule(state: &AppState, tenant_id: Uuid, id: Uuid) -> AppResult<ScheduleResponse> {
    let row=sqlx::query("SELECT id,name,cron_expression,timezone,input_json,misfire_policy,status,version FROM application_schedules WHERE tenant_id=? AND id=?").bind(tenant_id).bind(id).fetch_one(&state.pool).await?;
    schedule_from_row(row)
}
fn session_from_row(row: sqlx::mysql::MySqlRow) -> AppResult<SessionResponse> {
    Ok(SessionResponse {
        id: row.try_get("id")?,
        application_id: row.try_get("application_id")?,
        application_deployment_id: row.try_get("application_deployment_id")?,
        workflow_version_id: row.try_get("workflow_version_id")?,
        version_policy: row.try_get("version_policy")?,
        external_user_id: row.try_get("external_user_id")?,
        title: row.try_get("title")?,
        status: row.try_get("status")?,
        version: row.try_get("version")?,
        updated_at: row.try_get("updated_at")?,
    })
}
async fn load_session(state: &AppState, tenant_id: Uuid, id: Uuid) -> AppResult<SessionResponse> {
    let row=sqlx::query("SELECT id,application_id,application_deployment_id,workflow_version_id,version_policy,external_user_id,title,status,version,updated_at FROM application_sessions WHERE tenant_id=? AND id=?").bind(tenant_id).bind(id).fetch_one(&state.pool).await?;
    session_from_row(row)
}
fn generate_api_key() -> (Uuid, String, String, [u8; 32]) {
    let id = Uuid::now_v7();
    let secret = random_token(32);
    let prefix = format!("axk_{}", id.simple());
    let value = format!("{prefix}_{secret}");
    let hash: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    (id, value, prefix, hash)
}
fn random_token(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}
fn validate_slug(value: &str) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(AppError::bad_request(
            "INVALID_APPLICATION_SLUG",
            "Slug must use lowercase letters, digits and hyphens",
        ));
    }
    Ok(value)
}
fn validate_visibility(value: &str) -> AppResult<()> {
    if matches!(value, "private" | "department" | "company") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_VISIBILITY",
            "Visibility is invalid",
        ))
    }
}
fn validate_version_policy(value: &str) -> AppResult<()> {
    if matches!(value, "pinned" | "follow_deployment" | "manual_upgrade") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_SESSION_VERSION_POLICY",
            "Session version policy is invalid",
        ))
    }
}
fn validate_schema(value: &Value) -> AppResult<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_JSON_SCHEMA",
            "JSON Schema must be an object",
        ))
    }
}
fn validate_schedule(cron: &str, timezone: &str) -> AppResult<()> {
    if cron.split_whitespace().count() != 5 {
        return Err(AppError::bad_request(
            "INVALID_CRON",
            "Cron expression must contain five fields",
        ));
    }
    if timezone.trim().is_empty() || timezone.len() > 64 {
        return Err(AppError::bad_request(
            "INVALID_TIMEZONE",
            "Timezone is invalid",
        ));
    }
    Ok(())
}

fn default_misfire_policy() -> String {
    "fire_once".into()
}

fn validate_misfire_policy(policy: &str) -> AppResult<()> {
    if matches!(policy, "skip" | "fire_once") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_MISFIRE_POLICY",
            "Misfire policy must be 'skip' or 'fire_once'",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        generate_api_key, validate_misfire_policy, validate_schedule, validate_slug,
        validate_version_policy,
    };
    use sha2::{Digest, Sha256};
    #[test]
    fn validates_public_configuration() {
        assert!(validate_slug("support-agent").is_ok());
        assert!(validate_slug("Bad Slug").is_err());
        assert!(validate_version_policy("pinned").is_ok());
        assert!(validate_schedule("0 9 * * 1", "Asia/Shanghai").is_ok());
        assert!(validate_schedule("bad", "UTC").is_err());
        assert!(validate_misfire_policy("fire_once").is_ok());
        assert!(validate_misfire_policy("skip").is_ok());
        assert!(validate_misfire_policy("catch_up_all").is_err());
    }

    #[test]
    fn api_key_is_prefixed_and_only_its_hash_is_persistable() {
        let (id, value, prefix, hash) = generate_api_key();
        assert_eq!(prefix, format!("axk_{}", id.simple()));
        let secret = value.strip_prefix(&format!("{prefix}_")).unwrap();
        assert!(!secret.is_empty());
        assert_eq!(hash.as_slice(), Sha256::digest(value.as_bytes()).as_slice());
    }
}
