use std::collections::{HashSet, VecDeque};

use agentx_api_types::PageResponse;
use agentx_domain::{
    MissingGrant, ResourceOperation, ResourceReference, ResourceType, ResourceVersionSnapshot,
    WorkflowDefinition, canonical_content_hash,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{
        IdempotencyReservation, audit, complete_idempotency, idempotency_key,
        require_department_scope, require_workflow_access, reserve_idempotency,
    },
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantResponse {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateGrantRequest {
    pub subject_type: String,
    pub subject_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantableResourceListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub resource_type: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantableResourceResponse {
    pub id: Uuid,
    pub resource_type: String,
    pub name: String,
    pub detail: String,
    pub status: String,
    pub owner_department_id: Uuid,
    pub grant_count: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
}

pub type GrantableResourcePage = PageResponse<GrantableResourceResponse>;

const GRANTABLE_RESOURCE_CTE: &str = r#"
WITH grantable_resources AS (
    SELECT c.tenant_id,c.id,
        CONVERT('credential' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci resource_type,
        CONVERT(c.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci name,
        CONVERT(CAST(c.credential_type AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci detail,
        CONVERT(CAST(c.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci status,
        c.owner_department_id,c.updated_at FROM credentials c
    UNION ALL
    SELECT a.tenant_id,a.id,
        CONVERT('model' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(a.alias USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(p.name,' / ',d.model_name) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(a.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        p.owner_department_id,a.updated_at FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id
    UNION ALL
    SELECT s.tenant_id,s.id,
        CONVERT('mcp_server' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(s.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(CAST(sv.transport AS CHAR),' / ',sv.endpoint) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(s.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        s.owner_department_id,s.updated_at FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number
    UNION ALL
    SELECT t.tenant_id,t.id,
        CONVERT('mcp_tool' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(COALESCE(t.title,t.name) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(s.name,' / ',t.name) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(IF(t.availability='available','active','unavailable') USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        s.owner_department_id,t.updated_at FROM mcp_tools t JOIN mcp_servers s ON s.id=t.server_id
    UNION ALL
    SELECT s.tenant_id,s.id,
        CONVERT('skill' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(s.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(COALESCE(s.description,'') USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(s.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        s.owner_department_id,s.updated_at FROM skills s
    UNION ALL
    SELECT r.tenant_id,r.id,
        CONVERT('rag' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(r.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(c.name,' / ',r.external_resource_id) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(r.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        r.owner_department_id,r.updated_at FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id
    UNION ALL
    SELECT n.tenant_id,n.id,
        CONVERT('memory' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(n.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(c.name,' / ',n.external_namespace) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(n.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        n.owner_department_id,n.updated_at FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id
    UNION ALL
    SELECT p.tenant_id,p.id,
        CONVERT('sandbox_profile' USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(p.name USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CONCAT(v.runner,' / ',v.image_digest) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        CONVERT(CAST(p.status AS CHAR) USING utf8mb4) COLLATE utf8mb4_0900_ai_ci,
        p.owner_department_id,p.updated_at FROM sandbox_profiles p JOIN sandbox_profile_versions v ON v.profile_id=p.id AND v.version_number=p.current_version_number
)
"#;

#[utoipa::path(get, path = "/api/v1/resources/grantable")]
pub async fn list_grantable_resources(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<GrantableResourceListQuery>,
) -> AppResult<Json<GrantableResourcePage>> {
    actor.require("resource:grant")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let resource_type = query.resource_type.unwrap_or_default();
    if !resource_type.is_empty() {
        parse_resource_type(&resource_type)?;
    }
    let visibility = if actor.company_admin {
        ""
    } else {
        "AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=r.tenant_id AND ur.user_id=? AND dc.descendant_id=r.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants vg JOIN department_closure dc ON dc.tenant_id=vg.tenant_id AND dc.ancestor_id=vg.subject_id WHERE vg.tenant_id=r.tenant_id AND vg.subject_type='department' AND vg.resource_type=r.resource_type AND vg.resource_id=r.id AND vg.operation_key IN ('view','manage') AND dc.descendant_id=?))"
    };
    let sql = format!(
        "{GRANTABLE_RESOURCE_CTE} SELECT r.id,r.resource_type,r.name,r.detail,r.status,r.owner_department_id,r.updated_at,(SELECT COUNT(*) FROM resource_grants g WHERE g.tenant_id=r.tenant_id AND g.resource_type=r.resource_type AND g.resource_id=r.id) grant_count,COUNT(*) OVER() total_count FROM grantable_resources r WHERE r.tenant_id=? AND (?='' OR r.resource_type=?) AND (?='%%' OR r.name LIKE ? OR r.detail LIKE ?) {visibility} ORDER BY r.resource_type,r.name LIMIT ? OFFSET ?"
    );
    let mut statement = sqlx::query(&sql)
        .bind(actor.tenant_id)
        .bind(&resource_type)
        .bind(&resource_type)
        .bind(&search)
        .bind(&search)
        .bind(&search);
    if !actor.company_admin {
        statement = statement.bind(actor.user_id).bind(actor.department_id);
    }
    let rows = statement
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(GrantableResourceResponse {
                id: row.try_get("id")?,
                resource_type: row.try_get("resource_type")?,
                name: row.try_get("name")?,
                detail: row.try_get("detail")?,
                status: row.try_get("status")?,
                owner_department_id: row.try_get("owner_department_id")?,
                grant_count: row.try_get::<i64, _>("grant_count")? as u64,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceValidationResponse {
    pub valid: bool,
    pub missing_grants: Vec<MissingGrantResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingGrantResponse {
    pub node_id: String,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub operation: String,
    pub reason: String,
    pub required_by_resource_id: Option<Uuid>,
}

impl From<MissingGrant> for MissingGrantResponse {
    fn from(value: MissingGrant) -> Self {
        Self {
            node_id: value.node_id,
            resource_type: value.resource_type.as_str().to_owned(),
            resource_id: value.resource_id,
            operation: value.operation.as_str().to_owned(),
            reason: value.reason,
            required_by_resource_id: value.required_by_resource_id,
        }
    }
}

#[utoipa::path(get, path = "/api/v1/resources/{resource_type}/{resource_id}/grants", params(("resource_type" = String, Path), ("resource_id" = Uuid, Path)))]
pub async fn list_grants(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((resource_type, resource_id)): Path<(String, Uuid)>,
) -> AppResult<Json<Vec<GrantResponse>>> {
    actor.require(permission_for(&resource_type, false)?)?;
    require_resource_visible(&state, &actor, &resource_type, resource_id).await?;
    let rows = sqlx::query("SELECT id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_at FROM resource_grants WHERE tenant_id=? AND resource_type=? AND resource_id=? ORDER BY created_at")
        .bind(actor.tenant_id).bind(&resource_type).bind(resource_id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(grant_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/resources/{resource_type}/{resource_id}/grants", request_body = CreateGrantRequest, params(("resource_type" = String, Path), ("resource_id" = Uuid, Path)))]
pub async fn create_grant(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((resource_type, resource_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<CreateGrantRequest>,
) -> AppResult<(StatusCode, Json<GrantResponse>)> {
    actor.require("resource:grant")?;
    parse_resource_type(&resource_type)?;
    parse_operation(&input.operation)?;
    require_resource_visible(&state, &actor, &resource_type, resource_id).await?;
    validate_grant_version(
        &state.pool,
        actor.tenant_id,
        &resource_type,
        resource_id,
        input.resource_version_id,
    )
    .await?;
    match input.subject_type.as_str() {
        "department" => require_department_scope(&state.pool, &actor, input.subject_id).await?,
        "workflow_service_identity" => {
            let workflow_id:Uuid=sqlx::query_scalar("SELECT workflow_id FROM workflow_service_identities WHERE id=? AND tenant_id=? AND status='active'").bind(input.subject_id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Workflow service identity"))?;
            require_workflow_access(&state.pool, &actor, workflow_id, true).await?;
        }
        _ => {
            return Err(AppError::bad_request(
                "INVALID_GRANT_SUBJECT",
                "Grant subject type is invalid",
            ));
        }
    }
    let id = Uuid::now_v7();
    let key = idempotency_key(&headers)?;
    let operation = format!("resource.grant:{resource_type}:{resource_id}");
    let mut tx = state.pool.begin().await?;
    if let IdempotencyReservation::Replay { response, .. } =
        reserve_idempotency(&mut tx, actor.tenant_id, &operation, key.as_deref(), &input).await?
    {
        let response = response.ok_or_else(|| {
            AppError::conflict(
                "IDEMPOTENCY_INCOMPLETE",
                "The original request did not complete",
            )
        })?;
        tx.rollback().await?;
        return Ok((
            StatusCode::OK,
            Json(serde_json::from_value(response).map_err(AppError::internal)?),
        ));
    }
    sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE resource_version_id=VALUES(resource_version_id)")
        .bind(id).bind(actor.tenant_id).bind(&input.subject_type).bind(input.subject_id).bind(&resource_type).bind(resource_id).bind(input.resource_version_id).bind(&input.operation).bind(actor.user_id).execute(&mut *tx).await?;
    let stored=sqlx::query("SELECT id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_at FROM resource_grants WHERE tenant_id=? AND subject_type=? AND subject_id=? AND resource_type=? AND resource_id=? AND operation_key=?").bind(actor.tenant_id).bind(&input.subject_type).bind(input.subject_id).bind(&resource_type).bind(resource_id).bind(&input.operation).fetch_one(&mut *tx).await?;
    let response = grant_from_row(stored)?;
    audit(&mut tx,&actor,"resource.granted","resource",resource_id,json!({"subjectType":input.subject_type,"subjectId":input.subject_id,"resourceType":resource_type,"operation":input.operation})).await?;
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation,
        key.as_deref(),
        response.id,
        &response,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(delete, path = "/api/v1/resources/{resource_type}/{resource_id}/grants/{grant_id}", params(("resource_type" = String, Path), ("resource_id" = Uuid, Path), ("grant_id" = Uuid, Path)))]
pub async fn delete_grant(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((resource_type, resource_id, grant_id)): Path<(String, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    actor.require("resource:grant")?;
    require_resource_visible(&state, &actor, &resource_type, resource_id).await?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("DELETE FROM resource_grants WHERE id=? AND tenant_id=? AND resource_type=? AND resource_id=?").bind(grant_id).bind(actor.tenant_id).bind(&resource_type).bind(resource_id).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Resource grant"));
    }
    audit(
        &mut tx,
        &actor,
        "resource.grant_revoked",
        "resource",
        resource_id,
        json!({"grantId":grant_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/resource-validation", params(("id" = Uuid, Path)))]
pub async fn validate_workflow_resources(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ResourceValidationResponse>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let definition: Value = sqlx::query_scalar(
        "SELECT definition_json FROM workflow_drafts WHERE tenant_id=? AND workflow_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Workflow draft"))?;
    let definition: WorkflowDefinition =
        serde_json::from_value(definition).map_err(AppError::internal)?;
    let missing = missing_for_definition(&state, &actor, id, &definition).await?;
    Ok(Json(ResourceValidationResponse {
        valid: missing.is_empty(),
        missing_grants: missing.into_iter().map(Into::into).collect(),
    }))
}

pub async fn validate_and_snapshot(
    state: &AppState,
    actor: &AuthActor,
    workflow_id: Uuid,
    definition: &WorkflowDefinition,
) -> AppResult<Vec<ResourceVersionSnapshot>> {
    let missing = missing_for_definition(state, actor, workflow_id, definition).await?;
    if !missing.is_empty() {
        let first = &missing[0];
        return Err(AppError::unprocessable(
            "RESOURCE_GRANT_MISSING",
            format!(
                "{} {} requires {} grant",
                first.resource_type.as_str(),
                first.resource_id,
                first.operation.as_str()
            ),
        ));
    }
    build_snapshots(state, actor.tenant_id, definition).await
}

pub async fn validate_version_grants(
    state: &AppState,
    actor: &AuthActor,
    version_id: Uuid,
) -> AppResult<Vec<MissingGrant>> {
    let identity:Uuid=sqlx::query_scalar("SELECT si.id FROM workflow_versions v JOIN workflow_service_identities si ON si.workflow_id=v.workflow_id AND si.tenant_id=v.tenant_id WHERE v.id=? AND v.tenant_id=?").bind(version_id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Workflow version"))?;
    let rows=sqlx::query("SELECT node_id,resource_type,resource_id,resource_version_id,operation_key FROM workflow_version_resources WHERE tenant_id=? AND workflow_version_id=?").bind(actor.tenant_id).bind(version_id).fetch_all(&state.pool).await?;
    let mut missing = Vec::new();
    for row in rows {
        let resource_type: String = row.try_get("resource_type")?;
        let resource_id: Uuid = row.try_get("resource_id")?;
        let resource_version_id: Option<Uuid> = row.try_get("resource_version_id")?;
        let operation: String = row.try_get("operation_key")?;
        let kind = parse_resource_type(&resource_type)?;
        if !resource_active(
            &state.pool,
            actor.tenant_id,
            kind,
            resource_id,
            resource_version_id,
        )
        .await?
        {
            missing.push(MissingGrant {
                node_id: row.try_get("node_id")?,
                resource_type: kind,
                resource_id,
                operation: parse_operation(&operation)?,
                reason: "resource_missing_or_disabled".to_owned(),
                required_by_resource_id: None,
            });
        } else if !has_grant(
            &state.pool,
            actor.tenant_id,
            identity,
            &resource_type,
            resource_id,
            &operation,
        )
        .await?
        {
            missing.push(MissingGrant {
                node_id: row.try_get("node_id")?,
                resource_type: kind,
                resource_id,
                operation: parse_operation(&operation)?,
                reason: "grant_revoked".to_owned(),
                required_by_resource_id: None,
            });
        }
    }
    Ok(missing)
}

async fn missing_for_definition(
    state: &AppState,
    actor: &AuthActor,
    workflow_id: Uuid,
    definition: &WorkflowDefinition,
) -> AppResult<Vec<MissingGrant>> {
    let identity:Uuid=sqlx::query_scalar("SELECT id FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=? AND status='active'").bind(actor.tenant_id).bind(workflow_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Workflow service identity"))?;
    let mut required = VecDeque::new();
    for node in &definition.nodes {
        for reference in &node.resource_references {
            required.push_back((node.id.clone(), reference.clone(), None));
        }
    }
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    while let Some((node_id, reference, required_by)) = required.pop_front() {
        let key = (
            node_id.clone(),
            reference.resource_type,
            reference.resource_id,
            reference.operation,
        );
        if !seen.insert(key) {
            continue;
        }
        if !resource_active(
            &state.pool,
            actor.tenant_id,
            reference.resource_type,
            reference.resource_id,
            reference.resource_version_id,
        )
        .await?
        {
            missing.push(MissingGrant {
                node_id: node_id.clone(),
                resource_type: reference.resource_type,
                resource_id: reference.resource_id,
                operation: reference.operation,
                reason: "resource_missing_or_disabled".to_owned(),
                required_by_resource_id: required_by,
            });
            continue;
        }
        if !has_grant(
            &state.pool,
            actor.tenant_id,
            identity,
            reference.resource_type.as_str(),
            reference.resource_id,
            reference.operation.as_str(),
        )
        .await?
        {
            missing.push(MissingGrant {
                node_id: node_id.clone(),
                resource_type: reference.resource_type,
                resource_id: reference.resource_id,
                operation: reference.operation,
                reason: "workflow_grant_missing".to_owned(),
                required_by_resource_id: required_by,
            });
        }
        if reference.resource_type == ResourceType::Skill {
            let version_id = resolve_version_id(
                &state.pool,
                actor.tenant_id,
                ResourceType::Skill,
                reference.resource_id,
                reference.resource_version_id,
            )
            .await?;
            let rows=sqlx::query("SELECT resource_type,resource_id,resource_version_id,operation_key FROM skill_dependencies WHERE tenant_id=? AND skill_version_id=?").bind(actor.tenant_id).bind(version_id).fetch_all(&state.pool).await?;
            for row in rows {
                let resource_type: String = row.try_get("resource_type")?;
                let operation: String = row.try_get("operation_key")?;
                required.push_back((
                    node_id.clone(),
                    ResourceReference {
                        resource_type: parse_resource_type(&resource_type)?,
                        resource_id: row.try_get("resource_id")?,
                        resource_version_id: row.try_get("resource_version_id")?,
                        operation: parse_operation(&operation)?,
                    },
                    Some(reference.resource_id),
                ));
            }
        }
        if let Some(server_id) =
            mcp_server_dependency(&state.pool, actor.tenant_id, &reference).await?
        {
            required.push_back((
                node_id.clone(),
                ResourceReference {
                    resource_type: ResourceType::McpServer,
                    resource_id: server_id,
                    resource_version_id: None,
                    operation: ResourceOperation::Use,
                },
                Some(reference.resource_id),
            ));
        }
        for credential_id in
            credential_dependencies(&state.pool, actor.tenant_id, &reference).await?
        {
            required.push_back((
                node_id.clone(),
                ResourceReference {
                    resource_type: ResourceType::Credential,
                    resource_id: credential_id,
                    resource_version_id: None,
                    operation: ResourceOperation::Use,
                },
                Some(reference.resource_id),
            ));
        }
    }
    Ok(missing)
}

async fn build_snapshots(
    state: &AppState,
    tenant: Uuid,
    definition: &WorkflowDefinition,
) -> AppResult<Vec<ResourceVersionSnapshot>> {
    let mut queue = VecDeque::new();
    for node in &definition.nodes {
        for reference in &node.resource_references {
            queue.push_back((node.id.clone(), reference.clone()));
        }
    }
    let mut seen = HashSet::new();
    let mut snapshots = Vec::new();
    while let Some((node_id, mut reference)) = queue.pop_front() {
        if !seen.insert((
            node_id.clone(),
            reference.resource_type,
            reference.resource_id,
            reference.operation,
        )) {
            continue;
        }
        let snapshot = resource_snapshot(&state.pool, tenant, &mut reference).await?;
        let hash = canonical_content_hash(&snapshot).map_err(AppError::internal)?;
        if reference.resource_type == ResourceType::Skill {
            let version_id = reference
                .resource_version_id
                .expect("skill version resolved");
            let rows=sqlx::query("SELECT resource_type,resource_id,resource_version_id,operation_key FROM skill_dependencies WHERE tenant_id=? AND skill_version_id=?").bind(tenant).bind(version_id).fetch_all(&state.pool).await?;
            for row in rows {
                let resource_type: String = row.try_get("resource_type")?;
                let operation: String = row.try_get("operation_key")?;
                queue.push_back((
                    node_id.clone(),
                    ResourceReference {
                        resource_type: parse_resource_type(&resource_type)?,
                        resource_id: row.try_get("resource_id")?,
                        resource_version_id: row.try_get("resource_version_id")?,
                        operation: parse_operation(&operation)?,
                    },
                ));
            }
        }
        if let Some(server_id) = mcp_server_dependency(&state.pool, tenant, &reference).await? {
            queue.push_back((
                node_id.clone(),
                ResourceReference {
                    resource_type: ResourceType::McpServer,
                    resource_id: server_id,
                    resource_version_id: None,
                    operation: ResourceOperation::Use,
                },
            ));
        }
        for credential_id in credential_dependencies(&state.pool, tenant, &reference).await? {
            queue.push_back((
                node_id.clone(),
                ResourceReference {
                    resource_type: ResourceType::Credential,
                    resource_id: credential_id,
                    resource_version_id: None,
                    operation: ResourceOperation::Use,
                },
            ));
        }
        snapshots.push(ResourceVersionSnapshot {
            node_id,
            reference,
            snapshot_hash: hash,
            snapshot,
        });
    }
    Ok(snapshots)
}

async fn credential_dependencies(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    reference: &ResourceReference,
) -> AppResult<Vec<Uuid>> {
    let ids = match reference.resource_type {
        ResourceType::Model => sqlx::query_scalar("SELECT credential_id FROM (SELECT d.credential_id FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.id=? UNION SELECT p.credential_id FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND a.id=?) credential_refs WHERE credential_id IS NOT NULL")
            .bind(tenant).bind(reference.resource_id).bind(tenant).bind(reference.resource_id).fetch_all(pool).await?,
        ResourceType::McpServer => {
            sqlx::query_scalar("SELECT sv.credential_id FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number WHERE s.tenant_id=? AND s.id=? AND sv.credential_id IS NOT NULL").bind(tenant).bind(reference.resource_id).fetch_all(pool).await?
        }
        ResourceType::Rag => sqlx::query_scalar("SELECT c.credential_id FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND r.id=? AND c.credential_id IS NOT NULL").bind(tenant).bind(reference.resource_id).fetch_all(pool).await?,
        ResourceType::Memory => sqlx::query_scalar("SELECT c.credential_id FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND n.id=? AND c.credential_id IS NOT NULL").bind(tenant).bind(reference.resource_id).fetch_all(pool).await?,
        ResourceType::Credential | ResourceType::McpTool | ResourceType::Skill | ResourceType::SandboxProfile => Vec::new(),
    };
    Ok(ids)
}

async fn mcp_server_dependency(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    reference: &ResourceReference,
) -> AppResult<Option<Uuid>> {
    if reference.resource_type != ResourceType::McpTool {
        return Ok(None);
    }
    Ok(
        sqlx::query_scalar("SELECT server_id FROM mcp_tools WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(reference.resource_id)
            .fetch_optional(pool)
            .await?,
    )
}

async fn resource_snapshot(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    reference: &mut ResourceReference,
) -> AppResult<Value> {
    match reference.resource_type {
        ResourceType::Credential => {
            let r=sqlx::query("SELECT id,credential_type,current_secret_version,version FROM credentials WHERE tenant_id=? AND id=? AND status='active'").bind(tenant).bind(reference.resource_id).fetch_one(pool).await?;
            Ok(
                json!({"id":r.try_get::<Uuid,_>("id")?,"credentialType":r.try_get::<String,_>("credential_type")?,"secretVersion":r.try_get::<u64,_>("current_secret_version")?,"resourceVersion":r.try_get::<u64,_>("version")?}),
            )
        }
        ResourceType::Model => {
            let r=sqlx::query("SELECT a.id alias_id,a.alias,a.version alias_version,d.id deployment_id,d.model_name,d.version deployment_version,d.default_parameters,COALESCE(d.endpoint_override,p.endpoint) endpoint,p.id provider_id,p.provider_type,p.version provider_version,COALESCE(d.credential_id,p.credential_id) credential_id,pv.id price_version_id,pv.version_number price_version_number,pv.currency,CAST(pv.input_per_million AS CHAR) input_per_million,CAST(pv.output_per_million AS CHAR) output_per_million FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id LEFT JOIN model_price_versions pv ON pv.id=(SELECT latest.id FROM model_price_versions latest WHERE latest.deployment_id=d.id ORDER BY latest.version_number DESC LIMIT 1) WHERE a.tenant_id=? AND a.id=? AND a.status='active' AND d.status='active' AND p.status='active'").bind(tenant).bind(reference.resource_id).fetch_one(pool).await?;
            reference.resource_version_id = Some(r.try_get("deployment_id")?);
            Ok(
                json!({"aliasId":r.try_get::<Uuid,_>("alias_id")?,"alias":r.try_get::<String,_>("alias")?,"aliasVersion":r.try_get::<u64,_>("alias_version")?,"deploymentId":r.try_get::<Uuid,_>("deployment_id")?,"deploymentVersion":r.try_get::<u64,_>("deployment_version")?,"modelName":r.try_get::<String,_>("model_name")?,"defaultParameters":r.try_get::<Value,_>("default_parameters")?,"providerId":r.try_get::<Uuid,_>("provider_id")?,"providerType":r.try_get::<String,_>("provider_type")?,"endpoint":r.try_get::<String,_>("endpoint")?,"providerVersion":r.try_get::<u64,_>("provider_version")?,"credentialId":r.try_get::<Option<Uuid>,_>("credential_id")?,"price":{"versionId":r.try_get::<Option<Uuid>,_>("price_version_id")?,"versionNumber":r.try_get::<Option<u64>,_>("price_version_number")?,"currency":r.try_get::<Option<String>,_>("currency")?,"inputPerMillion":r.try_get::<Option<String>,_>("input_per_million")?,"outputPerMillion":r.try_get::<Option<String>,_>("output_per_million")?}}),
            )
        }
        ResourceType::McpServer => {
            let r=sqlx::query("SELECT s.id,s.name,s.status,s.version,sv.id server_version_id,sv.version_number,sv.transport,sv.endpoint,sv.credential_id,sv.configuration_hash FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number WHERE s.tenant_id=? AND s.id=? AND s.status='active'").bind(tenant).bind(reference.resource_id).fetch_one(pool).await?;
            reference.resource_version_id = Some(r.try_get("server_version_id")?);
            Ok(
                json!({"serverId":r.try_get::<Uuid,_>("id")?,"name":r.try_get::<String,_>("name")?,"serverVersionId":r.try_get::<Uuid,_>("server_version_id")?,"versionNumber":r.try_get::<u64,_>("version_number")?,"transport":r.try_get::<String,_>("transport")?,"endpoint":r.try_get::<String,_>("endpoint")?,"credentialId":r.try_get::<Option<Uuid>,_>("credential_id")?,"configurationHash":r.try_get::<String,_>("configuration_hash")?}),
            )
        }
        ResourceType::McpTool => {
            let version = resolve_version_id(
                pool,
                tenant,
                ResourceType::McpTool,
                reference.resource_id,
                reference.resource_version_id,
            )
            .await?;
            reference.resource_version_id = Some(version);
            let r=sqlx::query("SELECT tv.id,tv.version_number,tv.input_schema,tv.output_schema,tv.annotations_json,tv.schema_hash,t.name,t.title,s.id server_id,sv.id server_version_id,sv.transport,sv.endpoint,sv.credential_id,sv.configuration_hash,p.timeout_seconds,p.side_effect FROM mcp_tool_versions tv JOIN mcp_tools t ON t.id=tv.tool_id JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE tv.tenant_id=? AND tv.id=? AND t.availability='available' AND s.status='active' AND p.enabled=TRUE").bind(tenant).bind(version).fetch_one(pool).await?;
            Ok(
                json!({"toolVersionId":r.try_get::<Uuid,_>("id")?,"toolVersionNumber":r.try_get::<u64,_>("version_number")?,"toolName":r.try_get::<String,_>("name")?,"title":r.try_get::<Option<String>,_>("title")?,"serverId":r.try_get::<Uuid,_>("server_id")?,"serverVersionId":r.try_get::<Uuid,_>("server_version_id")?,"transport":r.try_get::<String,_>("transport")?,"endpoint":r.try_get::<String,_>("endpoint")?,"credentialId":r.try_get::<Option<Uuid>,_>("credential_id")?,"configurationHash":r.try_get::<String,_>("configuration_hash")?,"inputSchema":r.try_get::<Value,_>("input_schema")?,"outputSchema":r.try_get::<Option<Value>,_>("output_schema")?,"annotations":r.try_get::<Value,_>("annotations_json")?,"schemaHash":r.try_get::<String,_>("schema_hash")?,"timeoutSeconds":r.try_get::<u32,_>("timeout_seconds")?,"sideEffect":r.try_get::<String,_>("side_effect")?}),
            )
        }
        ResourceType::Skill => {
            let version = resolve_version_id(
                pool,
                tenant,
                ResourceType::Skill,
                reference.resource_id,
                reference.resource_version_id,
            )
            .await?;
            reference.resource_version_id = Some(version);
            let r=sqlx::query("SELECT sv.id,sv.version_number,sv.source_revision,sv.manifest_json,sv.content_hash FROM skill_versions sv JOIN skills s ON s.id=sv.skill_id WHERE sv.tenant_id=? AND sv.id=? AND s.status='active'").bind(tenant).bind(version).fetch_one(pool).await?;
            let files=sqlx::query("SELECT path,mime_type,artifact_id,content_hash,size_bytes FROM skill_version_files WHERE tenant_id=? AND skill_version_id=? ORDER BY path")
                .bind(tenant).bind(version).fetch_all(pool).await?.into_iter().map(|file|Ok(json!({"path":file.try_get::<String,_>("path")?,"mimeType":file.try_get::<String,_>("mime_type")?,"artifactId":file.try_get::<Uuid,_>("artifact_id")?,"contentHash":file.try_get::<String,_>("content_hash")?,"sizeBytes":file.try_get::<u64,_>("size_bytes")?}))).collect::<Result<Vec<_>,sqlx::Error>>()?;
            Ok(
                json!({"versionId":r.try_get::<Uuid,_>("id")?,"versionNumber":r.try_get::<u64,_>("version_number")?,"sourceRevision":r.try_get::<u64,_>("source_revision")?,"manifest":r.try_get::<Value,_>("manifest_json")?,"contentHash":r.try_get::<String,_>("content_hash")?,"files":files}),
            )
        }
        ResourceType::Rag => {
            let r=sqlx::query("SELECT rr.id,rr.external_resource_id,rr.version,c.id connection_id,c.endpoint,c.version connection_version,c.credential_id,c.configuration_json FROM rag_resources rr JOIN rag_connections c ON c.id=rr.connection_id WHERE rr.tenant_id=? AND rr.id=? AND rr.status='active' AND c.status='active'").bind(tenant).bind(reference.resource_id).fetch_one(pool).await?;
            Ok(
                json!({"resourceId":r.try_get::<Uuid,_>("id")?,"externalResourceId":r.try_get::<String,_>("external_resource_id")?,"resourceVersion":r.try_get::<u64,_>("version")?,"connectionId":r.try_get::<Uuid,_>("connection_id")?,"endpoint":r.try_get::<String,_>("endpoint")?,"connectionVersion":r.try_get::<u64,_>("connection_version")?,"credentialId":r.try_get::<Option<Uuid>,_>("credential_id")?,"configuration":r.try_get::<Value,_>("configuration_json")?}),
            )
        }
        ResourceType::Memory => {
            let r=sqlx::query("SELECT n.id,n.external_namespace,n.access_mode,n.version,c.id connection_id,c.endpoint,c.version connection_version,c.credential_id,c.configuration_json FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND n.id=? AND n.status='active' AND c.status='active'").bind(tenant).bind(reference.resource_id).fetch_one(pool).await?;
            Ok(
                json!({"namespaceId":r.try_get::<Uuid,_>("id")?,"externalNamespace":r.try_get::<String,_>("external_namespace")?,"accessMode":r.try_get::<String,_>("access_mode")?,"resourceVersion":r.try_get::<u64,_>("version")?,"connectionId":r.try_get::<Uuid,_>("connection_id")?,"endpoint":r.try_get::<String,_>("endpoint")?,"connectionVersion":r.try_get::<u64,_>("connection_version")?,"credentialId":r.try_get::<Option<Uuid>,_>("credential_id")?,"configuration":r.try_get::<Value,_>("configuration_json")?}),
            )
        }
        ResourceType::SandboxProfile => {
            let version = if let Some(version) = reference.resource_version_id {
                version
            } else {
                sqlx::query_scalar("SELECT v.id FROM sandbox_profile_versions v JOIN sandbox_profiles p ON p.id=v.profile_id WHERE p.tenant_id=? AND p.id=? AND p.status='active' ORDER BY v.version_number DESC LIMIT 1").bind(tenant).bind(reference.resource_id).fetch_optional(pool).await?.ok_or_else(||AppError::unprocessable("RESOURCE_VERSION_MISSING","Sandbox Profile has no available version"))?
            };
            reference.resource_version_id = Some(version);
            let r=sqlx::query("SELECT v.id,v.version_number,v.runner,v.image_digest,v.cpu_millis,v.memory_bytes,v.pids_limit,v.disk_bytes,v.timeout_seconds,v.output_limit_bytes,v.network_policy_json,v.configuration_hash FROM sandbox_profile_versions v JOIN sandbox_profiles p ON p.id=v.profile_id WHERE v.tenant_id=? AND v.id=? AND v.profile_id=? AND p.status='active'").bind(tenant).bind(version).bind(reference.resource_id).fetch_one(pool).await?;
            Ok(
                json!({"profileVersionId":r.try_get::<Uuid,_>("id")?,"versionNumber":r.try_get::<u64,_>("version_number")?,"runner":r.try_get::<String,_>("runner")?,"imageDigest":r.try_get::<String,_>("image_digest")?,"cpuMillis":r.try_get::<u32,_>("cpu_millis")?,"memoryBytes":r.try_get::<u64,_>("memory_bytes")?,"pidsLimit":r.try_get::<u32,_>("pids_limit")?,"diskBytes":r.try_get::<u64,_>("disk_bytes")?,"timeoutSeconds":r.try_get::<u32,_>("timeout_seconds")?,"outputLimitBytes":r.try_get::<u64,_>("output_limit_bytes")?,"networkPolicy":r.try_get::<Value,_>("network_policy_json")?,"configurationHash":r.try_get::<String,_>("configuration_hash")?}),
            )
        }
    }
}

async fn resolve_version_id(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    resource_type: ResourceType,
    resource_id: Uuid,
    requested: Option<Uuid>,
) -> AppResult<Uuid> {
    if let Some(id) = requested {
        return Ok(id);
    }
    let result=match resource_type{ResourceType::McpTool=>sqlx::query_scalar("SELECT tv.id FROM mcp_tool_versions tv JOIN mcp_tools t ON t.id=tv.tool_id JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE tv.tenant_id=? AND tv.tool_id=? AND t.availability='available' AND p.enabled=TRUE ORDER BY tv.version_number DESC LIMIT 1").bind(tenant).bind(resource_id).fetch_optional(pool).await?,ResourceType::Skill=>sqlx::query_scalar("SELECT sv.id FROM skill_versions sv JOIN skills s ON s.id=sv.skill_id WHERE sv.tenant_id=? AND sv.skill_id=? AND s.status='active' ORDER BY sv.version_number DESC LIMIT 1").bind(tenant).bind(resource_id).fetch_optional(pool).await?,_=>None};
    result.ok_or_else(|| {
        AppError::unprocessable(
            "RESOURCE_VERSION_MISSING",
            "Resource has no available version",
        )
    })
}

async fn resource_active(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    kind: ResourceType,
    id: Uuid,
    version: Option<Uuid>,
) -> AppResult<bool> {
    let active = match kind {
        ResourceType::Credential => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credentials WHERE tenant_id=? AND id=? AND status='active')").bind(tenant).bind(id).fetch_one(pool).await?,
        ResourceType::Model => if let Some(v) = version {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_aliases a JOIN model_deployments d ON d.tenant_id=a.tenant_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND a.id=? AND a.status='active' AND d.id=? AND d.status='active' AND p.status='active')").bind(tenant).bind(id).bind(v).fetch_one(pool).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND a.id=? AND a.status='active' AND d.status='active' AND p.status='active')").bind(tenant).bind(id).fetch_one(pool).await?
        },
        ResourceType::McpServer => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE tenant_id=? AND id=? AND status='active')").bind(tenant).bind(id).fetch_one(pool).await?,
        ResourceType::McpTool => if let Some(v) = version {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_tool_versions tv JOIN mcp_tools t ON t.id=tv.tool_id JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE tv.tenant_id=? AND tv.id=? AND tv.tool_id=? AND t.availability='available' AND s.status='active' AND p.enabled=TRUE)").bind(tenant).bind(v).bind(id).fetch_one(pool).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_tools t JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE t.tenant_id=? AND t.id=? AND t.availability='available' AND s.status='active' AND p.enabled=TRUE)").bind(tenant).bind(id).fetch_one(pool).await?
        },
        ResourceType::Skill => if let Some(v) = version {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skill_versions sv JOIN skills s ON s.id=sv.skill_id WHERE sv.tenant_id=? AND sv.id=? AND sv.skill_id=? AND s.status='active')").bind(tenant).bind(v).bind(id).fetch_one(pool).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skills s JOIN skill_versions sv ON sv.skill_id=s.id WHERE s.tenant_id=? AND s.id=? AND s.status='active')").bind(tenant).bind(id).fetch_one(pool).await?
        },
        ResourceType::Rag => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND r.id=? AND r.status='active' AND c.status='active')").bind(tenant).bind(id).fetch_one(pool).await?,
        ResourceType::Memory => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND n.id=? AND n.status='active' AND c.status='active')").bind(tenant).bind(id).fetch_one(pool).await?,
        ResourceType::SandboxProfile => if let Some(v)=version { sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profile_versions v JOIN sandbox_profiles p ON p.id=v.profile_id WHERE v.tenant_id=? AND v.id=? AND v.profile_id=? AND p.status='active')").bind(tenant).bind(v).bind(id).fetch_one(pool).await? } else { sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profiles WHERE tenant_id=? AND id=? AND status='active')").bind(tenant).bind(id).fetch_one(pool).await? },
    };
    Ok(active)
}
async fn has_grant(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    identity: Uuid,
    kind: &str,
    id: Uuid,
    operation: &str,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_grants WHERE tenant_id=? AND subject_type='workflow_service_identity' AND subject_id=? AND resource_type=? AND resource_id=? AND (operation_key=? OR operation_key='manage'))").bind(tenant).bind(identity).bind(kind).bind(id).bind(operation).fetch_one(pool).await?)
}

pub async fn require_resource_visible(
    state: &AppState,
    actor: &AuthActor,
    kind: &str,
    id: Uuid,
) -> AppResult<()> {
    let department = resource_department(&state.pool, actor.tenant_id, kind, id)
        .await?
        .ok_or_else(|| AppError::not_found("Resource"))?;
    if actor.company_admin {
        return Ok(());
    }
    let in_scope: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND ur.user_id=? AND dc.descendant_id=?)")
        .bind(actor.tenant_id).bind(actor.user_id).bind(department).fetch_one(&state.pool).await?;
    let granted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=? AND rg.subject_type='department' AND rg.resource_type=? AND rg.resource_id=? AND rg.operation_key IN ('view','manage') AND dc.descendant_id=?)")
        .bind(actor.tenant_id).bind(kind).bind(id).bind(actor.department_id).fetch_one(&state.pool).await?;
    if in_scope || granted {
        Ok(())
    } else {
        Err(AppError::not_found("Resource"))
    }
}
async fn resource_department(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    kind: &str,
    id: Uuid,
) -> AppResult<Option<Uuid>> {
    let result=match kind{"credential"=>sqlx::query_scalar("SELECT owner_department_id FROM credentials WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"model"=>sqlx::query_scalar("SELECT p.owner_department_id FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id JOIN model_providers p ON p.id=d.provider_id WHERE a.tenant_id=? AND a.id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"mcp_server"=>sqlx::query_scalar("SELECT owner_department_id FROM mcp_servers WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"mcp_tool"=>sqlx::query_scalar("SELECT s.owner_department_id FROM mcp_tools t JOIN mcp_servers s ON s.id=t.server_id WHERE t.tenant_id=? AND t.id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"skill"=>sqlx::query_scalar("SELECT owner_department_id FROM skills WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"rag"=>sqlx::query_scalar("SELECT owner_department_id FROM rag_resources WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"memory"=>sqlx::query_scalar("SELECT owner_department_id FROM memory_namespaces WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,"sandbox_profile"=>sqlx::query_scalar("SELECT owner_department_id FROM sandbox_profiles WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(pool).await?,_=>return Err(AppError::bad_request("INVALID_RESOURCE_TYPE","Resource type is invalid"))};
    Ok(result)
}

async fn validate_grant_version(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    kind: &str,
    resource_id: Uuid,
    version_id: Option<Uuid>,
) -> AppResult<()> {
    let Some(version_id) = version_id else {
        return Ok(());
    };
    let valid: bool = match kind {
        "model" => {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM model_deployments WHERE tenant_id=? AND id=?)",
            )
            .bind(tenant)
            .bind(version_id)
            .fetch_one(pool)
            .await?
        }
        "mcp_tool" => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mcp_tool_versions WHERE tenant_id=? AND id=? AND tool_id=?)",
        )
        .bind(tenant)
        .bind(version_id)
        .bind(resource_id)
        .fetch_one(pool)
        .await?,
        "skill" => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM skill_versions WHERE tenant_id=? AND id=? AND skill_id=?)",
        )
        .bind(tenant)
        .bind(version_id)
        .bind(resource_id)
        .fetch_one(pool)
        .await?,
        "sandbox_profile" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profile_versions WHERE tenant_id=? AND id=? AND profile_id=?)").bind(tenant).bind(version_id).bind(resource_id).fetch_one(pool).await?,
        "credential" | "mcp_server" | "rag" | "memory" => false,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::unprocessable(
            "RESOURCE_VERSION_INVALID",
            "Resource version does not belong to the selected resource",
        ))
    }
}

fn parse_resource_type(value: &str) -> AppResult<ResourceType> {
    match value {
        "credential" => Ok(ResourceType::Credential),
        "model" => Ok(ResourceType::Model),
        "mcp_server" => Ok(ResourceType::McpServer),
        "mcp_tool" => Ok(ResourceType::McpTool),
        "skill" => Ok(ResourceType::Skill),
        "rag" => Ok(ResourceType::Rag),
        "memory" => Ok(ResourceType::Memory),
        "sandbox_profile" => Ok(ResourceType::SandboxProfile),
        _ => Err(AppError::bad_request(
            "INVALID_RESOURCE_TYPE",
            "Resource type is invalid",
        )),
    }
}
fn parse_operation(value: &str) -> AppResult<ResourceOperation> {
    match value {
        "view" => Ok(ResourceOperation::View),
        "use" => Ok(ResourceOperation::Use),
        "read" => Ok(ResourceOperation::Read),
        "write" => Ok(ResourceOperation::Write),
        "manage" => Ok(ResourceOperation::Manage),
        _ => Err(AppError::bad_request(
            "INVALID_RESOURCE_OPERATION",
            "Resource operation is invalid",
        )),
    }
}
fn permission_for(kind: &str, manage: bool) -> AppResult<&'static str> {
    match (kind, manage) {
        ("credential", false) => Ok("credential:view"),
        ("credential", true) => Ok("credential:manage"),
        ("model", false) => Ok("model:view"),
        ("model", true) => Ok("model:manage"),
        ("mcp_server" | "mcp_tool", false) => Ok("mcp:view"),
        ("mcp_server" | "mcp_tool", true) => Ok("mcp:manage"),
        ("skill", false) => Ok("skill:view"),
        ("skill", true) => Ok("skill:manage"),
        ("rag", false) => Ok("knowledge:view"),
        ("rag", true) => Ok("knowledge:manage"),
        ("memory", false) => Ok("memory:view"),
        ("memory", true) => Ok("memory:manage"),
        _ => Err(AppError::bad_request(
            "INVALID_RESOURCE_TYPE",
            "Resource type is invalid",
        )),
    }
}
fn grant_from_row(r: sqlx::mysql::MySqlRow) -> Result<GrantResponse, sqlx::Error> {
    Ok(GrantResponse {
        id: r.try_get("id")?,
        subject_type: r.try_get("subject_type")?,
        subject_id: r.try_get("subject_id")?,
        resource_type: r.try_get("resource_type")?,
        resource_id: r.try_get("resource_id")?,
        resource_version_id: r.try_get("resource_version_id")?,
        operation: r.try_get("operation_key")?,
        created_at: r.try_get("created_at")?,
    })
}
