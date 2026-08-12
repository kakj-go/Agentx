use std::collections::HashSet;

use agentx_api_types::PageResponse;
use agentx_domain::{ResourceReference, ResourceType, canonical_content_hash};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{MySql, MySqlConnection, Row, Transaction};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    control_common::{
        IdempotencyReservation, audit, complete_idempotency, idempotency_key,
        require_workflow_access, reserve_idempotency,
    },
    deletion,
    error::{AppError, AppResult},
    grants::{
        GRANTABLE_RESOURCE_CTE, ResolvedRequirement, expand_reference_requirements_on,
        has_grant_on, parse_operation, parse_resource_type, require_resource_visible_on,
        resource_active_on, resource_department_on,
    },
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceOptionQuery {
    pub resource_type: String,
    pub operation: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRequirementResponse {
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
    pub name: Option<String>,
    pub required_by_resource_id: Option<Uuid>,
    pub owner_department_id: Option<Uuid>,
    pub authorized: bool,
    pub active: bool,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowResourceOptionResponse {
    pub id: Uuid,
    pub resource_type: String,
    pub name: String,
    pub detail: String,
    pub status: String,
    pub resource_version_id: Option<Uuid>,
    pub access_state: String,
    pub pending_request_id: Option<Uuid>,
    pub requirements: Vec<ResourceRequirementResponse>,
}

pub type WorkflowResourceOptionPage = PageResponse<WorkflowResourceOptionResponse>;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAuthorizationInput {
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAuthorizationResponse {
    pub workflow_id: Uuid,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub granted_count: u64,
    pub already_granted_count: u64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateResourceGrantRequest {
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
    pub source_node_id: Option<String>,
    pub source_revision: Option<u64>,
    pub message: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResourceGrantRequest {
    pub expected_version: u64,
    pub comment: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelResourceGrantRequest {
    pub expected_version: u64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantRequestReviewResponse {
    pub id: Uuid,
    pub owner_department_id: Uuid,
    pub owner_department_name: String,
    pub status: String,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_by_name: Option<String>,
    pub review_comment: Option<String>,
    pub version: u64,
    pub can_act: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub reviewed_at: Option<time::OffsetDateTime>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantRequestAuditResponse {
    pub id: Uuid,
    pub action: String,
    pub actor_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantRequestResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub workflow_service_identity_id: Uuid,
    pub primary_resource_type: String,
    pub primary_resource_id: Uuid,
    pub primary_resource_name: Option<String>,
    pub operation: String,
    pub source_node_id: Option<String>,
    pub source_revision: Option<u64>,
    pub message: Option<String>,
    pub status: String,
    pub requested_by: Uuid,
    pub requested_by_name: String,
    pub version: u64,
    pub items: Vec<ResourceRequirementResponse>,
    pub reviews: Vec<ResourceGrantRequestReviewResponse>,
    pub history: Vec<ResourceGrantRequestAuditResponse>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrantRequestListQuery {
    pub status: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

pub type ResourceGrantRequestPage = PageResponse<ResourceGrantRequestResponse>;

#[utoipa::path(get, path = "/api/v1/workflows/{id}/resource-options", params(("id" = Uuid, Path), ResourceOptionQuery))]
pub async fn list_resource_options(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(workflow_id): Path<Uuid>,
    Query(query): Query<ResourceOptionQuery>,
) -> AppResult<Json<WorkflowResourceOptionPage>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, workflow_id, false).await?;
    actor.require(resource_view_permission(&query.resource_type)?)?;
    let kind = parse_resource_type(&query.resource_type)?;
    let operation = parse_operation(query.operation.as_deref().unwrap_or("use"))?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(100).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let identity = workflow_identity(&state.pool, actor.tenant_id, workflow_id).await?;
    let can_edit = actor.company_admin
        || (actor
            .permissions
            .iter()
            .any(|permission| permission == "workflow:edit")
            && require_workflow_access(&state.pool, &actor, workflow_id, true)
                .await
                .is_ok());
    let visibility = if actor.company_admin {
        String::new()
    } else {
        "AND (EXISTS(SELECT 1 FROM user_roles ur JOIN roles role ON role.id=ur.role_id AND role.tenant_id=ur.tenant_id WHERE ur.tenant_id=r.tenant_id AND ur.user_id=? AND role.status='active' AND role.data_scope='company') OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=r.tenant_id AND ur.user_id=? AND dc.descendant_id=r.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants vg JOIN department_closure dc ON dc.tenant_id=vg.tenant_id AND dc.ancestor_id=vg.subject_id WHERE vg.tenant_id=r.tenant_id AND vg.subject_type='department' AND vg.resource_type=r.resource_type AND vg.resource_id=r.id AND vg.operation_key IN ('view','manage') AND dc.descendant_id=?))".to_owned()
    };
    let sql = format!(
        "{GRANTABLE_RESOURCE_CTE} SELECT r.id,r.resource_type,r.name,r.detail,r.status,COUNT(*) OVER() total_count FROM grantable_resources r WHERE r.tenant_id=? AND r.resource_type=? AND (?='%%' OR r.name LIKE ? OR r.detail LIKE ?) {visibility} ORDER BY r.name LIMIT ? OFFSET ?"
    );
    let mut statement = sqlx::query(&sql)
        .bind(actor.tenant_id)
        .bind(kind.as_str())
        .bind(&search)
        .bind(&search)
        .bind(&search);
    if !actor.company_admin {
        statement = statement
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.department_id);
    }
    let rows = statement
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    let mut connection = state.pool.acquire().await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let resource_id: Uuid = row.try_get("id")?;
        let version_id =
            current_resource_version_on(&mut connection, actor.tenant_id, kind, resource_id)
                .await?;
        let root = ResourceReference {
            binding_id: None,
            binding_role: None,
            resource_type: kind,
            resource_id,
            resource_version_id: version_id,
            operation,
        };
        let requirements =
            expand_reference_requirements_on(&mut connection, actor.tenant_id, root).await?;
        let requirement_responses =
            describe_requirements_on(&mut connection, &actor, identity, &requirements).await?;
        let active = requirement_responses.iter().all(|item| item.active);
        let authorized = active && requirement_responses.iter().all(|item| item.authorized);
        let pending_request_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM resource_grant_requests WHERE tenant_id=? AND workflow_service_identity_id=? AND primary_resource_type=? AND primary_resource_id=? AND operation_key=? AND status='pending' ORDER BY created_at DESC LIMIT 1")
            .bind(actor.tenant_id).bind(identity).bind(kind.as_str()).bind(resource_id).bind(operation.as_str()).fetch_optional(&mut *connection).await?;
        let rejected: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_grant_requests WHERE tenant_id=? AND workflow_service_identity_id=? AND primary_resource_type=? AND primary_resource_id=? AND operation_key=? AND status='rejected')")
            .bind(actor.tenant_id).bind(identity).bind(kind.as_str()).bind(resource_id).bind(operation.as_str()).fetch_one(&mut *connection).await?;
        let can_grant = can_edit
            && actor
                .permissions
                .iter()
                .any(|permission| permission == "resource:grant")
            && requirements_visible_on(&mut connection, &actor, &requirements).await?;
        let access_state = resource_access_state(
            active,
            authorized,
            pending_request_id.is_some(),
            can_grant,
            rejected,
        );
        items.push(WorkflowResourceOptionResponse {
            id: resource_id,
            resource_type: kind.as_str().to_owned(),
            name: row.try_get("name")?,
            detail: row.try_get("detail")?,
            status: row.try_get("status")?,
            resource_version_id: version_id,
            access_state: access_state.to_owned(),
            pending_request_id,
            requirements: requirement_responses,
        });
    }
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/resource-authorizations", request_body = ResourceAuthorizationInput, params(("id" = Uuid, Path)))]
pub async fn authorize_resource(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(workflow_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<ResourceAuthorizationInput>,
) -> AppResult<Json<ResourceAuthorizationResponse>> {
    actor.require("workflow:edit")?;
    actor.require("resource:grant")?;
    require_workflow_access(&state.pool, &actor, workflow_id, true).await?;
    ensure_workflow_active(&state.pool, actor.tenant_id, workflow_id).await?;
    let key = required_idempotency_key(&headers)?;
    let root = input_reference(&input)?;
    let operation_key = format!(
        "workflow.resource_authorization:{workflow_id}:{}:{}",
        input.resource_type, input.resource_id
    );
    let mut tx = state.pool.begin().await?;
    if let IdempotencyReservation::Replay { response } =
        reserve_idempotency(&mut tx, actor.tenant_id, &operation_key, Some(&key), &input).await?
    {
        let response = response.ok_or_else(|| {
            AppError::conflict(
                "IDEMPOTENCY_INCOMPLETE",
                "The original request did not complete",
            )
        })?;
        tx.rollback().await?;
        return Ok(Json(
            serde_json::from_value(response).map_err(AppError::internal)?,
        ));
    }
    let identity = workflow_identity_on(&mut tx, actor.tenant_id, workflow_id).await?;
    let requirements = expand_reference_requirements_on(&mut tx, actor.tenant_id, root).await?;
    ensure_requirements_active_and_visible(&mut tx, &actor, &requirements).await?;
    deletion::lock_resource_targets(
        &mut tx,
        actor.tenant_id,
        requirements
            .iter()
            .map(|item| {
                (
                    item.reference.resource_type.as_str().to_owned(),
                    item.reference.resource_id,
                )
            })
            .collect(),
    )
    .await?;
    let mut granted_count = 0;
    let mut already_granted_count = 0;
    for item in &requirements {
        if has_grant_on(
            &mut tx,
            actor.tenant_id,
            identity,
            item.reference.resource_type.as_str(),
            item.reference.resource_id,
            item.reference.operation.as_str(),
        )
        .await?
        {
            already_granted_count += 1;
            continue;
        }
        sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,?,?,?,?,?) ON DUPLICATE KEY UPDATE resource_version_id=VALUES(resource_version_id)")
            .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(identity).bind(item.reference.resource_type.as_str()).bind(item.reference.resource_id).bind(item.reference.resource_version_id).bind(item.reference.operation.as_str()).bind(actor.user_id).execute(&mut *tx).await?;
        granted_count += 1;
        audit(&mut tx, &actor, "resource.granted", "resource", item.reference.resource_id, json!({"workflowId":workflow_id,"subjectId":identity,"resourceType":item.reference.resource_type.as_str(),"operation":item.reference.operation.as_str(),"requiredByResourceId":item.required_by_resource_id})).await?;
    }
    let response = ResourceAuthorizationResponse {
        workflow_id,
        resource_type: input.resource_type.clone(),
        resource_id: input.resource_id,
        granted_count,
        already_granted_count,
    };
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation_key,
        Some(&key),
        input.resource_id,
        &response,
    )
    .await?;
    satisfy_open_requests(
        &mut tx,
        &actor,
        identity,
        &input.resource_type,
        input.resource_id,
        &input.operation,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(response))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/resource-grant-requests", request_body = CreateResourceGrantRequest, params(("id" = Uuid, Path)))]
pub async fn create_resource_grant_request(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(workflow_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<CreateResourceGrantRequest>,
) -> AppResult<(StatusCode, Json<ResourceGrantRequestResponse>)> {
    actor.require("workflow:edit")?;
    require_workflow_access(&state.pool, &actor, workflow_id, true).await?;
    ensure_workflow_active(&state.pool, actor.tenant_id, workflow_id).await?;
    let key = required_idempotency_key(&headers)?;
    let root = input_reference(&ResourceAuthorizationInput {
        resource_type: input.resource_type.clone(),
        resource_id: input.resource_id,
        resource_version_id: input.resource_version_id,
        operation: input.operation.clone(),
    })?;
    let operation_key = format!(
        "resource_grant_request.create:{workflow_id}:{}:{}",
        input.resource_type, input.resource_id
    );
    let mut tx = state.pool.begin().await?;
    if let IdempotencyReservation::Replay { response } =
        reserve_idempotency(&mut tx, actor.tenant_id, &operation_key, Some(&key), &input).await?
    {
        let request_id = response
            .as_ref()
            .and_then(idempotent_request_id)
            .ok_or_else(|| {
                AppError::conflict(
                    "IDEMPOTENCY_INCOMPLETE",
                    "The original request did not complete",
                )
            })?;
        tx.rollback().await?;
        return Ok((
            StatusCode::OK,
            Json(load_request(&state, &actor, request_id).await?),
        ));
    }
    let identity = workflow_identity_on(&mut tx, actor.tenant_id, workflow_id).await?;
    require_resource_visible_on(&mut tx, &actor, &input.resource_type, input.resource_id).await?;
    let requirements = expand_reference_requirements_on(&mut tx, actor.tenant_id, root).await?;
    ensure_requirements_active(&mut tx, actor.tenant_id, &requirements).await?;
    let dependency_hash = requirement_hash(&mut tx, actor.tenant_id, &requirements).await?;
    let open_key = canonical_content_hash(&json!({"identity":identity,"resourceType":input.resource_type,"resourceId":input.resource_id,"operation":input.operation})).map_err(AppError::internal)?;
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM resource_grant_requests WHERE tenant_id=? AND open_dedupe_key=? AND status='pending'").bind(actor.tenant_id).bind(&open_key).fetch_optional(&mut *tx).await? {
        complete_idempotency(&mut tx, actor.tenant_id, &operation_key, Some(&key), id, &json!({"requestId":id})).await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(load_request(&state, &actor, id).await?)));
    }
    let request_id = Uuid::now_v7();
    let message = input
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if message
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1000)
    {
        return Err(AppError::bad_request(
            "RESOURCE_GRANT_REQUEST_MESSAGE_INVALID",
            "Request message must not exceed 1000 characters",
        ));
    }
    sqlx::query("INSERT INTO resource_grant_requests(id,tenant_id,workflow_id,workflow_service_identity_id,primary_resource_type,primary_resource_id,primary_resource_version_id,operation_key,source_node_id,source_revision,request_message,dependency_hash,open_dedupe_key,requested_by) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id")
        .bind(request_id).bind(actor.tenant_id).bind(workflow_id).bind(identity).bind(&input.resource_type).bind(input.resource_id).bind(input.resource_version_id).bind(&input.operation).bind(&input.source_node_id).bind(input.source_revision).bind(message).bind(&dependency_hash).bind(&open_key).bind(actor.user_id).execute(&mut *tx).await?;
    let stored_request_id: Uuid = sqlx::query_scalar("SELECT id FROM resource_grant_requests WHERE tenant_id=? AND open_dedupe_key=? AND status='pending'")
        .bind(actor.tenant_id).bind(&open_key).fetch_one(&mut *tx).await?;
    if stored_request_id != request_id {
        complete_idempotency(
            &mut tx,
            actor.tenant_id,
            &operation_key,
            Some(&key),
            stored_request_id,
            &json!({"requestId":stored_request_id}),
        )
        .await?;
        tx.commit().await?;
        return Ok((
            StatusCode::OK,
            Json(load_request(&state, &actor, stored_request_id).await?),
        ));
    }
    let mut departments = HashSet::new();
    for item in &requirements {
        let department = resource_department_on(
            &mut tx,
            actor.tenant_id,
            item.reference.resource_type.as_str(),
            item.reference.resource_id,
        )
        .await?
        .ok_or_else(|| AppError::not_found("Resource"))?;
        departments.insert(department);
        sqlx::query("INSERT INTO resource_grant_request_items(id,tenant_id,request_id,resource_type,resource_id,resource_version_id,operation_key,required_by_resource_id,owner_department_id) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(request_id).bind(item.reference.resource_type.as_str()).bind(item.reference.resource_id).bind(item.reference.resource_version_id).bind(item.reference.operation.as_str()).bind(item.required_by_resource_id).bind(department).execute(&mut *tx).await?;
    }
    for department in departments {
        let review_id = Uuid::now_v7();
        sqlx::query("INSERT INTO resource_grant_request_reviews(id,tenant_id,request_id,owner_department_id) VALUES(?,?,?,?)").bind(review_id).bind(actor.tenant_id).bind(request_id).bind(department).execute(&mut *tx).await?;
        notify_department_reviewers(&mut tx, actor.tenant_id, review_id, request_id, department)
            .await?;
    }
    audit(&mut tx, &actor, "resource_grant_request.created", "resource_grant_request", request_id, json!({"workflowId":workflow_id,"resourceType":input.resource_type,"resourceId":input.resource_id,"dependencyHash":dependency_hash})).await?;
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation_key,
        Some(&key),
        request_id,
        &json!({"requestId":request_id}),
    )
    .await?;
    tx.commit().await?;
    let response = load_request(&state, &actor, request_id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/api/v1/resource-grant-requests",
    params(ResourceGrantRequestListQuery)
)]
pub async fn list_resource_grant_requests(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ResourceGrantRequestListQuery>,
) -> AppResult<Json<ResourceGrantRequestPage>> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(100).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let status = query.status.unwrap_or_default();
    let visible = request_visibility_sql(&actor);
    let sql = format!(
        "SELECT r.id,COUNT(*) OVER() total_count FROM resource_grant_requests r WHERE r.tenant_id=? AND (?='' OR r.status=?) AND ({visible}) ORDER BY r.updated_at DESC LIMIT ? OFFSET ?"
    );
    let mut statement = sqlx::query(&sql)
        .bind(actor.tenant_id)
        .bind(&status)
        .bind(&status);
    if !actor.company_admin {
        statement = statement.bind(actor.user_id).bind(actor.user_id);
    }
    let rows = statement
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(load_request(&state, &actor, row.try_get("id")?).await?);
    }
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(get, path = "/api/v1/resource-grant-requests/{id}", params(("id" = Uuid, Path)))]
pub async fn get_resource_grant_request(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ResourceGrantRequestResponse>> {
    Ok(Json(load_request(&state, &actor, id).await?))
}

#[utoipa::path(post, path = "/api/v1/resource-grant-requests/{id}/reviews/{department_id}/approve", request_body = ReviewResourceGrantRequest)]
pub async fn approve_resource_grant_request(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, department_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ReviewResourceGrantRequest>,
) -> AppResult<Json<ResourceGrantRequestResponse>> {
    review_request(&state, &actor, id, department_id, headers, input, true).await?;
    Ok(Json(load_request(&state, &actor, id).await?))
}

#[utoipa::path(post, path = "/api/v1/resource-grant-requests/{id}/reviews/{department_id}/reject", request_body = ReviewResourceGrantRequest)]
pub async fn reject_resource_grant_request(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, department_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ReviewResourceGrantRequest>,
) -> AppResult<Json<ResourceGrantRequestResponse>> {
    review_request(&state, &actor, id, department_id, headers, input, false).await?;
    Ok(Json(load_request(&state, &actor, id).await?))
}

#[utoipa::path(post, path = "/api/v1/resource-grant-requests/{id}/cancel", request_body = CancelResourceGrantRequest)]
pub async fn cancel_resource_grant_request(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<CancelResourceGrantRequest>,
) -> AppResult<Json<ResourceGrantRequestResponse>> {
    let key = required_idempotency_key(&headers)?;
    let row = sqlx::query(
        "SELECT requested_by,workflow_id FROM resource_grant_requests WHERE tenant_id=? AND id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Resource grant request"))?;
    let requested_by: Uuid = row.try_get("requested_by")?;
    if requested_by != actor.user_id {
        require_workflow_access(&state.pool, &actor, row.try_get("workflow_id")?, true).await?;
    }
    let mut tx = state.pool.begin().await?;
    let operation_key = format!("resource_grant_request.cancel:{id}");
    if let IdempotencyReservation::Replay { response } =
        reserve_idempotency(&mut tx, actor.tenant_id, &operation_key, Some(&key), &input).await?
    {
        let request_id = response
            .as_ref()
            .and_then(idempotent_request_id)
            .ok_or_else(|| {
                AppError::conflict(
                    "IDEMPOTENCY_INCOMPLETE",
                    "The original request did not complete",
                )
            })?;
        tx.rollback().await?;
        return Ok(Json(load_request(&state, &actor, request_id).await?));
    }
    let changed = sqlx::query("UPDATE resource_grant_requests SET status='cancelled',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=? AND status='pending' AND version=?")
        .bind(actor.tenant_id).bind(id).bind(input.expected_version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "RESOURCE_GRANT_REQUEST_STATE_CONFLICT",
            "Resource grant request changed on the server",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "resource_grant_request.cancelled",
        "resource_grant_request",
        id,
        json!({}),
    )
    .await?;
    notify_requester(
        &mut tx,
        actor.tenant_id,
        id,
        requested_by,
        "resource_grant_request_cancelled",
        "notifications.resourceGrantCancelled.title",
        "notifications.resourceGrantCancelled.body",
        "neutral",
    )
    .await?;
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation_key,
        Some(&key),
        id,
        &json!({"requestId":id}),
    )
    .await?;
    tx.commit().await?;
    let response = load_request(&state, &actor, id).await?;
    Ok(Json(response))
}

