use std::{convert::Infallible, env, sync::Arc, time::Duration};

use agentx_api_types::{ApiErrorResponse, FieldError};
use agentx_application::{
    AcceptedExecution, ExecutionRuntime, RequestExecution, ResumeExecutionCommand,
};
use agentx_domain::{
    ExecutionId, ExecutionStatus, InvocationId, SessionId, TenantId, WorkflowVersionId,
};
use agentx_infrastructure::{config::MySqlSettings, credential::CredentialKeyring, mysql};
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
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
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

#[derive(Clone)]
struct GatewayState {
    pool: MySqlPool,
    jwt: Arc<JwtSettings>,
    keyring: Arc<CredentialKeyring>,
    runtime: Arc<dyn ExecutionRuntime>,
    resume_runtime: Arc<dyn ExecutionRuntime>,
    wait_resume_secret: Arc<SecretString>,
}

struct UnavailableExecutionRuntime;
#[async_trait::async_trait]
impl ExecutionRuntime for UnavailableExecutionRuntime {
    async fn request_execution(&self, _request: RequestExecution) -> Result<AcceptedExecution> {
        anyhow::bail!("RUNTIME_UNAVAILABLE")
    }
    async fn get_execution(
        &self,
        _tenant_id: TenantId,
        _id: ExecutionId,
    ) -> Result<Option<ExecutionStatus>> {
        anyhow::bail!("RUNTIME_UNAVAILABLE")
    }
    async fn cancel_execution(&self, _tenant_id: TenantId, _id: ExecutionId) -> Result<()> {
        anyhow::bail!("RUNTIME_UNAVAILABLE")
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
    fn not_found(resource: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("{resource} was not found"),
        )
    }
    fn runtime_unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "RUNTIME_UNAVAILABLE",
            "Workflow Runtime is not available yet",
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

struct InvocationCommand<'a> {
    application_id: Uuid,
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
#[openapi(paths(create_session,get_session,create_invocation,send_message,get_invocation,cancel_invocation,invocation_events,webhook_trigger,resume_wait),components(schemas(CreateSessionRequest,SessionResponse,InvocationRequest,MessageRequest,MessagePartInput,InvocationResponse,WaitResumeRequest,WaitResumeResponse,ApiErrorResponse,FieldError)),tags((name="Agentx Gateway",description="Application invocation and runtime wait API")))]
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
    let key_id = env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")
        .context("AGENTX_CREDENTIAL_ACTIVE_KEY_ID is required")?;
    let keys = SecretString::from(
        env::var("AGENTX_CREDENTIAL_KEYS_JSON")
            .context("AGENTX_CREDENTIAL_KEYS_JSON is required")?,
    );
    let state = GatewayState {
        pool: pool.clone(),
        jwt: Arc::new(JwtSettings::from_env()?),
        keyring: Arc::new(CredentialKeyring::from_json(key_id, &keys)?),
        runtime: Arc::new(UnavailableExecutionRuntime),
        resume_runtime: Arc::new(
            agentx_infrastructure::runtime_client::GrpcExecutionRuntime::connect(
                &env::var("AGENTX_RUNTIME_COORDINATOR_URL")
                    .unwrap_or_else(|_| "http://workflow-coordinator:9090".into()),
                pool.clone(),
            )?,
        ),
        wait_resume_secret: Arc::new(SecretString::from(
            env::var("AGENTX_WAIT_RESUME_SECRET").unwrap_or_else(|_| {
                env::var("AGENTX_JWT_SIGNING_SECRET")
                    .unwrap_or_else(|_| "development-wait-resume-secret-change-me".into())
            }),
        )),
    };
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
                .route("/sessions/{id}/messages", post(send_message))
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

#[utoipa::path(post,path="/gateway/v1/applications/{slug}/invocations",request_body=InvocationRequest)]
async fn create_invocation(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(input): Json<InvocationRequest>,
) -> GatewayResult<Json<InvocationResponse>> {
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
    let workflow_version_id: Uuid = app.try_get("workflow_version_id")?;
    request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            session_id: input.session_id,
            workflow_version_id,
            input: input.input,
            idempotency_key,
            message_parts: None,
        },
    )
    .await
}

