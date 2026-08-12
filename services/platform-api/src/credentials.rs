use agentx_api_types::PageResponse;
use agentx_infrastructure::credential::PlainSecret;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerResolveRequest {
    secret_ref: String,
    version: Option<u64>,
    tenant_id: Uuid,
    credential_id: Uuid,
    credential_version: u64,
    execution_id: Uuid,
    node_execution_id: Uuid,
    attempt_id: Uuid,
    lease_token: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    deadline: OffsetDateTime,
    handle: Option<String>,
    handle_id: Option<Uuid>,
    #[serde(default = "default_consume_handle")]
    consume_handle: bool,
}

const fn default_consume_handle() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerResolveResponse {
    secret_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookBrokerResolveRequest {
    secret_ref: String,
    version: u64,
    tenant_id: Uuid,
    webhook_id: Uuid,
}

pub async fn broker_resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<BrokerResolveRequest>,
) -> AppResult<Json<BrokerResolveResponse>> {
    require_broker_token(&headers)?;
    Ok(Json(resolve_broker_credential(&state, input).await?))
}

async fn resolve_broker_credential(
    state: &AppState,
    input: BrokerResolveRequest,
) -> AppResult<BrokerResolveResponse> {
    let (tenant_id, credential_id) = parse_credential_secret_ref(&input.secret_ref)?;
    if tenant_id != input.tenant_id || credential_id != input.credential_id {
        return Err(AppError::forbidden(
            "Credential scope does not match the Secret reference",
        ));
    }
    let now = OffsetDateTime::now_utc();
    if input.deadline <= now || input.deadline > now + time::Duration::minutes(15) {
        return Err(AppError::forbidden("Credential Handle deadline is invalid"));
    }
    enum HandleSelector {
        TokenHash(String),
        Id(Uuid),
    }
    let (selector, selector_value) = match (&input.handle, input.handle_id) {
        (Some(handle), None) => (
            "h.token_hash=?",
            HandleSelector::TokenHash(format!("{:x}", Sha256::digest(handle.as_bytes()))),
        ),
        (None, Some(handle_id)) => ("h.id=?", HandleSelector::Id(handle_id)),
        _ => {
            return Err(AppError::bad_request(
                "INVALID_CREDENTIAL_HANDLE",
                "Exactly one Credential Handle selector is required",
            ));
        }
    };
    let mut transaction = state.pool.begin().await?;
    let lock_clause = if input.consume_handle {
        " FOR UPDATE"
    } else {
        ""
    };
    let query = format!(
        "SELECT h.id,h.resource_id,h.resource_version,h.expires_at,h.consumed_at,h.revoked_at,a.status attempt_status,a.deadline_at,n.status node_status,e.status execution_status,e.cancellation_requested_at,l.expires_at lease_expires,l.released_at,s.resource_snapshot_json FROM node_invocation_handles h JOIN node_attempts a ON a.id=h.attempt_id AND a.tenant_id=h.tenant_id JOIN node_executions n ON n.id=h.node_execution_id AND n.tenant_id=h.tenant_id JOIN workflow_executions e ON e.id=h.execution_id AND e.tenant_id=h.tenant_id JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id JOIN worker_leases l ON l.node_attempt_id=h.attempt_id AND l.lease_token=h.lease_token WHERE {selector} AND h.handle_kind='credential' AND h.tenant_id=? AND h.execution_id=? AND h.node_execution_id=? AND h.attempt_id=? AND h.lease_token=?{lock_clause}"
    );
    let scope_query = sqlx::query(&query);
    let scope_query = match selector_value {
        HandleSelector::TokenHash(value) => scope_query.bind(value),
        HandleSelector::Id(value) => scope_query.bind(value),
    };
    let scope = scope_query
        .bind(input.tenant_id)
        .bind(input.execution_id)
        .bind(input.node_execution_id)
        .bind(input.attempt_id)
        .bind(input.lease_token)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::forbidden("Credential Handle scope is invalid"))?;
    let handle_resource_id: Option<Uuid> = scope.try_get("resource_id")?;
    let handle_version: Option<u64> = scope.try_get("resource_version")?;
    let handle_expires: OffsetDateTime = scope.try_get("expires_at")?;
    let attempt_deadline: Option<OffsetDateTime> = scope.try_get("deadline_at")?;
    let lease_expires: OffsetDateTime = scope.try_get("lease_expires")?;
    let active_scope = handle_resource_id == Some(credential_id)
        && handle_version == Some(input.credential_version)
        && handle_expires > now
        && input.deadline <= handle_expires
        && attempt_deadline.is_none_or(|deadline| input.deadline <= deadline)
        && lease_expires > now
        && scope
            .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
            .is_none()
        && scope
            .try_get::<Option<OffsetDateTime>, _>("revoked_at")?
            .is_none()
        && scope
            .try_get::<Option<OffsetDateTime>, _>("released_at")?
            .is_none()
        && scope
            .try_get::<Option<OffsetDateTime>, _>("cancellation_requested_at")?
            .is_none()
        && scope.try_get::<String, _>("attempt_status")? == "running"
        && scope.try_get::<String, _>("node_status")? == "running"
        && scope.try_get::<String, _>("execution_status")? == "running";
    if !active_scope {
        return Err(AppError::forbidden(
            "Credential Handle is expired, consumed, revoked, or detached from its active Lease",
        ));
    }
    let snapshot: Value = scope.try_get("resource_snapshot_json")?;
    let snapshot_contains_credential = snapshot
        .get("resources")
        .and_then(Value::as_array)
        .is_some_and(|resources| {
            resources.iter().any(|resource| {
                resource
                    .get("reference")
                    .and_then(|reference| reference.get("resourceType"))
                    .and_then(Value::as_str)
                    == Some("credential")
                    && resource
                        .get("reference")
                        .and_then(|reference| reference.get("resourceId"))
                        .and_then(Value::as_str)
                        .is_some_and(|id| id == credential_id.to_string())
                    && resource
                        .get("snapshot")
                        .and_then(|value| value.get("secretVersion"))
                        .and_then(Value::as_u64)
                        == Some(input.credential_version)
            })
        });
    if !snapshot_contains_credential {
        return Err(AppError::forbidden(
            "Credential is not present in the Execution resource snapshot",
        ));
    }
    let provider = state.secret_provider.as_ref().ok_or_else(|| {
        AppError::service_unavailable(
            "CREDENTIAL_PROVIDER_UNAVAILABLE",
            "External credential provider is unavailable",
        )
    })?;
    let secret = provider
        .read(&input.secret_ref, input.version)
        .await
        .map_err(|_| {
            AppError::service_unavailable(
                "CREDENTIAL_PROVIDER_UNAVAILABLE",
                "External credential provider cannot resolve the secret",
            )
        })?;
    if input.consume_handle {
        let handle_id: Uuid = scope.try_get("id")?;
        let consumed = sqlx::query("UPDATE node_invocation_handles SET consumed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND consumed_at IS NULL AND revoked_at IS NULL")
            .bind(handle_id)
            .execute(&mut *transaction)
            .await?;
        if consumed.rows_affected() != 1 {
            return Err(AppError::forbidden(
                "Credential Handle has already been consumed",
            ));
        }
    }
    transaction.commit().await?;
    Ok(BrokerResolveResponse {
        secret_base64: STANDARD.encode(secret.expose()),
    })
}