async fn review_request(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    department_id: Uuid,
    headers: HeaderMap,
    input: ReviewResourceGrantRequest,
    approve: bool,
) -> AppResult<()> {
    actor.require("approval:act")?;
    actor.require("resource:grant")?;
    require_department_reviewer(&state.pool, actor, department_id).await?;
    let key = required_idempotency_key(&headers)?;
    let mut tx = state.pool.begin().await?;
    let operation_key = format!(
        "resource_grant_request.review:{id}:{department_id}:{}",
        if approve { "approve" } else { "reject" }
    );
    if let IdempotencyReservation::Replay { .. } =
        reserve_idempotency(&mut tx, actor.tenant_id, &operation_key, Some(&key), &input).await?
    {
        tx.rollback().await?;
        return Ok(());
    }
    let request = sqlx::query("SELECT r.workflow_id,r.workflow_service_identity_id,r.primary_resource_type,r.primary_resource_id,r.primary_resource_version_id,r.operation_key,r.dependency_hash,r.requested_by,r.status,w.status workflow_status FROM resource_grant_requests r JOIN workflows w ON w.id=r.workflow_id AND w.tenant_id=r.tenant_id WHERE r.tenant_id=? AND r.id=? FOR UPDATE")
        .bind(actor.tenant_id).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(|| AppError::not_found("Resource grant request"))?;
    if request.try_get::<String, _>("status")? != "pending" {
        return Err(AppError::conflict(
            "RESOURCE_GRANT_REQUEST_STATE_CONFLICT",
            "Resource grant request is no longer pending",
        ));
    }
    let workflow_status: String = request.try_get("workflow_status")?;
    let requested_by: Uuid = request.try_get("requested_by")?;
    let requester_can_edit = requester_can_edit_workflow_on(
        &mut tx,
        actor.tenant_id,
        requested_by,
        request.try_get("workflow_id")?,
    )
    .await?;
    if workflow_status != "active" || !requester_can_edit {
        sqlx::query("UPDATE resource_grant_requests SET status='stale',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=? AND status='pending'")
            .bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            actor,
            "resource_grant_request.stale",
            "resource_grant_request",
            id,
            json!({"reason":"workflow_or_requester_access_changed"}),
        )
        .await?;
        notify_requester(
            &mut tx,
            actor.tenant_id,
            id,
            requested_by,
            "resource_grant_request_stale",
            "notifications.resourceGrantStale.title",
            "notifications.resourceGrantStale.body",
            "danger",
        )
        .await?;
        tx.commit().await?;
        return Err(AppError::conflict(
            "RESOURCE_GRANT_REQUEST_STALE",
            "The workflow or requester permissions changed",
        ));
    }
    let root = ResourceReference {
        binding_id: None,
        binding_role: None,
        resource_type: parse_resource_type(
            &request.try_get::<String, _>("primary_resource_type")?,
        )?,
        resource_id: request.try_get("primary_resource_id")?,
        resource_version_id: request.try_get("primary_resource_version_id")?,
        operation: parse_operation(&request.try_get::<String, _>("operation_key")?)?,
    };
    let requirements = expand_reference_requirements_on(&mut tx, actor.tenant_id, root).await?;
    let dependency_hash = requirement_hash(&mut tx, actor.tenant_id, &requirements).await?;
    if dependency_hash != request.try_get::<String, _>("dependency_hash")?
        || ensure_requirements_active(&mut tx, actor.tenant_id, &requirements)
            .await
            .is_err()
    {
        sqlx::query("UPDATE resource_grant_requests SET status='stale',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=?").bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            actor,
            "resource_grant_request.stale",
            "resource_grant_request",
            id,
            json!({}),
        )
        .await?;
        notify_requester(
            &mut tx,
            actor.tenant_id,
            id,
            requested_by,
            "resource_grant_request_stale",
            "notifications.resourceGrantStale.title",
            "notifications.resourceGrantStale.body",
            "danger",
        )
        .await?;
        tx.commit().await?;
        return Err(AppError::conflict(
            "RESOURCE_GRANT_REQUEST_STALE",
            "Resource dependencies changed; submit a new request",
        ));
    }
    let target = if approve { "approved" } else { "rejected" };
    let changed = sqlx::query("UPDATE resource_grant_request_reviews SET status=?,reviewed_by=?,review_comment=?,reviewed_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND request_id=? AND owner_department_id=? AND status='pending' AND version=?")
        .bind(target).bind(actor.user_id).bind(input.comment.as_deref().map(str::trim).filter(|value| !value.is_empty())).bind(actor.tenant_id).bind(id).bind(department_id).bind(input.expected_version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "RESOURCE_GRANT_REQUEST_STATE_CONFLICT",
            "Review changed on the server",
        ));
    }
    audit(
        &mut tx,
        actor,
        &format!("resource_grant_request.{target}"),
        "resource_grant_request",
        id,
        json!({"departmentId":department_id}),
    )
    .await?;
    if !approve {
        sqlx::query("UPDATE resource_grant_requests SET status='rejected',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=?").bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
        notify_requester(
            &mut tx,
            actor.tenant_id,
            id,
            requested_by,
            "resource_grant_request_rejected",
            "notifications.resourceGrantRejected.title",
            "notifications.resourceGrantRejected.body",
            "danger",
        )
        .await?;
    } else {
        let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_grant_request_reviews WHERE tenant_id=? AND request_id=? AND status='pending')").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
        if !pending {
            finalize_approved_request(
                &mut tx,
                actor,
                id,
                request.try_get("workflow_service_identity_id")?,
                &requirements,
            )
            .await?;
            notify_requester(
                &mut tx,
                actor.tenant_id,
                id,
                requested_by,
                "resource_grant_request_approved",
                "notifications.resourceGrantApproved.title",
                "notifications.resourceGrantApproved.body",
                "success",
            )
            .await?;
        }
    }
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation_key,
        Some(&key),
        id,
        &json!({"requestId":id,"status":target}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn finalize_approved_request(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    request_id: Uuid,
    identity: Uuid,
    requirements: &[ResolvedRequirement],
) -> AppResult<()> {
    for item in requirements {
        let department = resource_department_on(
            &mut *tx,
            actor.tenant_id,
            item.reference.resource_type.as_str(),
            item.reference.resource_id,
        )
        .await?
        .ok_or_else(|| AppError::not_found("Resource"))?;
        let approved_by: Uuid = sqlx::query_scalar("SELECT reviewed_by FROM resource_grant_request_reviews WHERE tenant_id=? AND request_id=? AND owner_department_id=? AND status='approved'").bind(actor.tenant_id).bind(request_id).bind(department).fetch_one(&mut **tx).await?;
        sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,?,?,?,?,?) ON DUPLICATE KEY UPDATE resource_version_id=VALUES(resource_version_id)")
            .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(identity).bind(item.reference.resource_type.as_str()).bind(item.reference.resource_id).bind(item.reference.resource_version_id).bind(item.reference.operation.as_str()).bind(approved_by).execute(&mut **tx).await?;
        audit(tx, actor, "resource.granted", "resource", item.reference.resource_id, json!({"requestId":request_id,"reviewDepartmentId":department,"approvedBy":approved_by,"subjectId":identity,"resourceType":item.reference.resource_type.as_str(),"operation":item.reference.operation.as_str(),"requiredByResourceId":item.required_by_resource_id})).await?;
    }
    sqlx::query("UPDATE resource_grant_requests SET status='approved',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=?").bind(actor.tenant_id).bind(request_id).execute(&mut **tx).await?;
    audit(
        tx,
        actor,
        "resource_grant_request.fulfilled",
        "resource_grant_request",
        request_id,
        json!({"grantCount":requirements.len()}),
    )
    .await?;
    Ok(())
}