#[utoipa::path(post,path="/gateway/v1/sessions/{id}/messages",request_body=MessageRequest)]
async fn send_message(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<MessageRequest>,
) -> GatewayResult<Json<InvocationResponse>> {
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
    let row=sqlx::query("SELECT a.id application_id,COALESCE(s.workflow_version_id,ad.workflow_version_id) workflow_version_id FROM application_sessions s JOIN applications a ON a.id=s.application_id AND a.tenant_id=s.tenant_id JOIN application_deployment_heads h ON h.application_id=a.id AND h.tenant_id=a.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id WHERE s.id=? AND s.tenant_id=? AND s.status='active' AND a.status='active'")
        .bind(id).bind(caller.tenant_id()).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Session"))?;
    let application_id: Uuid = row.try_get("application_id")?;
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let input_value = serde_json::to_value(&input.parts).map_err(GatewayError::internal)?;
    let response = request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            session_id: Some(id),
            workflow_version_id,
            input: json!({"parts":input_value}),
            idempotency_key,
            message_parts: Some(&input.parts),
        },
    )
    .await?;
    Ok(response)
}

async fn request_invocation(
    state: &GatewayState,
    caller: &Caller,
    command: InvocationCommand<'_>,
) -> GatewayResult<Json<InvocationResponse>> {
    let InvocationCommand {
        application_id,
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
    let mut tx = state.pool.begin().await?;
    let insert_result = sqlx::query("INSERT INTO application_invocations(id,tenant_id,application_id,session_id,workflow_version_id,execution_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(?,?,?,?,?,NULL,?,?,?,?, 'queued')")
        .bind(invocation_id.as_uuid()).bind(caller.tenant_id()).bind(application_id).bind(session_id).bind(workflow_version_id)
        .bind(caller_type).bind(caller_id).bind(&request_hash).bind(idempotency_key).execute(&mut *tx).await;
    let inserted = match insert_result {
        Ok(_) => true,
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => false,
        Err(error) => return Err(error.into()),
    };
    if !inserted {
        let row=sqlx::query("SELECT id,application_id,session_id,execution_id,status,created_at,request_hash FROM application_invocations WHERE tenant_id=? AND application_id=? AND caller_type=? AND caller_id <=> ? AND idempotency_key=?")
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
    let accepted = match state
        .runtime
        .request_execution(RequestExecution {
            tenant_id: TenantId::from_uuid(caller.tenant_id()),
            invocation_id: Some(invocation_id),
            session_id: session_id.map(SessionId::from_uuid),
            workflow_version_id: WorkflowVersionId::from_uuid(workflow_version_id),
            requested_by: caller.user_id().map(agentx_domain::UserId::from_uuid),
            trigger_type: "application".into(),
            input,
            idempotency_key: Some(idempotency_key.into()),
        })
        .await
    {
        Ok(accepted) => accepted,
        Err(_) => {
            tx.rollback().await?;
            return Err(GatewayError::runtime_unavailable());
        }
    };
    let status = execution_status_name(&accepted.status);
    sqlx::query(
        "UPDATE application_invocations SET execution_id=?,status=? WHERE id=? AND tenant_id=?",
    )
    .bind(accepted.execution_id.as_uuid())
    .bind(status)
    .bind(invocation_id.as_uuid())
    .bind(caller.tenant_id())
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,1,'invocation.accepted',?)")
        .bind(caller.tenant_id()).bind(invocation_id.as_uuid()).bind(json!({"executionId":accepted.execution_id.as_uuid(),"status":status})).execute(&mut *tx).await?;
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
        }
    }
    let row=sqlx::query("SELECT id,application_id,session_id,execution_id,status,created_at FROM application_invocations WHERE id=? AND tenant_id=?")
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