pub async fn broker_resolve_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WebhookBrokerResolveRequest>,
) -> AppResult<Json<BrokerResolveResponse>> {
    require_broker_token(&headers)?;
    let parts = input.secret_ref.split('/').collect::<Vec<_>>();
    if parts.len() != 4
        || parts[0] != "tenants"
        || parts[2] != "webhooks"
        || Uuid::parse_str(parts[1]).ok() != Some(input.tenant_id)
        || Uuid::parse_str(parts[3]).ok() != Some(input.webhook_id)
    {
        return Err(AppError::forbidden(
            "Webhook scope does not match the Secret reference",
        ));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM application_webhooks WHERE tenant_id=? AND id=? AND status='active' AND secret_provider='vault_kv_v2' AND secret_ref=? AND secret_provider_version=?)")
        .bind(input.tenant_id).bind(input.webhook_id).bind(&input.secret_ref)
        .bind(input.version.to_string()).fetch_one(&state.pool).await?;
    if !exists {
        return Err(AppError::forbidden("Webhook Secret reference is inactive"));
    }
    let provider = state.secret_provider.as_ref().ok_or_else(|| {
        AppError::service_unavailable(
            "WEBHOOK_SECRET_PROVIDER_UNAVAILABLE",
            "Webhook Secret provider is unavailable",
        )
    })?;
    let secret = provider
        .read(&input.secret_ref, Some(input.version))
        .await
        .map_err(|_| {
            AppError::service_unavailable(
                "WEBHOOK_SECRET_PROVIDER_UNAVAILABLE",
                "Webhook Secret provider cannot resolve the secret",
            )
        })?;
    Ok(Json(BrokerResolveResponse {
        secret_base64: STANDARD.encode(secret.expose()),
    }))
}