async fn load_request(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<ResourceGrantRequestResponse> {
    let visible = request_visibility_sql(actor);
    let sql = format!(
        "SELECT r.id,r.workflow_id,w.name workflow_name,r.workflow_service_identity_id,r.primary_resource_type,r.primary_resource_id,r.operation_key,r.source_node_id,r.source_revision,r.request_message,r.status,r.requested_by,u.display_name requested_by_name,r.version,r.created_at,r.updated_at FROM resource_grant_requests r JOIN workflows w ON w.id=r.workflow_id JOIN users u ON u.id=r.requested_by WHERE r.tenant_id=? AND r.id=? AND ({visible})"
    );
    let mut statement = sqlx::query(&sql).bind(actor.tenant_id).bind(id);
    if !actor.company_admin {
        statement = statement.bind(actor.user_id).bind(actor.user_id);
    }
    let row = statement
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Resource grant request"))?;
    let identity: Uuid = row.try_get("workflow_service_identity_id")?;
    let item_rows = sqlx::query("SELECT resource_type,resource_id,resource_version_id,operation_key,required_by_resource_id,owner_department_id FROM resource_grant_request_items WHERE tenant_id=? AND request_id=? ORDER BY resource_type,resource_id").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let mut connection = state.pool.acquire().await?;
    let mut items = Vec::with_capacity(item_rows.len());
    for item in item_rows {
        let resource_type: String = item.try_get("resource_type")?;
        let resource_id: Uuid = item.try_get("resource_id")?;
        let operation: String = item.try_get("operation_key")?;
        let can_see =
            require_resource_visible_on(&mut connection, actor, &resource_type, resource_id)
                .await
                .is_ok();
        items.push(ResourceRequirementResponse {
            resource_type: resource_type.clone(),
            resource_id,
            resource_version_id: item.try_get("resource_version_id")?,
            operation: operation.clone(),
            name: if can_see {
                resource_name_on(
                    &mut connection,
                    actor.tenant_id,
                    &resource_type,
                    resource_id,
                )
                .await?
            } else {
                None
            },
            required_by_resource_id: item.try_get("required_by_resource_id")?,
            owner_department_id: Some(item.try_get("owner_department_id")?),
            authorized: has_grant_on(
                &mut connection,
                actor.tenant_id,
                identity,
                &resource_type,
                resource_id,
                &operation,
            )
            .await?,
            active: resource_active_on(
                &mut connection,
                actor.tenant_id,
                parse_resource_type(&resource_type)?,
                resource_id,
                item.try_get("resource_version_id")?,
            )
            .await?,
        });
    }
    let review_rows = sqlx::query("SELECT rv.id,rv.owner_department_id,d.name owner_department_name,rv.status,rv.reviewed_by,u.display_name reviewed_by_name,rv.review_comment,rv.reviewed_at,rv.version FROM resource_grant_request_reviews rv JOIN departments d ON d.id=rv.owner_department_id LEFT JOIN users u ON u.id=rv.reviewed_by WHERE rv.tenant_id=? AND rv.request_id=? ORDER BY d.name").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let mut reviews = Vec::with_capacity(review_rows.len());
    for review in review_rows {
        let department_id: Uuid = review.try_get("owner_department_id")?;
        reviews.push(ResourceGrantRequestReviewResponse {
            id: review.try_get("id")?,
            owner_department_id: department_id,
            owner_department_name: review.try_get("owner_department_name")?,
            status: review.try_get("status")?,
            reviewed_by: review.try_get("reviewed_by")?,
            reviewed_by_name: review.try_get("reviewed_by_name")?,
            review_comment: review.try_get("review_comment")?,
            version: review.try_get("version")?,
            can_act: can_review_department(&state.pool, actor, department_id).await?,
            reviewed_at: review.try_get("reviewed_at")?,
        });
    }
    let primary_resource_type: String = row.try_get("primary_resource_type")?;
    let primary_resource_id: Uuid = row.try_get("primary_resource_id")?;
    let primary_resource_name = if require_resource_visible_on(
        &mut connection,
        actor,
        &primary_resource_type,
        primary_resource_id,
    )
    .await
    .is_ok()
    {
        resource_name_on(
            &mut connection,
            actor.tenant_id,
            &primary_resource_type,
            primary_resource_id,
        )
        .await?
    } else {
        None
    };
    let history = sqlx::query("SELECT a.id,a.action,u.display_name actor_name,a.occurred_at FROM audit_events a LEFT JOIN users u ON u.id=a.actor_user_id AND u.tenant_id=a.tenant_id WHERE a.tenant_id=? AND a.target_type='resource_grant_request' AND a.target_id=? ORDER BY a.occurred_at")
        .bind(actor.tenant_id)
        .bind(id.to_string())
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|event| {
            Ok(ResourceGrantRequestAuditResponse {
                id: event.try_get("id")?,
                action: event.try_get("action")?,
                actor_name: event.try_get("actor_name")?,
                occurred_at: event.try_get("occurred_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(ResourceGrantRequestResponse {
        id: row.try_get("id")?,
        workflow_id: row.try_get("workflow_id")?,
        workflow_name: row.try_get("workflow_name")?,
        workflow_service_identity_id: identity,
        primary_resource_type,
        primary_resource_id,
        primary_resource_name,
        operation: row.try_get("operation_key")?,
        source_node_id: row.try_get("source_node_id")?,
        source_revision: row.try_get("source_revision")?,
        message: row.try_get("request_message")?,
        status: row.try_get("status")?,
        requested_by: row.try_get("requested_by")?,
        requested_by_name: row.try_get("requested_by_name")?,
        version: row.try_get("version")?,
        items,
        reviews,
        history,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn input_reference(input: &ResourceAuthorizationInput) -> AppResult<ResourceReference> {
    Ok(ResourceReference {
        binding_id: None,
        binding_role: None,
        resource_type: parse_resource_type(&input.resource_type)?,
        resource_id: input.resource_id,
        resource_version_id: input.resource_version_id,
        operation: parse_operation(&input.operation)?,
    })
}

async fn workflow_identity(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    workflow: Uuid,
) -> AppResult<Uuid> {
    let mut connection = pool.acquire().await?;
    workflow_identity_on(&mut connection, tenant, workflow).await
}

async fn ensure_workflow_active(
    pool: &sqlx::MySqlPool,
    tenant: Uuid,
    workflow: Uuid,
) -> AppResult<()> {
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflows WHERE tenant_id=? AND id=? AND status='active')",
    )
    .bind(tenant)
    .bind(workflow)
    .fetch_one(pool)
    .await?;
    if active {
        Ok(())
    } else {
        Err(AppError::conflict(
            "WORKFLOW_NOT_ACTIVE",
            "The workflow is no longer active",
        ))
    }
}

async fn workflow_identity_on(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    workflow: Uuid,
) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT id FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=? AND status='active'").bind(tenant).bind(workflow).fetch_optional(&mut *connection).await?.ok_or_else(|| AppError::not_found("Workflow service identity"))
}

async fn current_resource_version_on(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    kind: ResourceType,
    id: Uuid,
) -> AppResult<Option<Uuid>> {
    match kind {
        ResourceType::Model => Ok(sqlx::query_scalar("SELECT deployment_id FROM model_aliases WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(&mut *connection).await?),
        ResourceType::McpTool => Ok(sqlx::query_scalar("SELECT tv.id FROM mcp_tools t JOIN mcp_tool_versions tv ON tv.tool_id=t.id AND tv.version_number=t.current_version_number WHERE t.tenant_id=? AND t.id=?").bind(tenant).bind(id).fetch_optional(&mut *connection).await?),
        ResourceType::Skill => Ok(sqlx::query_scalar("SELECT id FROM skill_versions WHERE tenant_id=? AND skill_id=? ORDER BY version_number DESC LIMIT 1").bind(tenant).bind(id).fetch_optional(&mut *connection).await?),
        ResourceType::SandboxProfile => Ok(sqlx::query_scalar("SELECT v.id FROM sandbox_profiles p JOIN sandbox_profile_versions v ON v.profile_id=p.id AND v.version_number=p.current_version_number WHERE p.tenant_id=? AND p.id=?").bind(tenant).bind(id).fetch_optional(&mut *connection).await?),
        _ => Ok(None),
    }
}

async fn describe_requirements_on(
    connection: &mut MySqlConnection,
    actor: &AuthActor,
    identity: Uuid,
    requirements: &[ResolvedRequirement],
) -> AppResult<Vec<ResourceRequirementResponse>> {
    let mut result = Vec::with_capacity(requirements.len());
    for item in requirements {
        let kind = item.reference.resource_type.as_str();
        let visible =
            require_resource_visible_on(&mut *connection, actor, kind, item.reference.resource_id)
                .await
                .is_ok();
        result.push(ResourceRequirementResponse {
            resource_type: kind.to_owned(),
            resource_id: item.reference.resource_id,
            resource_version_id: item.reference.resource_version_id,
            operation: item.reference.operation.as_str().to_owned(),
            name: if visible {
                resource_name_on(
                    &mut *connection,
                    actor.tenant_id,
                    kind,
                    item.reference.resource_id,
                )
                .await?
            } else {
                None
            },
            required_by_resource_id: item.required_by_resource_id,
            owner_department_id: resource_department_on(
                &mut *connection,
                actor.tenant_id,
                kind,
                item.reference.resource_id,
            )
            .await?,
            authorized: has_grant_on(
                &mut *connection,
                actor.tenant_id,
                identity,
                kind,
                item.reference.resource_id,
                item.reference.operation.as_str(),
            )
            .await?,
            active: resource_active_on(
                &mut *connection,
                actor.tenant_id,
                item.reference.resource_type,
                item.reference.resource_id,
                item.reference.resource_version_id,
            )
            .await?,
        });
    }
    Ok(result)
}

async fn requirements_visible_on(
    connection: &mut MySqlConnection,
    actor: &AuthActor,
    requirements: &[ResolvedRequirement],
) -> AppResult<bool> {
    for item in requirements {
        if require_resource_visible_on(
            &mut *connection,
            actor,
            item.reference.resource_type.as_str(),
            item.reference.resource_id,
        )
        .await
        .is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn ensure_requirements_active_and_visible(
    connection: &mut MySqlConnection,
    actor: &AuthActor,
    requirements: &[ResolvedRequirement],
) -> AppResult<()> {
    ensure_requirements_active(connection, actor.tenant_id, requirements).await?;
    for item in requirements {
        require_resource_visible_on(
            &mut *connection,
            actor,
            item.reference.resource_type.as_str(),
            item.reference.resource_id,
        )
        .await?;
    }
    Ok(())
}

async fn ensure_requirements_active(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    requirements: &[ResolvedRequirement],
) -> AppResult<()> {
    for item in requirements {
        if !resource_active_on(
            &mut *connection,
            tenant,
            item.reference.resource_type,
            item.reference.resource_id,
            item.reference.resource_version_id,
        )
        .await?
        {
            return Err(AppError::unprocessable(
                "RESOURCE_UNAVAILABLE",
                format!(
                    "{} {} is unavailable",
                    item.reference.resource_type.as_str(),
                    item.reference.resource_id
                ),
            ));
        }
    }
    Ok(())
}

async fn requirement_hash(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    requirements: &[ResolvedRequirement],
) -> AppResult<String> {
    let mut values = Vec::with_capacity(requirements.len());
    for item in requirements {
        values.push(json!({"resourceType":item.reference.resource_type.as_str(),"resourceId":item.reference.resource_id,"resourceVersionId":item.reference.resource_version_id,"operation":item.reference.operation.as_str(),"requiredByResourceId":item.required_by_resource_id,"ownerDepartmentId":resource_department_on(&mut *connection, tenant, item.reference.resource_type.as_str(), item.reference.resource_id).await?}));
    }
    values.sort_by_key(ToString::to_string);
    canonical_content_hash(&Value::Array(values)).map_err(AppError::internal)
}

async fn resource_name_on(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    kind: &str,
    id: Uuid,
) -> AppResult<Option<String>> {
    let sql = format!(
        "{GRANTABLE_RESOURCE_CTE} SELECT name FROM grantable_resources WHERE tenant_id=? AND resource_type=? AND id=?"
    );
    Ok(sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(kind)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?)
}

async fn require_department_reviewer(
    pool: &sqlx::MySqlPool,
    actor: &AuthActor,
    department: Uuid,
) -> AppResult<()> {
    if can_review_department(pool, actor, department).await? {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "You cannot review resources owned by this department",
        ))
    }
}

async fn can_review_department(
    pool: &sqlx::MySqlPool,
    actor: &AuthActor,
    department: Uuid,
) -> AppResult<bool> {
    if actor.company_admin {
        return Ok(true);
    }
    if !actor
        .permissions
        .iter()
        .any(|value| value == "approval:act")
        || !actor
            .permissions
            .iter()
            .any(|value| value == "resource:grant")
    {
        return Ok(false);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=? AND ur.user_id=? AND r.code='department_admin' AND r.status='active' AND dc.descendant_id=? AND EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.tenant_id=ur.tenant_id AND rp.role_id=r.id AND p.permission_key='approval:act') AND EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.tenant_id=ur.tenant_id AND rp.role_id=r.id AND p.permission_key='resource:grant'))").bind(actor.tenant_id).bind(actor.user_id).bind(department).fetch_one(pool).await?)
}

async fn requester_can_edit_workflow_on(
    connection: &mut MySqlConnection,
    tenant: Uuid,
    requester: Uuid,
    workflow: Uuid,
) -> AppResult<bool> {
    let permission_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users u JOIN user_roles ur ON ur.tenant_id=u.tenant_id AND ur.user_id=u.id JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id AND r.status='active' JOIN role_permissions rp ON rp.tenant_id=ur.tenant_id AND rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE u.tenant_id=? AND u.id=? AND u.status='active' AND p.permission_key='workflow:edit'")
        .bind(tenant).bind(requester).fetch_one(&mut *connection).await?;
    if permission_count == 0 {
        return Ok(false);
    }
    let owned_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflows WHERE tenant_id=? AND id=? AND status='active' AND owner_user_id=?")
        .bind(tenant).bind(workflow).bind(requester).fetch_one(&mut *connection).await?;
    if owned_count > 0 {
        return Ok(true);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflows w WHERE w.tenant_id=? AND w.id=? AND w.status='active' AND (EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.tenant_id=w.tenant_id AND wm.workflow_id=w.id AND wm.user_id=? AND wm.member_role IN ('editor','manager')) OR EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id AND r.status='active' WHERE ur.tenant_id=w.tenant_id AND ur.user_id=? AND r.code='company_admin') OR EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id AND r.status='active' JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=w.tenant_id AND ur.user_id=? AND dc.descendant_id=w.owner_department_id)))")
        .bind(tenant).bind(workflow).bind(requester).bind(requester).bind(requester).fetch_one(&mut *connection).await?)
}

