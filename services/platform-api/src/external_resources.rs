use agentx_api_types::PageResponse;
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
    connection_test::{self, HealthCheckResponse},
    control_common::{audit, require_department_scope, validate_name},
    credentials,
    error::{AppError, AppResult, UniqueConstraint, map_unique},
    grants::require_resource_visible,
    security::AuthActor,
    state::AppState,
};

pub(crate) const KNOWLEDGE_EXTERNAL_ID: UniqueConstraint = UniqueConstraint {
    index: "uq_rag_resource_external",
    code: "KNOWLEDGE_EXTERNAL_RESOURCE_ID_EXISTS",
    field: "externalResourceId",
    message: "This external knowledge resource ID already exists for the selected connection",
};
pub(crate) const MEMORY_EXTERNAL_NAMESPACE: UniqueConstraint = UniqueConstraint {
    index: "uq_memory_namespace_external",
    code: "MEMORY_EXTERNAL_NAMESPACE_EXISTS",
    field: "externalNamespace",
    message: "This external namespace already exists for the selected connection",
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    pub id: Uuid,
    pub name: String,
    pub endpoint: String,
    pub health_path: String,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
    pub configuration: Value,
    pub status: String,
    pub version: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionRequest {
    pub name: String,
    pub endpoint: String,
    pub health_path: Option<String>,
    pub credential_id: Option<Uuid>,
    pub owner_department_id: Uuid,
    pub configuration: Value,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeResponse {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub connection_name: String,
    pub name: String,
    pub external_resource_id: String,
    pub owner_department_id: Uuid,
    pub sync_status: String,
    pub status: String,
    pub grant_count: u64,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type KnowledgePage = PageResponse<KnowledgeResponse>;
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateKnowledgeRequest {
    pub connection_id: Uuid,
    pub name: String,
    pub external_resource_id: String,
    pub owner_department_id: Uuid,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryResponse {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub connection_name: String,
    pub name: String,
    pub external_namespace: String,
    pub access_mode: String,
    pub owner_department_id: Uuid,
    pub status: String,
    pub grant_count: u64,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type MemoryPage = PageResponse<MemoryResponse>;
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryRequest {
    pub connection_id: Uuid,
    pub name: String,
    pub external_namespace: String,
    pub access_mode: String,
    pub owner_department_id: Uuid,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateExternalResourceRequest {
    pub name: String,
    pub status: String,
    pub version: u64,
}

#[utoipa::path(get, path = "/api/v1/knowledge/connections")]
pub async fn list_rag_connections(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<ConnectionResponse>>> {
    actor.require("knowledge:view")?;
    Ok(Json(
        list_connections(&state, &actor, "rag_connections").await?,
    ))
}
#[utoipa::path(post,path="/api/v1/knowledge/connections",request_body=CreateConnectionRequest)]
pub async fn create_rag_connection(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateConnectionRequest>,
) -> AppResult<(StatusCode, Json<ConnectionResponse>)> {
    create_connection(
        &state,
        &actor,
        input,
        "rag_connections",
        "knowledge.connection_created",
    )
    .await
}
#[utoipa::path(post,path="/api/v1/knowledge/connections/{id}/test-connection",params(("id"=Uuid,Path)))]
pub async fn test_rag_connection(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<HealthCheckResponse>> {
    actor.require("knowledge:manage")?;
    test_connection(&state, &actor, "rag_connections", "rag", id).await
}

#[utoipa::path(get, path = "/api/v1/knowledge/resources")]
pub async fn list_knowledge(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ResourceListQuery>,
) -> AppResult<Json<KnowledgePage>> {
    actor.require("knowledge:view")?;
    let (page, page_size, offset, search, status) = list_values(query);
    let sql = if actor.company_admin {
        KNOWLEDGE_ADMIN
    } else {
        KNOWLEDGE_SCOPED
    };
    let mut q = sqlx::query(sql).bind(actor.tenant_id);
    if !actor.company_admin {
        q = q.bind(actor.user_id).bind(actor.department_id);
    }
    let rows = q
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(knowledge_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total,
    }))
}
const KNOWLEDGE_ADMIN: &str = "SELECT r.id,r.connection_id,c.name connection_name,r.name,r.external_resource_id,r.owner_department_id,r.sync_status,r.status,r.version,r.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=r.tenant_id AND g.resource_type='rag' AND g.resource_id=r.id AND g.subject_type='workflow_service_identity') grant_count,COUNT(*) OVER() total_count FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND (?='' OR r.status=?) AND (?='%%' OR r.name LIKE ?) ORDER BY r.updated_at DESC LIMIT ? OFFSET ?";
const KNOWLEDGE_SCOPED: &str = "SELECT r.id,r.connection_id,c.name connection_name,r.name,r.external_resource_id,r.owner_department_id,r.sync_status,r.status,r.version,r.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=r.tenant_id AND g.resource_type='rag' AND g.resource_id=r.id AND g.subject_type='workflow_service_identity') grant_count,COUNT(*) OVER() total_count FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=r.tenant_id AND dc.descendant_id=r.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=r.tenant_id AND rg.subject_type='department' AND rg.resource_type='rag' AND rg.resource_id=r.id AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)) AND (?='' OR r.status=?) AND (?='%%' OR r.name LIKE ?) ORDER BY r.updated_at DESC LIMIT ? OFFSET ?";
#[utoipa::path(post,path="/api/v1/knowledge/resources",request_body=CreateKnowledgeRequest)]
pub async fn create_knowledge(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateKnowledgeRequest>,
) -> AppResult<(StatusCode, Json<KnowledgeResponse>)> {
    actor.require("knowledge:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    let name = validate_name(&input.name, 160)?;
    let external = validate_name(&input.external_resource_id, 512)?;
    require_connection_scope(&state, &actor, "rag_connections", input.connection_id).await?;
    ensure_external_available(
        &state,
        "rag_resources",
        "external_resource_id",
        actor.tenant_id,
        input.connection_id,
        &external,
        KNOWLEDGE_EXTERNAL_ID,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO rag_resources(id,tenant_id,connection_id,name,external_resource_id,owner_department_id) VALUES(?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(input.connection_id).bind(name).bind(external).bind(input.owner_department_id).execute(&mut *tx).await.map_err(|error| map_unique(error, &[KNOWLEDGE_EXTERNAL_ID]))?;
    audit(
        &mut tx,
        &actor,
        "knowledge.resource_created",
        "rag",
        id,
        json!({"connectionId":input.connection_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_knowledge(&state, actor.tenant_id, id).await?),
    ))
}
#[utoipa::path(get,path="/api/v1/knowledge/resources/{id}",params(("id"=Uuid,Path)))]
pub async fn get_knowledge(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<KnowledgeResponse>> {
    actor.require("knowledge:view")?;
    require_resource_visible(&state, &actor, "rag", id).await?;
    Ok(Json(load_knowledge(&state, actor.tenant_id, id).await?))
}
#[utoipa::path(patch,path="/api/v1/knowledge/resources/{id}",request_body=UpdateExternalResourceRequest,params(("id"=Uuid,Path)))]
pub async fn update_knowledge(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateExternalResourceRequest>,
) -> AppResult<Json<KnowledgeResponse>> {
    actor.require("knowledge:manage")?;
    require_resource_visible(&state, &actor, "rag", id).await?;
    let name = validate_name(&input.name, 160)?;
    validate_status(&input.status)?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE rag_resources SET name=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(name).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "KNOWLEDGE_VERSION_CONFLICT",
            "Knowledge resource changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "knowledge.resource_updated",
        "rag",
        id,
        json!({"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_knowledge(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get, path = "/api/v1/memory/connections")]
pub async fn list_memory_connections(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<ConnectionResponse>>> {
    actor.require("memory:view")?;
    Ok(Json(
        list_connections(&state, &actor, "memory_connections").await?,
    ))
}
#[utoipa::path(post,path="/api/v1/memory/connections",request_body=CreateConnectionRequest)]
pub async fn create_memory_connection(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateConnectionRequest>,
) -> AppResult<(StatusCode, Json<ConnectionResponse>)> {
    create_connection(
        &state,
        &actor,
        input,
        "memory_connections",
        "memory.connection_created",
    )
    .await
}
#[utoipa::path(post,path="/api/v1/memory/connections/{id}/test-connection",params(("id"=Uuid,Path)))]
pub async fn test_memory_connection(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<HealthCheckResponse>> {
    actor.require("memory:manage")?;
    test_connection(&state, &actor, "memory_connections", "memory", id).await
}

#[utoipa::path(get, path = "/api/v1/memory/namespaces")]
pub async fn list_memory(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ResourceListQuery>,
) -> AppResult<Json<MemoryPage>> {
    actor.require("memory:view")?;
    let (page, page_size, offset, search, status) = list_values(query);
    let sql = if actor.company_admin {
        MEMORY_ADMIN
    } else {
        MEMORY_SCOPED
    };
    let mut q = sqlx::query(sql).bind(actor.tenant_id);
    if !actor.company_admin {
        q = q.bind(actor.user_id).bind(actor.department_id);
    }
    let rows = q
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(memory_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total,
    }))
}
const MEMORY_ADMIN: &str = "SELECT n.id,n.connection_id,c.name connection_name,n.name,n.external_namespace,n.access_mode,n.owner_department_id,n.status,n.version,n.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=n.tenant_id AND g.resource_type='memory' AND g.resource_id=n.id AND g.subject_type='workflow_service_identity') grant_count,COUNT(*) OVER() total_count FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND (?='' OR n.status=?) AND (?='%%' OR n.name LIKE ?) ORDER BY n.updated_at DESC LIMIT ? OFFSET ?";
const MEMORY_SCOPED: &str = "SELECT n.id,n.connection_id,c.name connection_name,n.name,n.external_namespace,n.access_mode,n.owner_department_id,n.status,n.version,n.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=n.tenant_id AND g.resource_type='memory' AND g.resource_id=n.id AND g.subject_type='workflow_service_identity') grant_count,COUNT(*) OVER() total_count FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=n.tenant_id AND dc.descendant_id=n.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=n.tenant_id AND rg.subject_type='department' AND rg.resource_type='memory' AND rg.resource_id=n.id AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)) AND (?='' OR n.status=?) AND (?='%%' OR n.name LIKE ?) ORDER BY n.updated_at DESC LIMIT ? OFFSET ?";
#[utoipa::path(post,path="/api/v1/memory/namespaces",request_body=CreateMemoryRequest)]
pub async fn create_memory(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateMemoryRequest>,
) -> AppResult<(StatusCode, Json<MemoryResponse>)> {
    actor.require("memory:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    let name = validate_name(&input.name, 160)?;
    let namespace = validate_name(&input.external_namespace, 512)?;
    if !matches!(input.access_mode.as_str(), "read" | "read_write") {
        return Err(AppError::bad_request(
            "INVALID_MEMORY_ACCESS",
            "Memory access mode is invalid",
        ));
    }
    require_connection_scope(&state, &actor, "memory_connections", input.connection_id).await?;
    ensure_external_available(
        &state,
        "memory_namespaces",
        "external_namespace",
        actor.tenant_id,
        input.connection_id,
        &namespace,
        MEMORY_EXTERNAL_NAMESPACE,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO memory_namespaces(id,tenant_id,connection_id,name,external_namespace,access_mode,owner_department_id) VALUES(?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(input.connection_id).bind(name).bind(namespace).bind(input.access_mode).bind(input.owner_department_id).execute(&mut *tx).await.map_err(|error| map_unique(error, &[MEMORY_EXTERNAL_NAMESPACE]))?;
    audit(
        &mut tx,
        &actor,
        "memory.namespace_created",
        "memory",
        id,
        json!({"connectionId":input.connection_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_memory(&state, actor.tenant_id, id).await?),
    ))
}
#[utoipa::path(get,path="/api/v1/memory/namespaces/{id}",params(("id"=Uuid,Path)))]
pub async fn get_memory(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<MemoryResponse>> {
    actor.require("memory:view")?;
    require_resource_visible(&state, &actor, "memory", id).await?;
    Ok(Json(load_memory(&state, actor.tenant_id, id).await?))
}
#[utoipa::path(patch,path="/api/v1/memory/namespaces/{id}",request_body=UpdateExternalResourceRequest,params(("id"=Uuid,Path)))]
pub async fn update_memory(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateExternalResourceRequest>,
) -> AppResult<Json<MemoryResponse>> {
    actor.require("memory:manage")?;
    require_resource_visible(&state, &actor, "memory", id).await?;
    let name = validate_name(&input.name, 160)?;
    validate_status(&input.status)?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE memory_namespaces SET name=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(name).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "MEMORY_VERSION_CONFLICT",
            "Memory namespace changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "memory.namespace_updated",
        "memory",
        id,
        json!({"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_memory(&state, actor.tenant_id, id).await?))
}

async fn create_connection(
    state: &AppState,
    actor: &AuthActor,
    input: CreateConnectionRequest,
    table: &str,
    action: &str,
) -> AppResult<(StatusCode, Json<ConnectionResponse>)> {
    let permission = if table == "rag_connections" {
        "knowledge:manage"
    } else {
        "memory:manage"
    };
    actor.require(permission)?;
    require_department_scope(&state.pool, actor, input.owner_department_id).await?;
    credentials::require_reference(state, actor, input.credential_id).await?;
    let name = validate_name(&input.name, 160)?;
    let endpoint = url::Url::parse(&input.endpoint)
        .map_err(|_| AppError::bad_request("INVALID_ENDPOINT", "Connection endpoint is invalid"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(AppError::bad_request(
            "INVALID_ENDPOINT",
            "Connection endpoint must use HTTP or HTTPS",
        ));
    }
    let health = input.health_path.unwrap_or_else(|| "/health".to_owned());
    if !health.starts_with('/') || health.len() > 512 {
        return Err(AppError::bad_request(
            "INVALID_HEALTH_PATH",
            "Health path must begin with /",
        ));
    }
    let id = Uuid::now_v7();
    let sql = format!(
        "INSERT INTO {table}(id,tenant_id,name,endpoint,health_path,credential_id,owner_department_id,configuration_json) VALUES(?,?,?,?,?,?,?,?)"
    );
    let mut tx = state.pool.begin().await?;
    sqlx::query(&sql)
        .bind(id)
        .bind(actor.tenant_id)
        .bind(name)
        .bind(input.endpoint)
        .bind(health)
        .bind(input.credential_id)
        .bind(input.owner_department_id)
        .bind(input.configuration)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, actor, action, "connection", id, json!({})).await?;
    tx.commit().await?;
    let row = load_connection(state, actor.tenant_id, table, id).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn ensure_external_available(
    state: &AppState,
    table: &'static str,
    column: &'static str,
    tenant: Uuid,
    connection: Uuid,
    value: &str,
    constraint: UniqueConstraint,
) -> AppResult<()> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {table} WHERE tenant_id=? AND connection_id=? AND {column}=?)"
    );
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(connection)
        .bind(value)
        .fetch_one(&state.pool)
        .await?;
    if exists {
        Err(AppError::unique(constraint))
    } else {
        Ok(())
    }
}
async fn list_connections(
    state: &AppState,
    actor: &AuthActor,
    table: &str,
) -> AppResult<Vec<ConnectionResponse>> {
    let sql = if actor.company_admin {
        format!(
            "SELECT id,name,endpoint,health_path,credential_id,owner_department_id,configuration_json,status,version FROM {table} WHERE tenant_id=? ORDER BY name"
        )
    } else {
        format!(
            "SELECT c.id,c.name,c.endpoint,c.health_path,c.credential_id,c.owner_department_id,c.configuration_json,c.status,c.version FROM {table} c WHERE c.tenant_id=? AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=c.tenant_id AND dc.descendant_id=c.owner_department_id) ORDER BY c.name"
        )
    };
    let mut query = sqlx::query(&sql).bind(actor.tenant_id);
    if !actor.company_admin {
        query = query.bind(actor.user_id);
    }
    Ok(query
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(connection_from_row)
        .collect::<Result<_, _>>()?)
}
async fn load_connection(
    state: &AppState,
    tenant: Uuid,
    table: &str,
    id: Uuid,
) -> AppResult<ConnectionResponse> {
    let sql = format!(
        "SELECT id,name,endpoint,health_path,credential_id,owner_department_id,configuration_json,status,version FROM {table} WHERE tenant_id=? AND id=?"
    );
    let row = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Connection"))?;
    connection_from_row(row).map_err(Into::into)
}
async fn test_connection(
    state: &AppState,
    actor: &AuthActor,
    table: &str,
    resource_type: &str,
    id: Uuid,
) -> AppResult<Json<HealthCheckResponse>> {
    require_connection_scope(state, actor, table, id).await?;
    let sql = format!(
        "SELECT endpoint,health_path,credential_id FROM {table} WHERE tenant_id=? AND id=? AND status='active'"
    );
    let row = sqlx::query(&sql)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Connection"))?;
    let endpoint: String = row.try_get("endpoint")?;
    let path: String = row.try_get("health_path")?;
    let url = format!("{}{}", endpoint.trim_end_matches('/'), path);
    connection_test::run_http_check(
        state,
        actor,
        resource_type,
        id,
        &url,
        row.try_get("credential_id")?,
    )
    .await
}

async fn require_connection_scope(
    state: &AppState,
    actor: &AuthActor,
    table: &str,
    id: Uuid,
) -> AppResult<()> {
    let sql = format!(
        "SELECT owner_department_id FROM {table} WHERE tenant_id=? AND id=? AND status='active'"
    );
    let department: Option<Uuid> = sqlx::query_scalar(&sql)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    require_department_scope(
        &state.pool,
        actor,
        department.ok_or_else(|| AppError::not_found("Connection"))?,
    )
    .await
}
async fn load_knowledge(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<KnowledgeResponse> {
    let row=sqlx::query("SELECT r.id,r.connection_id,c.name connection_name,r.name,r.external_resource_id,r.owner_department_id,r.sync_status,r.status,r.version,r.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=r.tenant_id AND g.resource_type='rag' AND g.resource_id=r.id AND g.subject_type='workflow_service_identity') grant_count FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND r.id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Knowledge resource"))?;
    knowledge_from_row(row).map_err(Into::into)
}
async fn load_memory(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<MemoryResponse> {
    let row=sqlx::query("SELECT n.id,n.connection_id,c.name connection_name,n.name,n.external_namespace,n.access_mode,n.owner_department_id,n.status,n.version,n.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=n.tenant_id AND g.resource_type='memory' AND g.resource_id=n.id AND g.subject_type='workflow_service_identity') grant_count FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND n.id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Memory namespace"))?;
    memory_from_row(row).map_err(Into::into)
}
fn list_values(q: ResourceListQuery) -> (u32, u32, u64, String, String) {
    let page = q.page.unwrap_or(1).max(1);
    let size = q.page_size.unwrap_or(20).clamp(1, 100);
    (
        page,
        size,
        u64::from((page - 1) * size),
        format!("%{}%", q.search.unwrap_or_default().trim()),
        q.status.unwrap_or_default(),
    )
}
fn connection_from_row(r: sqlx::mysql::MySqlRow) -> Result<ConnectionResponse, sqlx::Error> {
    Ok(ConnectionResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        endpoint: r.try_get("endpoint")?,
        health_path: r.try_get("health_path")?,
        credential_id: r.try_get("credential_id")?,
        owner_department_id: r.try_get("owner_department_id")?,
        configuration: r.try_get("configuration_json")?,
        status: r.try_get("status")?,
        version: r.try_get("version")?,
    })
}
fn knowledge_from_row(r: sqlx::mysql::MySqlRow) -> Result<KnowledgeResponse, sqlx::Error> {
    Ok(KnowledgeResponse {
        id: r.try_get("id")?,
        connection_id: r.try_get("connection_id")?,
        connection_name: r.try_get("connection_name")?,
        name: r.try_get("name")?,
        external_resource_id: r.try_get("external_resource_id")?,
        owner_department_id: r.try_get("owner_department_id")?,
        sync_status: r.try_get("sync_status")?,
        status: r.try_get("status")?,
        grant_count: r.try_get::<i64, _>("grant_count")? as u64,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn memory_from_row(r: sqlx::mysql::MySqlRow) -> Result<MemoryResponse, sqlx::Error> {
    Ok(MemoryResponse {
        id: r.try_get("id")?,
        connection_id: r.try_get("connection_id")?,
        connection_name: r.try_get("connection_name")?,
        name: r.try_get("name")?,
        external_namespace: r.try_get("external_namespace")?,
        access_mode: r.try_get("access_mode")?,
        owner_department_id: r.try_get("owner_department_id")?,
        status: r.try_get("status")?,
        grant_count: r.try_get::<i64, _>("grant_count")? as u64,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn validate_status(value: &str) -> AppResult<()> {
    if matches!(value, "active" | "disabled") {
        Ok(())
    } else {
        Err(AppError::bad_request("INVALID_STATUS", "Status is invalid"))
    }
}
