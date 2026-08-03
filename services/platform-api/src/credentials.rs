use agentx_api_types::PageResponse;
use agentx_infrastructure::credential::PlainSecret;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, require_department_scope, validate_name},
    error::{AppError, AppResult},
    grants::require_resource_visible,
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialResponse {
    pub id: Uuid,
    pub name: String,
    pub credential_type: String,
    pub storage_mode: String,
    pub masked_hint: String,
    pub status: String,
    pub current_secret_version: u64,
    pub owner_department_id: Uuid,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type CredentialPage = PageResponse<CredentialResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialRequest {
    pub name: String,
    pub credential_type: String,
    pub secret: Value,
    pub owner_department_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateCredentialRequest {
    pub secret: Value,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCredentialRequest {
    pub name: String,
    pub status: String,
    pub version: u64,
}

pub struct ResolvedCredential {
    pub credential_type: String,
    pub secret: PlainSecret,
}

pub async fn require_reference(
    state: &AppState,
    actor: &AuthActor,
    id: Option<Uuid>,
) -> AppResult<()> {
    let Some(id) = id else {
        return Ok(());
    };
    actor.require("credential:view")?;
    require_resource_visible(state, actor, "credential", id).await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM credentials WHERE tenant_id=? AND id=? AND status='active')",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if active {
        Ok(())
    } else {
        Err(AppError::unprocessable(
            "CREDENTIAL_UNAVAILABLE",
            "Credential is disabled or unavailable",
        ))
    }
}

#[utoipa::path(get, path = "/api/v1/credentials")]
pub async fn list_credentials(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<CredentialListQuery>,
) -> AppResult<Json<CredentialPage>> {
    actor.require("credential:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let rows = if actor.company_admin {
        sqlx::query("SELECT id,name,credential_type,storage_mode,masked_hint,status,current_secret_version,owner_department_id,version,updated_at,COUNT(*) OVER() total_count FROM credentials WHERE tenant_id=? AND (?='' OR status=?) AND (?='%%' OR name LIKE ?) ORDER BY updated_at DESC LIMIT ? OFFSET ?").bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(offset).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT c.id,c.name,c.credential_type,c.storage_mode,c.masked_hint,c.status,c.current_secret_version,c.owner_department_id,c.version,c.updated_at,COUNT(*) OVER() total_count FROM credentials c WHERE c.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=c.tenant_id AND dc.descendant_id=c.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=c.tenant_id AND rg.subject_type='department' AND rg.resource_type='credential' AND rg.resource_id=c.id AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)) AND (?='' OR c.status=?) AND (?='%%' OR c.name LIKE ?) ORDER BY c.updated_at DESC LIMIT ? OFFSET ?").bind(actor.tenant_id).bind(actor.user_id).bind(actor.department_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(offset).fetch_all(&state.pool).await?
    };
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(credential_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(post,path="/api/v1/credentials",request_body=CreateCredentialRequest)]
pub async fn create_credential(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateCredentialRequest>,
) -> AppResult<(StatusCode, Json<CredentialResponse>)> {
    actor.require("credential:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    let name = validate_name(&input.name, 160)?;
    validate_type(&input.credential_type)?;
    let bytes = secret_bytes(&input.secret)?;
    let masked = mask_hint(&input.secret);
    let id = Uuid::now_v7();
    let secret_id = Uuid::now_v7();
    let encrypted = encrypt(&state, actor.tenant_id, id, 1, &bytes)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(&name).bind(&input.credential_type).bind(&masked).bind(input.owner_department_id).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,?,?,?,?,?,?)").bind(secret_id).bind(actor.tenant_id).bind(id).bind(1_u64).bind(encrypted.algorithm).bind(encrypted.key_id).bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "credential.created",
        "credential",
        id,
        json!({"name":name,"credentialType":input.credential_type}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get,path="/api/v1/credentials/{id}",params(("id"=Uuid,Path)))]