async fn notify_department_reviewers(
    tx: &mut Transaction<'_, MySql>,
    tenant: Uuid,
    source_event: Uuid,
    request_id: Uuid,
    department: Uuid,
) -> AppResult<()> {
    let mut users: Vec<Uuid> = sqlx::query_scalar("SELECT ur.user_id FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id JOIN role_permissions rp ON rp.tenant_id=ur.tenant_id AND rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id JOIN users u ON u.id=ur.user_id AND u.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND r.code='department_admin' AND r.status='active' AND dc.descendant_id=? AND u.status='active' AND p.permission_key IN ('approval:act','resource:grant') GROUP BY ur.user_id HAVING COUNT(DISTINCT p.permission_key)=2").bind(tenant).bind(department).fetch_all(&mut **tx).await?;
    if users.is_empty() {
        users = sqlx::query_scalar("SELECT ur.user_id FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id JOIN role_permissions rp ON rp.tenant_id=ur.tenant_id AND rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id JOIN users u ON u.id=ur.user_id AND u.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND r.code='company_admin' AND r.status='active' AND u.status='active' AND p.permission_key IN ('approval:act','resource:grant') GROUP BY ur.user_id HAVING COUNT(DISTINCT p.permission_key)=2").bind(tenant).fetch_all(&mut **tx).await?;
    }
    create_notification(
        tx,
        tenant,
        source_event,
        request_id,
        &users,
        "resource_grant_request_created",
        "notifications.resourceGrantRequested.title",
        "notifications.resourceGrantRequested.body",
        "warning",
    )
    .await
}

