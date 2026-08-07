use std::{convert::Infallible, env, sync::Arc, time::Duration};

use agentx_api_types::{ApiErrorResponse, FieldError};
use agentx_application::{
    CancelExecutionCommandPayload, ResumeExecutionCommandPayload, RuntimeCommand,
    RuntimeCommandType, StartExecutionCommandPayload,
};
use agentx_domain::{InvocationId, TenantId};
use agentx_infrastructure::{
    config::MySqlSettings,
    config::{SecretProviderMode, secret_provider_mode},
    credential::{CredentialKeyring, PlainSecret},
    mysql,
    runtime_commands::RuntimeCommandRepository,
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION, request::Parts},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::stream;
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

mod binding_loop;
mod schedule_loop;

#[derive(Clone)]
struct GatewayState {
    pool: MySqlPool,
    jwt: Arc<JwtSettings>,
    webhook_secrets: WebhookSecretSource,
    wait_resume_secret: Arc<SecretString>,
}

#[derive(Clone)]
enum WebhookSecretSource {
    Local(Arc<CredentialKeyring>),
    Broker {
        client: reqwest::Client,
        endpoint: String,
        token: Arc<SecretString>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookSecretRequest<'a> {
    secret_ref: &'a str,
    version: u64,
    tenant_id: Uuid,
    webhook_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookSecretResponse {
    secret_base64: String,
}

impl WebhookSecretSource {
    async fn resolve(
        &self,
        row: &sqlx::mysql::MySqlRow,
        tenant_id: Uuid,
        webhook_id: Uuid,
    ) -> GatewayResult<PlainSecret> {
        let provider: String = row.try_get("secret_provider")?;
        match (provider.as_str(), self) {
            ("local_encrypted", Self::Local(keyring)) => {
                let aad = format!("{tenant_id}/{webhook_id}/webhook/1");
                keyring
                    .decrypt(
                        row.try_get("secret_key_id")?,
                        row.try_get::<Vec<u8>, _>("secret_nonce")?.as_slice(),
                        row.try_get::<Vec<u8>, _>("secret_ciphertext")?.as_slice(),
                        aad.as_bytes(),
                    )
                    .map_err(GatewayError::internal)
            }
            (
                "vault_kv_v2",
                Self::Broker {
                    client,
                    endpoint,
                    token,
                },
            ) => {
                let secret_ref: String = row.try_get("secret_ref")?;
                let version = row
                    .try_get::<String, _>("secret_provider_version")?
                    .parse::<u64>()
                    .map_err(GatewayError::internal)?;
                let response = client
                    .post(format!(
                        "{}/internal/v1/webhooks/resolve",
                        endpoint.trim_end_matches('/')
                    ))
                    .header("X-Agentx-Credential-Broker-Token", token.expose_secret())
                    .json(&WebhookSecretRequest {
                        secret_ref: &secret_ref,
                        version,
                        tenant_id,
                        webhook_id,
                    })
                    .send()
                    .await
                    .map_err(GatewayError::internal)?;
                if !response.status().is_success() {
                    return Err(GatewayError::service_unavailable(
                        "WEBHOOK_SECRET_PROVIDER_UNAVAILABLE",
                        "Webhook Secret provider is unavailable",
                    ));
                }
                let response = response
                    .json::<WebhookSecretResponse>()
                    .await
                    .map_err(GatewayError::internal)?;
                let secret = STANDARD
                    .decode(response.secret_base64)
                    .map_err(GatewayError::internal)?;
                Ok(PlainSecret::new(secret))
            }
            _ => Err(GatewayError::service_unavailable(
                "WEBHOOK_SECRET_PROVIDER_UNAVAILABLE",
                "Webhook Secret storage does not match the Gateway configuration",
            )),
        }
    }
}

#[derive(Clone)]
struct JwtSettings {
    secret: SecretString,
    issuer: String,
    audience: String,
}

impl JwtSettings {
    fn from_env() -> Result<Self> {
        Ok(Self {
            secret: SecretString::from(
                env::var("AGENTX_JWT_SIGNING_SECRET")
                    .context("AGENTX_JWT_SIGNING_SECRET is required")?,
            ),
            issuer: env::var("AGENTX_JWT_ISSUER").unwrap_or_else(|_| "agentx-platform".into()),
            audience: env::var("AGENTX_JWT_AUDIENCE").unwrap_or_else(|_| "agentx-web".into()),
        })
    }
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: Uuid,
    tid: Uuid,
    ver: u64,
    kind: String,
    #[serde(rename = "iss")]
    _iss: String,
    #[serde(rename = "aud")]
    _aud: String,
    #[serde(rename = "exp")]
    _exp: i64,
}

#[derive(Clone, Debug)]
enum Caller {
    User {
        tenant_id: Uuid,
        user_id: Uuid,
    },
    ApiKey {
        tenant_id: Uuid,
        application_id: Uuid,
        key_id: Uuid,
    },
    Webhook {
        tenant_id: Uuid,
        application_id: Uuid,
        webhook_id: Uuid,
    },
}

impl Caller {
    fn tenant_id(&self) -> Uuid {
        match self {
            Self::User { tenant_id, .. }
            | Self::ApiKey { tenant_id, .. }
            | Self::Webhook { tenant_id, .. } => *tenant_id,
        }
    }
    fn user_id(&self) -> Option<Uuid> {
        match self {
            Self::User { user_id, .. } => Some(*user_id),
            _ => None,
        }
    }
    fn application_id(&self) -> Option<Uuid> {
        match self {
            Self::ApiKey { application_id, .. } | Self::Webhook { application_id, .. } => {
                Some(*application_id)
            }
            Self::User { .. } => None,
        }
    }
    fn identity(&self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::User { user_id, .. } => ("user", Some(*user_id)),
            Self::ApiKey { key_id, .. } => ("api_key", Some(*key_id)),
            Self::Webhook { webhook_id, .. } => ("webhook", Some(*webhook_id)),
        }
    }
}

#[derive(Debug)]
struct GatewayError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
type GatewayResult<T> = std::result::Result<T, GatewayError>;

impl GatewayError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    fn service_unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }
    fn not_found(resource: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("{resource} was not found"),
        )
    }
    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error,"gateway request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "The request could not be completed",
        )
    }
}
impl From<sqlx::Error> for GatewayError {
    fn from(value: sqlx::Error) -> Self {
        Self::internal(value)
    }
}
impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let request_id = Uuid::now_v7();
        (
            self.status,
            Json(ApiErrorResponse {
                code: self.code.into(),
                message: self.message,
                request_id,
                field_errors: Vec::<FieldError>::new(),
            }),
        )
            .into_response()
    }
}

