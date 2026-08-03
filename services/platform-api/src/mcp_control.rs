use std::{
    pin::Pin,
    time::{Duration, Instant},
};

use agentx_api_types::PageResponse;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
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

const MAX_MCP_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERED_TOOLS: usize = 500;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpServerResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_department_id: Uuid,
    pub transport: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    pub status: String,
    pub current_version_number: u64,
    pub version: u64,
    pub tool_count: u64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_discovered_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type McpServerPage = PageResponse<McpServerResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMcpServerRequest {
    pub name: String,
    pub description: Option<String>,
    pub owner_department_id: Uuid,
    pub transport: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMcpServerRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub transport: String,
    pub endpoint: String,
    pub credential_id: Option<Uuid>,
    #[serde(default)]
    pub configuration: Value,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpToolResponse {
    pub id: Uuid,
    pub server_id: Uuid,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub availability: String,
    pub current_version_id: Uuid,
    pub current_version_number: u64,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub annotations: Value,
    pub schema_hash: String,
    pub enabled: bool,
    pub debug_enabled: bool,
    pub timeout_seconds: u32,
    pub side_effect: String,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMcpToolPolicyRequest {
    pub enabled: bool,
    pub debug_enabled: bool,
    pub timeout_seconds: u32,
    pub side_effect: String,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugMcpToolRequest {
    pub arguments: Value,
    pub expected_tool_version_id: Uuid,
    pub confirmed: bool,
    pub confirmation_text: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugMcpToolResponse {
    pub result: Value,
    pub duration_ms: u64,
    pub tool_version_id: Uuid,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpDiscoveryResponse {
    pub run_id: Uuid,
    pub discovered_count: u32,
    pub tools: Vec<McpToolResponse>,
}

#[utoipa::path(get, path = "/api/v1/mcp/servers")]
pub async fn list_servers(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<McpServerListQuery>,
) -> AppResult<Json<McpServerPage>> {
    actor.require("mcp:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let base = "SELECT s.id,s.name,s.description,s.owner_department_id,s.status,s.current_version_number,s.version,s.last_discovered_at,s.updated_at,sv.transport,sv.endpoint,sv.credential_id,(SELECT COUNT(*) FROM mcp_tools t WHERE t.server_id=s.id AND t.availability='available') tool_count,COUNT(*) OVER() total_count FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number";
    let rows = if actor.company_admin {
        sqlx::query(&format!("{base} WHERE s.tenant_id=? AND (?='' OR s.status=?) AND (?='%%' OR s.name LIKE ?) ORDER BY s.updated_at DESC LIMIT ? OFFSET ?"))
            .bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(u64::from((page-1)*page_size)).fetch_all(&state.pool).await?
    } else {
        sqlx::query(&format!("{base} WHERE s.tenant_id=? AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=s.tenant_id AND dc.descendant_id=s.owner_department_id) AND (?='' OR s.status=?) AND (?='%%' OR s.name LIKE ?) ORDER BY s.updated_at DESC LIMIT ? OFFSET ?"))
            .bind(actor.tenant_id).bind(actor.user_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(u64::from((page-1)*page_size)).fetch_all(&state.pool).await?
    };
    let total = rows
        .first()
        .map_or(Ok(0_i64), |r| r.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(server_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(post,path="/api/v1/mcp/servers",request_body=CreateMcpServerRequest)]
pub async fn create_server(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateMcpServerRequest>,
) -> AppResult<(StatusCode, Json<McpServerResponse>)> {
    actor.require("mcp:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    validate_server_config(
        &state,
        &input.transport,
        &input.endpoint,
        &input.configuration,
    )
    .await?;
    let name = validate_name(&input.name, 160)?;
    let id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let hash = config_hash(
        &input.transport,
        &input.endpoint,
        input.credential_id,
        &input.configuration,
    )?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO mcp_servers(id,tenant_id,name,description,owner_department_id,created_by) VALUES(?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(name).bind(input.description).bind(input.owner_department_id).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_server_versions(id,tenant_id,server_id,version_number,transport,endpoint,credential_id,configuration_json,configuration_hash,created_by) VALUES(?,?,?,1,?,?,?,?,?,?)").bind(version_id).bind(actor.tenant_id).bind(id).bind(input.transport).bind(input.endpoint).bind(input.credential_id).bind(input.configuration).bind(hash).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "mcp.server_created",
        "mcp_server",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_server(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get,path="/api/v1/mcp/servers/{id}",params(("id"=Uuid,Path)))]
pub async fn get_server(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<McpServerResponse>> {
    actor.require("mcp:view")?;
    require_resource_visible(&state, &actor, "mcp_server", id).await?;
    Ok(Json(load_server(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(patch,path="/api/v1/mcp/servers/{id}",request_body=UpdateMcpServerRequest,params(("id"=Uuid,Path)))]
pub async fn update_server(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateMcpServerRequest>,
) -> AppResult<Json<McpServerResponse>> {
    actor.require("mcp:manage")?;
    require_resource_visible(&state, &actor, "mcp_server", id).await?;
    credentials::require_reference(&state, &actor, input.credential_id).await?;
    validate_server_config(
        &state,
        &input.transport,
        &input.endpoint,
        &input.configuration,
    )
    .await?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "MCP server status is invalid",
        ));
    }
    let name = validate_name(&input.name, 160)?;
    let hash = config_hash(
        &input.transport,
        &input.endpoint,
        input.credential_id,
        &input.configuration,
    )?;
    let mut tx = state.pool.begin().await?;
    let current:Option<(u64,u64)>=sqlx::query_as("SELECT current_version_number,version FROM mcp_servers WHERE tenant_id=? AND id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_optional(&mut *tx).await?;
    let (current_number, current_version) =
        current.ok_or_else(|| AppError::not_found("MCP server"))?;
    if current_version != input.version {
        return Err(AppError::conflict(
            "MCP_SERVER_VERSION_CONFLICT",
            "MCP server changed",
        ));
    }
    let next = current_number + 1;
    sqlx::query("INSERT INTO mcp_server_versions(id,tenant_id,server_id,version_number,transport,endpoint,credential_id,configuration_json,configuration_hash,created_by) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(next).bind(input.transport).bind(input.endpoint).bind(input.credential_id).bind(input.configuration).bind(hash).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE mcp_servers SET name=?,description=?,status=?,current_version_number=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?").bind(name).bind(input.description).bind(input.status).bind(next).bind(actor.tenant_id).bind(id).bind(input.version).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "mcp.server_updated",
        "mcp_server",
        id,
        json!({"serverVersion":next}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_server(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get,path="/api/v1/mcp/servers/{id}/tools",params(("id"=Uuid,Path)))]
pub async fn list_tools(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<McpToolResponse>>> {
    actor.require("mcp:view")?;
    require_resource_visible(&state, &actor, "mcp_server", id).await?;
    Ok(Json(load_tools(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get, path = "/api/v1/mcp/tools")]
pub async fn list_all_tools(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<McpToolResponse>>> {
    actor.require("mcp:view")?;
    let sql = if actor.company_admin {
        format!("{TOOL_SELECT_PREFIX} WHERE t.tenant_id=? ORDER BY s.name,t.name")
    } else {
        format!(
            "{TOOL_SELECT_PREFIX} WHERE t.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=t.tenant_id AND ur.user_id=? AND dc.descendant_id=s.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg WHERE rg.tenant_id=t.tenant_id AND rg.subject_type='department' AND rg.subject_id=? AND rg.resource_type='mcp_tool' AND rg.resource_id=t.id AND rg.operation_key IN ('view','use','manage'))) ORDER BY s.name,t.name"
        )
    };
    let mut query = sqlx::query(&sql).bind(actor.tenant_id);
    if !actor.company_admin {
        query = query.bind(actor.user_id).bind(actor.department_id);
    }
    let rows = query.fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(tool_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/mcp/servers/{id}/test-connection",params(("id"=Uuid,Path)))]
pub async fn test_connection(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<HealthCheckResponse>> {
    actor.require("mcp:manage")?;
    require_resource_visible(&state, &actor, "mcp_server", id).await?;
    let config = load_current_config(&state, actor.tenant_id, id).await?;
    let started = Instant::now();
    let checked_at = OffsetDateTime::now_utc();
    let result = mcp_initialize(&state, actor.tenant_id, &config).await;
    let (status, error_code, error_message) = match result {
        Ok(_) => ("healthy", None, None),
        Err(error) => (
            "unhealthy",
            Some("MCP_CONNECTION_FAILED".to_owned()),
            Some(error.message),
        ),
    };
    let latency = started.elapsed().as_millis() as u64;
    let sequence = u64::try_from(checked_at.unix_timestamp_nanos()).unwrap_or(u64::MAX);
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO resource_health_checks(id,tenant_id,resource_type,resource_id,check_sequence,status,latency_ms,error_code,error_message,checked_by,checked_at) VALUES(?,?, 'mcp_server',?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(sequence).bind(status).bind(latency).bind(&error_code).bind(&error_message).bind(actor.user_id).bind(checked_at).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "mcp.connection_tested",
        "mcp_server",
        id,
        json!({"status":status,"latencyMs":latency}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(HealthCheckResponse {
        status: status.to_owned(),
        latency_ms: Some(latency),
        error_code,
        error_message,
        checked_at,
    }))
}

#[utoipa::path(post,path="/api/v1/mcp/servers/{id}/discover",params(("id"=Uuid,Path)))]
pub async fn discover_tools(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<McpDiscoveryResponse>> {
    actor.require("mcp:discover")?;
    require_resource_visible(&state, &actor, "mcp_server", id).await?;
    let config = load_current_config(&state, actor.tenant_id, id).await?;
    let run_id = Uuid::now_v7();
    sqlx::query("INSERT INTO mcp_discovery_runs(id,tenant_id,server_id,server_version_id,status,started_by) VALUES(?,?,?,?,'running',?)").bind(run_id).bind(actor.tenant_id).bind(id).bind(config.version_id).bind(actor.user_id).execute(&state.pool).await?;
    let discovered = match discover_remote(&state, actor.tenant_id, &config).await {
        Ok(value) => value,
        Err(error) => {
            sqlx::query("UPDATE mcp_discovery_runs SET status='failed',error_code='MCP_DISCOVERY_FAILED',error_message=?,finished_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(&error.message).bind(run_id).execute(&state.pool).await?;
            return Err(error);
        }
    };
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE mcp_tools SET availability='unavailable' WHERE tenant_id=? AND server_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    for tool in &discovered {
        upsert_tool(&mut tx, &actor, id, run_id, tool).await?;
    }
    sqlx::query("UPDATE mcp_discovery_runs SET status='succeeded',discovered_count=?,finished_at=CURRENT_TIMESTAMP(6) WHERE id=?").bind(discovered.len() as u32).bind(run_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE mcp_servers SET last_discovered_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &actor,
        "mcp.tools_discovered",
        "mcp_server",
        id,
        json!({"runId":run_id,"count":discovered.len()}),
    )
    .await?;
    tx.commit().await?;
    let tools = load_tools(&state, actor.tenant_id, id).await?;
    Ok(Json(McpDiscoveryResponse {
        run_id,
        discovered_count: tools.len() as u32,
        tools,
    }))
}

#[utoipa::path(patch,path="/api/v1/mcp/tools/{id}/policy",request_body=UpdateMcpToolPolicyRequest,params(("id"=Uuid,Path)))]
pub async fn update_tool_policy(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateMcpToolPolicyRequest>,
) -> AppResult<Json<McpToolResponse>> {
    actor.require("mcp:manage")?;
    require_resource_visible(&state, &actor, "mcp_tool", id).await?;
    if !(1..=120).contains(&input.timeout_seconds)
        || !matches!(
            input.side_effect.as_str(),
            "unknown" | "none" | "read_only" | "idempotent" | "non_idempotent" | "irreversible"
        )
    {
        return Err(AppError::bad_request(
            "INVALID_MCP_POLICY",
            "MCP tool policy is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE mcp_tool_policies SET enabled=?,debug_enabled=?,timeout_seconds=?,side_effect=?,version=version+1,updated_by=? WHERE tenant_id=? AND tool_id=? AND version=?").bind(input.enabled).bind(input.debug_enabled).bind(input.timeout_seconds).bind(input.side_effect).bind(actor.user_id).bind(actor.tenant_id).bind(id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "MCP_TOOL_POLICY_CONFLICT",
            "MCP tool policy changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "mcp.tool_policy_updated",
        "mcp_tool",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_tool(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/mcp/tools/{id}/debug-invoke",request_body=DebugMcpToolRequest,params(("id"=Uuid,Path)))]
pub async fn debug_invoke(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<DebugMcpToolRequest>,
) -> AppResult<Json<DebugMcpToolResponse>> {
    actor.require("mcp:debug")?;
    require_resource_visible(&state, &actor, "mcp_tool", id).await?;
    let tool = load_tool(&state, actor.tenant_id, id).await?;
    if !tool.debug_enabled || !tool.enabled || tool.availability != "available" {
        return Err(AppError::unprocessable(
            "MCP_DEBUG_DISABLED",
            "MCP tool debugging is disabled",
        ));
    }
    if input.expected_tool_version_id != tool.current_version_id {
        return Err(AppError::conflict(
            "MCP_TOOL_VERSION_CONFLICT",
            "MCP tool schema changed",
        ));
    }
    if !input.confirmed {
        return Err(AppError::bad_request(
            "MCP_CONFIRMATION_REQUIRED",
            "MCP tool invocation requires confirmation",
        ));
    }
    if matches!(
        tool.side_effect.as_str(),
        "unknown" | "non_idempotent" | "irreversible"
    ) && input.confirmation_text.as_deref() != Some(tool.name.as_str())
    {
        return Err(AppError::bad_request(
            "MCP_CONFIRMATION_TEXT_INVALID",
            "Type the tool name to confirm this invocation",
        ));
    }
    validate_arguments(&tool.input_schema, &input.arguments)?;
    let config = load_current_config_for_tool(&state, actor.tenant_id, id).await?;
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(u64::from(tool.timeout_seconds)),
        call_remote_tool(
            &state,
            actor.tenant_id,
            &config,
            &tool.name,
            input.arguments.clone(),
        ),
    )
    .await
    .map_err(|_| {
        AppError::service_unavailable("MCP_DEBUG_TIMEOUT", "MCP tool invocation timed out")
    })??;
    let duration = started.elapsed().as_millis() as u64;
    let argument_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&input.arguments).map_err(AppError::internal)?)
    );
    let mut tx = state.pool.begin().await?;
    audit(&mut tx,&actor,"mcp.tool_debug_invoked","mcp_tool",id,json!({"toolVersionId":tool.current_version_id,"argumentHash":argument_hash,"durationMs":duration,"status":"succeeded"})).await?;
    tx.commit().await?;
    Ok(Json(DebugMcpToolResponse {
        result,
        duration_ms: duration,
        tool_version_id: tool.current_version_id,
    }))
}

#[derive(Clone)]
struct ServerConfig {
    version_id: Uuid,
    transport: String,
    endpoint: String,
    credential_id: Option<Uuid>,
}
#[derive(Clone)]
struct RemoteTool {
    name: String,
    title: Option<String>,
    description: Option<String>,
    input_schema: Value,
    output_schema: Option<Value>,
    annotations: Value,
}
struct McpSession {
    id: Option<String>,
}

async fn load_current_config(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<ServerConfig> {
    let r=sqlx::query("SELECT sv.id,sv.endpoint,sv.credential_id,sv.transport FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number WHERE s.tenant_id=? AND s.id=? AND s.status='active'").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("MCP server"))?;
    Ok(ServerConfig {
        version_id: r.try_get("id")?,
        transport: r.try_get("transport")?,
        endpoint: r.try_get("endpoint")?,
        credential_id: r.try_get("credential_id")?,
    })
}
async fn load_current_config_for_tool(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<ServerConfig> {
    let server: Uuid =
        sqlx::query_scalar("SELECT server_id FROM mcp_tools WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("MCP tool"))?;
    load_current_config(state, tenant, server).await
}

async fn mcp_initialize(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
) -> AppResult<McpSession> {
    let (value,session)=rpc(state,tenant,config,"initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"Agentx","version":"0.1.0"}}),None,1).await?;
    if value.get("serverInfo").is_none() {
        return Err(AppError::service_unavailable(
            "MCP_INITIALIZE_INVALID",
            "MCP server returned an invalid initialize response",
        ));
    }
    let _ = rpc_notification(
        state,
        tenant,
        config,
        "notifications/initialized",
        json!({}),
        session.as_deref(),
    )
    .await;
    Ok(McpSession { id: session })
}
async fn discover_remote(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
) -> AppResult<Vec<RemoteTool>> {
    if config.transport == "sse" {
        return discover_remote_sse(state, tenant, config).await;
    }
    let session = mcp_initialize(state, tenant, config).await?;
    let mut cursor: Option<String> = None;
    let mut result = Vec::new();
    let mut request_id = 2;
    loop {
        let mut params = Map::new();
        if let Some(value) = &cursor {
            params.insert("cursor".to_owned(), Value::String(value.clone()));
        }
        let (value, _) = rpc(
            state,
            tenant,
            config,
            "tools/list",
            Value::Object(params),
            session.id.as_deref(),
            request_id,
        )
        .await?;
        request_id += 1;
        let tools = value
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::service_unavailable(
                    "MCP_TOOLS_INVALID",
                    "MCP server returned an invalid tools list",
                )
            })?;
        for tool in tools {
            if result.len() >= MAX_DISCOVERED_TOOLS {
                return Err(AppError::unprocessable(
                    "MCP_TOOL_LIMIT",
                    "MCP server exposes more than 500 tools",
                ));
            }
            result.push(RemoteTool {
                name: tool
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::service_unavailable(
                            "MCP_TOOL_INVALID",
                            "MCP tool name is missing",
                        )
                    })?
                    .to_owned(),
                title: tool.get("title").and_then(Value::as_str).map(str::to_owned),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                input_schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"})),
                output_schema: tool.get("outputSchema").cloned(),
                annotations: tool
                    .get("annotations")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            });
        }
        cursor = value
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    Ok(result)
}
async fn call_remote_tool(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    name: &str,
    arguments: Value,
) -> AppResult<Value> {
    if config.transport == "sse" {
        return call_remote_tool_sse(state, tenant, config, name, arguments).await;
    }
    let session = mcp_initialize(state, tenant, config).await?;
    let (value, _) = rpc(
        state,
        tenant,
        config,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
        session.id.as_deref(),
        2,
    )
    .await?;
    Ok(value)
}

async fn rpc(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    method: &str,
    params: Value,
    session: Option<&str>,
    id: u64,
) -> AppResult<(Value, Option<String>)> {
    connection_test::validate_target(state, &config.endpoint).await?;
    let mut request = state
        .http
        .post(&config.endpoint)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .timeout(Duration::from_secs(state.connections.timeout_seconds))
        .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    if let Some(credential) = config.credential_id {
        request = connection_test::authorize(
            request,
            credentials::resolve(state, tenant, credential).await?,
        )?;
    }
    let response = request.send().await.map_err(|e| {
        if e.is_timeout() {
            AppError::service_unavailable("MCP_TIMEOUT", "MCP request timed out")
        } else {
            AppError::service_unavailable(
                "MCP_CONNECTION_FAILED",
                "MCP server could not be reached",
            )
        }
    })?;
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| session.map(str::to_owned));
    let status = response.status();
    let bytes = response.bytes().await.map_err(AppError::internal)?;
    if bytes.len() > MAX_MCP_RESPONSE_BYTES {
        return Err(AppError::service_unavailable(
            "MCP_RESPONSE_TOO_LARGE",
            "MCP response exceeds 1 MiB",
        ));
    }
    if !status.is_success() {
        return Err(AppError::service_unavailable(
            "MCP_HTTP_STATUS",
            format!("MCP server returned HTTP {}", status.as_u16()),
        ));
    }
    let value = parse_rpc_response(&bytes)?;
    if let Some(error) = value.get("error") {
        return Err(AppError::service_unavailable(
            "MCP_REMOTE_ERROR",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("MCP server returned an error"),
        ));
    }
    Ok((value.get("result").cloned().unwrap_or(Value::Null), session))
}
async fn rpc_notification(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    method: &str,
    params: Value,
    session: Option<&str>,
) -> AppResult<()> {
    let mut request = state
        .http
        .post(&config.endpoint)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","method":method,"params":params}));
    if let Some(value) = session {
        request = request.header("mcp-session-id", value);
    }
    if let Some(id) = config.credential_id {
        request =
            connection_test::authorize(request, credentials::resolve(state, tenant, id).await?)?;
    }
    request.send().await.map_err(AppError::internal)?;
    Ok(())
}
fn parse_rpc_response(bytes: &[u8]) -> AppResult<Value> {
    if let Ok(value) = serde_json::from_slice(bytes) {
        return Ok(value);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        AppError::service_unavailable("MCP_RESPONSE_INVALID", "MCP response is invalid")
    })?;
    for line in text.lines().rev() {
        if let Some(data) = line.strip_prefix("data:") {
            if let Ok(value) = serde_json::from_str(data.trim()) {
                return Ok(value);
            }
        }
    }
    Err(AppError::service_unavailable(
        "MCP_RESPONSE_INVALID",
        "MCP response is not valid JSON or SSE",
    ))
}

type LegacySseStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

struct LegacySseConnection {
    stream: LegacySseStream,
    buffer: String,
    post_endpoint: String,
}

async fn open_legacy_sse(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
) -> AppResult<LegacySseConnection> {
    let configured = connection_test::validate_target(state, &config.endpoint).await?;
    let mut request = state
        .http
        .get(configured.clone())
        .header(ACCEPT, "text/event-stream")
        .timeout(Duration::from_secs(state.connections.timeout_seconds));
    if let Some(id) = config.credential_id {
        request =
            connection_test::authorize(request, credentials::resolve(state, tenant, id).await?)?;
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            AppError::service_unavailable("MCP_TIMEOUT", "MCP SSE connection timed out")
        } else {
            AppError::service_unavailable(
                "MCP_CONNECTION_FAILED",
                "MCP SSE server could not be reached",
            )
        }
    })?;
    if !response.status().is_success() {
        return Err(AppError::service_unavailable(
            "MCP_HTTP_STATUS",
            format!("MCP server returned HTTP {}", response.status().as_u16()),
        ));
    }
    let mut connection = LegacySseConnection {
        stream: Box::pin(response.bytes_stream()),
        buffer: String::new(),
        post_endpoint: String::new(),
    };
    loop {
        let (event, data) = next_legacy_event(state, &mut connection).await?;
        if event.as_deref() != Some("endpoint") {
            continue;
        }
        let endpoint = configured.join(data.trim()).map_err(|_| {
            AppError::service_unavailable(
                "MCP_SSE_ENDPOINT_INVALID",
                "MCP SSE server returned an invalid message endpoint",
            )
        })?;
        connection_test::validate_target(state, endpoint.as_str()).await?;
        if configured.scheme() != endpoint.scheme()
            || configured.host_str() != endpoint.host_str()
            || configured.port_or_known_default() != endpoint.port_or_known_default()
        {
            return Err(AppError::new(
                StatusCode::FORBIDDEN,
                "MCP_SSE_ENDPOINT_ORIGIN",
                "MCP SSE message endpoint must use the configured origin",
            ));
        }
        connection.post_endpoint = endpoint.into();
        return Ok(connection);
    }
}

async fn next_legacy_event(
    state: &AppState,
    connection: &mut LegacySseConnection,
) -> AppResult<(Option<String>, String)> {
    loop {
        if let Some(boundary) = connection.buffer.find("\n\n") {
            let raw = connection.buffer[..boundary].to_owned();
            connection.buffer.drain(..boundary + 2);
            let mut event = None;
            let mut data = Vec::new();
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    event = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.trim_start().to_owned());
                }
            }
            if !data.is_empty() {
                return Ok((event, data.join("\n")));
            }
        }
        let chunk = tokio::time::timeout(
            Duration::from_secs(state.connections.timeout_seconds),
            connection.stream.next(),
        )
        .await
        .map_err(|_| AppError::service_unavailable("MCP_TIMEOUT", "MCP SSE event timed out"))?
        .ok_or_else(|| {
            AppError::service_unavailable("MCP_SSE_CLOSED", "MCP SSE connection closed")
        })?
        .map_err(|_| {
            AppError::service_unavailable("MCP_SSE_READ_FAILED", "MCP SSE event could not be read")
        })?;
        if connection.buffer.len() + chunk.len() > MAX_MCP_RESPONSE_BYTES {
            return Err(AppError::service_unavailable(
                "MCP_RESPONSE_TOO_LARGE",
                "MCP SSE response exceeds 1 MiB",
            ));
        }
        connection.buffer.push_str(&String::from_utf8_lossy(&chunk));
        connection.buffer = connection.buffer.replace("\r\n", "\n");
    }
}

async fn legacy_post(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    post_endpoint: &str,
    payload: &Value,
) -> AppResult<()> {
    let mut request = state
        .http
        .post(post_endpoint)
        .header(CONTENT_TYPE, "application/json")
        .timeout(Duration::from_secs(state.connections.timeout_seconds))
        .json(payload);
    if let Some(id) = config.credential_id {
        request =
            connection_test::authorize(request, credentials::resolve(state, tenant, id).await?)?;
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            AppError::service_unavailable("MCP_TIMEOUT", "MCP SSE message timed out")
        } else {
            AppError::service_unavailable(
                "MCP_CONNECTION_FAILED",
                "MCP SSE message could not be sent",
            )
        }
    })?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::service_unavailable(
            "MCP_HTTP_STATUS",
            format!("MCP server returned HTTP {}", response.status().as_u16()),
        ))
    }
}

async fn legacy_rpc(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    connection: &mut LegacySseConnection,
    method: &str,
    params: Value,
    id: u64,
) -> AppResult<Value> {
    legacy_post(
        state,
        tenant,
        config,
        &connection.post_endpoint,
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .await?;
    loop {
        let (_, data) = next_legacy_event(state, connection).await?;
        let value: Value = serde_json::from_str(&data).map_err(|_| {
            AppError::service_unavailable(
                "MCP_RESPONSE_INVALID",
                "MCP SSE message is not valid JSON",
            )
        })?;
        if value.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(AppError::service_unavailable(
                "MCP_REMOTE_ERROR",
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("MCP server returned an error"),
            ));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
}

async fn legacy_initialize(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    connection: &mut LegacySseConnection,
) -> AppResult<()> {
    let value = legacy_rpc(
        state,
        tenant,
        config,
        connection,
        "initialize",
        json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"Agentx","version":"0.1.0"}}),
        1,
    )
    .await?;
    if value.get("serverInfo").is_none() {
        return Err(AppError::service_unavailable(
            "MCP_INITIALIZE_INVALID",
            "MCP server returned an invalid initialize response",
        ));
    }
    legacy_post(
        state,
        tenant,
        config,
        &connection.post_endpoint,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await
}

async fn discover_remote_sse(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
) -> AppResult<Vec<RemoteTool>> {
    let mut connection = open_legacy_sse(state, tenant, config).await?;
    legacy_initialize(state, tenant, config, &mut connection).await?;
    let mut cursor = None;
    let mut request_id = 2;
    let mut result = Vec::new();
    loop {
        let params = cursor
            .as_ref()
            .map_or_else(|| json!({}), |value| json!({"cursor": value}));
        let value = legacy_rpc(
            state,
            tenant,
            config,
            &mut connection,
            "tools/list",
            params,
            request_id,
        )
        .await?;
        request_id += 1;
        let tools = value
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::service_unavailable(
                    "MCP_TOOLS_INVALID",
                    "MCP server returned an invalid tools list",
                )
            })?;
        for tool in tools {
            if result.len() >= MAX_DISCOVERED_TOOLS {
                return Err(AppError::unprocessable(
                    "MCP_TOOL_LIMIT",
                    "MCP server exposes more than 500 tools",
                ));
            }
            result.push(remote_tool_from_value(tool)?);
        }
        cursor = value
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            return Ok(result);
        }
    }
}

async fn call_remote_tool_sse(
    state: &AppState,
    tenant: Uuid,
    config: &ServerConfig,
    name: &str,
    arguments: Value,
) -> AppResult<Value> {
    let mut connection = open_legacy_sse(state, tenant, config).await?;
    legacy_initialize(state, tenant, config, &mut connection).await?;
    legacy_rpc(
        state,
        tenant,
        config,
        &mut connection,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
        2,
    )
    .await
}

fn remote_tool_from_value(tool: &Value) -> AppResult<RemoteTool> {
    Ok(RemoteTool {
        name: tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::service_unavailable("MCP_TOOL_INVALID", "MCP tool name is missing")
            })?
            .to_owned(),
        title: tool.get("title").and_then(Value::as_str).map(str::to_owned),
        description: tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        input_schema: tool
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object"})),
        output_schema: tool.get("outputSchema").cloned(),
        annotations: tool
            .get("annotations")
            .cloned()
            .unwrap_or_else(|| json!({})),
    })
}

async fn upsert_tool(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    actor: &AuthActor,
    server_id: Uuid,
    run_id: Uuid,
    tool: &RemoteTool,
) -> AppResult<()> {
    let value = json!({"input":tool.input_schema,"output":tool.output_schema,"annotations":tool.annotations});
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(AppError::internal)?)
    );
    let existing=sqlx::query("SELECT id,current_version_number FROM mcp_tools WHERE tenant_id=? AND server_id=? AND name=? FOR UPDATE").bind(actor.tenant_id).bind(server_id).bind(&tool.name).fetch_optional(&mut **tx).await?;
    let (id, current) = if let Some(r) = existing {
        (r.try_get("id")?, r.try_get("current_version_number")?)
    } else {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO mcp_tools(id,tenant_id,server_id,name,title,description,current_version_number) VALUES(?,?,?,?,?,?,1)").bind(id).bind(actor.tenant_id).bind(server_id).bind(&tool.name).bind(&tool.title).bind(&tool.description).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO mcp_tool_policies(tenant_id,tool_id,updated_by) VALUES(?,?,?)")
            .bind(actor.tenant_id)
            .bind(id)
            .bind(actor.user_id)
            .execute(&mut **tx)
            .await?;
        (id, 1)
    };
    let known: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM mcp_tool_versions WHERE tenant_id=? AND tool_id=? AND schema_hash=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .bind(&hash)
    .fetch_optional(&mut **tx)
    .await?;
    let version_number = if known.is_some() {
        current
    } else {
        if current == 1
            && sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM mcp_tool_versions WHERE tool_id=?",
            )
            .bind(id)
            .fetch_one(&mut **tx)
            .await?
                == 0
        {
            1
        } else {
            current + 1
        }
    };
    if known.is_none() {
        sqlx::query("INSERT INTO mcp_tool_versions(id,tenant_id,tool_id,discovery_run_id,version_number,input_schema,output_schema,annotations_json,schema_hash) VALUES(?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(run_id).bind(version_number).bind(&tool.input_schema).bind(&tool.output_schema).bind(&tool.annotations).bind(&hash).execute(&mut **tx).await?;
    }
    sqlx::query("UPDATE mcp_tools SET title=?,description=?,availability='available',current_version_number=?,version=version+1,last_seen_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=?").bind(&tool.title).bind(&tool.description).bind(version_number).bind(actor.tenant_id).bind(id).execute(&mut **tx).await?;
    Ok(())
}

async fn load_server(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<McpServerResponse> {
    let r=sqlx::query("SELECT s.id,s.name,s.description,s.owner_department_id,s.status,s.current_version_number,s.version,s.last_discovered_at,s.updated_at,sv.transport,sv.endpoint,sv.credential_id,(SELECT COUNT(*) FROM mcp_tools t WHERE t.server_id=s.id AND t.availability='available') tool_count FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number WHERE s.tenant_id=? AND s.id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("MCP server"))?;
    server_from_row(r).map_err(Into::into)
}
async fn load_tools(
    state: &AppState,
    tenant: Uuid,
    server: Uuid,
) -> AppResult<Vec<McpToolResponse>> {
    let rows = sqlx::query(TOOL_SELECT)
        .bind(tenant)
        .bind(server)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(tool_from_row)
        .collect::<Result<_, _>>()?)
}
async fn load_tool(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<McpToolResponse> {
    let r = sqlx::query(&format!(
        "{TOOL_SELECT_PREFIX} WHERE t.tenant_id=? AND t.id=?"
    ))
    .bind(tenant)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("MCP tool"))?;
    tool_from_row(r).map_err(Into::into)
}
const TOOL_SELECT_PREFIX: &str = "SELECT t.id,t.server_id,t.name,t.title,t.description,t.availability,t.current_version_number,t.version,tv.id current_version_id,tv.input_schema,tv.output_schema,tv.annotations_json,tv.schema_hash,p.enabled,p.debug_enabled,p.timeout_seconds,p.side_effect,p.version policy_version FROM mcp_tools t JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_tool_versions tv ON tv.tool_id=t.id AND tv.version_number=t.current_version_number JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id";
const TOOL_SELECT: &str = "SELECT t.id,t.server_id,t.name,t.title,t.description,t.availability,t.current_version_number,t.version,tv.id current_version_id,tv.input_schema,tv.output_schema,tv.annotations_json,tv.schema_hash,p.enabled,p.debug_enabled,p.timeout_seconds,p.side_effect,p.version policy_version FROM mcp_tools t JOIN mcp_tool_versions tv ON tv.tool_id=t.id AND tv.version_number=t.current_version_number JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE t.tenant_id=? AND t.server_id=? ORDER BY t.name";
fn server_from_row(r: sqlx::mysql::MySqlRow) -> Result<McpServerResponse, sqlx::Error> {
    Ok(McpServerResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        owner_department_id: r.try_get("owner_department_id")?,
        transport: r.try_get("transport")?,
        endpoint: r.try_get("endpoint")?,
        credential_id: r.try_get("credential_id")?,
        status: r.try_get("status")?,
        current_version_number: r.try_get("current_version_number")?,
        version: r.try_get("version")?,
        tool_count: r.try_get::<i64, _>("tool_count")? as u64,
        last_discovered_at: r.try_get("last_discovered_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn tool_from_row(r: sqlx::mysql::MySqlRow) -> Result<McpToolResponse, sqlx::Error> {
    Ok(McpToolResponse {
        id: r.try_get("id")?,
        server_id: r.try_get("server_id")?,
        name: r.try_get("name")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        availability: r.try_get("availability")?,
        current_version_id: r.try_get("current_version_id")?,
        current_version_number: r.try_get("current_version_number")?,
        input_schema: r.try_get("input_schema")?,
        output_schema: r.try_get("output_schema")?,
        annotations: r.try_get("annotations_json")?,
        schema_hash: r.try_get("schema_hash")?,
        enabled: r.try_get("enabled")?,
        debug_enabled: r.try_get("debug_enabled")?,
        timeout_seconds: r.try_get("timeout_seconds")?,
        side_effect: r.try_get("side_effect")?,
        version: r.try_get("policy_version")?,
    })
}
async fn validate_server_config(
    state: &AppState,
    transport: &str,
    endpoint: &str,
    configuration: &Value,
) -> AppResult<()> {
    if !matches!(transport, "streamable_http" | "sse") {
        return Err(AppError::bad_request(
            "INVALID_MCP_TRANSPORT",
            "MCP transport is invalid",
        ));
    }
    connection_test::validate_target(state, endpoint).await?;
    if !configuration.is_object() {
        return Err(AppError::bad_request(
            "INVALID_MCP_CONFIGURATION",
            "MCP configuration must be an object",
        ));
    }
    Ok(())
}
fn config_hash(
    transport: &str,
    endpoint: &str,
    credential: Option<Uuid>,
    configuration: &Value,
) -> AppResult<String> {
    Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"transport":transport,"endpoint":endpoint,"credentialId":credential,"configuration":configuration})).map_err(AppError::internal)?)))
}
fn validate_arguments(schema: &Value, args: &Value) -> AppResult<()> {
    let object = args.as_object().ok_or_else(|| {
        AppError::bad_request(
            "MCP_ARGUMENTS_INVALID",
            "MCP tool arguments must be an object",
        )
    })?;
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(AppError::bad_request(
                    "MCP_ARGUMENTS_INVALID",
                    format!("Required argument '{key}' is missing"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::Arc};

    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode},
        response::sse::{Event, Sse},
        routing::{get, post},
    };
    use futures::{StreamExt, stream};
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use sqlx::mysql::MySqlPoolOptions;
    use tokio::sync::broadcast;
    use uuid::Uuid;

    use crate::{
        config::{AuthSettings, ConnectionSettings},
        state::AppState,
    };

    use super::{ServerConfig, call_remote_tool, discover_remote};

    fn test_state() -> AppState {
        let pool = MySqlPoolOptions::new()
            .connect_lazy("mysql://agentx:agentx@127.0.0.1:1/agentx")
            .expect("lazy test pool");
        AppState::new(
            pool,
            AuthSettings {
                signing_secret: SecretString::from(
                    "mcp-test-signing-secret-with-at-least-32-bytes".to_owned(),
                ),
                issuer: "agentx-test".to_owned(),
                audience: "agentx-test".to_owned(),
                access_ttl_seconds: 60,
                refresh_ttl_seconds: 60,
                change_password_ttl_seconds: 60,
                cookie_secure: false,
                login_max_failures: 5,
                login_failure_window_seconds: 60,
                login_lock_seconds: 60,
            },
        )
        .with_m2(
            agentx_infrastructure::credential::CredentialKeyring::from_json(
                "test".to_owned(),
                &SecretString::from(
                    r#"{"keys":{"test":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#
                        .to_owned(),
                ),
            )
            .expect("credential keyring"),
            None,
            ConnectionSettings {
                timeout_seconds: 2,
                max_concurrency: 4,
                allow_private_networks: false,
                allowed_hosts: vec!["127.0.0.1".to_owned()],
                allowed_cidrs: Vec::new(),
            },
        )
    }

    fn rpc_result(payload: &Value) -> Value {
        let result = match payload.get("method").and_then(Value::as_str) {
            Some("initialize") => json!({"serverInfo":{"name":"fake","version":"1"}}),
            Some("tools/list") => json!({"tools":[{
                "name":"echo",
                "inputSchema":{"type":"object","required":["text"]}
            }]}),
            Some("tools/call") => json!({"content":[{
                "type":"text",
                "text":payload["params"]["arguments"]["text"]
            }]}),
            _ => Value::Null,
        };
        json!({"jsonrpc":"2.0","id":payload.get("id").cloned().unwrap_or(Value::Null),"result":result})
    }

    async fn streamable(Json(payload): Json<Value>) -> (HeaderMap, Json<Value>) {
        let mut headers = HeaderMap::new();
        headers.insert("mcp-session-id", HeaderValue::from_static("fake-session"));
        (headers, Json(rpc_result(&payload)))
    }

    #[tokio::test]
    async fn streamable_http_discovers_and_invokes_tools() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind streamable fake server");
        let address = listener.local_addr().expect("streamable address");
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(streamable)))
                .await
                .expect("serve streamable fake server");
        });
        let state = test_state();
        let config = ServerConfig {
            version_id: Uuid::now_v7(),
            transport: "streamable_http".to_owned(),
            endpoint: format!("http://{address}/mcp"),
            credential_id: None,
        };
        let tools = discover_remote(&state, Uuid::now_v7(), &config)
            .await
            .expect("discover streamable tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let result = call_remote_tool(
            &state,
            Uuid::now_v7(),
            &config,
            "echo",
            json!({"text":"streamable"}),
        )
        .await
        .expect("invoke streamable tool");
        assert_eq!(result["content"][0]["text"], "streamable");
    }

    #[derive(Clone)]
    struct SseState {
        events: broadcast::Sender<String>,
    }

    async fn legacy_events(
        State(state): State<Arc<SseState>>,
    ) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
        let receiver = state.events.subscribe();
        let endpoint =
            stream::once(async { Ok(Event::default().event("endpoint").data("/messages")) });
        let responses = stream::unfold(receiver, |mut receiver| async move {
            receiver
                .recv()
                .await
                .ok()
                .map(|value| (Ok(Event::default().event("message").data(value)), receiver))
        });
        Sse::new(endpoint.chain(responses))
    }

    async fn legacy_messages(
        State(state): State<Arc<SseState>>,
        Json(payload): Json<Value>,
    ) -> StatusCode {
        if payload.get("id").is_some() {
            let _ = state.events.send(rpc_result(&payload).to_string());
        }
        StatusCode::ACCEPTED
    }

    #[tokio::test]
    async fn legacy_sse_discovers_and_invokes_tools() {
        let (events, _) = broadcast::channel(16);
        let app_state = Arc::new(SseState { events });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind SSE fake server");
        let address = listener.local_addr().expect("SSE address");
        let app = Router::new()
            .route("/sse", get(legacy_events))
            .route("/messages", post(legacy_messages))
            .with_state(app_state);
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve SSE fake server");
        });
        let state = test_state();
        let config = ServerConfig {
            version_id: Uuid::now_v7(),
            transport: "sse".to_owned(),
            endpoint: format!("http://{address}/sse"),
            credential_id: None,
        };
        let tools = discover_remote(&state, Uuid::now_v7(), &config)
            .await
            .expect("discover SSE tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let result = call_remote_tool(
            &state,
            Uuid::now_v7(),
            &config,
            "echo",
            json!({"text":"legacy"}),
        )
        .await
        .expect("invoke SSE tool");
        assert_eq!(result["content"][0]["text"], "legacy");
    }
}