async fn notify_requester(
    tx: &mut Transaction<'_, MySql>,
    tenant: Uuid,
    request_id: Uuid,
    requester: Uuid,
    kind: &str,
    title: &str,
    body: &str,
    tone: &str,
) -> AppResult<()> {
    create_notification(
        tx,
        tenant,
        request_id,
        request_id,
        &[requester],
        kind,
        title,
        body,
        tone,
    )
    .await
}

async fn create_notification(
    tx: &mut Transaction<'_, MySql>,
    tenant: Uuid,
    source_event: Uuid,
    request_id: Uuid,
    users: &[Uuid],
    kind: &str,
    title: &str,
    body: &str,
    tone: &str,
) -> AppResult<()> {
    let notification_id = Uuid::now_v7();
    sqlx::query("INSERT INTO notifications(id,tenant_id,source_event_id,notification_type,title_key,body_key,arguments_json,target_type,target_id,target_path,tone) VALUES(?,?,?,?,?,?,JSON_OBJECT(), 'resource_grant_request',?,?,?) ON DUPLICATE KEY UPDATE id=id")
        .bind(notification_id).bind(tenant).bind(source_event).bind(kind).bind(title).bind(body).bind(request_id).bind(format!("/approvals/resource-grants/{request_id}")).bind(tone).execute(&mut **tx).await?;
    let stored: Uuid = sqlx::query_scalar("SELECT id FROM notifications WHERE tenant_id=? AND source_event_id=? AND notification_type=?").bind(tenant).bind(source_event).bind(kind).fetch_one(&mut **tx).await?;
    for user in users {
        sqlx::query("INSERT IGNORE INTO notification_receipts(tenant_id,notification_id,user_id) VALUES(?,?,?)").bind(tenant).bind(stored).bind(user).execute(&mut **tx).await?;
    }
    Ok(())
}