impl FromRequestParts<GatewayState> for Caller {
    type Rejection = GatewayError;
    async fn from_request_parts(parts: &mut Parts, state: &GatewayState) -> GatewayResult<Self> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| {
                GatewayError::unauthorized("AUTHENTICATION_REQUIRED", "Authentication is required")
            })?;
        if token.starts_with("axk_") {
            authenticate_api_key(state, token).await
        } else {
            authenticate_jwt(state, token).await
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateSessionRequest {
    external_user_id: Option<String>,
    title: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    id: Uuid,
    application_id: Uuid,
    application_deployment_id: Uuid,
    workflow_version_id: Option<Uuid>,
    version_policy: String,
    external_user_id: Option<String>,
    title: Option<String>,
    status: String,
    version: u64,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct InvocationRequest {
    input: Value,
    session_id: Option<Uuid>,
    response_mode: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MessageRequest {
    parts: Vec<MessagePartInput>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MessagePartInput {
    part_type: String,
    content: Option<Value>,
    artifact_id: Option<Uuid>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MessagePartResponse {
    part_type: String,
    content: Option<Value>,
    artifact_id: Option<Uuid>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MessageResponse {
    id: Uuid,
    invocation_id: Option<Uuid>,
    sequence: u64,
    role: String,
    parts: Vec<MessagePartResponse>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

struct InvocationCommand<'a> {
    application_id: Uuid,
    application_deployment_id: Uuid,
    session_id: Option<Uuid>,
    workflow_version_id: Uuid,
    input: Value,
    idempotency_key: &'a str,
    message_parts: Option<&'a [MessagePartInput]>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct InvocationResponse {
    id: Uuid,
    application_id: Uuid,
    session_id: Option<Uuid>,
    execution_id: Option<Uuid>,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct WaitResumeRequest {
    output_port: Option<String>,
    #[serde(default)]
    payload: Value,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct WaitResumeResponse {
    accepted: bool,
    replayed: bool,
}

#[derive(OpenApi)]
#[openapi(paths(create_session,get_session,list_messages,create_invocation,send_message,get_invocation,cancel_invocation,invocation_events,webhook_trigger,resume_wait),components(schemas(CreateSessionRequest,SessionResponse,InvocationRequest,MessageRequest,MessagePartInput,MessageResponse,MessagePartResponse,InvocationResponse,WaitResumeRequest,WaitResumeResponse,ApiErrorResponse,FieldError)),tags((name="Agentx Gateway",description="Application invocation and runtime wait API")))]
struct GatewayApi;

#[tokio::main]
async fn main() -> Result<()> {
    let command = env::args().nth(1);
    if command.as_deref() == Some("openapi") {
        let path = env::args()
            .nth(2)
            .unwrap_or_else(|| "openapi/trigger-gateway.json".into());
        let content = serde_json::to_string_pretty(&GatewayApi::openapi())?;
        std::fs::write(&path, format!("{content}\n"))
            .with_context(|| format!("failed to write {path}"))?;
        return Ok(());
    }
    let pool = mysql::connect(&MySqlSettings::from_env()?).await?;
    let webhook_secrets = match secret_provider_mode()? {
        SecretProviderMode::VaultKvV2 => WebhookSecretSource::Broker {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: env::var("AGENTX_CREDENTIAL_BROKER_URL")
                .context("AGENTX_CREDENTIAL_BROKER_URL is required")?,
            token: Arc::new(SecretString::from(
                env::var("AGENTX_CREDENTIAL_BROKER_TOKEN")
                    .context("AGENTX_CREDENTIAL_BROKER_TOKEN is required")?,
            )),
        },
        SecretProviderMode::LocalEncrypted => {
            let key_id = env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")
                .context("AGENTX_CREDENTIAL_ACTIVE_KEY_ID is required")?;
            let keys = SecretString::from(
                env::var("AGENTX_CREDENTIAL_KEYS_JSON")
                    .context("AGENTX_CREDENTIAL_KEYS_JSON is required")?,
            );
            WebhookSecretSource::Local(Arc::new(CredentialKeyring::from_json(key_id, &keys)?))
        }
    };
    let state = GatewayState {
        pool: pool.clone(),
        jwt: Arc::new(JwtSettings::from_env()?),
        webhook_secrets,
        wait_resume_secret: Arc::new(SecretString::from(
            env::var("AGENTX_WAIT_RESUME_SECRET").unwrap_or_else(|_| {
                env::var("AGENTX_JWT_SIGNING_SECRET")
                    .unwrap_or_else(|_| "development-wait-resume-secret-change-me".into())
            }),
        )),
    };
    tokio::spawn(schedule_loop::run(pool.clone()));
    tokio::spawn(binding_loop::run(pool.clone()));
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.set_status("mysql", "ready").await;
    agentx_service_kit::serve("trigger-gateway", router(state), health).await
}

fn router(state: GatewayState) -> Router {
    Router::new()
        .nest(
            "/gateway/v1",
            Router::new()
                .route("/applications/{slug}/sessions", post(create_session))
                .route("/applications/{slug}/invocations", post(create_invocation))
                .route("/sessions/{id}", get(get_session))
                .route(
                    "/sessions/{id}/messages",
                    get(list_messages).post(send_message),
                )
                .route("/invocations/{id}", get(get_invocation))
                .route("/invocations/{id}/cancel", post(cancel_invocation))
                .route("/invocations/{id}/events", get(invocation_events))
                .route("/waits/{binding_id}/resume", post(resume_wait))
                .route("/webhooks/{public_id}", post(webhook_trigger)),
        )
        .with_state(state)
}

#[utoipa::path(post,path="/gateway/v1/applications/{slug}/sessions",request_body=CreateSessionRequest)]
async fn create_session(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(slug): Path<String>,
    Json(input): Json<CreateSessionRequest>,
) -> GatewayResult<(StatusCode, Json<SessionResponse>)> {
    let app = load_active_application(&state, &caller, &slug).await?;
    let application_id: Uuid = app.try_get("id")?;
    let deployment_id: Uuid = app.try_get("deployment_id")?;
    let policy: String = app.try_get("session_version_policy")?;
    let deployed_version: Uuid = app.try_get("workflow_version_id")?;
    let workflow_version = if policy == "follow_deployment" {
        None
    } else {
        Some(deployed_version)
    };
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO application_sessions(id,tenant_id,application_id,application_deployment_id,workflow_version_id,version_policy,external_user_id,title,created_by_user_id) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(id).bind(caller.tenant_id()).bind(application_id).bind(deployment_id).bind(workflow_version).bind(policy).bind(input.external_user_id).bind(input.title).bind(caller.user_id()).execute(&state.pool).await?;
    Ok((
        StatusCode::CREATED,
        Json(load_session(&state, caller.tenant_id(), id).await?),
    ))
}

#[utoipa::path(get, path = "/gateway/v1/sessions/{id}")]
async fn get_session(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> GatewayResult<Json<SessionResponse>> {
    let response = load_session(&state, caller.tenant_id(), id).await?;
    authorize_application(&state, &caller, response.application_id).await?;
    Ok(Json(response))
}

#[utoipa::path(get, path = "/gateway/v1/sessions/{id}/messages", params(("id" = Uuid, Path)))]
async fn list_messages(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> GatewayResult<Json<Vec<MessageResponse>>> {
    let session = load_session(&state, caller.tenant_id(), id).await?;
    authorize_application(&state, &caller, session.application_id).await?;
    let rows = sqlx::query("SELECT id,invocation_id,sequence_number,role,created_at FROM application_messages WHERE tenant_id=? AND session_id=? ORDER BY sequence_number,id LIMIT 1000")
        .bind(caller.tenant_id()).bind(id).fetch_all(&state.pool).await?;
    let part_rows = sqlx::query("SELECT p.message_id,p.part_type,p.content_json,p.artifact_id FROM application_message_parts p JOIN application_messages m ON m.id=p.message_id AND m.tenant_id=p.tenant_id WHERE p.tenant_id=? AND m.session_id=? ORDER BY m.sequence_number,p.part_index")
        .bind(caller.tenant_id()).bind(id).fetch_all(&state.pool).await?;
    let mut parts = std::collections::BTreeMap::<Uuid, Vec<MessagePartResponse>>::new();
    for row in part_rows {
        parts
            .entry(row.try_get("message_id")?)
            .or_default()
            .push(MessagePartResponse {
                part_type: row.try_get("part_type")?,
                content: row.try_get("content_json")?,
                artifact_id: row.try_get("artifact_id")?,
            });
    }
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let message_id: Uuid = row.try_get("id")?;
                Ok(MessageResponse {
                    id: message_id,
                    invocation_id: row.try_get("invocation_id")?,
                    sequence: row.try_get("sequence_number")?,
                    role: row.try_get("role")?,
                    parts: parts.remove(&message_id).unwrap_or_default(),
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<GatewayResult<Vec<_>>>()?,
    ))
}

#[utoipa::path(post,path="/gateway/v1/applications/{slug}/invocations",request_body=InvocationRequest)]
async fn create_invocation(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(input): Json<InvocationRequest>,
) -> GatewayResult<(StatusCode, Json<InvocationResponse>)> {
    let app = load_active_application(&state, &caller, &slug).await?;
    validate_response_mode(input.response_mode.as_deref())?;
    if let Some(session_id) = input.session_id {
        let session = load_session(&state, caller.tenant_id(), session_id).await?;
        let application_id: Uuid = app.try_get("id")?;
        if session.application_id != application_id {
            return Err(GatewayError::bad_request(
                "SESSION_APPLICATION_MISMATCH",
                "Session does not belong to the selected Application",
            ));
        }
    }
    let idempotency_key = validate_idempotency(&headers)?;
    validate_input(&input.input, app.try_get("input_schema_json")?)?;
    let application_id: Uuid = app.try_get("id")?;
    let application_deployment_id: Uuid = app.try_get("deployment_id")?;
    let workflow_version_id: Uuid = app.try_get("workflow_version_id")?;
    let response = request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            application_deployment_id,
            session_id: input.session_id,
            workflow_version_id,
            input: input.input,
            idempotency_key,
            message_parts: None,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, response))
}

#[utoipa::path(post,path="/gateway/v1/sessions/{id}/messages",request_body=MessageRequest)]
async fn send_message(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<MessageRequest>,
) -> GatewayResult<(StatusCode, Json<InvocationResponse>)> {
    let session = load_session(&state, caller.tenant_id(), id).await?;
    authorize_application(&state, &caller, session.application_id).await?;
    let idempotency_key = validate_idempotency(&headers)?;
    if input.parts.is_empty() {
        return Err(GatewayError::bad_request(
            "MESSAGE_PARTS_REQUIRED",
            "At least one message part is required",
        ));
    }
    for part in &input.parts {
        if !matches!(
            part.part_type.as_str(),
            "text" | "json" | "image" | "audio" | "file"
        ) {
            return Err(GatewayError::bad_request(
                "INVALID_MESSAGE_PART",
                "Message part type is invalid",
            ));
        }
        if part.content.is_none() && part.artifact_id.is_none() {
            return Err(GatewayError::bad_request(
                "EMPTY_MESSAGE_PART",
                "Message part requires content or an Artifact",
            ));
        }
    }
    let row=sqlx::query("SELECT a.id application_id,COALESCE(s.workflow_version_id,ad.workflow_version_id) workflow_version_id,IF(s.workflow_version_id IS NULL,h.deployment_id,s.application_deployment_id) application_deployment_id FROM application_sessions s JOIN applications a ON a.id=s.application_id AND a.tenant_id=s.tenant_id JOIN application_deployment_heads h ON h.application_id=a.id AND h.tenant_id=a.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id WHERE s.id=? AND s.tenant_id=? AND s.status='active' AND a.status='active'")
        .bind(id).bind(caller.tenant_id()).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Session"))?;
    let application_id: Uuid = row.try_get("application_id")?;
    let application_deployment_id: Uuid = row.try_get("application_deployment_id")?;
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let input_value = serde_json::to_value(&input.parts).map_err(GatewayError::internal)?;
    let response = request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            application_deployment_id,
            session_id: Some(id),
            workflow_version_id,
            input: json!({"parts":input_value}),
            idempotency_key,
            message_parts: Some(&input.parts),
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, response))
}

async fn request_invocation(
    state: &GatewayState,
    caller: &Caller,
    command: InvocationCommand<'_>,
) -> GatewayResult<Json<InvocationResponse>> {
    let InvocationCommand {
        application_id,
        application_deployment_id,
        session_id,
        workflow_version_id,
        input,
        idempotency_key,
        message_parts,
    } = command;
    let (caller_type, caller_id) = caller.identity();
    let request_hash = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({"sessionId":session_id,"input":input}))
                .map_err(GatewayError::internal)?
        )
    );
    let invocation_id = stable_invocation_id(
        caller.tenant_id(),
        application_id,
        caller_type,
        caller_id,
        idempotency_key,
    );
    let runtime_command = RuntimeCommand::new(
        TenantId::from_uuid(caller.tenant_id()),
        RuntimeCommandType::StartExecution,
        "application_invocation",
        invocation_id.to_string(),
        format!("invocation:{invocation_id}"),
        serde_json::to_value(StartExecutionCommandPayload {
            workflow_version_id,
            invocation_id: Some(invocation_id.as_uuid()),
            session_id,
            requested_by: caller.user_id(),
            trigger_type: "application".into(),
            input,
            runtime_settings: json!({}),
        })
        .map_err(GatewayError::internal)?,
    );
    let mut tx = state.pool.begin().await?;
    RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &runtime_command)
        .await
        .map_err(GatewayError::internal)?;
    let insert_result = sqlx::query("INSERT INTO application_invocations(id,tenant_id,application_id,application_deployment_id,session_id,workflow_version_id,execution_id,runtime_command_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(?,?,?,?,?,?,NULL,?,?,?,?,?, 'queued')")
        .bind(invocation_id.as_uuid()).bind(caller.tenant_id()).bind(application_id).bind(application_deployment_id).bind(session_id).bind(workflow_version_id)
        .bind(runtime_command.id).bind(caller_type).bind(caller_id).bind(&request_hash).bind(idempotency_key).execute(&mut *tx).await;
    let inserted = match insert_result {
        Ok(_) => true,
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => false,
        Err(error) => return Err(error.into()),
    };
    if !inserted {
        let row=sqlx::query("SELECT i.id,i.application_id,i.session_id,i.execution_id,i.status,i.created_at,i.request_hash,(SELECT JSON_UNQUOTE(JSON_EXTRACT(e.payload_json,'$.errorCode')) FROM invocation_events e WHERE e.tenant_id=i.tenant_id AND e.invocation_id=i.id AND e.event_type='application.output_invalid' ORDER BY e.sequence_number DESC LIMIT 1) error_code,(SELECT JSON_UNQUOTE(JSON_EXTRACT(e.payload_json,'$.errorMessage')) FROM invocation_events e WHERE e.tenant_id=i.tenant_id AND e.invocation_id=i.id AND e.event_type='application.output_invalid' ORDER BY e.sequence_number DESC LIMIT 1) error_message FROM application_invocations i WHERE i.tenant_id=? AND i.application_id=? AND i.caller_type=? AND i.caller_id <=> ? AND i.idempotency_key=?")
            .bind(caller.tenant_id()).bind(application_id).bind(caller_type).bind(caller_id).bind(idempotency_key).fetch_one(&mut *tx).await?;
        let existing_hash: String = row.try_get("request_hash")?;
        tx.rollback().await?;
        if existing_hash != request_hash {
            return Err(GatewayError::new(
                StatusCode::CONFLICT,
                "IDEMPOTENCY_KEY_REUSED",
                "Idempotency-Key was already used with a different request",
            ));
        }
        return Ok(Json(invocation_from_row(row)?));
    }
    sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,1,'invocation.queued',?)")
        .bind(caller.tenant_id()).bind(invocation_id.as_uuid()).bind(json!({"commandId":runtime_command.id,"status":"queued"})).execute(&mut *tx).await?;
    if let (Some(session_id), Some(parts)) = (session_id, message_parts) {
        sqlx::query("SELECT id FROM application_sessions WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(session_id)
            .bind(caller.tenant_id())
            .fetch_one(&mut *tx)
            .await?;
        let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM application_messages WHERE tenant_id=? AND session_id=?")
            .bind(caller.tenant_id()).bind(session_id).fetch_one(&mut *tx).await?;
        let message_id = Uuid::now_v7();
        sqlx::query("INSERT INTO application_messages(id,tenant_id,session_id,invocation_id,sequence_number,role) VALUES(?,?,?,?,?,'user')")
            .bind(message_id).bind(caller.tenant_id()).bind(session_id).bind(invocation_id.as_uuid()).bind(sequence).execute(&mut *tx).await?;
        for (index, part) in parts.iter().enumerate() {
            sqlx::query("INSERT INTO application_message_parts(id,tenant_id,message_id,part_index,part_type,content_json,artifact_id) VALUES(?,?,?,?,?,?,?)")
                .bind(Uuid::now_v7()).bind(caller.tenant_id()).bind(message_id).bind(index as u32).bind(&part.part_type).bind(&part.content).bind(part.artifact_id).execute(&mut *tx).await?;
            if let Some(artifact_id) = part.artifact_id {
                sqlx::query("INSERT IGNORE INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'application_message',?,'part')")
                    .bind(caller.tenant_id())
                    .bind(artifact_id)
                    .bind(message_id.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    let row=sqlx::query("SELECT i.id,i.application_id,i.session_id,i.execution_id,i.status,i.created_at,NULL error_code,NULL error_message FROM application_invocations i WHERE i.id=? AND i.tenant_id=?")
        .bind(invocation_id.as_uuid()).bind(caller.tenant_id()).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(invocation_from_row(row)?))
}

fn stable_invocation_id(
    tenant_id: Uuid,
    application_id: Uuid,
    caller_type: &str,
    caller_id: Option<Uuid>,
    idempotency_key: &str,
) -> InvocationId {
    let mut hasher = Sha256::new();
    hasher.update(tenant_id.as_bytes());
    hasher.update(application_id.as_bytes());
    hasher.update(caller_type.as_bytes());
    if let Some(id) = caller_id {
        hasher.update(id.as_bytes());
    }
    hasher.update(idempotency_key.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    InvocationId::from_uuid(Uuid::from_bytes(bytes))
}

#[utoipa::path(get, path = "/gateway/v1/invocations/{id}")]
async fn get_invocation(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> GatewayResult<Json<InvocationResponse>> {
    let row=sqlx::query("SELECT i.id,i.application_id,i.session_id,i.execution_id,i.status,i.created_at,(SELECT JSON_UNQUOTE(JSON_EXTRACT(e.payload_json,'$.errorCode')) FROM invocation_events e WHERE e.tenant_id=i.tenant_id AND e.invocation_id=i.id AND e.event_type='application.output_invalid' ORDER BY e.sequence_number DESC LIMIT 1) error_code,(SELECT JSON_UNQUOTE(JSON_EXTRACT(e.payload_json,'$.errorMessage')) FROM invocation_events e WHERE e.tenant_id=i.tenant_id AND e.invocation_id=i.id AND e.event_type='application.output_invalid' ORDER BY e.sequence_number DESC LIMIT 1) error_message FROM application_invocations i WHERE i.id=? AND i.tenant_id=?").bind(id).bind(caller.tenant_id()).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Invocation"))?;
    let response = invocation_from_row(row)?;
    authorize_application(&state, &caller, response.application_id).await?;
    Ok(Json(response))
}

#[utoipa::path(post, path = "/gateway/v1/invocations/{id}/cancel")]
async fn cancel_invocation(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> GatewayResult<StatusCode> {
    let invocation = get_invocation(State(state.clone()), caller.clone(), Path(id))
        .await?
        .0;
    if matches!(
        invocation.status.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Err(GatewayError::new(
            StatusCode::CONFLICT,
            "INVOCATION_TERMINAL",
            "Invocation is already terminal",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let locked = sqlx::query("SELECT execution_id,runtime_command_id,status FROM application_invocations WHERE id=? AND tenant_id=? FOR UPDATE")
        .bind(id)
        .bind(caller.tenant_id())
        .fetch_one(&mut *tx)
        .await?;
    let locked_status: String = locked.try_get("status")?;
    if matches!(locked_status.as_str(), "completed" | "failed" | "cancelled") {
        tx.rollback().await?;
        return Err(GatewayError::new(
            StatusCode::CONFLICT,
            "INVOCATION_TERMINAL",
            "Invocation is already terminal",
        ));
    }
    let execution_id: Option<Uuid> = locked.try_get("execution_id")?;
    if execution_id.is_none() {
        let runtime_command_id: Uuid = locked.try_get("runtime_command_id")?;
        let stopped = sqlx::query("UPDATE runtime_commands SET status='failed',error_code='INVOCATION_CANCELLED',error_message='Invocation was cancelled before execution creation',completed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND tenant_id=? AND status='pending'")
            .bind(runtime_command_id)
            .bind(caller.tenant_id())
            .execute(&mut *tx)
            .await?;
        if stopped.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(GatewayError::new(
                StatusCode::CONFLICT,
                "INVOCATION_STARTING",
                "Invocation execution is being created; retry cancellation shortly",
            ));
        }
        sqlx::query("UPDATE application_invocations SET status='cancelled',completed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND tenant_id=?")
            .bind(id)
            .bind(caller.tenant_id())
            .execute(&mut *tx)
            .await?;
        let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=?").bind(caller.tenant_id()).bind(id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,?,'invocation.cancelled',?)")
            .bind(caller.tenant_id())
            .bind(id)
            .bind(sequence)
            .bind(json!({"commandId":runtime_command_id,"status":"cancelled","beforeExecution":true}))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(StatusCode::ACCEPTED);
    }
    let execution_id = execution_id.expect("checked above");
    let command = RuntimeCommand::new(
        TenantId::from_uuid(caller.tenant_id()),
        RuntimeCommandType::CancelExecution,
        "application_invocation",
        id.to_string(),
        format!("invocation:{id}:cancel"),
        serde_json::to_value(CancelExecutionCommandPayload { execution_id })
            .map_err(GatewayError::internal)?,
    );
    let inserted = RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &command)
        .await
        .map_err(GatewayError::internal)?;
    if inserted {
        let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=?").bind(caller.tenant_id()).bind(id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,?,'invocation.cancel_queued',?)").bind(caller.tenant_id()).bind(id).bind(sequence).bind(json!({"commandId":command.id,"status":"queued"})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(get, path = "/gateway/v1/invocations/{id}/events")]
async fn invocation_events(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> GatewayResult<Sse<impl futures::Stream<Item = std::result::Result<Event, Infallible>>>> {
    let invocation = get_invocation(State(state.clone()), caller.clone(), Path(id))
        .await?
        .0;
    authorize_application(&state, &caller, invocation.application_id).await?;
    let after = last_event_cursor(&headers);
    let tenant_id = caller.tenant_id();
    let pool = state.pool.clone();
    let events = stream::unfold(
        (pool, tenant_id, id, after),
        |(pool, tenant_id, invocation_id, mut cursor)| async move {
            loop {
                let row = sqlx::query("SELECT sequence_number,event_type,payload_json FROM invocation_events WHERE tenant_id=? AND invocation_id=? AND sequence_number>? ORDER BY sequence_number LIMIT 1")
                    .bind(tenant_id)
                    .bind(invocation_id)
                    .bind(cursor)
                    .fetch_optional(&pool)
                    .await;
                match row {
                    Ok(Some(row)) => {
                        let sequence: u64 = row.try_get("sequence_number").unwrap_or_default();
                        let event_type: String = row
                            .try_get("event_type")
                            .unwrap_or_else(|_| "message".into());
                        let payload: Value = row.try_get("payload_json").unwrap_or(Value::Null);
                        cursor = sequence;
                        let event = Event::default()
                            .id(sequence.to_string())
                            .event(event_type)
                            .json_data(payload)
                            .expect("JSON values serialize");
                        return Some((Ok(event), (pool, tenant_id, invocation_id, cursor)));
                    }
                    Ok(None) => {
                        let terminal = sqlx::query_scalar::<_, bool>("SELECT status IN ('completed','failed','cancelled') FROM application_invocations WHERE tenant_id=? AND id=?")
                            .bind(tenant_id)
                            .bind(invocation_id)
                            .fetch_optional(&pool)
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or(false);
                        if terminal {
                            return None;
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                }
            }
        },
    );
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn last_event_cursor(headers: &HeaderMap) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

#[utoipa::path(
    post,
    path = "/gateway/v1/waits/{binding_id}/resume",
    request_body = WaitResumeRequest,
    responses((status = 200, body = WaitResumeResponse))
)]
async fn resume_wait(
    State(state): State<GatewayState>,
    Path(binding_id): Path<Uuid>,
    headers: HeaderMap,
    body: String,
) -> GatewayResult<Json<WaitResumeResponse>> {
    let row=sqlx::query("SELECT w.tenant_id,w.execution_id,w.node_execution_id,w.wait_kind,w.authentication_mode,w.payload_schema_json,w.status,b.authentication_config_hash FROM wait_subscriptions w JOIN resume_webhook_bindings b ON b.wait_subscription_id=w.id AND b.tenant_id=w.tenant_id WHERE w.id=? AND b.id=?")
        .bind(binding_id).bind(binding_id).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Wait Resume Binding"))?;
    let authentication: String = row.try_get("authentication_mode")?;
    if authentication == "signed" {
        let signature = headers
            .get("x-agentx-wait-signature")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                GatewayError::unauthorized(
                    "WAIT_SIGNATURE_REQUIRED",
                    "Wait resume signature is required",
                )
            })?;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(state.wait_resume_secret.expose_secret().as_bytes())
                .map_err(GatewayError::internal)?;
        mac.update(binding_id.to_string().as_bytes());
        mac.update(b".");
        mac.update(body.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        if !constant_time_eq(signature.as_bytes(), expected.as_bytes()) {
            return Err(GatewayError::unauthorized(
                "INVALID_WAIT_SIGNATURE",
                "Wait resume signature is invalid",
            ));
        }
    } else if matches!(authentication.as_str(), "header" | "basic") {
        let presented = if authentication == "header" {
            headers.get("x-agentx-wait-auth")
        } else {
            headers.get(AUTHORIZATION)
        }
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            GatewayError::unauthorized(
                "WAIT_AUTH_REQUIRED",
                "Wait resume authentication is required",
            )
        })?;
        let actual = format!("{:x}", Sha256::digest(presented.as_bytes()));
        let expected: String = row
            .try_get::<Option<String>, _>("authentication_config_hash")?
            .ok_or_else(|| {
                GatewayError::unauthorized(
                    "WAIT_AUTH_INVALID",
                    "Wait authentication is not configured",
                )
            })?;
        if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
            return Err(GatewayError::unauthorized(
                "WAIT_AUTH_INVALID",
                "Wait resume authentication is invalid",
            ));
        }
    }
    let input: WaitResumeRequest = serde_json::from_str(&body).map_err(|error| {
        GatewayError::bad_request("INVALID_WAIT_RESUME_BODY", error.to_string())
    })?;
    let output_port = input.output_port.as_deref().unwrap_or("resumed");
    if output_port != "resumed" {
        return Err(GatewayError::bad_request(
            "WAIT_OUTPUT_PORT_INVALID",
            "External wait resume can only use the resumed output",
        ));
    }
    if row.try_get::<String, _>("wait_kind")? == "form" {
        let schema = row
            .try_get::<Option<Value>, _>("payload_schema_json")?
            .ok_or_else(|| {
                GatewayError::bad_request(
                    "FORM_SCHEMA_MISSING",
                    "Form wait payload schema is missing",
                )
            })?;
        let validator = jsonschema::validator_for(&schema)
            .map_err(|error| GatewayError::bad_request("FORM_SCHEMA_INVALID", error.to_string()))?;
        if let Err(error) = validator.validate(&input.payload) {
            return Err(GatewayError::bad_request(
                "FORM_PAYLOAD_INVALID",
                error.to_string(),
            ));
        }
    }
    let idempotency_key = validate_idempotency(&headers)?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let execution_id: Uuid = row.try_get("execution_id")?;
    let command = RuntimeCommand::new(
        TenantId::from_uuid(tenant_id),
        RuntimeCommandType::ResumeExecution,
        "wait_subscription",
        binding_id.to_string(),
        format!("wait:{binding_id}:{idempotency_key}"),
        serde_json::to_value(ResumeExecutionCommandPayload {
            execution_id,
            node_execution_id: row.try_get("node_execution_id")?,
            resume_token: binding_id.to_string(),
            output_port: output_port.into(),
            payload: input.payload,
        })
        .map_err(GatewayError::internal)?,
    );
    let commands = RuntimeCommandRepository::new(state.pool.clone());
    let wait_status: String = row.try_get("status")?;
    if wait_status != "waiting" {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_commands WHERE tenant_id=? AND command_type=? AND idempotency_key=?)")
            .bind(tenant_id)
            .bind(RuntimeCommandType::ResumeExecution.as_str())
            .bind(&command.idempotency_key)
            .fetch_one(&state.pool)
            .await?;
        if !exists {
            return Err(GatewayError::new(
                StatusCode::CONFLICT,
                "WAIT_NOT_RESUMABLE",
                format!("Wait subscription is {wait_status}"),
            ));
        }
    }
    let replayed = !commands
        .enqueue(&command)
        .await
        .map_err(GatewayError::internal)?;
    Ok(Json(WaitResumeResponse {
        accepted: true,
        replayed,
    }))
}

#[utoipa::path(post, path = "/gateway/v1/webhooks/{public_id}")]
async fn webhook_trigger(
    State(state): State<GatewayState>,
    Path(public_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> GatewayResult<(StatusCode, Json<InvocationResponse>)> {
    let row=sqlx::query("SELECT w.id,w.tenant_id,w.application_id,w.secret_provider,w.secret_ref,w.secret_provider_version,w.secret_key_id,w.secret_nonce,w.secret_ciphertext,h.deployment_id,ad.workflow_version_id FROM application_webhooks w JOIN applications a ON a.id=w.application_id JOIN application_deployment_heads h ON h.application_id=a.id AND h.tenant_id=a.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id WHERE w.public_id=? AND w.status='active' AND a.status='active'").bind(&public_id).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Webhook"))?;
    let timestamp = headers
        .get("x-agentx-timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| {
            GatewayError::unauthorized("INVALID_WEBHOOK_SIGNATURE", "Webhook timestamp is required")
        })?;
    if (OffsetDateTime::now_utc().unix_timestamp() - timestamp).abs() > 300 {
        return Err(GatewayError::unauthorized(
            "WEBHOOK_REPLAY_WINDOW",
            "Webhook timestamp is outside the replay window",
        ));
    }
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let webhook_id: Uuid = row.try_get("id")?;
    let secret = state
        .webhook_secrets
        .resolve(&row, tenant_id, webhook_id)
        .await?;
    let signature = headers
        .get("x-agentx-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            GatewayError::unauthorized("INVALID_WEBHOOK_SIGNATURE", "Webhook signature is required")
        })?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.expose()).map_err(GatewayError::internal)?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if !constant_time_eq(signature.as_bytes(), expected.as_bytes()) {
        return Err(GatewayError::unauthorized(
            "INVALID_WEBHOOK_SIGNATURE",
            "Webhook signature is invalid",
        ));
    }
    let application_id: Uuid = row.try_get("application_id")?;
    let application_deployment_id: Uuid = row.try_get("deployment_id")?;
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let idempotency_key = validate_idempotency(&headers)?;
    let input = serde_json::from_str(&body)
        .map_err(|error| GatewayError::bad_request("INVALID_WEBHOOK_BODY", error.to_string()))?;
    let caller = Caller::Webhook {
        tenant_id,
        application_id,
        webhook_id,
    };
    let response = request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            application_deployment_id,
            session_id: None,
            workflow_version_id,
            input,
            idempotency_key,
            message_parts: None,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, response))
}

async fn authenticate_jwt(state: &GatewayState, token: &str) -> GatewayResult<Caller> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[&state.jwt.issuer]);
    validation.set_audience(&[&state.jwt.audience]);
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt.secret.expose_secret().as_bytes()),
        &validation,
    )
    .map_err(|_| GatewayError::unauthorized("INVALID_TOKEN", "Token is invalid or expired"))?
    .claims;
    if claims.kind != "access" {
        return Err(GatewayError::unauthorized(
            "INVALID_TOKEN",
            "Token type is invalid",
        ));
    }
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN user_roles ur ON ur.user_id=u.id AND ur.tenant_id=u.tenant_id JOIN role_permissions rp ON rp.role_id=ur.role_id JOIN permissions p ON p.id=rp.permission_id WHERE u.id=? AND u.tenant_id=? AND u.status='active' AND u.token_version=? AND p.permission_key='application:invoke')").bind(claims.sub).bind(claims.tid).bind(claims.ver).fetch_one(&state.pool).await?;
    if !allowed {
        return Err(GatewayError::forbidden(
            "Missing permission application:invoke",
        ));
    }
    Ok(Caller::User {
        tenant_id: claims.tid,
        user_id: claims.sub,
    })
}
async fn authenticate_api_key(state: &GatewayState, token: &str) -> GatewayResult<Caller> {
    let (prefix, _) = token
        .rsplit_once('_')
        .ok_or_else(|| GatewayError::unauthorized("INVALID_API_KEY", "API key is invalid"))?;
    let row=sqlx::query("SELECT id,tenant_id,application_id,secret_hash FROM application_api_keys WHERE key_prefix=? AND status='active'").bind(prefix).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::unauthorized("INVALID_API_KEY","API key is invalid"))?;
    let expected: Vec<u8> = row.try_get("secret_hash")?;
    let actual = Sha256::digest(token.as_bytes());
    if !constant_time_eq(&expected, actual.as_slice()) {
        return Err(GatewayError::unauthorized(
            "INVALID_API_KEY",
            "API key is invalid",
        ));
    }
    let id: Uuid = row.try_get("id")?;
    sqlx::query("UPDATE application_api_keys SET last_used_at=CURRENT_TIMESTAMP(6) WHERE id=?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Caller::ApiKey {
        tenant_id: row.try_get("tenant_id")?,
        application_id: row.try_get("application_id")?,
        key_id: id,
    })
}
async fn load_active_application(
    state: &GatewayState,
    caller: &Caller,
    slug: &str,
) -> GatewayResult<sqlx::mysql::MySqlRow> {
    let row=sqlx::query("SELECT a.id,a.tenant_id,h.deployment_id,ad.workflow_version_id,ad.session_version_policy,ad.input_schema_json FROM applications a JOIN application_deployment_heads h ON h.application_id=a.id AND h.tenant_id=a.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id WHERE a.slug=? AND a.tenant_id=? AND a.status='active'").bind(slug).bind(caller.tenant_id()).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Application"))?;
    authorize_application(state, caller, row.try_get("id")?).await?;
    Ok(row)
}
async fn authorize_application(
    state: &GatewayState,
    caller: &Caller,
    id: Uuid,
) -> GatewayResult<()> {
    if let Some(allowed) = caller.application_id() {
        return if allowed == id {
            Ok(())
        } else {
            Err(GatewayError::not_found("Application"))
        };
    }
    let Some(user_id) = caller.user_id() else {
        return Err(GatewayError::not_found("Application"));
    };
    let visible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM applications a WHERE a.id=? AND a.tenant_id=? AND (a.visibility='company' OR a.owner_user_id=? OR EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id WHERE ur.tenant_id=a.tenant_id AND ur.user_id=? AND (r.data_scope='company' OR (ur.scope_department_id IS NOT NULL AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=a.tenant_id AND dc.ancestor_id=ur.scope_department_id AND dc.descendant_id=a.owner_department_id)))) OR (a.visibility='department' AND EXISTS(SELECT 1 FROM user_departments ud JOIN department_closure dc ON dc.tenant_id=ud.tenant_id AND ((dc.ancestor_id=a.owner_department_id AND dc.descendant_id=ud.department_id) OR (dc.ancestor_id=ud.department_id AND dc.descendant_id=a.owner_department_id)) WHERE ud.tenant_id=a.tenant_id AND ud.user_id=?))))",
    )
    .bind(id)
    .bind(caller.tenant_id())
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if visible {
        Ok(())
    } else {
        Err(GatewayError::not_found("Application"))
    }
}
async fn load_session(
    state: &GatewayState,
    tenant_id: Uuid,
    id: Uuid,
) -> GatewayResult<SessionResponse> {
    let row=sqlx::query("SELECT id,application_id,application_deployment_id,workflow_version_id,version_policy,external_user_id,title,status,version,updated_at FROM application_sessions WHERE id=? AND tenant_id=?").bind(id).bind(tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Session"))?;
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
fn invocation_from_row(row: sqlx::mysql::MySqlRow) -> GatewayResult<InvocationResponse> {
    Ok(InvocationResponse {
        id: row.try_get("id")?,
        application_id: row.try_get("application_id")?,
        session_id: row.try_get("session_id")?,
        execution_id: row.try_get("execution_id")?,
        status: row.try_get("status")?,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
    })
}
fn validate_idempotency(headers: &HeaderMap) -> GatewayResult<&str> {
    let value = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or_else(|| {
            GatewayError::bad_request("IDEMPOTENCY_KEY_REQUIRED", "Idempotency-Key is required")
        })?;
    Ok(value)
}
fn validate_response_mode(value: Option<&str>) -> GatewayResult<()> {
    if value.is_none_or(|v| matches!(v, "sync" | "async")) {
        Ok(())
    } else {
        Err(GatewayError::bad_request(
            "INVALID_RESPONSE_MODE",
            "Response mode must be sync or async",
        ))
    }
}
fn validate_input(input: &Value, schema: Value) -> GatewayResult<()> {
    if schema.get("type").and_then(Value::as_str) == Some("object") && !input.is_object() {
        return Err(GatewayError::bad_request(
            "INPUT_SCHEMA_VALIDATION_FAILED",
            "Input must be an object",
        ));
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            if input.get(field).is_none() {
                return Err(GatewayError::bad_request(
                    "INPUT_SCHEMA_VALIDATION_FAILED",
                    format!("Required field {field} is missing"),
                ));
            }
        }
    }
    Ok(())
}
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::{
        InvocationResponse, constant_time_eq, last_event_cursor, stable_invocation_id,
        validate_input,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;
    use uuid::Uuid;
    #[test]
    fn validates_input_and_constant_time_comparison() {
        assert!(
            validate_input(
                &json!({"name":"ok"}),
                json!({"type":"object","required":["name"]})
            )
            .is_ok()
        );
        assert!(validate_input(&json!({}), json!({"type":"object","required":["name"]})).is_err());
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
    }

    #[test]
    fn invocation_identity_is_stable_and_scoped() {
        let tenant_id = Uuid::now_v7();
        let application_id = Uuid::now_v7();
        let caller_id = Uuid::now_v7();
        let first = stable_invocation_id(
            tenant_id,
            application_id,
            "user",
            Some(caller_id),
            "request-1",
        );
        let retry = stable_invocation_id(
            tenant_id,
            application_id,
            "user",
            Some(caller_id),
            "request-1",
        );
        let other = stable_invocation_id(
            tenant_id,
            application_id,
            "user",
            Some(caller_id),
            "request-2",
        );
        assert_eq!(first, retry);
        assert_ne!(first, other);
    }

    #[test]
    fn last_event_id_is_a_monotonic_numeric_cursor() {
        let mut headers = HeaderMap::new();
        assert_eq!(last_event_cursor(&headers), 0);
        headers.insert("last-event-id", HeaderValue::from_static("42"));
        assert_eq!(last_event_cursor(&headers), 42);
        headers.insert("last-event-id", HeaderValue::from_static("invalid"));
        assert_eq!(last_event_cursor(&headers), 0);
    }

    #[test]
    fn invocation_response_exposes_structured_terminal_errors() {
        let value = serde_json::to_value(InvocationResponse {
            id: Uuid::nil(),
            application_id: Uuid::nil(),
            session_id: None,
            execution_id: Some(Uuid::nil()),
            status: "failed".into(),
            error_code: Some("APPLICATION_PRIMARY_OUTPUT_NOT_REACHED".into()),
            error_message: Some("Primary output was not reached".into()),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        })
        .unwrap();
        assert_eq!(value["errorCode"], "APPLICATION_PRIMARY_OUTPUT_NOT_REACHED");
        assert_eq!(value["errorMessage"], "Primary output was not reached");
    }
}