fn require_broker_token(headers: &HeaderMap) -> AppResult<()> {
    let configured = std::env::var("AGENTX_CREDENTIAL_BROKER_TOKEN").map_err(|_| {
        AppError::service_unavailable(
            "CREDENTIAL_BROKER_UNAVAILABLE",
            "Credential broker is not configured",
        )
    })?;
    let provided = headers
        .get("X-Agentx-Credential-Broker-Token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if Sha256::digest(configured.as_bytes()) != Sha256::digest(provided.as_bytes()) {
        return Err(AppError::unauthorized(
            "INVALID_CREDENTIAL_BROKER_TOKEN",
            "Invalid credential broker token",
        ));
    }
    Ok(())
}

fn parse_credential_secret_ref(value: &str) -> AppResult<(Uuid, Uuid)> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "tenants" || parts[2] != "credentials" {
        return Err(AppError::bad_request(
            "INVALID_SECRET_REFERENCE",
            "Credential secret reference is invalid",
        ));
    }
    let tenant_id = Uuid::parse_str(parts[1]).map_err(|_| {
        AppError::bad_request("INVALID_SECRET_REFERENCE", "Credential tenant is invalid")
    })?;
    let credential_id = Uuid::parse_str(parts[3]).map_err(|_| {
        AppError::bad_request("INVALID_SECRET_REFERENCE", "Credential id is invalid")
    })?;
    Ok((tenant_id, credential_id))
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
    let stored = store_secret(&state, actor.tenant_id, id, 1, &bytes).await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,storage_mode,masked_hint,owner_department_id,created_by) VALUES(?,?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(&name).bind(&input.credential_type).bind(stored.storage_mode).bind(&masked).bind(input.owner_department_id).bind(actor.user_id).execute(&mut *tx).await?;
    insert_secret_version(
        &mut tx,
        secret_id,
        actor.tenant_id,
        id,
        1,
        actor.user_id,
        &stored,
    )
    .await?;
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
    let row = sqlx::query(
        "SELECT current_secret_version,version FROM credentials WHERE tenant_id=? AND id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Credential"))?;
    let entity_version: u64 = row.try_get("version")?;
    if entity_version != input.version {
        return Err(AppError::conflict(
            "CREDENTIAL_VERSION_CONFLICT",
            "Credential changed",
        ));
    }
    let next: u64 = row.try_get::<u64, _>("current_secret_version")? + 1;
    let stored = store_secret(&state, actor.tenant_id, id, next, &bytes).await?;
    let mut tx = state.pool.begin().await?;
    let locked=sqlx::query("SELECT current_secret_version,version FROM credentials WHERE tenant_id=? AND id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::not_found("Credential"))?;
    if locked.try_get::<u64, _>("version")? != input.version
        || locked.try_get::<u64, _>("current_secret_version")? + 1 != next
    {
        tx.rollback().await?;
        destroy_stored_secret(&state, &stored).await;
        return Err(AppError::conflict(
            "CREDENTIAL_VERSION_CONFLICT",
            "Credential changed",
        ));
    }
    insert_secret_version(
        &mut tx,
        Uuid::now_v7(),
        actor.tenant_id,
        id,
        next,
        actor.user_id,
        &stored,
    )
    .await?;
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
    let r=sqlx::query("SELECT c.credential_type,c.current_secret_version,s.provider,s.secret_ref,s.provider_version,s.key_id,s.nonce,s.ciphertext FROM credentials c JOIN credential_secret_versions s ON s.credential_id=c.id AND s.version_number=c.current_secret_version WHERE c.tenant_id=? AND c.id=? AND c.status='active'").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Credential"))?;
    let version: u64 = r.try_get("current_secret_version")?;
    let provider: String = r.try_get("provider")?;
    let secret = if provider == "local_encrypted" {
        let keyring = state.credential_keyring.as_ref().ok_or_else(|| {
            AppError::service_unavailable(
                "CREDENTIAL_STORE_UNAVAILABLE",
                "Local credential keyring is unavailable",
            )
        })?;
        keyring
            .decrypt(
                &r.try_get::<String, _>("key_id")?,
                &r.try_get::<Vec<u8>, _>("nonce")?,
                &r.try_get::<Vec<u8>, _>("ciphertext")?,
                aad(tenant, id, version).as_bytes(),
            )
            .map_err(|_| {
                AppError::service_unavailable(
                    "CREDENTIAL_DECRYPTION_FAILED",
                    "Credential cannot be decrypted",
                )
            })?
    } else {
        let provider_impl = state.secret_provider.as_ref().ok_or_else(|| {
            AppError::service_unavailable(
                "CREDENTIAL_STORE_UNAVAILABLE",
                "External credential provider is unavailable",
            )
        })?;
        let secret_ref: String = r.try_get("secret_ref")?;
        let provider_version: String = r.try_get("provider_version")?;
        provider_impl
            .read(&secret_ref, provider_version.parse().ok())
            .await
            .map_err(|_| {
                AppError::service_unavailable(
                    "CREDENTIAL_PROVIDER_UNAVAILABLE",
                    "External credential provider cannot resolve the secret",
                )
            })?
    };
    Ok(ResolvedCredential {
        credential_type: r.try_get("credential_type")?,
        secret,
    })
}