async fn satisfy_open_requests(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    identity: Uuid,
    kind: &str,
    resource_id: Uuid,
    operation: &str,
) -> AppResult<()> {
    let requests = sqlx::query("SELECT id,requested_by FROM resource_grant_requests WHERE tenant_id=? AND workflow_service_identity_id=? AND primary_resource_type=? AND primary_resource_id=? AND operation_key=? AND status='pending' FOR UPDATE")
        .bind(actor.tenant_id).bind(identity).bind(kind).bind(resource_id).bind(operation).fetch_all(&mut **tx).await?;
    for request in requests {
        let request_id: Uuid = request.try_get("id")?;
        sqlx::query("UPDATE resource_grant_request_reviews SET status='approved',reviewed_by=?,review_comment='fulfilled_by_direct_authorization',reviewed_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND request_id=? AND status='pending'")
            .bind(actor.user_id).bind(actor.tenant_id).bind(request_id).execute(&mut **tx).await?;
        sqlx::query("UPDATE resource_grant_requests SET status='approved',open_dedupe_key=NULL,resolved_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE tenant_id=? AND id=? AND status='pending'")
            .bind(actor.tenant_id).bind(request_id).execute(&mut **tx).await?;
        audit(
            tx,
            actor,
            "resource_grant_request.fulfilled_by_direct_authorization",
            "resource_grant_request",
            request_id,
            json!({"resourceType":kind,"resourceId":resource_id,"operation":operation}),
        )
        .await?;
        notify_requester(
            tx,
            actor.tenant_id,
            request_id,
            request.try_get("requested_by")?,
            "resource_grant_request_approved",
            "notifications.resourceGrantApproved.title",
            "notifications.resourceGrantApproved.body",
            "success",
        )
        .await?;
    }
    Ok(())
}