fn execution_status_name(status: &ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Queued | ExecutionStatus::Created => "queued",
        ExecutionStatus::Running
        | ExecutionStatus::Waiting
        | ExecutionStatus::WaitingApproval
        | ExecutionStatus::Suspended => "running",
        ExecutionStatus::Succeeded => "completed",
        ExecutionStatus::Failed | ExecutionStatus::TimedOut => "failed",
        ExecutionStatus::Cancelled => "cancelled",
    }
}

#[utoipa::path(get, path = "/gateway/v1/invocations/{id}")]
async fn get_invocation(
    State(state): State<GatewayState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> GatewayResult<Json<InvocationResponse>> {
    let row=sqlx::query("SELECT id,application_id,session_id,execution_id,status,created_at FROM application_invocations WHERE id=? AND tenant_id=?").bind(id).bind(caller.tenant_id()).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Invocation"))?;
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
    let execution_id = invocation
        .execution_id
        .ok_or_else(GatewayError::runtime_unavailable)?;
    state
        .runtime
        .cancel_execution(
            TenantId::from_uuid(caller.tenant_id()),
            ExecutionId::from_uuid(execution_id),
        )
        .await
        .map_err(|_| GatewayError::runtime_unavailable())?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE application_invocations SET status='cancelled',completed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND tenant_id=? AND status NOT IN ('completed','failed','cancelled')").bind(id).bind(caller.tenant_id()).execute(&mut *tx).await?;
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=?").bind(caller.tenant_id()).bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,?,'invocation.cancelled',JSON_OBJECT())").bind(caller.tenant_id()).bind(id).bind(sequence).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
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
    let replayed = state
        .resume_runtime
        .resume_execution(ResumeExecutionCommand {
            tenant_id: TenantId::from_uuid(row.try_get("tenant_id")?),
            execution_id: ExecutionId::from_uuid(row.try_get("execution_id")?),
            node_execution_id: agentx_domain::NodeExecutionId::from_uuid(
                row.try_get("node_execution_id")?,
            ),
            resume_token: binding_id.to_string(),
            output_port: output_port.into(),
            payload: input.payload,
            idempotency_key: idempotency_key.into(),
            actor_user_id: None,
        })
        .await
        .map_err(|_| GatewayError::runtime_unavailable())?;
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
) -> GatewayResult<Json<InvocationResponse>> {
    let row=sqlx::query("SELECT w.id,w.tenant_id,w.application_id,w.secret_key_id,w.secret_nonce,w.secret_ciphertext,w.version,ad.workflow_version_id FROM application_webhooks w JOIN applications a ON a.id=w.application_id JOIN application_deployment_heads h ON h.application_id=a.id AND h.tenant_id=a.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id WHERE w.public_id=? AND w.status='active' AND a.status='active'").bind(&public_id).fetch_optional(&state.pool).await?.ok_or_else(||GatewayError::not_found("Webhook"))?;
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
    let version: u64 = row.try_get("version")?;
    let aad = format!("{tenant_id}/{webhook_id}/webhook/{version}");
    let secret = state
        .keyring
        .decrypt(
            row.try_get("secret_key_id")?,
            row.try_get::<Vec<u8>, _>("secret_nonce")?.as_slice(),
            row.try_get::<Vec<u8>, _>("secret_ciphertext")?.as_slice(),
            aad.as_bytes(),
        )
        .map_err(GatewayError::internal)?;
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
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let idempotency_key = validate_idempotency(&headers)?;
    let input = serde_json::from_str(&body)
        .map_err(|error| GatewayError::bad_request("INVALID_WEBHOOK_BODY", error.to_string()))?;
    let caller = Caller::Webhook {
        tenant_id,
        application_id,
        webhook_id,
    };
    request_invocation(
        &state,
        &caller,
        InvocationCommand {
            application_id,
            session_id: None,
            workflow_version_id,
            input,
            idempotency_key,
            message_parts: None,
        },
    )
    .await
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
    use super::{constant_time_eq, last_event_cursor, stable_invocation_id, validate_input};
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
}