struct StoredSecret {
    storage_mode: &'static str,
    provider: &'static str,
    secret_ref: Option<String>,
    provider_version: Option<String>,
    algorithm: Option<String>,
    key_id: Option<String>,
    nonce: Option<Vec<u8>>,
    ciphertext: Option<Vec<u8>>,
}

async fn store_secret(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
    logical_version: u64,
    secret: &PlainSecret,
) -> AppResult<StoredSecret> {
    if let Some(provider) = &state.secret_provider {
        let secret_ref = format!("tenants/{tenant}/credentials/{id}");
        let version = provider.write(&secret_ref, secret).await.map_err(|_| {
            AppError::service_unavailable(
                "CREDENTIAL_PROVIDER_UNAVAILABLE",
                "External credential provider cannot store the secret",
            )
        })?;
        return Ok(StoredSecret {
            storage_mode: "external_reference",
            provider: "vault_kv_v2",
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
            "Credential keyring is not configured",
        )
    })?;
    let encrypted = keyring
        .encrypt(secret, aad(tenant, id, logical_version).as_bytes())
        .map_err(AppError::internal)?;
    Ok(StoredSecret {
        storage_mode: "local_encrypted",
        provider: "local_encrypted",
        secret_ref: None,
        provider_version: None,
        algorithm: Some(encrypted.algorithm.into()),
        key_id: Some(encrypted.key_id),
        nonce: Some(encrypted.nonce.to_vec()),
        ciphertext: Some(encrypted.ciphertext),
    })
}

async fn insert_secret_version(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    secret_id: Uuid,
    tenant: Uuid,
    credential_id: Uuid,
    version: u64,
    actor: Uuid,
    stored: &StoredSecret,
) -> AppResult<()> {
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,provider,secret_ref,provider_version,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(secret_id).bind(tenant).bind(credential_id).bind(version).bind(stored.provider)
        .bind(&stored.secret_ref).bind(&stored.provider_version).bind(&stored.algorithm)
        .bind(&stored.key_id).bind(&stored.nonce).bind(&stored.ciphertext).bind(actor)
        .execute(&mut **tx).await?;
    Ok(())
}