fn request_visibility_sql(actor: &AuthActor) -> &'static str {
    if actor.company_admin {
        "TRUE"
    } else {
        "r.requested_by=? OR EXISTS(SELECT 1 FROM resource_grant_request_reviews rv JOIN user_roles ur ON ur.tenant_id=rv.tenant_id AND ur.user_id=? JOIN roles role ON role.id=ur.role_id AND role.tenant_id=ur.tenant_id JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE rv.tenant_id=r.tenant_id AND rv.request_id=r.id AND role.code='department_admin' AND role.status='active' AND dc.descendant_id=rv.owner_department_id AND EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.tenant_id=ur.tenant_id AND rp.role_id=role.id AND p.permission_key='approval:act') AND EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.tenant_id=ur.tenant_id AND rp.role_id=role.id AND p.permission_key='resource:grant'))"
    }
}

fn required_idempotency_key(headers: &HeaderMap) -> AppResult<String> {
    idempotency_key(headers)?.ok_or_else(|| {
        AppError::bad_request("IDEMPOTENCY_KEY_REQUIRED", "Idempotency-Key is required")
    })
}

fn idempotent_request_id(response: &Value) -> Option<Uuid> {
    response
        .get("requestId")
        .or_else(|| response.get("id"))
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn resource_view_permission(kind: &str) -> AppResult<&'static str> {
    match kind {
        "credential" => Ok("credential:view"),
        "model" => Ok("model:view"),
        "mcp_server" | "mcp_tool" => Ok("mcp:view"),
        "skill" => Ok("skill:view"),
        "rag" => Ok("knowledge:view"),
        "memory" => Ok("memory:view"),
        "sandbox_profile" => Ok("sandbox:view"),
        _ => Err(AppError::bad_request(
            "INVALID_RESOURCE_TYPE",
            "Resource type is invalid",
        )),
    }
}