pub async fn get_credential(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CredentialResponse>> {
    actor.require("credential:view")?;
    require_resource_visible(&state, &actor, "credential", id).await?;
    Ok(Json(load(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(patch,path="/api/v1/credentials/{id}",request_body=UpdateCredentialRequest,params(("id"=Uuid,Path)))]
pub async fn update_credential(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateCredentialRequest>,
) -> AppResult<Json<CredentialResponse>> {
    actor.require("credential:manage")?;
    require_resource_visible(&state, &actor, "credential", id).await?;
    let name = validate_name(&input.name, 160)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Credential status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE credentials SET name=?,status=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?").bind(name).bind(&input.status).bind(actor.tenant_id).bind(id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "CREDENTIAL_VERSION_CONFLICT",
            "Credential changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "credential.updated",
        "credential",
        id,
        json!({"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/credentials/{id}/rotate",request_body=RotateCredentialRequest,params(("id"=Uuid,Path)))]
pub async fn rotate_credential(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<RotateCredentialRequest>,
) -> AppResult<Json<CredentialResponse>> {
    actor.require("credential:manage")?;
    require_resource_visible(&state, &actor, "credential", id).await?;
    let bytes = secret_bytes(&input.secret)?;
    let masked = mask_hint(&input.secret);
    let mut tx = state.pool.begin().await?;
    let row=sqlx::query("SELECT current_secret_version,version FROM credentials WHERE tenant_id=? AND id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::not_found("Credential"))?;
    let entity_version: u64 = row.try_get("version")?;
    if entity_version != input.version {
        return Err(AppError::conflict(
            "CREDENTIAL_VERSION_CONFLICT",
            "Credential changed",
        ));
    }
    let next: u64 = row.try_get::<u64, _>("current_secret_version")? + 1;
    let encrypted = encrypt(&state, actor.tenant_id, id, next, &bytes)?;
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(next).bind(encrypted.algorithm).bind(encrypted.key_id).bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE credentials SET current_secret_version=?,masked_hint=?,version=version+1 WHERE tenant_id=? AND id=?").bind(next).bind(masked).bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "credential.rotated",
        "credential",
        id,
        json!({"secretVersion":next}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load(&state, actor.tenant_id, id).await?))
}

pub async fn resolve(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ResolvedCredential> {
    let r=sqlx::query("SELECT c.credential_type,c.current_secret_version,s.key_id,s.nonce,s.ciphertext FROM credentials c JOIN credential_secret_versions s ON s.credential_id=c.id AND s.version_number=c.current_secret_version WHERE c.tenant_id=? AND c.id=? AND c.status='active'").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Credential"))?;
    let version: u64 = r.try_get("current_secret_version")?;
    let aad = aad(tenant, id, version);
    let keyring = state.credential_keyring.as_ref().ok_or_else(|| {
        AppError::service_unavailable(
            "CREDENTIAL_STORE_UNAVAILABLE",
            "Credential keyring is not configured",
        )
    })?;
    let secret = keyring
        .decrypt(
            &r.try_get::<String, _>("key_id")?,
            &r.try_get::<Vec<u8>, _>("nonce")?,
            &r.try_get::<Vec<u8>, _>("ciphertext")?,
            aad.as_bytes(),
        )
        .map_err(|_| {
            AppError::service_unavailable(
                "CREDENTIAL_DECRYPTION_FAILED",
                "Credential cannot be decrypted",
            )
        })?;
    Ok(ResolvedCredential {
        credential_type: r.try_get("credential_type")?,
        secret,
    })
}

fn encrypt(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
    version: u64,
    secret: &PlainSecret,
) -> AppResult<agentx_infrastructure::credential::EncryptedSecret> {
    state
        .credential_keyring
        .as_ref()
        .ok_or_else(|| {
            AppError::service_unavailable(
                "CREDENTIAL_STORE_UNAVAILABLE",
                "Credential keyring is not configured",
            )
        })?
        .encrypt(secret, aad(tenant, id, version).as_bytes())
        .map_err(AppError::internal)
}
fn aad(tenant: Uuid, id: Uuid, version: u64) -> String {
    format!("{tenant}/{id}/{version}")
}
fn secret_bytes(value: &Value) -> AppResult<PlainSecret> {
    if value.is_null() {
        return Err(AppError::bad_request(
            "SECRET_REQUIRED",
            "Credential secret is required",
        ));
    }
    let bytes = serde_json::to_vec(value).map_err(AppError::internal)?;
    if bytes.len() > 64 * 1024 {
        return Err(AppError::bad_request(
            "SECRET_TOO_LARGE",
            "Credential secret must not exceed 64 KiB",
        ));
    }
    Ok(PlainSecret::new(bytes))
}
fn mask_hint(value: &Value) -> String {
    let raw = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| "JSON".to_owned());
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= 4 {
        "••••".to_owned()
    } else {
        format!(
            "••••{}",
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    }
}
fn validate_type(value: &str) -> AppResult<()> {
    if matches!(value, "api_key" | "bearer" | "basic" | "custom_json") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_CREDENTIAL_TYPE",
            "Credential type is invalid",
        ))
    }
}
async fn load(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<CredentialResponse> {
    let row=sqlx::query("SELECT id,name,credential_type,storage_mode,masked_hint,status,current_secret_version,owner_department_id,version,updated_at FROM credentials WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Credential"))?;
    credential_from_row(row).map_err(Into::into)
}
fn credential_from_row(r: sqlx::mysql::MySqlRow) -> Result<CredentialResponse, sqlx::Error> {
    Ok(CredentialResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        credential_type: r.try_get("credential_type")?,
        storage_mode: r.try_get("storage_mode")?,
        masked_hint: r.try_get("masked_hint")?,
        status: r.try_get("status")?,
        current_secret_version: r.try_get("current_secret_version")?,
        owner_department_id: r.try_get("owner_department_id")?,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
