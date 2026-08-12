use agentx_api_types::PageResponse;
use agentx_application::{
    CancelExecutionCommandPayload, RuntimeCommand, RuntimeCommandType, StartExecutionCommandPayload,
};
use agentx_domain::TenantId;
use agentx_infrastructure::runtime_commands::RuntimeCommandRepository;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::{BTreeMap, HashMap};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, require_department_scope, require_workflow_access, validate_name},
    error::{AppError, AppResult, UniqueConstraint, map_unique},
    security::AuthActor,
    state::AppState,
};

pub(crate) const DATASET_CASE_KEY: UniqueConstraint = UniqueConstraint {
    index: "uq_dataset_case_key",
    code: "DATASET_CASE_KEY_EXISTS",
    field: "caseKey",
    message: "A Test Case with this key already exists in the Dataset",
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
pub struct DatasetResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub status: String,
    pub owner_department_id: Uuid,
    pub revision: u64,
    pub case_count: u64,
    pub latest_version: Option<u64>,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type DatasetPage = PageResponse<DatasetResponse>;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatasetRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDatasetRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub status: String,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseResponse {
    pub id: Uuid,
    pub case_key: String,
    pub name: String,
    pub input: Value,
    pub expected_output: Option<Value>,
    pub context: Option<Value>,
    pub tags: Vec<String>,
    pub evaluator_override: Option<Value>,
    pub sort_order: u64,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaseInput {
    pub case_key: String,
    pub name: String,
    pub input: Value,
    pub expected_output: Option<Value>,
    pub context: Option<Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub evaluator_override: Option<Value>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCaseRequest {
    pub expected_revision: u64,
    #[serde(flatten)]
    pub case: CaseInput,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCaseRequest {
    pub expected_revision: u64,
    pub version: u64,
    #[serde(flatten)]
    pub case: CaseInput,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCaseRequest {
    pub expected_revision: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportCasesRequest {
    pub expected_revision: u64,
    pub format: String,
    pub content: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetVersionResponse {
    pub id: Uuid,
    pub dataset_id: Uuid,
    pub version_number: u64,
    pub source_revision: u64,
    pub content_hash: String,
    pub case_count: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRuleInput {
    pub key: String,
    pub name: String,
    pub evaluator_type: String,
    pub configuration: Value,
    #[serde(default = "default_weight")]
    pub weight: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRuleResponse {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub evaluator_type: String,
    pub configuration: Value,
    pub weight: String,
    pub required: bool,
    pub sort_order: u32,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationProfileResponse {
    pub id: Uuid,
    pub version_id: Uuid,
    pub version_number: u64,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub owner_department_id: Uuid,
    pub status: String,
    pub aggregation: String,
    pub pass_threshold: String,
    pub rules: Vec<EvaluationRuleResponse>,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEvaluationProfileRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    #[serde(default = "default_aggregation")]
    pub aggregation: String,
    #[serde(default = "default_pass_threshold")]
    pub pass_threshold: String,
    pub rules: Vec<EvaluationRuleInput>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRunResponse {
    pub id: Uuid,
    pub name: String,
    pub workflow_version_id: Uuid,
    pub workflow_name: String,
    pub dataset_version_id: Uuid,
    pub dataset_name: String,
    pub evaluation_profile_version_id: Uuid,
    pub visibility: String,
    pub owner_department_id: Uuid,
    pub status: String,
    pub result_count: u64,
    pub parameters: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEvaluationRunRequest {
    pub name: String,
    pub workflow_version_id: Uuid,
    pub dataset_version_id: Uuid,
    pub evaluation_profile_version_id: Uuid,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub parameters: Value,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationReportResponse {
    pub run: EvaluationRunResponse,
    pub results: Vec<EvaluationCaseResultResponse>,
    pub metrics: Vec<Value>,
    pub report_status: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationCaseResultResponse {
    pub case_id: Uuid,
    pub source_case_id: Uuid,
    pub case_key: String,
    pub status: String,
    pub score: Option<f64>,
    pub detail: Option<Value>,
    pub target_execution_id: Option<Uuid>,
    pub duration_ms: Option<u64>,
    pub cost_micros: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub rule_results: Vec<EvaluationRuleResultResponse>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRuleResultResponse {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub evaluator_type: String,
    pub status: String,
    pub passed: Option<bool>,
    pub score: Option<f64>,
    pub detail: Value,
    pub evaluator_execution_id: Option<Uuid>,
    pub duration_ms: Option<u64>,
    pub cost_micros: u64,
}

#[utoipa::path(get, path = "/api/v1/datasets")]
pub async fn list_datasets(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<DatasetPage>> {
    actor.require("dataset:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let rows = if actor.company_admin {
        sqlx::query(DATASET_LIST)
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
        sqlx::query(DATASET_VISIBLE)
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
        sqlx::query_scalar("SELECT COUNT(*) FROM datasets WHERE tenant_id=? AND (?='' OR status=?) AND (?='%%' OR name LIKE ?)").bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).fetch_one(&state.pool).await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM datasets d WHERE d.tenant_id=? AND (d.visibility='company' OR d.owner_user_id=? OR (d.visibility='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=d.tenant_id AND ((dc.ancestor_id=d.owner_department_id AND dc.descendant_id=?) OR (dc.ancestor_id=? AND dc.descendant_id=d.owner_department_id)))) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=d.tenant_id AND ur.user_id=? AND dc.descendant_id=d.owner_department_id)) AND (?='' OR d.status=?) AND (?='%%' OR d.name LIKE ?)").bind(actor.tenant_id).bind(actor.user_id).bind(actor.department_id).bind(actor.department_id).bind(actor.user_id).bind(&status).bind(&status).bind(&search).bind(&search).fetch_one(&state.pool).await?
    };
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(dataset_from_row)
            .collect::<AppResult<_>>()?,
        page,
        page_size,
        total: total as u64,
    }))
}

#[utoipa::path(post,path="/api/v1/datasets",request_body=CreateDatasetRequest)]
pub async fn create_dataset(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateDatasetRequest>,
) -> AppResult<(StatusCode, Json<DatasetResponse>)> {
    actor.require("dataset:manage")?;
    validate_visibility(&input.visibility)?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO datasets(id,tenant_id,name,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(validate_name(&input.name,160)?).bind(&input.description).bind(&input.visibility).bind(actor.user_id).bind(actor.department_id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "dataset.created",
        "dataset",
        id,
        json!({"visibility":input.visibility}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_dataset(&state, &actor, id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/datasets/{id}")]
pub async fn get_dataset(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<DatasetResponse>> {
    actor.require("dataset:view")?;
    Ok(Json(load_dataset(&state, &actor, id).await?))
}

#[utoipa::path(patch,path="/api/v1/datasets/{id}",request_body=UpdateDatasetRequest)]
pub async fn update_dataset(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateDatasetRequest>,
) -> AppResult<Json<DatasetResponse>> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    validate_visibility(&input.visibility)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_DATASET_STATUS",
            "Dataset status is invalid",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let changed=sqlx::query("UPDATE datasets SET name=?,description=?,visibility=?,status=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(validate_name(&input.name,160)?).bind(&input.description).bind(&input.visibility).bind(&input.status).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "DATASET_VERSION_CONFLICT",
            "Dataset changed on the server",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "dataset.updated",
        "dataset",
        id,
        json!({"visibility":input.visibility,"status":input.status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_dataset(&state, &actor, id).await?))
}

#[utoipa::path(get, path = "/api/v1/datasets/{id}/cases")]
pub async fn list_cases(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<TestCaseResponse>>> {
    actor.require("dataset:view")?;
    require_dataset_access(&state, &actor, id, false).await?;
    let rows = sqlx::query(CASE_SELECT)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(case_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/datasets/{id}/cases",request_body=CreateCaseRequest)]
pub async fn create_case(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateCaseRequest>,
) -> AppResult<(StatusCode, Json<TestCaseResponse>)> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    validate_case(&input.case)?;
    ensure_case_key_available(
        &state,
        actor.tenant_id,
        id,
        &input.case.case_key,
        None,
        "caseKey",
        None,
    )
    .await?;
    let case_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    if let Err(error) =
        advance_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await
    {
        drop(tx);
        ensure_case_key_available(
            &state,
            actor.tenant_id,
            id,
            &input.case.case_key,
            None,
            "caseKey",
            None,
        )
        .await?;
        return Err(error);
    }
    let order:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sort_order),0)+1 AS UNSIGNED) FROM dataset_cases WHERE tenant_id=? AND dataset_id=?").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    insert_case(&mut tx, actor.tenant_id, id, case_id, order, &input.case).await?;
    audit(
        &mut tx,
        &actor,
        "dataset.case.created",
        "dataset_case",
        case_id,
        json!({"datasetId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_case(&state, actor.tenant_id, id, case_id).await?),
    ))
}

#[utoipa::path(patch,path="/api/v1/datasets/{id}/cases/{case_id}",request_body=UpdateCaseRequest)]
pub async fn update_case(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, case_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateCaseRequest>,
) -> AppResult<Json<TestCaseResponse>> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    validate_case(&input.case)?;
    ensure_case_key_available(
        &state,
        actor.tenant_id,
        id,
        &input.case.case_key,
        Some(case_id),
        "caseKey",
        None,
    )
    .await?;
    let mut tx = state.pool.begin().await?;
    if let Err(error) =
        advance_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await
    {
        drop(tx);
        ensure_case_key_available(
            &state,
            actor.tenant_id,
            id,
            &input.case.case_key,
            Some(case_id),
            "caseKey",
            None,
        )
        .await?;
        return Err(error);
    }
    let changed=sqlx::query("UPDATE dataset_cases SET case_key=?,name=?,input_json=?,expected_output_json=?,context_json=?,tags_json=?,evaluator_override_json=?,version=version+1 WHERE id=? AND dataset_id=? AND tenant_id=? AND version=?").bind(&input.case.case_key).bind(&input.case.name).bind(&input.case.input).bind(&input.case.expected_output).bind(&input.case.context).bind(json!(input.case.tags)).bind(&input.case.evaluator_override).bind(case_id).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await.map_err(|error| map_unique(error, &[DATASET_CASE_KEY]))?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "CASE_VERSION_CONFLICT",
            "Test Case changed on the server",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "dataset.case.updated",
        "dataset_case",
        case_id,
        json!({"datasetId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_case(&state, actor.tenant_id, id, case_id).await?))
}

#[utoipa::path(delete,path="/api/v1/datasets/{id}/cases/{case_id}",request_body=DeleteCaseRequest)]
pub async fn delete_case(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, case_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<DeleteCaseRequest>,
) -> AppResult<StatusCode> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    advance_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await?;
    let changed =
        sqlx::query("DELETE FROM dataset_cases WHERE id=? AND dataset_id=? AND tenant_id=?")
            .bind(case_id)
            .bind(id)
            .bind(actor.tenant_id)
            .execute(&mut *tx)
            .await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::not_found("Test Case"));
    }
    audit(
        &mut tx,
        &actor,
        "dataset.case.deleted",
        "dataset_case",
        case_id,
        json!({"datasetId":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post,path="/api/v1/datasets/{id}/import",request_body=ImportCasesRequest)]
pub async fn import_cases(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ImportCasesRequest>,
) -> AppResult<Json<Vec<TestCaseResponse>>> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    let cases = parse_import(&input.format, &input.content)?;
    if cases.is_empty() {
        return Err(AppError::bad_request(
            "EMPTY_DATASET_IMPORT",
            "Import contains no Test Cases",
        ));
    }
    let mut first_lines = HashMap::new();
    for (index, case) in cases.iter().enumerate() {
        if let Some(first) = first_lines.insert(case.case_key.clone(), index + 1) {
            return Err(case_key_error(
                "file",
                &case.case_key,
                Some(index + 1),
                Some(first),
            ));
        }
    }
    let mut tx = state.pool.begin().await?;
    if let Err(error) =
        advance_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await
    {
        drop(tx);
        for (index, case) in cases.iter().enumerate() {
            ensure_case_key_available(
                &state,
                actor.tenant_id,
                id,
                &case.case_key,
                None,
                "file",
                Some(index + 1),
            )
            .await?;
        }
        return Err(error);
    }
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT case_key FROM dataset_cases WHERE tenant_id=? AND dataset_id=? FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let existing = existing
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if let Some((index, case)) = cases
        .iter()
        .enumerate()
        .find(|(_, case)| existing.contains(&case.case_key))
    {
        return Err(case_key_error(
            "file",
            &case.case_key,
            Some(index + 1),
            None,
        ));
    }
    let start: u64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(sort_order),0) AS UNSIGNED) FROM dataset_cases WHERE tenant_id=? AND dataset_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let mut ids = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        validate_case(case)?;
        let case_id = Uuid::now_v7();
        insert_case(
            &mut tx,
            actor.tenant_id,
            id,
            case_id,
            start + index as u64 + 1,
            case,
        )
        .await?;
        ids.push(case_id);
    }
    audit(
        &mut tx,
        &actor,
        "dataset.cases.imported",
        "dataset",
        id,
        json!({"count":ids.len(),"format":input.format}),
    )
    .await?;
    tx.commit().await?;
    let mut result = Vec::new();
    for case_id in ids {
        result.push(load_case(&state, actor.tenant_id, id, case_id).await?);
    }
    Ok(Json(result))
}

#[utoipa::path(get, path = "/api/v1/datasets/{id}/export")]
pub async fn export_cases(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    actor.require("dataset:view")?;
    require_dataset_access(&state, &actor, id, false).await?;
    let rows = sqlx::query(CASE_SELECT)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    let mut body = String::new();
    for row in rows {
        body.push_str(&serde_json::to_string(&case_from_row(row)?).map_err(AppError::internal)?);
        body.push('\n');
    }
    Ok((
        [
            ("content-type", "application/x-ndjson"),
            ("content-disposition", "attachment; filename=dataset.jsonl"),
        ],
        body,
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/datasets/{id}/versions",
    operation_id = "list_dataset_versions"
)]
pub async fn list_versions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<DatasetVersionResponse>>> {
    actor.require("dataset:view")?;
    require_dataset_access(&state, &actor, id, false).await?;
    let rows=sqlx::query("SELECT id,dataset_id,version_number,source_revision,content_hash,case_count,created_at FROM dataset_versions WHERE tenant_id=? AND dataset_id=? ORDER BY version_number DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(dataset_version_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/datasets/{id}/versions")]
pub async fn publish_version(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<DatasetVersionResponse>)> {
    actor.require("dataset:manage")?;
    require_dataset_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    let revision: u64 = sqlx::query_scalar(
        "SELECT revision FROM datasets WHERE id=? AND tenant_id=? AND status='active' FOR UPDATE",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("Dataset"))?;
    let rows = sqlx::query(CASE_SELECT)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    if rows.is_empty() {
        return Err(AppError::unprocessable(
            "DATASET_EMPTY",
            "Dataset requires at least one Test Case",
        ));
    }
    let cases = rows
        .into_iter()
        .map(case_from_row)
        .collect::<AppResult<Vec<_>>>()?;
    let canonical = serde_json::to_vec(&cases).map_err(AppError::internal)?;
    let hash = format!("{:x}", Sha256::digest(canonical));
    if let Some(existing)=sqlx::query("SELECT id,dataset_id,version_number,source_revision,content_hash,case_count,created_at FROM dataset_versions WHERE tenant_id=? AND dataset_id=? AND source_revision=? AND content_hash=?").bind(actor.tenant_id).bind(id).bind(revision).bind(&hash).fetch_optional(&mut *tx).await?{tx.commit().await?;return Ok((StatusCode::OK,Json(dataset_version_from_row(existing)?)));}
    let version_id = Uuid::now_v7();
    let version_number:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(version_number),0)+1 AS UNSIGNED) FROM dataset_versions WHERE tenant_id=? AND dataset_id=?").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO dataset_versions(id,tenant_id,dataset_id,version_number,source_revision,content_hash,case_count,created_by) VALUES(?,?,?,?,?,?,?,?)").bind(version_id).bind(actor.tenant_id).bind(id).bind(version_number).bind(revision).bind(&hash).bind(cases.len() as u64).bind(actor.user_id).execute(&mut *tx).await?;
    for case in &cases {
        let case_hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(case).map_err(AppError::internal)?)
        );
        sqlx::query("INSERT INTO dataset_version_cases(tenant_id,dataset_version_id,source_case_id,case_key,name,input_json,expected_output_json,context_json,tags_json,evaluator_override_json,sort_order,content_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)").bind(actor.tenant_id).bind(version_id).bind(case.id).bind(&case.case_key).bind(&case.name).bind(&case.input).bind(&case.expected_output).bind(&case.context).bind(json!(case.tags)).bind(&case.evaluator_override).bind(case.sort_order).bind(case_hash).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        &actor,
        "dataset.version.created",
        "dataset",
        id,
        json!({"datasetVersionId":version_id,"versionNumber":version_number}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_dataset_version(&state, actor.tenant_id, version_id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/evaluation-profiles")]
pub async fn list_profiles(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<EvaluationProfileResponse>>> {
    actor.require("evaluation_profile:view")?;
    let rows = if actor.company_admin {
        sqlx::query(PROFILE_SELECT)
            .bind(actor.tenant_id)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query(PROFILE_VISIBLE)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .fetch_all(&state.pool)
            .await?
    };
    let mut result = Vec::new();
    for row in rows {
        result.push(load_profile_from_row(&state, actor.tenant_id, row).await?);
    }
    Ok(Json(result))
}

#[utoipa::path(post,path="/api/v1/evaluation-profiles",request_body=CreateEvaluationProfileRequest)]
pub async fn create_profile(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateEvaluationProfileRequest>,
) -> AppResult<(StatusCode, Json<EvaluationProfileResponse>)> {
    actor.require("evaluation_profile:manage")?;
    validate_visibility(&input.visibility)?;
    validate_profile(&input)?;
    for rule in &input.rules {
        validate_evaluator_workflow(
            &state.pool,
            actor.tenant_id,
            &rule.evaluator_type,
            &rule.configuration,
        )
        .await?;
    }
    if input.rules.is_empty() {
        return Err(AppError::bad_request(
            "EVALUATION_RULE_REQUIRED",
            "An Evaluation Profile requires at least one rule",
        ));
    }
    let id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let hash = profile_hash(&input)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO evaluation_profiles(id,tenant_id,name,description,owner_user_id,owner_department_id,visibility) VALUES(?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(validate_name(&input.name,160)?).bind(input.description).bind(actor.user_id).bind(actor.department_id).bind(&input.visibility).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO evaluation_profile_versions(id,tenant_id,profile_id,version_number,aggregation,pass_threshold,content_hash,created_by) VALUES(?,?,?,1,?,?,?,?)").bind(version_id).bind(actor.tenant_id).bind(id).bind(&input.aggregation).bind(&input.pass_threshold).bind(hash).bind(actor.user_id).execute(&mut *tx).await?;
    for (index, rule) in input.rules.iter().enumerate() {
        sqlx::query("INSERT INTO evaluation_profile_rules(id,tenant_id,profile_version_id,rule_key,name,evaluator_type,configuration_json,weight,required,sort_order) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(version_id).bind(rule.key.trim()).bind(validate_name(&rule.name,160)?).bind(&rule.evaluator_type).bind(&rule.configuration).bind(&rule.weight).bind(rule.required).bind(index as u32).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        &actor,
        "evaluation.profile.created",
        "evaluation_profile",
        id,
        json!({"versionId":version_id,"ruleCount":input.rules.len(),"aggregation":input.aggregation}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_profile(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/evaluations")]
pub async fn list_evaluations(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<EvaluationRunResponse>>> {
    actor.require("evaluation:view")?;
    let rows = if actor.company_admin {
        sqlx::query(EVALUATION_SELECT)
            .bind(actor.tenant_id)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query(EVALUATION_VISIBLE)
            .bind(actor.tenant_id)
            .bind(actor.user_id)
            .bind(actor.department_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .bind(actor.user_id)
            .fetch_all(&state.pool)
            .await?
    };
    Ok(Json(
        rows.into_iter()
            .map(evaluation_from_row)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post,path="/api/v1/evaluations",request_body=CreateEvaluationRunRequest)]
pub async fn create_evaluation(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateEvaluationRunRequest>,
) -> AppResult<(StatusCode, Json<EvaluationRunResponse>)> {
    actor.require("evaluation:manage")?;
    validate_visibility(&input.visibility)?;
    let workflow_id: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM workflow_versions WHERE id=? AND tenant_id=?")
            .bind(input.workflow_version_id)
            .bind(actor.tenant_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| {
                AppError::unprocessable(
                    "INVALID_EVALUATION_SNAPSHOT",
                    "Workflow Version was not found",
                )
            })?;
    require_workflow_access(&state.pool, &actor, workflow_id, false).await?;
    let dataset_id: Uuid =
        sqlx::query_scalar("SELECT dataset_id FROM dataset_versions WHERE id=? AND tenant_id=?")
            .bind(input.dataset_version_id)
            .bind(actor.tenant_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| {
                AppError::unprocessable(
                    "INVALID_EVALUATION_SNAPSHOT",
                    "Dataset Version was not found",
                )
            })?;
    require_dataset_access(&state, &actor, dataset_id, false).await?;
    let profile_id: Uuid = sqlx::query_scalar(
        "SELECT profile_id FROM evaluation_profile_versions WHERE id=? AND tenant_id=?",
    )
    .bind(input.evaluation_profile_version_id)
    .bind(actor.tenant_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::unprocessable(
            "INVALID_EVALUATION_SNAPSHOT",
            "Evaluation Profile Version was not found",
        )
    })?;
    require_profile_access(&state, &actor, profile_id, false).await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO evaluation_runs(id,tenant_id,name,workflow_version_id,dataset_version_id,evaluation_profile_version_id,parameters_json,created_by,owner_department_id,visibility) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(validate_name(&input.name,160)?).bind(input.workflow_version_id).bind(input.dataset_version_id).bind(input.evaluation_profile_version_id).bind(&input.parameters).bind(actor.user_id).bind(actor.department_id).bind(&input.visibility).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "evaluation.created", "evaluation_run", id, json!({"workflowVersionId":input.workflow_version_id,"datasetVersionId":input.dataset_version_id,"profileVersionId":input.evaluation_profile_version_id})).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_evaluation(&state, &actor, id).await?),
    ))
}

#[utoipa::path(post, path = "/api/v1/evaluations/{id}/start")]
pub async fn start_evaluation(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("evaluation:manage")?;
    require_evaluation_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    let run = sqlx::query("SELECT workflow_version_id,dataset_version_id,parameters_json,status,created_by FROM evaluation_runs WHERE id=? AND tenant_id=? FOR UPDATE")
        .bind(id)
        .bind(actor.tenant_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("Evaluation Run"))?;
    if run.try_get::<String, _>("status")? != "created" {
        return Err(AppError::conflict(
            "EVALUATION_ALREADY_STARTED",
            "Evaluation Run has already been started",
        ));
    }
    let workflow_version_id: Uuid = run.try_get("workflow_version_id")?;
    let dataset_version_id: Uuid = run.try_get("dataset_version_id")?;
    let parameters: Value = run.try_get("parameters_json")?;
    let cases = sqlx::query("SELECT source_case_id,case_key,input_json,context_json FROM dataset_version_cases WHERE tenant_id=? AND dataset_version_id=? ORDER BY sort_order,source_case_id")
        .bind(actor.tenant_id)
        .bind(dataset_version_id)
        .fetch_all(&mut *tx)
        .await?;
    if cases.is_empty() {
        return Err(AppError::unprocessable(
            "EVALUATION_DATASET_EMPTY",
            "Dataset Version has no Cases",
        ));
    }
    for case in cases {
        let case_id = Uuid::now_v7();
        let source_case_id: Uuid = case.try_get("source_case_id")?;
        let case_key: String = case.try_get("case_key")?;
        let command = RuntimeCommand::new(
            TenantId::from_uuid(actor.tenant_id),
            RuntimeCommandType::StartExecution,
            "evaluation_run_case",
            case_id.to_string(),
            format!("evaluation:{id}:case:{source_case_id}"),
            serde_json::to_value(StartExecutionCommandPayload {
                workflow_version_id,
                invocation_id: None,
                session_id: None,
                requested_by: Some(actor.user_id),
                trigger_type: "evaluation".into(),
                input: case.try_get("input_json")?,
                runtime_settings: json!({
                    "evaluationRunId": id,
                    "evaluationCaseId": case_id,
                    "caseKey": case_key,
                    "context": case.try_get::<Option<Value>, _>("context_json")?,
                    "parameters": parameters.clone(),
                }),
            })
            .map_err(AppError::internal)?,
        );
        RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &command)
            .await
            .map_err(AppError::internal)?;
        sqlx::query("INSERT INTO evaluation_run_cases(id,tenant_id,evaluation_run_id,source_case_id,target_command_id) VALUES(?,?,?,?,?)")
            .bind(case_id)
            .bind(actor.tenant_id)
            .bind(id)
            .bind(source_case_id)
            .bind(command.id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE evaluation_runs SET status='queued',started_at=CURRENT_TIMESTAMP(6) WHERE id=? AND tenant_id=? AND status='created'")
        .bind(id)
        .bind(actor.tenant_id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        "evaluation.started",
        "evaluation_run",
        id,
        json!({"workflowVersionId":workflow_version_id,"datasetVersionId":dataset_version_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(post, path = "/api/v1/evaluations/{id}/cancel")]
pub async fn cancel_evaluation(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("evaluation:manage")?;
    require_evaluation_access(&state, &actor, id, true).await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT id FROM evaluation_runs WHERE id=? AND tenant_id=? FOR UPDATE")
        .bind(id)
        .bind(actor.tenant_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("Evaluation Run"))?;
    let executions = sqlx::query("SELECT id,target_command_id,target_execution_id FROM evaluation_run_cases WHERE tenant_id=? AND evaluation_run_id=? AND status NOT IN ('completed','failed','cancelled') FOR UPDATE")
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    for case in executions {
        let case_id: Uuid = case.try_get("id")?;
        if let Some(execution_id) = case.try_get::<Option<Uuid>, _>("target_execution_id")? {
            let command = RuntimeCommand::new(
                TenantId::from_uuid(actor.tenant_id),
                RuntimeCommandType::CancelExecution,
                "evaluation_run_case",
                case_id.to_string(),
                format!("evaluation:{id}:case:{case_id}:cancel"),
                serde_json::to_value(CancelExecutionCommandPayload { execution_id })
                    .map_err(AppError::internal)?,
            );
            RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &command)
                .await
                .map_err(AppError::internal)?;
        } else {
            let target_command_id: Uuid = case.try_get("target_command_id")?;
            sqlx::query("UPDATE runtime_commands SET status='failed',error_code='EVALUATION_CANCELLED',error_message='Evaluation was cancelled before execution creation',completed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND status='pending'")
                .bind(target_command_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE evaluation_run_cases SET status='cancelled',completed_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=?")
            .bind(actor.tenant_id)
            .bind(case_id)
            .execute(&mut *tx)
            .await?;
    }
    let changed=sqlx::query("UPDATE evaluation_runs SET status='cancelled',completed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND tenant_id=? AND status IN ('created','queued','running')").bind(id).bind(actor.tenant_id).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "EVALUATION_NOT_CANCELLABLE",
            "Evaluation Run is already terminal",
        ));
    }
    audit(
        &mut tx,
        &actor,
        "evaluation.cancelled",
        "evaluation_run",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/evaluations/{id}/report")]
pub async fn get_report(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<EvaluationReportResponse>> {
    actor.require("evaluation:view")?;
    let run = load_evaluation(&state, &actor, id).await?;
    let result_rows=sqlx::query("SELECT c.id case_id,c.source_case_id,dvc.case_key,c.status case_status,c.target_execution_id,c.duration_ms case_duration_ms,c.cost_micros case_cost_micros,c.error_code,c.error_message,cr.status result_status,CAST(cr.score AS DOUBLE) result_score,cr.detail_json,cr.duration_ms result_duration_ms,cr.cost_micros result_cost_micros FROM evaluation_run_cases c JOIN evaluation_runs er ON er.id=c.evaluation_run_id AND er.tenant_id=c.tenant_id JOIN dataset_version_cases dvc ON dvc.dataset_version_id=er.dataset_version_id AND dvc.source_case_id=c.source_case_id LEFT JOIN evaluation_case_results cr ON cr.tenant_id=c.tenant_id AND cr.evaluation_run_id=c.evaluation_run_id AND cr.source_case_id=c.source_case_id WHERE c.tenant_id=? AND c.evaluation_run_id=? ORDER BY dvc.sort_order,dvc.source_case_id").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let rule_rows=sqlx::query("SELECT rr.id,rr.evaluation_run_case_id,pr.rule_key,pr.name,pr.evaluator_type,rr.status,rr.passed,CAST(rr.score AS DOUBLE) score,rr.detail_json,rr.evaluator_execution_id,rr.duration_ms,rr.cost_micros FROM evaluation_rule_results rr JOIN evaluation_profile_rules pr ON pr.id=rr.profile_rule_id AND pr.tenant_id=rr.tenant_id JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id WHERE rr.tenant_id=? AND c.evaluation_run_id=? ORDER BY pr.sort_order,rr.id").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let metric_rows=sqlx::query("SELECT metric_key,metric_value,detail_json FROM evaluation_metrics WHERE tenant_id=? AND evaluation_run_id=? ORDER BY metric_key").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let mut rules = BTreeMap::<Uuid, Vec<EvaluationRuleResultResponse>>::new();
    for row in rule_rows {
        rules
            .entry(row.try_get("evaluation_run_case_id")?)
            .or_default()
            .push(EvaluationRuleResultResponse {
                id: row.try_get("id")?,
                key: row.try_get("rule_key")?,
                name: row.try_get("name")?,
                evaluator_type: row.try_get("evaluator_type")?,
                status: row.try_get("status")?,
                passed: row.try_get("passed")?,
                score: row.try_get("score")?,
                detail: row.try_get("detail_json")?,
                evaluator_execution_id: row.try_get("evaluator_execution_id")?,
                duration_ms: row.try_get("duration_ms")?,
                cost_micros: row.try_get("cost_micros")?,
            });
    }
    let results = result_rows
        .into_iter()
        .map(|row| {
            let case_id: Uuid = row.try_get("case_id")?;
            let result_status: Option<String> = row.try_get("result_status")?;
            let result_duration: Option<u64> = row.try_get("result_duration_ms")?;
            let result_cost: Option<u64> = row.try_get("result_cost_micros")?;
            Ok(EvaluationCaseResultResponse {
                case_id,
                source_case_id: row.try_get("source_case_id")?,
                case_key: row.try_get("case_key")?,
                status: result_status.unwrap_or(row.try_get("case_status")?),
                score: row.try_get("result_score")?,
                detail: row.try_get("detail_json")?,
                target_execution_id: row.try_get("target_execution_id")?,
                duration_ms: result_duration.or(row.try_get("case_duration_ms")?),
                cost_micros: result_cost.unwrap_or(row.try_get("case_cost_micros")?),
                error_code: row.try_get("error_code")?,
                error_message: row.try_get("error_message")?,
                rule_results: rules.remove(&case_id).unwrap_or_default(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let metrics=metric_rows.into_iter().map(|r|json!({"key":r.try_get::<String,_>("metric_key").ok(),"value":r.try_get::<f64,_>("metric_value").ok(),"detail":r.try_get::<Option<Value>,_>("detail_json").ok().flatten()})).collect();
    let report_status = if run.status == "created" {
        "not_started"
    } else {
        run.status.as_str()
    }
    .to_owned();
    Ok(Json(EvaluationReportResponse {
        run,
        results,
        metrics,
        report_status,
    }))
}

const DATASET_LIST: &str = "SELECT d.id,d.name,d.description,d.visibility,d.status,d.owner_department_id,d.revision,(SELECT COUNT(*) FROM dataset_cases c WHERE c.dataset_id=d.id) case_count,(SELECT MAX(version_number) FROM dataset_versions v WHERE v.dataset_id=d.id) latest_version,d.version,d.updated_at FROM datasets d WHERE d.tenant_id=? AND (?='' OR d.status=?) AND (?='%%' OR d.name LIKE ?) ORDER BY d.updated_at DESC LIMIT ? OFFSET ?";
const DATASET_VISIBLE: &str = "SELECT d.id,d.name,d.description,d.visibility,d.status,d.owner_department_id,d.revision,(SELECT COUNT(*) FROM dataset_cases c WHERE c.dataset_id=d.id) case_count,(SELECT MAX(version_number) FROM dataset_versions v WHERE v.dataset_id=d.id) latest_version,d.version,d.updated_at FROM datasets d WHERE d.tenant_id=? AND (d.visibility='company' OR d.owner_user_id=? OR (d.visibility='department' AND EXISTS(SELECT 1 FROM department_closure dc WHERE dc.tenant_id=d.tenant_id AND ((dc.ancestor_id=d.owner_department_id AND dc.descendant_id=?) OR (dc.ancestor_id=? AND dc.descendant_id=d.owner_department_id)))) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=d.tenant_id AND ur.user_id=? AND dc.descendant_id=d.owner_department_id)) AND (?='' OR d.status=?) AND (?='%%' OR d.name LIKE ?) ORDER BY d.updated_at DESC LIMIT ? OFFSET ?";
const CASE_SELECT: &str = "SELECT id,case_key,name,input_json,expected_output_json,context_json,tags_json,evaluator_override_json,sort_order,version FROM dataset_cases WHERE tenant_id=? AND dataset_id=? ORDER BY sort_order,id";
const PROFILE_SELECT: &str = "SELECT p.id,p.name,p.description,p.visibility,p.owner_department_id,p.status,p.version,pv.id version_id,pv.version_number,pv.aggregation,CAST(pv.pass_threshold AS CHAR) pass_threshold FROM evaluation_profiles p JOIN evaluation_profile_versions pv ON pv.profile_id=p.id AND pv.version_number=(SELECT MAX(v.version_number) FROM evaluation_profile_versions v WHERE v.profile_id=p.id) WHERE p.tenant_id=? ORDER BY p.updated_at DESC";
const PROFILE_VISIBLE: &str = "SELECT p.id,p.name,p.description,p.visibility,p.owner_department_id,p.status,p.version,pv.id version_id,pv.version_number,pv.aggregation,CAST(pv.pass_threshold AS CHAR) pass_threshold FROM evaluation_profiles p JOIN evaluation_profile_versions pv ON pv.profile_id=p.id AND pv.version_number=(SELECT MAX(v.version_number) FROM evaluation_profile_versions v WHERE v.profile_id=p.id) WHERE p.tenant_id=? AND (p.visibility='company' OR p.owner_user_id=? OR (p.visibility='department' AND p.owner_department_id=?) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=p.tenant_id AND ur.user_id=? AND dc.descendant_id=p.owner_department_id)) ORDER BY p.updated_at DESC";
const EVALUATION_SELECT: &str = "SELECT er.id,er.name,er.workflow_version_id,w.name workflow_name,er.dataset_version_id,d.name dataset_name,er.evaluation_profile_version_id,er.visibility,er.owner_department_id,er.status,(SELECT COUNT(*) FROM evaluation_case_results cr WHERE cr.evaluation_run_id=er.id) result_count,er.parameters_json,er.created_at FROM evaluation_runs er JOIN workflow_versions wv ON wv.id=er.workflow_version_id JOIN workflows w ON w.id=wv.workflow_id JOIN dataset_versions dv ON dv.id=er.dataset_version_id JOIN datasets d ON d.id=dv.dataset_id WHERE er.tenant_id=? ORDER BY er.created_at DESC";
const EVALUATION_VISIBLE: &str = "SELECT er.id,er.name,er.workflow_version_id,w.name workflow_name,er.dataset_version_id,d.name dataset_name,er.evaluation_profile_version_id,er.visibility,er.owner_department_id,er.status,(SELECT COUNT(*) FROM evaluation_case_results cr WHERE cr.evaluation_run_id=er.id) result_count,er.parameters_json,er.created_at FROM evaluation_runs er JOIN workflow_versions wv ON wv.id=er.workflow_version_id JOIN workflows w ON w.id=wv.workflow_id JOIN dataset_versions dv ON dv.id=er.dataset_version_id JOIN datasets d ON d.id=dv.dataset_id WHERE er.tenant_id=? AND (er.visibility='company' OR er.created_by=? OR (er.visibility='department' AND er.owner_department_id=?) OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=er.tenant_id AND ur.user_id=? AND dc.descendant_id=er.owner_department_id)) AND (w.visibility='company' OR w.owner_user_id=? OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=?) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.tenant_id=ur.tenant_id AND dc.ancestor_id=ur.scope_department_id WHERE ur.tenant_id=w.tenant_id AND ur.user_id=? AND dc.descendant_id=w.owner_department_id))) ORDER BY er.created_at DESC";

fn dataset_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<DatasetResponse> {
    Ok(DatasetResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        visibility: r.try_get("visibility")?,
        status: r.try_get("status")?,
        owner_department_id: r.try_get("owner_department_id")?,
        revision: r.try_get("revision")?,
        case_count: r.try_get::<i64, _>("case_count")? as u64,
        latest_version: r.try_get("latest_version")?,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
async fn require_dataset_access(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    write: bool,
) -> AppResult<()> {
    let r=sqlx::query("SELECT owner_user_id,owner_department_id,visibility FROM datasets WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Dataset"))?;
    if actor.company_admin {
        return Ok(());
    }
    let owner: Uuid = r.try_get("owner_user_id")?;
    let department: Uuid = r.try_get("owner_department_id")?;
    let visibility: String = r.try_get("visibility")?;
    if owner == actor.user_id
        || (!write && visibility == "company")
        || (!write && visibility == "department" && department == actor.department_id)
    {
        Ok(())
    } else {
        require_department_scope(&state.pool, actor, department).await
    }
}
async fn load_dataset(state: &AppState, actor: &AuthActor, id: Uuid) -> AppResult<DatasetResponse> {
    require_dataset_access(state, actor, id, false).await?;
    let r=sqlx::query("SELECT d.id,d.name,d.description,d.visibility,d.status,d.owner_department_id,d.revision,(SELECT COUNT(*) FROM dataset_cases c WHERE c.dataset_id=d.id) case_count,(SELECT MAX(version_number) FROM dataset_versions v WHERE v.dataset_id=d.id) latest_version,d.version,d.updated_at FROM datasets d WHERE d.id=? AND d.tenant_id=?").bind(id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    dataset_from_row(r)
}
fn case_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<TestCaseResponse> {
    let tags: Value = r.try_get("tags_json")?;
    Ok(TestCaseResponse {
        id: r.try_get("id")?,
        case_key: r.try_get("case_key")?,
        name: r.try_get("name")?,
        input: r.try_get("input_json")?,
        expected_output: r.try_get("expected_output_json")?,
        context: r.try_get("context_json")?,
        tags: serde_json::from_value(tags).unwrap_or_default(),
        evaluator_override: r.try_get("evaluator_override_json")?,
        sort_order: r.try_get("sort_order")?,
        version: r.try_get("version")?,
    })
}
async fn load_case(
    state: &AppState,
    tenant: Uuid,
    dataset: Uuid,
    id: Uuid,
) -> AppResult<TestCaseResponse> {
    let r=sqlx::query("SELECT id,case_key,name,input_json,expected_output_json,context_json,tags_json,evaluator_override_json,sort_order,version FROM dataset_cases WHERE tenant_id=? AND dataset_id=? AND id=?").bind(tenant).bind(dataset).bind(id).fetch_one(&state.pool).await?;
    case_from_row(r)
}
async fn advance_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant: Uuid,
    dataset: Uuid,
    expected: u64,
) -> AppResult<()> {
    let changed=sqlx::query("UPDATE datasets SET revision=revision+1,version=version+1 WHERE id=? AND tenant_id=? AND revision=? AND status='active'").bind(dataset).bind(tenant).bind(expected).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "DATASET_REVISION_CONFLICT",
            "Dataset Case workspace changed on the server",
        ));
    }
    Ok(())
}
async fn insert_case(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant: Uuid,
    dataset: Uuid,
    id: Uuid,
    order: u64,
    c: &CaseInput,
) -> AppResult<()> {
    sqlx::query("INSERT INTO dataset_cases(id,tenant_id,dataset_id,case_key,name,input_json,expected_output_json,context_json,tags_json,evaluator_override_json,sort_order) VALUES(?,?,?,?,?,?,?,?,?,?,?)").bind(id).bind(tenant).bind(dataset).bind(&c.case_key).bind(&c.name).bind(&c.input).bind(&c.expected_output).bind(&c.context).bind(json!(c.tags)).bind(&c.evaluator_override).bind(order).execute(&mut **tx).await.map_err(|error| map_unique(error, &[DATASET_CASE_KEY]))?;
    Ok(())
}

async fn ensure_case_key_available(
    state: &AppState,
    tenant: Uuid,
    dataset: Uuid,
    case_key: &str,
    exclude: Option<Uuid>,
    field: &str,
    line: Option<usize>,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM dataset_cases WHERE tenant_id=? AND dataset_id=? AND case_key=? AND (? IS NULL OR id<>?))")
        .bind(tenant).bind(dataset).bind(case_key).bind(exclude).bind(exclude).fetch_one(&state.pool).await?;
    if exists {
        Err(case_key_error(field, case_key, line, None))
    } else {
        Ok(())
    }
}

fn case_key_error(
    field: &str,
    case_key: &str,
    line: Option<usize>,
    first_line: Option<usize>,
) -> AppError {
    AppError::conflict(DATASET_CASE_KEY.code, DATASET_CASE_KEY.message)
        .with_field(field, DATASET_CASE_KEY.code, DATASET_CASE_KEY.message)
        .with_details(json!({"caseKey":case_key,"line":line,"firstLine":first_line}))
}
fn dataset_version_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<DatasetVersionResponse> {
    Ok(DatasetVersionResponse {
        id: r.try_get("id")?,
        dataset_id: r.try_get("dataset_id")?,
        version_number: r.try_get("version_number")?,
        source_revision: r.try_get("source_revision")?,
        content_hash: r.try_get("content_hash")?,
        case_count: r.try_get("case_count")?,
        created_at: r.try_get("created_at")?,
    })
}
async fn load_dataset_version(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<DatasetVersionResponse> {
    let r=sqlx::query("SELECT id,dataset_id,version_number,source_revision,content_hash,case_count,created_at FROM dataset_versions WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_one(&state.pool).await?;
    dataset_version_from_row(r)
}
async fn load_profile_from_row(
    state: &AppState,
    tenant: Uuid,
    r: sqlx::mysql::MySqlRow,
) -> AppResult<EvaluationProfileResponse> {
    let id: Uuid = r.try_get("id")?;
    let version_id: Uuid = r.try_get("version_id")?;
    let rule_rows=sqlx::query("SELECT id,rule_key,name,evaluator_type,configuration_json,CAST(weight AS CHAR) weight,required,sort_order FROM evaluation_profile_rules WHERE tenant_id=? AND profile_version_id=? ORDER BY sort_order")
        .bind(tenant).bind(version_id).fetch_all(&state.pool).await?;
    let rules = rule_rows
        .into_iter()
        .map(|row| {
            Ok(EvaluationRuleResponse {
                id: row.try_get("id")?,
                key: row.try_get("rule_key")?,
                name: row.try_get("name")?,
                evaluator_type: row.try_get("evaluator_type")?,
                configuration: row.try_get("configuration_json")?,
                weight: row.try_get("weight")?,
                required: row.try_get("required")?,
                sort_order: row.try_get("sort_order")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(EvaluationProfileResponse {
        id,
        version_id,
        version_number: r.try_get("version_number")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        visibility: r.try_get("visibility")?,
        owner_department_id: r.try_get("owner_department_id")?,
        status: r.try_get("status")?,
        aggregation: r.try_get("aggregation")?,
        pass_threshold: r.try_get("pass_threshold")?,
        rules,
        version: r.try_get("version")?,
    })
}
async fn load_profile(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<EvaluationProfileResponse> {
    let sql = format!(
        "{} AND p.id=?",
        PROFILE_SELECT.trim_end_matches(" ORDER BY p.updated_at DESC")
    );
    let r = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    load_profile_from_row(state, tenant, r).await
}

async fn require_profile_access(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    write: bool,
) -> AppResult<()> {
    let row = sqlx::query("SELECT owner_user_id,owner_department_id,visibility FROM evaluation_profiles WHERE tenant_id=? AND id=?")
        .bind(actor.tenant_id).bind(id).fetch_optional(&state.pool).await?
        .ok_or_else(|| AppError::not_found("Evaluation Profile"))?;
    require_owned_access(
        &state.pool,
        actor,
        row.try_get("owner_user_id")?,
        row.try_get("owner_department_id")?,
        row.try_get("visibility")?,
        write,
    )
    .await
}
fn evaluation_from_row(r: sqlx::mysql::MySqlRow) -> AppResult<EvaluationRunResponse> {
    Ok(EvaluationRunResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        workflow_version_id: r.try_get("workflow_version_id")?,
        workflow_name: r.try_get("workflow_name")?,
        dataset_version_id: r.try_get("dataset_version_id")?,
        dataset_name: r.try_get("dataset_name")?,
        evaluation_profile_version_id: r.try_get("evaluation_profile_version_id")?,
        visibility: r.try_get("visibility")?,
        owner_department_id: r.try_get("owner_department_id")?,
        status: r.try_get("status")?,
        result_count: r.try_get::<i64, _>("result_count")? as u64,
        parameters: r.try_get("parameters_json")?,
        created_at: r.try_get("created_at")?,
    })
}
async fn load_evaluation(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<EvaluationRunResponse> {
    require_evaluation_access(state, actor, id, false).await?;
    let sql = format!(
        "{} AND er.id=?",
        EVALUATION_SELECT.trim_end_matches(" ORDER BY er.created_at DESC")
    );
    let r = sqlx::query(&sql)
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let response = evaluation_from_row(r)?;
    let workflow_id: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM workflow_versions WHERE tenant_id=? AND id=?")
            .bind(actor.tenant_id)
            .bind(response.workflow_version_id)
            .fetch_one(&state.pool)
            .await?;
    require_workflow_access(&state.pool, actor, workflow_id, false).await?;
    Ok(response)
}

async fn require_evaluation_access(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
    write: bool,
) -> AppResult<()> {
    let row = sqlx::query("SELECT created_by,owner_department_id,visibility FROM evaluation_runs WHERE tenant_id=? AND id=?")
        .bind(actor.tenant_id).bind(id).fetch_optional(&state.pool).await?
        .ok_or_else(|| AppError::not_found("Evaluation Run"))?;
    require_owned_access(
        &state.pool,
        actor,
        row.try_get("created_by")?,
        row.try_get("owner_department_id")?,
        row.try_get("visibility")?,
        write,
    )
    .await
}

async fn require_owned_access(
    pool: &sqlx::MySqlPool,
    actor: &AuthActor,
    owner: Uuid,
    department: Uuid,
    visibility: String,
    write: bool,
) -> AppResult<()> {
    if actor.company_admin
        || owner == actor.user_id
        || (!write && visibility == "company")
        || (!write && visibility == "department" && department == actor.department_id)
    {
        Ok(())
    } else {
        require_department_scope(pool, actor, department).await
    }
}

fn default_visibility() -> String {
    "private".into()
}
fn default_aggregation() -> String {
    "all".into()
}
fn default_pass_threshold() -> String {
    "1".into()
}
fn default_weight() -> String {
    "1".into()
}
fn default_required() -> bool {
    true
}
fn validate_visibility(v: &str) -> AppResult<()> {
    if matches!(v, "private" | "department" | "company") {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "INVALID_VISIBILITY",
            "Visibility is invalid",
        ))
    }
}
fn validate_profile(input: &CreateEvaluationProfileRequest) -> AppResult<()> {
    if !matches!(input.aggregation.as_str(), "all" | "any" | "weighted") {
        return Err(AppError::bad_request(
            "INVALID_PROFILE_AGGREGATION",
            "Aggregation must be all, any, or weighted",
        ));
    }
    let threshold = input.pass_threshold.parse::<f64>().map_err(|_| {
        AppError::bad_request("INVALID_PASS_THRESHOLD", "Pass threshold must be a number")
    })?;
    if !(0.0..=1.0).contains(&threshold) {
        return Err(AppError::bad_request(
            "INVALID_PASS_THRESHOLD",
            "Pass threshold must be between 0 and 1",
        ));
    }
    if input.rules.is_empty() || input.rules.len() > 50 {
        return Err(AppError::bad_request(
            "INVALID_EVALUATION_RULES",
            "An Evaluation Profile requires between 1 and 50 rules",
        ));
    }
    let mut keys = std::collections::HashSet::new();
    for rule in &input.rules {
        let key = rule.key.trim();
        if key.is_empty()
            || key.len() > 128
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
            || !keys.insert(key)
        {
            return Err(AppError::bad_request(
                "INVALID_EVALUATION_RULE_KEY",
                "Rule keys must be unique ASCII identifiers",
            ));
        }
        validate_name(&rule.name, 160)?;
        validate_evaluator(&rule.evaluator_type, &rule.configuration)?;
        let weight = rule.weight.parse::<f64>().map_err(|_| {
            AppError::bad_request(
                "INVALID_EVALUATION_RULE_WEIGHT",
                "Rule weight must be a number",
            )
        })?;
        if !weight.is_finite() || weight <= 0.0 || weight > 1000.0 {
            return Err(AppError::bad_request(
                "INVALID_EVALUATION_RULE_WEIGHT",
                "Rule weight must be greater than 0 and no more than 1000",
            ));
        }
    }
    Ok(())
}
fn profile_hash(input: &CreateEvaluationProfileRequest) -> AppResult<String> {
    let snapshot = json!({
        "aggregation": &input.aggregation,
        "passThreshold": &input.pass_threshold,
        "rules": &input.rules,
    });
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&snapshot).map_err(AppError::internal)?)
    ))
}
fn validate_case(c: &CaseInput) -> AppResult<()> {
    if c.case_key.trim().is_empty() || c.case_key.len() > 128 {
        return Err(AppError::bad_request(
            "INVALID_CASE_KEY",
            "Case key is required and limited to 128 characters",
        ));
    }
    validate_name(&c.name, 255)?;
    Ok(())
}
fn parse_import(format: &str, content: &str) -> AppResult<Vec<CaseInput>> {
    match format {
        "jsonl" => content
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                serde_json::from_str(line).map_err(|e| {
                    AppError::bad_request(
                        "INVALID_DATASET_IMPORT",
                        format!("Line {}: {e}", index + 1),
                    )
                })
            })
            .collect(),
        "csv" => parse_csv(content),
        _ => Err(AppError::bad_request(
            "INVALID_IMPORT_FORMAT",
            "Format must be jsonl or csv",
        )),
    }
}
fn parse_csv(content: &str) -> AppResult<Vec<CaseInput>> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());
    let headers = reader
        .headers()
        .map_err(|error| {
            AppError::bad_request("INVALID_DATASET_IMPORT", format!("CSV header: {error}"))
        })?
        .clone();
    let key = headers
        .iter()
        .position(|v| v == "caseKey")
        .ok_or_else(|| AppError::bad_request("INVALID_DATASET_IMPORT", "CSV requires caseKey"))?;
    let name = headers
        .iter()
        .position(|v| v == "name")
        .ok_or_else(|| AppError::bad_request("INVALID_DATASET_IMPORT", "CSV requires name"))?;
    let input = headers
        .iter()
        .position(|v| v == "input")
        .ok_or_else(|| AppError::bad_request("INVALID_DATASET_IMPORT", "CSV requires input"))?;
    let expected = headers.iter().position(|v| v == "expectedOutput");
    let context = headers.iter().position(|v| v == "context");
    let tags = headers.iter().position(|v| v == "tags");
    reader
        .records()
        .enumerate()
        .map(|(index, record)| {
            let cols = record.map_err(|error| {
                AppError::bad_request(
                    "INVALID_DATASET_IMPORT",
                    format!("Line {}: {error}", index + 2),
                )
            })?;
            let value = |position: usize| {
                cols.get(position).map(|v| v.trim()).ok_or_else(|| {
                    AppError::bad_request(
                        "INVALID_DATASET_IMPORT",
                        format!("Line {} has too few columns", index + 2),
                    )
                })
            };
            Ok(CaseInput {
                case_key: value(key)?.to_owned(),
                name: value(name)?.to_owned(),
                input: serde_json::from_str(value(input)?).map_err(|e| {
                    AppError::bad_request(
                        "INVALID_DATASET_IMPORT",
                        format!("Line {} input: {e}", index + 2),
                    )
                })?,
                expected_output: optional_json(&cols, expected, index + 2, "expectedOutput")?,
                context: optional_json(&cols, context, index + 2, "context")?,
                tags: optional_json(&cols, tags, index + 2, "tags")?
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_default(),
                evaluator_override: None,
            })
        })
        .collect()
}

fn optional_json(
    record: &csv::StringRecord,
    position: Option<usize>,
    line: usize,
    field: &str,
) -> AppResult<Option<Value>> {
    let Some(raw) = position
        .and_then(|index| record.get(index))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    serde_json::from_str(raw).map(Some).map_err(|error| {
        AppError::bad_request(
            "INVALID_DATASET_IMPORT",
            format!("Line {line} {field}: {error}"),
        )
    })
}
fn validate_evaluator(kind: &str, config: &Value) -> AppResult<()> {
    match kind {
        "exact" | "contains" => Ok(()),
        "regex" => config
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::bad_request("INVALID_EVALUATOR_CONFIG", "Regex requires pattern")
            })
            .and_then(|pattern| {
                regex::Regex::new(pattern).map(|_| ()).map_err(|error| {
                    AppError::bad_request("INVALID_EVALUATOR_CONFIG", error.to_string())
                })
            }),
        "json_schema" => {
            if config.get("schema").is_some_and(Value::is_object) {
                Ok(())
            } else {
                Err(AppError::bad_request(
                    "INVALID_EVALUATOR_CONFIG",
                    "JSON Schema evaluator requires schema",
                ))
            }
        }
        "llm_judge" | "custom_code" => config
            .get("evaluatorWorkflowVersionId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::bad_request(
                    "INVALID_EVALUATOR_CONFIG",
                    "Runtime evaluator requires evaluatorWorkflowVersionId",
                )
            })
            .and_then(|value| {
                Uuid::parse_str(value).map(|_| ()).map_err(|_| {
                    AppError::bad_request(
                        "INVALID_EVALUATOR_CONFIG",
                        "evaluatorWorkflowVersionId must be a UUID",
                    )
                })
            }),
        _ => Err(AppError::bad_request(
            "INVALID_EVALUATOR_TYPE",
            "Evaluator type is invalid",
        )),
    }
}

async fn validate_evaluator_workflow(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    kind: &str,
    configuration: &Value,
) -> AppResult<()> {
    if !matches!(kind, "llm_judge" | "custom_code") {
        return Ok(());
    }
    let version_id = configuration
        .get("evaluatorWorkflowVersionId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            AppError::bad_request(
                "INVALID_EVALUATOR_CONFIG",
                "Runtime evaluator requires a valid evaluatorWorkflowVersionId",
            )
        })?;
    let definition: Value = sqlx::query_scalar(
        "SELECT definition_json FROM workflow_versions WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(version_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::unprocessable(
            "EVALUATOR_WORKFLOW_VERSION_NOT_FOUND",
            "Evaluator Workflow Version was not found",
        )
    })?;
    let nodes = definition
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let valid = if kind == "llm_judge" {
        nodes.iter().any(|node| {
            node.get("type").and_then(Value::as_str) == Some("agent")
                && node
                    .get("resourceReferences")
                    .and_then(Value::as_array)
                    .is_some_and(|references| {
                        references.iter().any(|reference| {
                            reference.get("resourceType").and_then(Value::as_str) == Some("model")
                        })
                    })
        })
    } else {
        nodes.iter().any(|node| {
            node.get("type").and_then(Value::as_str) == Some("code")
                && node
                    .get("resourceReferences")
                    .and_then(Value::as_array)
                    .is_some_and(|references| {
                        references.iter().any(|reference| {
                            reference.get("resourceType").and_then(Value::as_str)
                                == Some("sandbox_profile")
                        })
                    })
        })
    };
    if !valid {
        return Err(AppError::unprocessable(
            "INVALID_EVALUATOR_WORKFLOW",
            if kind == "llm_judge" {
                "LLM Judge Workflow Version must contain an Agent with a Model binding"
            } else {
                "Custom Code Workflow Version must contain a Code node with a Sandbox Profile"
            },
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn evaluate_rule(
    kind: &str,
    config: &Value,
    actual: &Value,
    expected: &Value,
) -> Result<bool, String> {
    match kind {
        "exact" => Ok(actual == expected),
        "contains" => Ok(actual
            .as_str()
            .unwrap_or_default()
            .contains(expected.as_str().unwrap_or_default())),
        "regex" => {
            let pattern = config
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or("pattern is required")?;
            regex::Regex::new(pattern)
                .map(|regex| regex.is_match(actual.as_str().unwrap_or_default()))
                .map_err(|error| error.to_string())
        }
        "json_schema" => {
            let schema = config.get("schema").ok_or("schema is required")?;
            Ok(schema
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| match kind {
                    "object" => actual.is_object(),
                    "array" => actual.is_array(),
                    "string" => actual.is_string(),
                    "number" => actual.is_number(),
                    "boolean" => actual.is_boolean(),
                    _ => false,
                }))
        }
        _ => Err("evaluator requires runtime".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::{
        CreateEvaluationProfileRequest, EvaluationRuleInput, evaluate_rule, parse_import,
        validate_profile,
    };
    use serde_json::json;
    #[test]
    fn deterministic_rules_are_repeatable() {
        assert!(evaluate_rule("exact", &json!({}), &json!(1), &json!(1)).unwrap());
        assert!(
            evaluate_rule(
                "contains",
                &json!({}),
                &json!("agentx platform"),
                &json!("agentx")
            )
            .unwrap()
        );
        assert!(
            evaluate_rule(
                "regex",
                &json!({"pattern":"^agentx"}),
                &json!("agentx platform"),
                &json!(null)
            )
            .unwrap()
        );
        assert!(
            evaluate_rule(
                "json_schema",
                &json!({"schema":{"type":"object"}}),
                &json!({}),
                &json!(null)
            )
            .unwrap()
        );
    }
    #[test]
    fn jsonl_errors_include_line() {
        let error = parse_import(
            "jsonl",
            "{\"caseKey\":\"a\",\"name\":\"A\",\"input\":{}}\nbad",
        )
        .unwrap_err();
        assert!(error.message.contains("Line 2"));
    }
    #[test]
    fn profile_requires_valid_unique_rules_and_threshold() {
        let rule = EvaluationRuleInput {
            key: "exact_output".into(),
            name: "Exact output".into(),
            evaluator_type: "exact".into(),
            configuration: json!({}),
            weight: "1".into(),
            required: true,
        };
        let mut profile = CreateEvaluationProfileRequest {
            name: "Regression".into(),
            description: None,
            visibility: "department".into(),
            aggregation: "all".into(),
            pass_threshold: "1".into(),
            rules: vec![rule.clone()],
        };
        assert!(validate_profile(&profile).is_ok());
        profile.rules.push(rule);
        assert!(validate_profile(&profile).is_err());
        profile.rules.pop();
        profile.pass_threshold = "1.1".into();
        assert!(validate_profile(&profile).is_err());
    }
}