fn resource_access_state(
    active: bool,
    authorized: bool,
    pending: bool,
    can_grant: bool,
    rejected: bool,
) -> &'static str {
    if !active {
        "unavailable"
    } else if authorized {
        "authorized"
    } else if pending {
        "pending"
    } else if can_grant {
        "grantable"
    } else if rejected {
        "rejected"
    } else {
        "requestable"
    }
}

#[cfg(test)]
mod tests {
    use super::{requester_can_edit_workflow_on, resource_access_state};
    use agentx_infrastructure::mysql;
    use uuid::Uuid;

    #[test]
    fn resource_option_access_states_have_stable_precedence() {
        assert_eq!(
            resource_access_state(false, true, true, true, true),
            "unavailable"
        );
        assert_eq!(
            resource_access_state(true, true, true, true, true),
            "authorized"
        );
        assert_eq!(
            resource_access_state(true, false, true, true, true),
            "pending"
        );
        assert_eq!(
            resource_access_state(true, false, false, true, true),
            "grantable"
        );
        assert_eq!(
            resource_access_state(true, false, false, false, true),
            "rejected"
        );
        assert_eq!(
            resource_access_state(true, false, false, false, false),
            "requestable"
        );
    }

    #[tokio::test]
    async fn requester_edit_permission_and_open_request_uniqueness_are_enforced() {
        let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
        let (_container, pool) = crate::migration_tests::start_mysql().await;
        mysql::run_migrations(&pool)
            .await
            .expect("apply resource grant request migrations");

        let tenant = Uuid::now_v7();
        let department = Uuid::now_v7();
        let requester = Uuid::now_v7();
        let role = Uuid::now_v7();
        let workflow = Uuid::now_v7();
        let identity = Uuid::now_v7();
        sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Resource Test','resource test')")
            .bind(tenant).execute(&pool).await.expect("tenant");
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
            .bind(department).bind(tenant).execute(&pool).await.expect("department");
        sqlx::query("INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(?,?,?,0)")
            .bind(tenant).bind(department).bind(department).execute(&pool).await.expect("department closure");
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status) VALUES(?,?,'requester','requester','Requester','active')")
            .bind(requester).bind(tenant).execute(&pool).await.expect("requester");
        sqlx::query("INSERT INTO roles(id,tenant_id,code,name,data_scope) VALUES(?,?,'workflow_editor','Workflow Editor','own')")
            .bind(role).bind(tenant).execute(&pool).await.expect("role");
        let permission: Uuid =
            sqlx::query_scalar("SELECT id FROM permissions WHERE permission_key='workflow:edit'")
                .fetch_one(&pool)
                .await
                .expect("workflow edit permission");
        sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) VALUES(?,?,?)")
            .bind(tenant)
            .bind(role)
            .bind(permission)
            .execute(&pool)
            .await
            .expect("role permission");
        sqlx::query("INSERT INTO user_roles(id,tenant_id,user_id,role_id,scope_department_id) VALUES(?,?,?,?,?)")
            .bind(Uuid::now_v7()).bind(tenant).bind(requester).bind(role).bind(department).execute(&pool).await.expect("user role");
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Permission Workflow',?,?)")
            .bind(workflow).bind(tenant).bind(requester).bind(department).execute(&pool).await.expect("workflow");
        sqlx::query(
            "INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)",
        )
        .bind(identity)
        .bind(tenant)
        .bind(workflow)
        .execute(&pool)
        .await
        .expect("identity");

        let mut connection = pool.acquire().await.expect("connection");
        assert!(
            requester_can_edit_workflow_on(&mut connection, tenant, requester, workflow)
                .await
                .expect("permission check")
        );
        sqlx::query(
            "DELETE FROM role_permissions WHERE tenant_id=? AND role_id=? AND permission_id=?",
        )
        .bind(tenant)
        .bind(role)
        .bind(permission)
        .execute(&mut *connection)
        .await
        .expect("revoke edit permission");
        assert!(
            !requester_can_edit_workflow_on(&mut connection, tenant, requester, workflow)
                .await
                .expect("revoked permission check")
        );

        let first = Uuid::now_v7();
        let open_key = "sha256:v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        sqlx::query("INSERT INTO resource_grant_requests(id,tenant_id,workflow_id,workflow_service_identity_id,primary_resource_type,primary_resource_id,operation_key,dependency_hash,open_dedupe_key,requested_by) VALUES(?,?,?,?,'model',?,'use',?,?,?)")
            .bind(first).bind(tenant).bind(workflow).bind(identity).bind(Uuid::now_v7()).bind(open_key).bind(open_key).bind(requester).execute(&mut *connection).await.expect("first open request");
        let duplicate = sqlx::query("INSERT INTO resource_grant_requests(id,tenant_id,workflow_id,workflow_service_identity_id,primary_resource_type,primary_resource_id,operation_key,dependency_hash,open_dedupe_key,requested_by) VALUES(?,?,?,?,'model',?,'use',?,?,?)")
            .bind(Uuid::now_v7()).bind(tenant).bind(workflow).bind(identity).bind(Uuid::now_v7()).bind(open_key).bind(open_key).bind(requester).execute(&mut *connection).await;
        assert!(
            duplicate.is_err(),
            "only one pending request may use the open dedupe key"
        );
        sqlx::query(
            "UPDATE resource_grant_requests SET status='rejected',open_dedupe_key=NULL WHERE id=?",
        )
        .bind(first)
        .execute(&mut *connection)
        .await
        .expect("close first request");
        sqlx::query("INSERT INTO resource_grant_requests(id,tenant_id,workflow_id,workflow_service_identity_id,primary_resource_type,primary_resource_id,operation_key,dependency_hash,open_dedupe_key,requested_by) VALUES(?,?,?,?,'model',?,'use',?,?,?)")
            .bind(Uuid::now_v7()).bind(tenant).bind(workflow).bind(identity).bind(Uuid::now_v7()).bind(open_key).bind(open_key).bind(requester).execute(&mut *connection).await.expect("new request after terminal state");
    }
}
