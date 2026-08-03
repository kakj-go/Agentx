use agentx_api_types::{FieldError, PageResponse};
use agentx_domain::{WorkflowDefinition, canonical_content_hash, validate_definition};
use agentx_runtime::{CompileContext, NodeRegistry, WorkflowCompiler};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{
        IdempotencyReservation, audit, complete_idempotency, idempotency_key, outbox,
        require_workflow_access, reserve_idempotency, validate_name,
    },
    error::{AppError, AppResult},
    grants,
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub visibility: String,
    pub owner_user_id: Uuid,
    pub owner_department_id: Uuid,
    pub service_identity_id: Uuid,
    pub owner_name: String,
    pub draft_revision: u64,
    pub latest_version: Option<u64>,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub type WorkflowPage = PageResponse<WorkflowResponse>;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub version: u64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub schema_version: String,
    pub revision: u64,
    pub definition: Value,
    pub content_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveDraftRequest {
    pub expected_revision: u64,
    pub definition: Value,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevisionResponse {
    pub id: Uuid,
    pub revision: u64,
    pub schema_version: String,
    pub content_hash: String,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVersionResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version_number: u64,
    pub source_revision: u64,
    pub schema_version: String,
    pub content_hash: String,
    pub definition: Value,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateVersionRequest {
    pub draft_revision: u64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowMemberResponse {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub member_role: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertWorkflowMemberRequest {
    pub user_id: Uuid,
    pub member_role: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub is_builtin: bool,
    pub status: String,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnvironmentRequest {
    pub code: String,
    pub name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnvironmentRequest {
    pub name: String,
    pub status: String,
    pub version: u64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub environment_id: Uuid,
    pub environment_name: String,
    pub workflow_version_id: Uuid,
    pub version_number: u64,
    pub sequence_number: u64,
    pub status: String,
    pub source: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishWorkflowRequest {
    pub environment_id: Uuid,
    pub workflow_version_id: Uuid,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RollbackWorkflowRequest {
    pub target_workflow_version_id: Uuid,
}

#[utoipa::path(get, path = "/api/v1/workflows")]
pub async fn list_workflows(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<WorkflowListQuery>,
) -> AppResult<Json<WorkflowPage>> {
    actor.require("workflow:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let visible = "(w.owner_user_id=? OR w.visibility='company' OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=?) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id)))";
    let rows = if actor.company_admin {
        sqlx::query("SELECT w.id,w.name,w.description,w.status,w.visibility,w.owner_user_id,w.owner_department_id,(SELECT id FROM workflow_service_identities si WHERE si.workflow_id=w.id) service_identity_id,u.display_name owner_name,w.version,w.updated_at,wd.revision,(SELECT MAX(version_number) FROM workflow_versions v WHERE v.workflow_id=w.id) latest_version FROM workflows w JOIN users u ON u.id=w.owner_user_id JOIN workflow_drafts wd ON wd.workflow_id=w.id WHERE w.tenant_id=? AND (?='' OR w.status=?) AND (?='%%' OR w.name LIKE ?) ORDER BY w.updated_at DESC LIMIT ? OFFSET ?")
            .bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(offset).fetch_all(&state.pool).await?
    } else {
        let sql = format!(
            "SELECT w.id,w.name,w.description,w.status,w.visibility,w.owner_user_id,w.owner_department_id,(SELECT id FROM workflow_service_identities si WHERE si.workflow_id=w.id) service_identity_id,u.display_name owner_name,w.version,w.updated_at,wd.revision,(SELECT MAX(version_number) FROM workflow_versions v WHERE v.workflow_id=w.id) latest_version FROM workflows w JOIN users u ON u.id=w.owner_user_id JOIN workflow_drafts wd ON wd.workflow_id=w.id WHERE w.tenant_id=? AND {visible} AND (?='' OR w.status=?) AND (?='%%' OR w.name LIKE ?) ORDER BY w.updated_at DESC LIMIT ? OFFSET ?"
        );
        sqlx::query(&sql)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&state.pool)
            .await?
    };
    let total: i64 = if actor.company_admin {
        sqlx::query_scalar("SELECT COUNT(*) FROM workflows w WHERE w.tenant_id=? AND (?='' OR w.status=?) AND (?='%%' OR w.name LIKE ?)").bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).fetch_one(&state.pool).await?
    } else {
        let sql = format!(
            "SELECT COUNT(*) FROM workflows w WHERE w.tenant_id=? AND {visible} AND (?='' OR w.status=?) AND (?='%%' OR w.name LIKE ?)"
        );
        sqlx::query_scalar(&sql)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(&status)
            .bind(&status)
            .bind(&search)
            .bind(&search)
            .fetch_one(&state.pool)
            .await?
    };
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(workflow_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size,
        total: total as u64,
    }))
}

#[utoipa::path(post, path = "/api/v1/workflows", request_body = CreateWorkflowRequest)]
pub async fn create_workflow(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateWorkflowRequest>,
) -> AppResult<(StatusCode, Json<WorkflowResponse>)> {
    actor.require("workflow:create")?;
    let name = validate_name(&input.name, 160)?;
    validate_visibility(&input.visibility)?;
    let workflow_id = Uuid::now_v7();
    let identity_id = Uuid::now_v7();
    let draft_id = Uuid::now_v7();
    let definition =
        serde_json::to_value(WorkflowDefinition::empty()).map_err(AppError::internal)?;
    let hash = canonical_content_hash(&definition).map_err(AppError::internal)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,?,?,?,?)")
        .bind(workflow_id).bind(actor.tenant_id).bind(&name).bind(&input.description).bind(&input.visibility).bind(actor.user_id).bind(actor.department_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)")
        .bind(identity_id)
        .bind(actor.tenant_id)
        .bind(workflow_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workflow_members(tenant_id,workflow_id,user_id,member_role,created_by) VALUES(?,?,?,'manager',?)").bind(actor.tenant_id).bind(workflow_id).bind(actor.user_id).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,revision,definition_json,content_hash,updated_by) VALUES(?,?,?,'2.0',0,?,?,?)").bind(draft_id).bind(actor.tenant_id).bind(workflow_id).bind(&definition).bind(&hash).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "workflow.created",
        "workflow",
        workflow_id,
        json!({"name": name}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "WorkflowCreated",
        "workflow",
        workflow_id,
        json!({"workflowId": workflow_id, "serviceIdentityId": identity_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_workflow(&state, &actor, workflow_id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}", params(("id" = Uuid, Path)))]
pub async fn get_workflow(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<WorkflowResponse>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    Ok(Json(load_workflow(&state, &actor, id).await?))
}

#[utoipa::path(patch, path = "/api/v1/workflows/{id}", request_body = UpdateWorkflowRequest, params(("id" = Uuid, Path)))]
pub async fn update_workflow(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateWorkflowRequest>,
) -> AppResult<Json<WorkflowResponse>> {
    actor.require("workflow:edit")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let name = validate_name(&input.name, 160)?;
    validate_visibility(&input.visibility)?;
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE workflows SET name=?,description=?,visibility=?,version=version+1 WHERE id=? AND tenant_id=? AND version=? AND status='active'")
        .bind(&name).bind(&input.description).bind(&input.visibility).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "WORKFLOW_VERSION_CONFLICT",
            "Workflow changed or is archived",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "workflow.updated",
        "workflow",
        id,
        json!({"version": input.version+1}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_workflow(&state, &actor, id).await?))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/archive", params(("id" = Uuid, Path)))]
pub async fn archive_workflow(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("workflow:archive")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE workflows SET status='archived',archived_at=CURRENT_TIMESTAMP(6),version=version+1 WHERE id=? AND tenant_id=? AND status='active'").bind(id).bind(actor.tenant_id).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "WORKFLOW_NOT_ACTIVE",
            "Workflow is already archived",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "workflow.archived",
        "workflow",
        id,
        json!({}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "WorkflowArchived",
        "workflow",
        id,
        json!({"workflowId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/draft", params(("id" = Uuid, Path)))]
pub async fn get_draft(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<DraftResponse>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    Ok(Json(load_draft(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(put, path = "/api/v1/workflows/{id}/draft", request_body = SaveDraftRequest, params(("id" = Uuid, Path)))]
pub async fn save_draft(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<SaveDraftRequest>,
) -> AppResult<Json<DraftResponse>> {
    actor.require("workflow:edit")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let definition: WorkflowDefinition =
        serde_json::from_value(input.definition.clone()).map_err(|error| {
            AppError::unprocessable("INVALID_WORKFLOW_DEFINITION", error.to_string())
        })?;
    let issues = validate_definition(&definition);
    if !issues.is_empty() {
        return Err(definition_error(issues));
    }
    let definition = serde_json::to_value(&definition).map_err(AppError::internal)?;
    let hash = canonical_content_hash(&definition).map_err(AppError::internal)?;
    let key = idempotency_key(&headers)?;
    let operation = format!("workflow.draft:{id}");
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
        return Ok(Json(
            serde_json::from_value(response).map_err(AppError::internal)?,
        ));
    }
    let active: Option<String> =
        sqlx::query_scalar("SELECT status FROM workflows WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_optional(&mut *tx)
            .await?;
    if active.as_deref() != Some("active") {
        return Err(AppError::conflict(
            "WORKFLOW_NOT_ACTIVE",
            "Archived workflows cannot be edited",
        ));
    }
    let row=sqlx::query("SELECT id,revision,content_hash FROM workflow_drafts WHERE workflow_id=? AND tenant_id=? FOR UPDATE").bind(id).bind(actor.tenant_id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::not_found("Workflow draft"))?;
    let draft_id: Uuid = row.try_get("id")?;
    let current: u64 = row.try_get("revision")?;
    let current_hash: String = row.try_get("content_hash")?;
    if current != input.expected_revision {
        return Err(AppError::conflict(
            "DRAFT_REVISION_CONFLICT",
            format!("Draft is now at revision {current}"),
        )
        .with_field("expectedRevision", "CURRENT_REVISION", current.to_string()));
    }
    if current_hash == hash {
        let response = load_draft(&state, actor.tenant_id, id).await?;
        complete_idempotency(
            &mut tx,
            actor.tenant_id,
            &operation,
            key.as_deref(),
            draft_id,
            &response,
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(response));
    }
    let next = current + 1;
    let revision_id = Uuid::now_v7();
    sqlx::query("UPDATE workflow_drafts SET revision=?,schema_version='2.0',definition_json=?,content_hash=?,updated_by=? WHERE id=?").bind(next).bind(&definition).bind(&hash).bind(actor.user_id).bind(draft_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_draft_revisions(id,tenant_id,workflow_id,draft_id,revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,?,?,'2.0',?,?,?)").bind(revision_id).bind(actor.tenant_id).bind(id).bind(draft_id).bind(next).bind(&definition).bind(&hash).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "workflow.draft_revised",
        "workflow",
        id,
        json!({"revision":next,"contentHash":&hash}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "DraftRevised",
        "workflow",
        id,
        json!({"workflowId":id,"revision":next}),
    )
    .await?;
    let stored = sqlx::query("SELECT id,workflow_id,schema_version,revision,definition_json,content_hash,updated_at FROM workflow_drafts WHERE id=?")
        .bind(draft_id)
        .fetch_one(&mut *tx)
        .await?;
    let response = DraftResponse {
        id: stored.try_get("id")?,
        workflow_id: stored.try_get("workflow_id")?,
        schema_version: stored.try_get("schema_version")?,
        revision: stored.try_get("revision")?,
        definition: stored.try_get("definition_json")?,
        content_hash: stored.try_get("content_hash")?,
        updated_at: stored.try_get("updated_at")?,
    };
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation,
        key.as_deref(),
        revision_id,
        &response,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(response))
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/revisions", params(("id" = Uuid, Path)))]
pub async fn list_revisions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<RevisionResponse>>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let rows=sqlx::query("SELECT id,revision,schema_version,content_hash,created_by,created_at FROM workflow_draft_revisions WHERE tenant_id=? AND workflow_id=? ORDER BY revision DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                Ok(RevisionResponse {
                    id: r.try_get("id")?,
                    revision: r.try_get("revision")?,
                    schema_version: r.try_get("schema_version")?,
                    content_hash: r.try_get("content_hash")?,
                    created_by: r.try_get("created_by")?,
                    created_at: r.try_get("created_at")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?,
    ))
}

#[utoipa::path(operation_id = "create_workflow_version", post, path = "/api/v1/workflows/{id}/versions", request_body = CreateVersionRequest, params(("id" = Uuid, Path)))]
pub async fn create_version(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<CreateVersionRequest>,
) -> AppResult<(StatusCode, Json<WorkflowVersionResponse>)> {
    actor.require("workflow:publish")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let key = idempotency_key(&headers)?;
    let operation = format!("workflow.version:{id}");
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
    let active: Option<String> =
        sqlx::query_scalar("SELECT status FROM workflows WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_optional(&mut *tx)
            .await?;
    if active.as_deref() != Some("active") {
        return Err(AppError::conflict(
            "WORKFLOW_NOT_ACTIVE",
            "Archived workflows cannot create versions",
        ));
    }
    let draft = load_draft(&state, actor.tenant_id, id).await?;
    if draft.revision != input.draft_revision {
        return Err(AppError::conflict(
            "DRAFT_REVISION_CONFLICT",
            format!("Draft is now at revision {}", draft.revision),
        ));
    }
    let definition: WorkflowDefinition =
        serde_json::from_value(draft.definition.clone()).map_err(AppError::internal)?;
    let snapshots = grants::validate_and_snapshot(&state, &actor, id, &definition).await?;
    if let Some(row)=sqlx::query("SELECT id,workflow_id,version_number,source_revision,schema_version,content_hash,definition_json,created_by,created_at FROM workflow_versions WHERE tenant_id=? AND workflow_id=? AND source_revision=? AND content_hash=?").bind(actor.tenant_id).bind(id).bind(draft.revision).bind(&draft.content_hash).fetch_optional(&mut *tx).await? {
        let response = version_from_row(row)?;
        complete_idempotency(&mut tx,actor.tenant_id,&operation,key.as_deref(),response.id,&response).await?;
        tx.commit().await?;
        return Ok((StatusCode::OK,Json(response)));
    }
    let version_number:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(version_number),0)+1 AS UNSIGNED) FROM workflow_versions WHERE tenant_id=? AND workflow_id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    let version_id = Uuid::now_v7();
    let registry = NodeRegistry::m4_defaults();
    let compiled = match WorkflowCompiler::new(&registry).compile(
        &definition,
        &CompileContext {
            current_workflow_version_id: Some(version_id.to_string()),
            ancestor_workflow_version_ids: Default::default(),
        },
    ) {
        Ok(compiled) => Some(compiled),
        Err(error)
            if error
                .issues
                .iter()
                .all(|issue| issue.code == "NODE_UNSUPPORTED_IN_M4") =>
        {
            None
        }
        Err(error) => {
            return Err(AppError::unprocessable(
                "WORKFLOW_COMPILE_FAILED",
                serde_json::to_string(&error.issues).unwrap_or_else(|_| error.to_string()),
            ));
        }
    };
    let compiled_json = compiled
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(AppError::internal)?;
    let compiled_hash = compiled.as_ref().map(|value| value.canonical_hash.as_str());
    let compiler_version = compiled
        .as_ref()
        .map(|value| value.compiler_version.as_str());
    let compiled_at = compiled.as_ref().map(|_| OffsetDateTime::now_utc());
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,compiled_ir_json,compiled_ir_hash,compiler_version,compiled_at,created_by) VALUES(?,?,?,?,?,'2.0',?,?,?,?,?,?,?)").bind(version_id).bind(actor.tenant_id).bind(id).bind(version_number).bind(draft.revision).bind(&draft.definition).bind(&draft.content_hash).bind(compiled_json).bind(compiled_hash).bind(compiler_version).bind(compiled_at).bind(actor.user_id).execute(&mut *tx).await?;
    for snapshot in snapshots {
        sqlx::query("INSERT INTO workflow_version_resources(id,tenant_id,workflow_version_id,node_id,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(version_id).bind(snapshot.node_id).bind(snapshot.reference.resource_type.as_str()).bind(snapshot.reference.resource_id).bind(snapshot.reference.resource_version_id).bind(snapshot.reference.operation.as_str()).bind(snapshot.snapshot).bind(snapshot.snapshot_hash).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        &actor,
        "workflow.version_created",
        "workflow",
        id,
        json!({"workflowVersionId":version_id,"versionNumber":version_number}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "WorkflowVersionCreated",
        "workflow",
        id,
        json!({"workflowId":id,"workflowVersionId":version_id}),
    )
    .await?;
    let row=sqlx::query("SELECT id,workflow_id,version_number,source_revision,schema_version,content_hash,definition_json,created_by,created_at FROM workflow_versions WHERE id=?").bind(version_id).fetch_one(&mut *tx).await?;
    let response = version_from_row(row)?;
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation,
        key.as_deref(),
        version_id,
        &response,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(operation_id = "list_workflow_versions", get, path = "/api/v1/workflows/{id}/versions", params(("id" = Uuid, Path)))]
pub async fn list_versions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<WorkflowVersionResponse>>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let rows=sqlx::query("SELECT id,workflow_id,version_number,source_revision,schema_version,content_hash,definition_json,created_by,created_at FROM workflow_versions WHERE tenant_id=? AND workflow_id=? ORDER BY version_number DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(version_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/members", params(("id" = Uuid, Path)))]
pub async fn list_members(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<WorkflowMemberResponse>>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let rows=sqlx::query("SELECT wm.user_id,u.username,u.display_name,wm.member_role FROM workflow_members wm JOIN users u ON u.id=wm.user_id WHERE wm.tenant_id=? AND wm.workflow_id=? ORDER BY u.display_name").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                Ok(WorkflowMemberResponse {
                    user_id: r.try_get("user_id")?,
                    username: r.try_get("username")?,
                    display_name: r.try_get("display_name")?,
                    member_role: r.try_get("member_role")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/members", request_body = UpsertWorkflowMemberRequest, params(("id" = Uuid, Path)))]
pub async fn upsert_member(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpsertWorkflowMemberRequest>,
) -> AppResult<StatusCode> {
    actor.require("workflow:manage_member")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    if !matches!(input.member_role.as_str(), "viewer" | "editor" | "manager") {
        return Err(AppError::bad_request(
            "INVALID_MEMBER_ROLE",
            "Member role is invalid",
        ));
    }
    let department: Option<Uuid> = sqlx::query_scalar(
        "SELECT ud.department_id FROM users u JOIN user_departments ud ON ud.user_id=u.id AND ud.tenant_id=u.tenant_id WHERE u.id=? AND u.tenant_id=? AND u.status<>'disabled'",
    )
    .bind(input.user_id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?;
    crate::control_common::require_department_scope(
        &state.pool,
        &actor,
        department.ok_or_else(|| AppError::not_found("User"))?,
    )
    .await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO workflow_members(tenant_id,workflow_id,user_id,member_role,created_by) VALUES(?,?,?,?,?) ON DUPLICATE KEY UPDATE member_role=VALUES(member_role)").bind(actor.tenant_id).bind(id).bind(input.user_id).bind(&input.member_role).bind(actor.user_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "workflow.member_upserted",
        "workflow",
        id,
        json!({"userId":input.user_id,"role":input.member_role}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/workflows/{id}/members/{user_id}", params(("id" = Uuid, Path), ("user_id" = Uuid, Path)))]
pub async fn delete_member(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    actor.require("workflow:manage_member")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let owner: Uuid =
        sqlx::query_scalar("SELECT owner_user_id FROM workflows WHERE id=? AND tenant_id=?")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_one(&state.pool)
            .await?;
    if owner == user_id {
        return Err(AppError::conflict(
            "OWNER_MEMBER_REQUIRED",
            "Workflow owner cannot be removed",
        ));
    }
    let department: Option<Uuid> = sqlx::query_scalar("SELECT ud.department_id FROM users u JOIN user_departments ud ON ud.user_id=u.id AND ud.tenant_id=u.tenant_id WHERE u.id=? AND u.tenant_id=?")
        .bind(user_id).bind(actor.tenant_id).fetch_optional(&state.pool).await?;
    crate::control_common::require_department_scope(
        &state.pool,
        &actor,
        department.ok_or_else(|| AppError::not_found("User"))?,
    )
    .await?;
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query(
        "DELETE FROM workflow_members WHERE tenant_id=? AND workflow_id=? AND user_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::not_found("Workflow member"));
    }
    audit(
        &mut tx,
        &actor,
        "workflow.member_removed",
        "workflow",
        id,
        json!({"userId":user_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/environments")]
pub async fn list_environments(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<EnvironmentResponse>>> {
    actor.require("workflow:view")?;
    let rows=sqlx::query("SELECT id,code,name,is_builtin,status,version FROM workflow_environments WHERE tenant_id=? ORDER BY is_builtin DESC,name").bind(actor.tenant_id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(environment_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/environments", request_body = CreateEnvironmentRequest)]
pub async fn create_environment(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateEnvironmentRequest>,
) -> AppResult<(StatusCode, Json<EnvironmentResponse>)> {
    actor.require("workflow:publish")?;
    let name = validate_name(&input.name, 100)?;
    let code = input.code.trim().to_ascii_lowercase().replace(' ', "-");
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || v == b'-')
    {
        return Err(AppError::bad_request(
            "INVALID_ENVIRONMENT_CODE",
            "Environment code is invalid",
        ));
    }
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO workflow_environments(id,tenant_id,code,name) VALUES(?,?,?,?)")
        .bind(id)
        .bind(actor.tenant_id)
        .bind(&code)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        "workflow.environment_created",
        "environment",
        id,
        json!({"code":code}),
    )
    .await?;
    let row=sqlx::query("SELECT id,code,name,is_builtin,status,version FROM workflow_environments WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(environment_from_row(row)?)))
}

#[utoipa::path(patch, path = "/api/v1/environments/{id}", request_body = UpdateEnvironmentRequest, params(("id" = Uuid, Path)))]
pub async fn update_environment(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateEnvironmentRequest>,
) -> AppResult<Json<EnvironmentResponse>> {
    actor.require("workflow:publish")?;
    let name = validate_name(&input.name, 100)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_ENVIRONMENT_STATUS",
            "Environment status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE workflow_environments SET name=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(name).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "ENVIRONMENT_VERSION_CONFLICT",
            "Environment changed",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "workflow.environment_updated",
        "environment",
        id,
        json!({"status":input.status}),
    )
    .await?;
    let row=sqlx::query("SELECT id,code,name,is_builtin,status,version FROM workflow_environments WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    let response = environment_from_row(row)?;
    tx.commit().await?;
    Ok(Json(response))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/run", params(("id" = Uuid, Path)))]
pub async fn runtime_unavailable(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    Err(AppError::service_unavailable(
        "RUNTIME_UNAVAILABLE",
        "Workflow runtime is not available in M2",
    ))
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/deployments", params(("id" = Uuid, Path)))]
pub async fn list_deployments(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<DeploymentResponse>>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let rows=sqlx::query("SELECT d.id,d.workflow_id,d.environment_id,e.name environment_name,d.workflow_version_id,v.version_number,d.sequence_number,d.status,d.source,d.created_at FROM workflow_deployments d JOIN workflow_environments e ON e.id=d.environment_id JOIN workflow_versions v ON v.id=d.workflow_version_id WHERE d.tenant_id=? AND d.workflow_id=? ORDER BY d.created_at DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(deployment_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

struct DeploymentCommand<'a> {
    workflow_id: Uuid,
    environment_id: Uuid,
    version_id: Uuid,
    source: &'a str,
    idempotency_key: Option<&'a str>,
    request: &'a Value,
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/deployments", request_body = PublishWorkflowRequest, params(("id" = Uuid, Path)))]
pub async fn publish(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<PublishWorkflowRequest>,
) -> AppResult<(StatusCode, Json<DeploymentResponse>)> {
    actor.require("workflow:publish")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let key = idempotency_key(&headers)?;
    let request =
        json!({"environmentId":input.environment_id,"workflowVersionId":input.workflow_version_id});
    deploy(
        &state,
        &actor,
        DeploymentCommand {
            workflow_id: id,
            environment_id: input.environment_id,
            version_id: input.workflow_version_id,
            source: "publish",
            idempotency_key: key.as_deref(),
            request: &request,
        },
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/deployments/{environment_id}/rollback", request_body = RollbackWorkflowRequest, params(("id" = Uuid, Path), ("environment_id" = Uuid, Path)))]
pub async fn rollback(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, environment_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<RollbackWorkflowRequest>,
) -> AppResult<(StatusCode, Json<DeploymentResponse>)> {
    actor.require("workflow:publish")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let key = idempotency_key(&headers)?;
    let request = json!({"environmentId":environment_id,"targetWorkflowVersionId":input.target_workflow_version_id});
    deploy(
        &state,
        &actor,
        DeploymentCommand {
            workflow_id: id,
            environment_id,
            version_id: input.target_workflow_version_id,
            source: "rollback",
            idempotency_key: key.as_deref(),
            request: &request,
        },
    )
    .await
}

async fn deploy(
    state: &AppState,
    actor: &AuthActor,
    command: DeploymentCommand<'_>,
) -> AppResult<(StatusCode, Json<DeploymentResponse>)> {
    let DeploymentCommand {
        workflow_id,
        environment_id,
        version_id,
        source,
        idempotency_key,
        request,
    } = command;
    let operation = format!("workflow.{source}:{workflow_id}");
    let mut tx = state.pool.begin().await?;
    if let IdempotencyReservation::Replay { response, .. } = reserve_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation,
        idempotency_key,
        request,
    )
    .await?
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
    let workflow_status: Option<String> =
        sqlx::query_scalar("SELECT status FROM workflows WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(workflow_id)
            .bind(actor.tenant_id)
            .fetch_optional(&mut *tx)
            .await?;
    if workflow_status.as_deref() != Some("active") {
        return Err(AppError::conflict(
            "WORKFLOW_NOT_ACTIVE",
            "Workflow is archived",
        ));
    }
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_versions v JOIN workflow_environments e ON e.id=? AND e.tenant_id=v.tenant_id AND e.status='active' WHERE v.id=? AND v.workflow_id=? AND v.tenant_id=?)").bind(environment_id).bind(version_id).bind(workflow_id).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(AppError::bad_request(
            "INVALID_DEPLOYMENT_TARGET",
            "Environment or workflow version is invalid",
        ));
    }
    let missing = grants::validate_version_grants(state, actor, version_id).await?;
    if !missing.is_empty() {
        if missing
            .iter()
            .any(|issue| issue.reason == "resource_missing_or_disabled")
        {
            return Err(AppError::unprocessable(
                "RESOURCE_UNAVAILABLE",
                "Workflow version references a missing or disabled resource",
            ));
        }
        return Err(AppError::unprocessable(
            "RESOURCE_GRANT_MISSING",
            "Workflow version no longer has all required grants",
        ));
    }
    let head=sqlx::query("SELECT active_deployment_id,version FROM workflow_deployment_heads WHERE tenant_id=? AND workflow_id=? AND environment_id=? FOR UPDATE").bind(actor.tenant_id).bind(workflow_id).bind(environment_id).fetch_optional(&mut *tx).await?;
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM workflow_deployments WHERE tenant_id=? AND workflow_id=? AND environment_id=?").bind(actor.tenant_id).bind(workflow_id).bind(environment_id).fetch_one(&mut *tx).await?;
    if let Some(ref row) = head {
        let previous: Uuid = row.try_get("active_deployment_id")?;
        sqlx::query("UPDATE workflow_deployments SET status=? WHERE id=?")
            .bind(if source == "rollback" {
                "rolled_back"
            } else {
                "superseded"
            })
            .bind(previous)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO deployment_history(id,tenant_id,workflow_id,environment_id,deployment_id,action,actor_user_id) VALUES(?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(workflow_id).bind(environment_id).bind(previous).bind(if source=="rollback"{"rolled_back"}else{"superseded"}).bind(actor.user_id).execute(&mut *tx).await?;
    }
    let deployment_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workflow_deployments(id,tenant_id,workflow_id,environment_id,workflow_version_id,sequence_number,source,created_by) VALUES(?,?,?,?,?,?,?,?)").bind(deployment_id).bind(actor.tenant_id).bind(workflow_id).bind(environment_id).bind(version_id).bind(sequence).bind(source).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_deployment_heads(tenant_id,workflow_id,environment_id,active_deployment_id) VALUES(?,?,?,?) ON DUPLICATE KEY UPDATE active_deployment_id=VALUES(active_deployment_id),version=version+1").bind(actor.tenant_id).bind(workflow_id).bind(environment_id).bind(deployment_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO deployment_history(id,tenant_id,workflow_id,environment_id,deployment_id,action,actor_user_id) VALUES(?,?,?,?,?,'published',?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(workflow_id).bind(environment_id).bind(deployment_id).bind(actor.user_id).execute(&mut *tx).await?;
    audit(&mut tx,actor,if source=="rollback"{"workflow.rolled_back"}else{"workflow.published"},"workflow",workflow_id,json!({"deploymentId":deployment_id,"workflowVersionId":version_id,"environmentId":environment_id})).await?;
    outbox(
        &mut tx,
        actor,
        if source == "rollback" {
            "WorkflowRolledBack"
        } else {
            "WorkflowPublished"
        },
        "workflow",
        workflow_id,
        json!({"deploymentId":deployment_id,"workflowVersionId":version_id}),
    )
    .await?;
    let row=sqlx::query("SELECT d.id,d.workflow_id,d.environment_id,e.name environment_name,d.workflow_version_id,v.version_number,d.sequence_number,d.status,d.source,d.created_at FROM workflow_deployments d JOIN workflow_environments e ON e.id=d.environment_id JOIN workflow_versions v ON v.id=d.workflow_version_id WHERE d.id=?").bind(deployment_id).fetch_one(&mut *tx).await?;
    let response = deployment_from_row(row)?;
    complete_idempotency(
        &mut tx,
        actor.tenant_id,
        &operation,
        idempotency_key,
        deployment_id,
        &response,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn load_workflow(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<WorkflowResponse> {
    let row=sqlx::query("SELECT w.id,w.name,w.description,w.status,w.visibility,w.owner_user_id,w.owner_department_id,(SELECT id FROM workflow_service_identities si WHERE si.workflow_id=w.id) service_identity_id,u.display_name owner_name,w.version,w.updated_at,wd.revision,(SELECT MAX(version_number) FROM workflow_versions v WHERE v.workflow_id=w.id) latest_version FROM workflows w JOIN users u ON u.id=w.owner_user_id JOIN workflow_drafts wd ON wd.workflow_id=w.id WHERE w.id=? AND w.tenant_id=?").bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Workflow"))?;
    workflow_from_row(row).map_err(Into::into)
}
async fn load_draft(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<DraftResponse> {
    let r=sqlx::query("SELECT id,workflow_id,schema_version,revision,definition_json,content_hash,updated_at FROM workflow_drafts WHERE tenant_id=? AND workflow_id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Workflow draft"))?;
    Ok(DraftResponse {
        id: r.try_get("id")?,
        workflow_id: r.try_get("workflow_id")?,
        schema_version: r.try_get("schema_version")?,
        revision: r.try_get("revision")?,
        definition: r.try_get("definition_json")?,
        content_hash: r.try_get("content_hash")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn workflow_from_row(r: sqlx::mysql::MySqlRow) -> Result<WorkflowResponse, sqlx::Error> {
    Ok(WorkflowResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        status: r.try_get("status")?,
        visibility: r.try_get("visibility")?,
        owner_user_id: r.try_get("owner_user_id")?,
        owner_department_id: r.try_get("owner_department_id")?,
        service_identity_id: r.try_get("service_identity_id")?,
        owner_name: r.try_get("owner_name")?,
        draft_revision: r.try_get("revision")?,
        latest_version: r.try_get("latest_version")?,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn version_from_row(r: sqlx::mysql::MySqlRow) -> Result<WorkflowVersionResponse, sqlx::Error> {
    Ok(WorkflowVersionResponse {
        id: r.try_get("id")?,
        workflow_id: r.try_get("workflow_id")?,
        version_number: r.try_get("version_number")?,
        source_revision: r.try_get("source_revision")?,
        schema_version: r.try_get("schema_version")?,
        content_hash: r.try_get("content_hash")?,
        definition: r.try_get("definition_json")?,
        created_by: r.try_get("created_by")?,
        created_at: r.try_get("created_at")?,
    })
}
fn environment_from_row(r: sqlx::mysql::MySqlRow) -> Result<EnvironmentResponse, sqlx::Error> {
    Ok(EnvironmentResponse {
        id: r.try_get("id")?,
        code: r.try_get("code")?,
        name: r.try_get("name")?,
        is_builtin: r.try_get("is_builtin")?,
        status: r.try_get("status")?,
        version: r.try_get("version")?,
    })
}
fn deployment_from_row(r: sqlx::mysql::MySqlRow) -> Result<DeploymentResponse, sqlx::Error> {
    Ok(DeploymentResponse {
        id: r.try_get("id")?,
        workflow_id: r.try_get("workflow_id")?,
        environment_id: r.try_get("environment_id")?,
        environment_name: r.try_get("environment_name")?,
        workflow_version_id: r.try_get("workflow_version_id")?,
        version_number: r.try_get("version_number")?,
        sequence_number: r.try_get("sequence_number")?,
        status: r.try_get("status")?,
        source: r.try_get("source")?,
        created_at: r.try_get("created_at")?,
    })
}
fn validate_visibility(value: &str) -> AppResult<()> {
    if matches!(value, "private" | "department" | "company") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_VISIBILITY",
            "Visibility must be private, department or company",
        ))
    }
}
fn definition_error(issues: Vec<agentx_domain::DefinitionIssue>) -> AppError {
    let mut error = AppError::unprocessable(
        "INVALID_WORKFLOW_DEFINITION",
        "Workflow definition is invalid",
    );
    error.fields = issues
        .into_iter()
        .map(|issue| FieldError {
            field: issue.path,
            code: issue.code,
            message: issue.message,
        })
        .collect();
    error
}
