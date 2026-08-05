use agentx_domain::{
    EditorDocument, WorkflowDefinition, canonical_content_hash, validate_definition,
    validate_editor_document,
};
use agentx_runtime::{
    CompileContext, ExpressionContext, ExpressionEngine, NodeRegistry, WorkflowCompiler,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::require_workflow_access,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateDraftRequest {
    pub definition: Value,
    #[serde(default)]
    pub editor_document: Value,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub code: String,
    pub severity: String,
    pub node_id: Option<String>,
    pub field_path: Option<String>,
    pub message: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateDraftResponse {
    pub issues: Vec<ValidationIssue>,
    pub definition_hash: Option<String>,
    pub editor_hash: Option<String>,
    pub compiler_version: Option<String>,
}

#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionPreviewRequest {
    pub expression: String,
    #[serde(default)]
    pub json: Value,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub linked_nodes: Value,
    #[serde(default)]
    pub item_index: usize,
    #[serde(default)]
    pub run_index: u32,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionPreviewResponse {
    pub value: Value,
    pub redacted: bool,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugOverlayResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub node_id: String,
    pub kind: String,
    pub payload: Value,
    pub artifact_id: Option<Uuid>,
    pub schema_hash: Option<String>,
    pub stale: bool,
    pub updated_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveDebugOverlayRequest {
    pub kind: String,
    pub payload: Value,
    pub artifact_id: Option<Uuid>,
    pub schema_hash: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/draft/validate", request_body = ValidateDraftRequest, params(("id" = Uuid, Path)))]
pub async fn validate_draft(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ValidateDraftRequest>,
) -> AppResult<Json<ValidateDraftResponse>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let mut issues = Vec::new();
    let definition = match serde_json::from_value::<WorkflowDefinition>(input.definition) {
        Ok(value) => Some(value),
        Err(error) => {
            issues.push(ValidationIssue {
                code: "INVALID_WORKFLOW_DEFINITION".into(),
                severity: "error".into(),
                node_id: None,
                field_path: None,
                message: error.to_string(),
            });
            None
        }
    };
    let editor_value = if input.editor_document.is_null() {
        json!({})
    } else {
        input.editor_document
    };
    let editor = serde_json::from_value::<EditorDocument>(editor_value.clone());
    if let Err(error) = &editor {
        issues.push(ValidationIssue {
            code: "INVALID_EDITOR_DOCUMENT".into(),
            severity: "error".into(),
            node_id: None,
            field_path: None,
            message: error.to_string(),
        });
    }
    let mut definition_hash = None;
    let mut compiler_version = None;
    if let (Some(definition), Ok(editor)) = (&definition, editor) {
        for issue in validate_definition(definition) {
            issues.push(definition_issue(
                definition,
                issue.code,
                issue.path,
                issue.message,
            ));
        }
        for issue in validate_editor_document(definition, &editor) {
            issues.push(domain_issue(issue.code, issue.path, issue.message));
        }
        match crate::grants::missing_for_definition(&state, &actor, id, definition).await {
            Ok(missing) => issues.extend(missing.into_iter().map(|missing| ValidationIssue {
                code: "RESOURCE_GRANT_MISSING".into(),
                severity: "error".into(),
                node_id: Some(missing.node_id),
                field_path: Some("resourceReferences".into()),
                message: format!(
                    "{} {} requires {} grant",
                    missing.resource_type.as_str(),
                    missing.resource_id,
                    missing.operation.as_str()
                ),
            })),
            Err(error) => issues.push(ValidationIssue {
                code: "RESOURCE_VALIDATION_FAILED".into(),
                severity: "error".into(),
                node_id: None,
                field_path: Some("resourceReferences".into()),
                message: error.message,
            }),
        }
        let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
        match WorkflowCompiler::new(&registry).compile(definition, &CompileContext::default()) {
            Ok(compiled) => {
                compiler_version = Some(compiled.compiler_version);
                definition_hash = Some(
                    canonical_content_hash(
                        &serde_json::to_value(definition).map_err(AppError::internal)?,
                    )
                    .map_err(AppError::internal)?,
                );
            }
            Err(error) => {
                issues.extend(error.issues.into_iter().map(|issue| {
                    definition_issue(definition, issue.code, issue.path, issue.message)
                }))
            }
        }
    }
    let editor_hash = canonical_content_hash(&editor_value).ok();
    Ok(Json(ValidateDraftResponse {
        issues,
        definition_hash,
        editor_hash,
        compiler_version,
    }))
}

#[utoipa::path(post, path = "/api/v1/workflows/{id}/expressions/preview", request_body = ExpressionPreviewRequest, params(("id" = Uuid, Path)))]
pub async fn preview_expression(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ExpressionPreviewRequest>,
) -> AppResult<Json<ExpressionPreviewResponse>> {
    actor.require("workflow:update")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    let source = input
        .expression
        .strip_prefix('=')
        .unwrap_or(&input.expression);
    if source.len() > 32_768 {
        return Err(AppError::unprocessable(
            "EXPRESSION_TOO_LARGE",
            "Expression preview is limited to 32768 bytes",
        ));
    }
    let value = ExpressionEngine
        .evaluate(
            source,
            &ExpressionContext {
                json: input.json,
                input: input.input,
                item_index: input.item_index,
                run_index: input.run_index,
                linked_nodes: input.linked_nodes,
            },
        )
        .map_err(|error| AppError::unprocessable("INVALID_EXPRESSION", error.to_string()))?;
    let (value, redacted) = redact_preview_value(value);
    Ok(Json(ExpressionPreviewResponse { value, redacted }))
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/debug-overlays/{node_id}", params(("id" = Uuid, Path), ("node_id" = String, Path)))]
pub async fn get_debug_overlay(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, node_id)): Path<(Uuid, String)>,
) -> AppResult<Json<DebugOverlayResponse>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let row = sqlx::query("SELECT id,workflow_id,node_id,kind,payload_json,artifact_id,schema_hash,stale,updated_by,updated_at FROM workflow_debug_overlays WHERE tenant_id=? AND workflow_id=? AND node_id=?")
        .bind(actor.tenant_id).bind(id).bind(&node_id).fetch_optional(&state.pool).await?.ok_or_else(|| AppError::not_found("Debug overlay"))?;
    let mut overlay = overlay_from_row(row)?;
    if !overlay.stale {
        let definition = load_definition(&state, actor.tenant_id, id).await?;
        let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
        let current_hash = node_overlay_schema_hash(&definition, &registry, &node_id).ok();
        if overlay.schema_hash.as_deref() != current_hash.as_deref() {
            sqlx::query("UPDATE workflow_debug_overlays SET stale=TRUE WHERE tenant_id=? AND workflow_id=? AND node_id=?")
                .bind(actor.tenant_id).bind(id).bind(&node_id).execute(&state.pool).await?;
            overlay.stale = true;
        }
    }
    Ok(Json(overlay))
}

#[utoipa::path(put, path = "/api/v1/workflows/{id}/debug-overlays/{node_id}", request_body = SaveDebugOverlayRequest, params(("id" = Uuid, Path), ("node_id" = String, Path)))]
pub async fn save_debug_overlay(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, node_id)): Path<(Uuid, String)>,
    Json(input): Json<SaveDebugOverlayRequest>,
) -> AppResult<Json<DebugOverlayResponse>> {
    actor.require("workflow:edit")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    if !matches!(
        input.kind.as_str(),
        "pin_data" | "mock_output" | "temporary_input" | "history_output" | "artifact"
    ) {
        return Err(AppError::bad_request(
            "INVALID_DEBUG_OVERLAY_KIND",
            "Unsupported debug overlay kind",
        ));
    }
    let definition = load_definition(&state, actor.tenant_id, id).await?;
    if !definition.nodes.iter().any(|node| node.id == node_id) {
        return Err(AppError::not_found("Workflow node"));
    }
    match (input.kind.as_str(), input.artifact_id) {
        ("artifact", None) => {
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_ARTIFACT_REQUIRED",
                "Artifact overlays require artifactId",
            ));
        }
        ("artifact", Some(artifact_id)) => {
            let execution_id = input
                .payload
                .get("executionId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unprocessable(
                        "DEBUG_OVERLAY_ARTIFACT_SOURCE_REQUIRED",
                        "Artifact overlays require payload.executionId",
                    )
                })?
                .parse()
                .map_err(|_| {
                    AppError::unprocessable(
                        "DEBUG_OVERLAY_ARTIFACT_SOURCE_INVALID",
                        "payload.executionId is invalid",
                    )
                })?;
            crate::runtime_operations::ensure_execution_artifact(
                &state.pool,
                actor.tenant_id,
                id,
                execution_id,
                artifact_id,
            )
            .await?;
        }
        (_, Some(_)) => {
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_ARTIFACT_UNEXPECTED",
                "artifactId is only valid for artifact overlays",
            ));
        }
        _ => {}
    }
    if input.kind == "history_output" {
        for field in ["executionId", "nodeExecutionId"] {
            input
                .payload
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unprocessable(
                        "DEBUG_OVERLAY_HISTORY_INVALID",
                        format!("{field} is required for history_output overlays"),
                    )
                })?;
        }
    }
    let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
    let current_schema_hash = node_overlay_schema_hash(&definition, &registry, &node_id)?;
    if input
        .schema_hash
        .as_deref()
        .is_some_and(|hash| hash != current_schema_hash)
    {
        return Err(AppError::unprocessable(
            "DEBUG_OVERLAY_SCHEMA_STALE",
            "The overlay schema no longer matches the selected node",
        ));
    }
    let overlay_id = Uuid::now_v7();
    sqlx::query("INSERT INTO workflow_debug_overlays(id,tenant_id,workflow_id,node_id,kind,payload_json,artifact_id,schema_hash,stale,updated_by) VALUES(?,?,?,?,?,?,?,?,FALSE,?) ON DUPLICATE KEY UPDATE kind=VALUES(kind),payload_json=VALUES(payload_json),artifact_id=VALUES(artifact_id),schema_hash=VALUES(schema_hash),stale=FALSE,updated_by=VALUES(updated_by)")
        .bind(overlay_id).bind(actor.tenant_id).bind(id).bind(&node_id).bind(&input.kind).bind(&input.payload).bind(input.artifact_id).bind(&current_schema_hash).bind(actor.user_id).execute(&state.pool).await?;
    let row = sqlx::query("SELECT id,workflow_id,node_id,kind,payload_json,artifact_id,schema_hash,stale,updated_by,updated_at FROM workflow_debug_overlays WHERE tenant_id=? AND workflow_id=? AND node_id=?")
        .bind(actor.tenant_id).bind(id).bind(&node_id).fetch_one(&state.pool).await?;
    Ok(Json(overlay_from_row(row)?))
}