async fn destroy_stored_secret(state: &AppState, stored: &StoredSecret) {
    let (Some(provider), Some(secret_ref), Some(version)) = (
        state.secret_provider.as_ref(),
        stored.secret_ref.as_deref(),
        stored
            .provider_version
            .as_deref()
            .and_then(|value| value.parse().ok()),
    ) else {
        return;
    };
    let _ = provider.destroy(secret_ref, version).await;
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

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration as StdDuration};

    use agentx_infrastructure::{
        config::{MySqlSettings, MySqlTlsMode},
        credential::SecretProvider,
        mysql,
    };
    use anyhow::Result;
    use async_trait::async_trait;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use secrecy::SecretString;
    use sha2::{Digest, Sha256};
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use time::{Duration, OffsetDateTime};
    use tokio::time::timeout;
    use uuid::Uuid;

    use super::{BrokerResolveRequest, PlainSecret, resolve_broker_credential};
    use crate::{config::AuthSettings, state::AppState};

    struct FixtureSecretProvider;

    #[async_trait]
    impl SecretProvider for FixtureSecretProvider {
        async fn read(&self, _secret_ref: &str, version: Option<u64>) -> Result<PlainSecret> {
            anyhow::ensure!(version == Some(1), "unexpected secret version");
            Ok(PlainSecret::new(br#"{"token":"vault-secret"}"#.to_vec()))
        }

        async fn write(&self, _secret_ref: &str, _value: &PlainSecret) -> Result<u64> {
            anyhow::bail!("test provider is read-only")
        }

        async fn destroy(&self, _secret_ref: &str, _version: u64) -> Result<()> {
            anyhow::bail!("test provider is read-only")
        }
    }

    #[tokio::test]
    async fn broker_consumes_tokens_and_allows_outer_handle_id_consumption_without_deadlock() {
        let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .expect("MySQL container should start");
        let port = container
            .get_host_port_ipv4(3306.tcp())
            .await
            .expect("mapped MySQL port");
        let settings = MySqlSettings {
            host: "127.0.0.1".into(),
            port,
            database: "agentx".into(),
            username: "agentx".into(),
            password: SecretString::from("agentx-test-password".to_owned()),
            max_connections: 5,
            tls_mode: MySqlTlsMode::Disabled,
            tls_ca_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
        };
        let pool = loop {
            if let Ok(pool) = mysql::connect(&settings).await {
                break pool;
            }
            tokio::time::sleep(StdDuration::from_millis(500)).await;
        };
        mysql::run_migrations(&pool).await.expect("run migrations");

        let tenant_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        let department_id = Uuid::now_v7();
        let workflow_id = Uuid::now_v7();
        let workflow_version_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        let node_execution_id = Uuid::now_v7();
        let attempt_id = Uuid::now_v7();
        let lease_token = Uuid::now_v7();
        let credential_id = Uuid::now_v7();
        let token_handle_id = Uuid::now_v7();
        let id_handle_id = Uuid::now_v7();
        let raw_token = "handle-token";
        let secret_ref = format!("tenants/{tenant_id}/credentials/{credential_id}");
        let resource_snapshot = serde_json::json!({"resources":[{
            "reference":{"resourceType":"credential","resourceId":credential_id},
            "snapshot":{"secretVersion":1}
        }]});

        sqlx::query(
            "INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Broker Test','broker test')",
        )
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
            .bind(department_id).bind(tenant_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,'broker','broker','Broker')")
            .bind(user_id).bind(tenant_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Broker Runtime',?,?)")
            .bind(workflow_id).bind(tenant_id).bind(user_id).bind(department_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'4.0',JSON_OBJECT('schemaVersion','4.0','start',JSON_OBJECT('inputs',JSON_OBJECT(),'contexts',JSON_OBJECT()),'nodes',JSON_ARRAY(),'connections',JSON_ARRAY(),'end',JSON_OBJECT('outputs',JSON_OBJECT())),'broker-fixture',?)")
            .bind(workflow_version_id).bind(tenant_id).bind(workflow_id).bind(user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,source_kind,source_id,trace_id,trigger_type,status,input_json,context_json,context_base_json,context_version,session_context_version,started_at) VALUES(?,?,?,?,'version',?,?, 'manual','running',JSON_OBJECT(),JSON_OBJECT(),JSON_OBJECT(),0,0,CURRENT_TIMESTAMP(6))")
            .bind(execution_id).bind(tenant_id).bind(workflow_id).bind(workflow_version_id).bind(workflow_version_id).bind(Uuid::now_v7()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO execution_snapshots(execution_id,tenant_id,workflow_version_id,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,resource_snapshot_json,runtime_settings_json,state_hash) VALUES(?,?,?,JSON_OBJECT(),JSON_OBJECT(),'compiled','agentx-test',?,JSON_OBJECT(),'state')")
            .bind(execution_id).bind(tenant_id).bind(workflow_version_id).bind(&resource_snapshot).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES(?,?,?,'remote','remote','Remote','remote_action',1,0,0,0,'running','remote_action')")
            .bind(node_execution_id).bind(tenant_id).bind(execution_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,lease_token,deadline_at) VALUES(?,?,?,?,1,'running','broker-attempt',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
            .bind(attempt_id).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(lease_token).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO worker_leases(node_attempt_id,tenant_id,node_execution_id,lease_token,worker_instance_id,capability,acquired_at,heartbeat_at,expires_at) VALUES(?,?,?,?,'broker-worker','remote_action',CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6),DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
            .bind(attempt_id).bind(tenant_id).bind(node_execution_id).bind(lease_token).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,storage_mode,masked_hint,owner_department_id,created_by) VALUES(?,?,'Vault Runtime','bearer','external_reference','****',?,?)")
            .bind(credential_id).bind(tenant_id).bind(department_id).bind(user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,provider,secret_ref,provider_version,created_by) VALUES(?,?,?,1,'vault_kv_v2',?,'1',?)")
            .bind(Uuid::now_v7()).bind(tenant_id).bind(credential_id).bind(&secret_ref).bind(user_id).execute(&pool).await.unwrap();

        let token_hash = format!("{:x}", Sha256::digest(raw_token.as_bytes()));
        for (id, hash) in [
            (token_handle_id, token_hash),
            (id_handle_id, "2".repeat(64)),
        ] {
            sqlx::query("INSERT INTO node_invocation_handles(id,tenant_id,execution_id,node_execution_id,attempt_id,lease_token,token_hash,handle_kind,resource_id,resource_version,scope_json,expires_at) VALUES(?,?,?,?,?,?,?,'credential',?,1,JSON_OBJECT(),DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))")
                .bind(id).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(attempt_id).bind(lease_token).bind(hash).bind(credential_id).execute(&pool).await.unwrap();
        }

        let mut state = AppState::new(pool.clone(), test_auth());
        state.secret_provider = Some(Arc::new(FixtureSecretProvider));
        let request = |handle: Option<String>, handle_id: Option<Uuid>, consume_handle| {
            BrokerResolveRequest {
                secret_ref: secret_ref.clone(),
                version: Some(1),
                tenant_id,
                credential_id,
                credential_version: 1,
                execution_id,
                node_execution_id,
                attempt_id,
                lease_token,
                deadline: OffsetDateTime::now_utc() + Duration::minutes(4),
                handle,
                handle_id,
                consume_handle,
            }
        };

        let resolved =
            resolve_broker_credential(&state, request(Some(raw_token.to_owned()), None, true))
                .await
                .expect("token Handle resolves once");
        assert_eq!(
            STANDARD.decode(resolved.secret_base64).unwrap(),
            br#"{"token":"vault-secret"}"#
        );
        assert!(
            resolve_broker_credential(&state, request(Some(raw_token.to_owned()), None, true))
                .await
                .is_err()
        );

        let mut outer = pool.begin().await.unwrap();
        let locked_id: Uuid =
            sqlx::query_scalar("SELECT id FROM node_invocation_handles WHERE id=? FOR UPDATE")
                .bind(id_handle_id)
                .fetch_one(&mut *outer)
                .await
                .unwrap();
        let resolved = timeout(
            StdDuration::from_secs(2),
            resolve_broker_credential(&state, request(None, Some(locked_id), false)),
        )
        .await
        .expect("non-consuming Handle ID lookup must not deadlock")
        .expect("Handle ID resolves while its owner holds the lock");
        assert_eq!(
            STANDARD.decode(resolved.secret_base64).unwrap(),
            br#"{"token":"vault-secret"}"#
        );
        let consumed = sqlx::query("UPDATE node_invocation_handles SET consumed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND consumed_at IS NULL")
            .bind(locked_id).execute(&mut *outer).await.unwrap();
        assert_eq!(consumed.rows_affected(), 1);
        outer.commit().await.unwrap();
        assert!(
            resolve_broker_credential(&state, request(None, Some(locked_id), false))
                .await
                .is_err()
        );
    }

    fn test_auth() -> AuthSettings {
        AuthSettings {
            signing_secret: SecretString::from(
                "broker-test-signing-secret-with-32-characters".to_owned(),
            ),
            issuer: "agentx-test".into(),
            audience: "agentx-test".into(),
            access_ttl_seconds: 900,
            refresh_ttl_seconds: 604_800,
            change_password_ttl_seconds: 600,
            cookie_secure: false,
            login_max_failures: 5,
            login_failure_window_seconds: 900,
            login_lock_seconds: 900,
        }
    }
}