#[utoipa::path(delete, path = "/api/v1/workflows/{id}/debug-overlays/{node_id}", params(("id" = Uuid, Path), ("node_id" = String, Path)))]
pub async fn delete_debug_overlay(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, node_id)): Path<(Uuid, String)>,
) -> AppResult<StatusCode> {
    actor.require("workflow:edit")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    sqlx::query(
        "DELETE FROM workflow_debug_overlays WHERE tenant_id=? AND workflow_id=? AND node_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .bind(&node_id)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn domain_issue(code: String, path: String, message: String) -> ValidationIssue {
    ValidationIssue {
        code,
        severity: "error".into(),
        node_id: None,
        field_path: Some(path),
        message,
    }
}

fn definition_issue(
    definition: &WorkflowDefinition,
    code: String,
    path: String,
    message: String,
) -> ValidationIssue {
    let Some(index) = path
        .strip_prefix("nodes[")
        .and_then(|value| value.split_once(']'))
        .and_then(|(index, _)| index.parse::<usize>().ok())
    else {
        return domain_issue(code, path, message);
    };
    let field_path = path
        .split_once(']')
        .map(|(_, suffix)| suffix.trim_start_matches('.').to_owned())
        .filter(|value| !value.is_empty());
    ValidationIssue {
        code,
        severity: "error".into(),
        node_id: definition.nodes.get(index).map(|node| node.id.clone()),
        field_path,
        message,
    }
}

fn overlay_from_row(row: sqlx::mysql::MySqlRow) -> Result<DebugOverlayResponse, sqlx::Error> {
    Ok(DebugOverlayResponse {
        id: row.try_get("id")?,
        workflow_id: row.try_get("workflow_id")?,
        node_id: row.try_get("node_id")?,
        kind: row.try_get("kind")?,
        payload: row.try_get("payload_json")?,
        artifact_id: row.try_get("artifact_id")?,
        schema_hash: row.try_get("schema_hash")?,
        stale: row.try_get("stale")?,
        updated_by: row.try_get("updated_by")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn redact_preview_value(value: Value) -> (Value, bool) {
    match value {
        Value::Object(values) => {
            let mut redacted = false;
            let values = values
                .into_iter()
                .map(|(key, value)| {
                    if sensitive_preview_key(&key) {
                        redacted = true;
                        (key, Value::String("[REDACTED]".into()))
                    } else {
                        let (value, child_redacted) = redact_preview_value(value);
                        redacted |= child_redacted;
                        (key, value)
                    }
                })
                .collect();
            (Value::Object(values), redacted)
        }
        Value::Array(values) => {
            let mut redacted = false;
            let values = values
                .into_iter()
                .map(|value| {
                    let (value, child_redacted) = redact_preview_value(value);
                    redacted |= child_redacted;
                    value
                })
                .collect();
            (Value::Array(values), redacted)
        }
        Value::String(value)
            if value.to_ascii_lowercase().starts_with("bearer ")
                || value.to_ascii_lowercase().starts_with("basic ") =>
        {
            (Value::String("[REDACTED]".into()), true)
        }
        value => (value, false),
    }
}

fn sensitive_preview_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "secret",
        "password",
        "passwd",
        "token",
        "apikey",
        "authorization",
        "cookie",
        "credential",
        "privatekey",
    ]
    .iter()
    .any(|candidate| normalized.contains(candidate))
}

async fn load_definition(
    state: &AppState,
    tenant_id: Uuid,
    workflow_id: Uuid,
) -> AppResult<WorkflowDefinition> {
    let definition: Value = sqlx::query_scalar(
        "SELECT definition_json FROM workflow_drafts WHERE tenant_id=? AND workflow_id=?",
    )
    .bind(tenant_id)
    .bind(workflow_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Workflow draft"))?;
    serde_json::from_value(definition).map_err(AppError::internal)
}

pub(crate) fn node_overlay_schema_hash(
    definition: &WorkflowDefinition,
    registry: &NodeRegistry,
    node_id: &str,
) -> AppResult<String> {
    let node = definition
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| AppError::not_found("Workflow node"))?;
    let manifest = registry
        .get(&node.node_type, node.type_version)
        .ok_or_else(|| {
            AppError::unprocessable(
                "NODE_MANIFEST_MISSING",
                format!(
                    "Manifest {}@{} is unavailable",
                    node.node_type, node.type_version
                ),
            )
        })?;
    canonical_content_hash(&json!({
        "nodeType": node.node_type,
        "typeVersion": node.type_version,
        "parameterSchema": manifest.parameter_schema,
        "inputPorts": manifest.input_ports,
        "outputPorts": manifest.output_ports,
    }))
    .map_err(AppError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlay_schema_hash_changes_with_manifest_contract() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion": "3.0",
            "nodes": [{"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger"}],
            "connections": [],
            "settings": {}
        }))
        .expect("valid workflow definition");
        let registry = NodeRegistry::m5_defaults();
        let original = node_overlay_schema_hash(&definition, &registry, "trigger").unwrap();

        let mut changed_manifest = registry
            .get("manual_trigger", 1)
            .expect("built-in manifest")
            .clone();
        changed_manifest.parameter_schema =
            json!({"type":"object","properties":{"input":{"type":"string"}}});
        let mut changed_registry = NodeRegistry::default();
        changed_registry
            .register(changed_manifest)
            .expect("changed manifest registers");
        let changed = node_overlay_schema_hash(&definition, &changed_registry, "trigger").unwrap();

        assert_ne!(original, changed);
    }

    #[test]
    fn expression_preview_recursively_redacts_sensitive_values() {
        let (value, redacted) = redact_preview_value(json!({
            "result": 4,
            "apiKey": "plain-secret",
            "nested": [{"authorization": "Bearer token"}],
            "header": "Basic encoded"
        }));

        assert!(redacted);
        assert_eq!(value["result"], 4);
        assert_eq!(value["apiKey"], "[REDACTED]");
        assert_eq!(value["nested"][0]["authorization"], "[REDACTED]");
        assert_eq!(value["header"], "[REDACTED]");
    }
}
