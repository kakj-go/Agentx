use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Cursor, Read, Write},
};

use agentx_api_types::PageResponse;
use agentx_application::{ArtifactStore, ArtifactWrite};
use agentx_domain::{ArtifactId, TenantId, canonical_content_hash};
use agentx_infrastructure::artifact::MySqlObjectArtifactStore;
use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{HeaderName, HeaderValue, Response, StatusCode, header},
};
use comrak::{Arena, Options, format_commonmark, nodes::NodeValue, parse_document};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
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

const MAX_FILE_SIZE: usize = 20 * 1024 * 1024;
const MAX_WORKSPACE_SIZE: u64 = 100 * 1024 * 1024;
const MAX_ENTRIES: i64 = 1000;
const MAX_DEPTH: usize = 20;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_department_id: Uuid,
    pub status: String,
    pub draft_revision: u64,
    pub latest_version: Option<u64>,
    pub grant_count: u64,
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
pub type SkillPage = PageResponse<SkillResponse>;
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    pub owner_department_id: Uuid,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSkillRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: u64,
}
#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillDependencyInput {
    pub resource_type: String,
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
    pub operation: String,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillWorkspaceEntry {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub mime_type: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub content_hash: Option<String>,
    pub size_bytes: u64,
    pub editable: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillWorkspaceResponse {
    pub revision: u64,
    pub entries: Vec<SkillWorkspaceEntry>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntryRequest {
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub entry_type: String,
    pub expected_revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveEntryRequest {
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub expected_revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMarkdownRequest {
    pub content: String,
    pub description: Option<String>,
    pub expected_revision: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct SkillDocumentMetadata {
    name: String,
    description: String,
}

#[derive(Debug)]
struct SkillDocument {
    metadata: SkillDocumentMetadata,
    body: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEntryQuery {
    pub expected_revision: u64,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileContentResponse {
    pub entry_id: Uuid,
    pub content: String,
    pub content_hash: String,
    pub revision: u64,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishSkillVersionRequest {
    pub expected_revision: u64,
    #[serde(default)]
    pub dependencies: Vec<SkillDependencyInput>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersionResponse {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub version_number: u64,
    pub source_revision: u64,
    pub manifest: Value,
    pub content_hash: String,
    pub file_count: u64,
    pub dependencies: Vec<SkillDependencyInput>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactUploadResponse {
    pub entry: SkillWorkspaceEntry,
    pub revision: u64,
}

const SKILL_COLUMNS: &str = "s.id,s.name,s.description,s.owner_department_id,s.status,s.draft_revision,s.version,s.updated_at,(SELECT version_number FROM skill_versions sv WHERE sv.skill_id=s.id ORDER BY version_number DESC LIMIT 1) latest_version,(SELECT COUNT(*) FROM resource_grants rg WHERE rg.tenant_id=s.tenant_id AND rg.resource_type='skill' AND rg.resource_id=s.id AND rg.subject_type='workflow_service_identity') grant_count,COUNT(*) OVER() total_count";

#[utoipa::path(get, path = "/api/v1/skills")]
pub async fn list_skills(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<SkillListQuery>,
) -> AppResult<Json<SkillPage>> {
    actor.require("skill:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let size = query.page_size.unwrap_or(20).clamp(1, 100);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let sql = if actor.company_admin {
        format!(
            "SELECT {SKILL_COLUMNS} FROM skills s WHERE s.tenant_id=? AND (?='' OR s.status=?) AND (?='%%' OR s.name LIKE ?) ORDER BY s.updated_at DESC LIMIT ? OFFSET ?"
        )
    } else {
        format!(
            "SELECT {SKILL_COLUMNS} FROM skills s WHERE s.tenant_id=? AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=s.tenant_id AND dc.descendant_id=s.owner_department_id) AND (?='' OR s.status=?) AND (?='%%' OR s.name LIKE ?) ORDER BY s.updated_at DESC LIMIT ? OFFSET ?"
        )
    };
    let mut q = sqlx::query(&sql).bind(actor.tenant_id);
    if !actor.company_admin {
        q = q.bind(actor.user_id);
    }
    let rows = q
        .bind(&status)
        .bind(&status)
        .bind(&search)
        .bind(&search)
        .bind(size)
        .bind(u64::from((page - 1) * size))
        .fetch_all(&state.pool)
        .await?;
    let total = rows
        .first()
        .map_or(Ok(0_i64), |r| r.try_get("total_count"))? as u64;
    Ok(Json(PageResponse {
        items: rows
            .into_iter()
            .map(skill_from_row)
            .collect::<Result<_, _>>()?,
        page,
        page_size: size,
        total,
    }))
}

#[utoipa::path(post,path="/api/v1/skills",request_body=CreateSkillRequest)]
pub async fn create_skill(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateSkillRequest>,
) -> AppResult<(StatusCode, Json<SkillResponse>)> {
    actor.require("skill:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    let name = validate_name(&input.name, 160)?;
    let description = validate_skill_description(Some(&input.description))?;
    let id = Uuid::now_v7();
    let content = render_skill_document(&name, &description, &format!("# {name}\n"))?;
    let artifact = put_artifact(
        &state,
        actor.tenant_id,
        "text/markdown; charset=utf-8",
        content.into_bytes(),
    )
    .await?;
    let entry_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO skills(id,tenant_id,name,description,owner_department_id,created_by) VALUES(?,?,?,?,?,?)").bind(id).bind(actor.tenant_id).bind(&name).bind(&description).bind(input.owner_department_id).bind(actor.user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,parent_id,name,path,path_hash,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable) VALUES(?,?,?,NULL,'SKILL.md','SKILL.md',?,'file','text/markdown; charset=utf-8',?,?,?,TRUE)").bind(entry_id).bind(actor.tenant_id).bind(id).bind(path_hash("SKILL.md")).bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO skill_file_revisions(id,tenant_id,skill_id,entry_id,workspace_revision,artifact_id,content_hash,size_bytes,created_by) VALUES(?,?,?,?,1,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(entry_id).bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).bind(actor.user_id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "skill.created", "skill", id, json!({})).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_skill(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(get,path="/api/v1/skills/{id}",params(("id"=Uuid,Path)))]
pub async fn get_skill(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SkillResponse>> {
    actor.require("skill:view")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    Ok(Json(load_skill(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(patch,path="/api/v1/skills/{id}",request_body=UpdateSkillRequest,params(("id"=Uuid,Path)))]
pub async fn update_skill(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateSkillRequest>,
) -> AppResult<Json<SkillResponse>> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let name = validate_name(&input.name, 160)?;
    let description = validate_skill_description(input.description.as_deref())?;
    if !matches!(input.status.as_str(), "draft" | "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Skill status is invalid",
        ));
    }
    let current = load_skill(&state, actor.tenant_id, id).await?;
    let metadata_changed =
        current.name != name || current.description.as_deref().unwrap_or_default() != description;
    let root_update = if metadata_changed {
        let row = sqlx::query("SELECT id,artifact_id,size_bytes FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? AND path='SKILL.md'")
            .bind(actor.tenant_id)
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
        let entry_id: Uuid = row.try_get("id")?;
        let artifact_id: Uuid = row.try_get("artifact_id")?;
        let old_size: u64 = row.try_get("size_bytes")?;
        let document = get_artifact(&state, actor.tenant_id, artifact_id).await?;
        let document = String::from_utf8(document.content).map_err(|_| {
            AppError::unprocessable("SKILL_MARKDOWN_UTF8", "Markdown files must use UTF-8")
        })?;
        let document = parse_skill_document(&document)?;
        let content = render_skill_document(&name, &description, &document.body)?;
        ensure_capacity(
            &state,
            actor.tenant_id,
            id,
            content.len() as i64 - old_size as i64,
        )
        .await?;
        let artifact = put_artifact(
            &state,
            actor.tenant_id,
            "text/markdown; charset=utf-8",
            content.into_bytes(),
        )
        .await?;
        Some((entry_id, artifact))
    } else {
        None
    };
    let mut tx = state.pool.begin().await?;
    if input.status == "active" {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM skill_versions WHERE tenant_id=? AND skill_id=?)",
        )
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(AppError::unprocessable(
                "SKILL_VERSION_REQUIRED",
                "Publish a Skill version before activation",
            ));
        }
    }
    let result = if root_update.is_some() {
        sqlx::query("UPDATE skills SET name=?,description=?,status=?,version=version+1,draft_revision=draft_revision+1 WHERE tenant_id=? AND id=? AND version=? AND draft_revision=?")
            .bind(&name).bind(&description).bind(&input.status).bind(actor.tenant_id).bind(id).bind(input.version).bind(current.draft_revision).execute(&mut *tx).await?
    } else {
        sqlx::query("UPDATE skills SET name=?,description=?,status=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?")
            .bind(&name).bind(&description).bind(&input.status).bind(actor.tenant_id).bind(id).bind(input.version).execute(&mut *tx).await?
    };
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "SKILL_VERSION_CONFLICT",
            "Skill changed",
        ));
    }
    if let Some((entry_id, artifact)) = root_update {
        sqlx::query("UPDATE skill_workspace_entries SET artifact_id=?,content_hash=?,size_bytes=? WHERE tenant_id=? AND skill_id=? AND id=?")
            .bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).bind(actor.tenant_id).bind(id).bind(entry_id).execute(&mut *tx).await?;
        insert_file_revision(
            &mut tx,
            &actor,
            id,
            entry_id,
            current.draft_revision + 1,
            &artifact,
        )
        .await?;
    }
    audit(&mut tx, &actor, "skill.updated", "skill", id, json!({})).await?;
    tx.commit().await?;
    Ok(Json(load_skill(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get,path="/api/v1/skills/{id}/workspace",params(("id"=Uuid,Path)))]
pub async fn get_workspace(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SkillWorkspaceResponse>> {
    actor.require("skill:view")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    Ok(Json(load_workspace(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(get,path="/api/v1/skills/{id}/workspace/export",params(("id"=Uuid,Path)))]
pub async fn export_workspace(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Response<Body>> {
    actor.require("skill:view")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let workspace = load_workspace(&state, actor.tenant_id, id).await?;
    let mut files = Vec::new();
    for entry in workspace.entries {
        let content = if let Some(artifact) = entry.artifact_id {
            Some(
                get_artifact(&state, actor.tenant_id, artifact)
                    .await?
                    .content,
            )
        } else {
            None
        };
        files.push((entry.path, entry.entry_type, content));
    }
    let bytes = tokio::task::spawn_blocking(move || build_workspace_zip(files))
        .await
        .map_err(AppError::internal)??;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=skill-workspace.zip"),
    );
    Ok(response)
}

#[utoipa::path(post,path="/api/v1/skills/{id}/workspace/import",params(("id"=Uuid,Path)))]
pub async fn import_workspace(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> AppResult<Json<SkillWorkspaceResponse>> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let mut expected = None;
    let mut archive = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("INVALID_MULTIPART", "Workspace import is invalid"))?
    {
        match field.name() {
            Some("expectedRevision") => {
                expected = Some(
                    field
                        .text()
                        .await
                        .map_err(AppError::internal)?
                        .parse::<u64>()
                        .map_err(|_| {
                            AppError::bad_request("INVALID_REVISION", "Revision is invalid")
                        })?,
                );
            }
            Some("file") if archive.is_none() => {
                let value = field.bytes().await.map_err(AppError::internal)?;
                if value.len() > MAX_WORKSPACE_SIZE as usize {
                    return Err(AppError::unprocessable(
                        "SKILL_WORKSPACE_TOO_LARGE",
                        "Skill workspace ZIP exceeds 100 MiB",
                    ));
                }
                archive = Some(value.to_vec());
            }
            _ => {}
        }
    }
    let expected = expected.ok_or_else(|| {
        AppError::bad_request("REVISION_REQUIRED", "Expected revision is required")
    })?;
    let archive = archive
        .ok_or_else(|| AppError::bad_request("FILE_REQUIRED", "Workspace ZIP is required"))?;
    let entries = tokio::task::spawn_blocking(move || parse_workspace_zip(&archive))
        .await
        .map_err(AppError::internal)??;
    let skill = load_skill(&state, actor.tenant_id, id).await?;
    let root = entries
        .iter()
        .find(|entry| entry.path == "SKILL.md")
        .and_then(|entry| entry.content.as_ref())
        .ok_or_else(|| {
            AppError::unprocessable("SKILL_ROOT_REQUIRED", "Root SKILL.md is required")
        })?;
    let root = String::from_utf8(root.clone()).map_err(|_| {
        AppError::unprocessable("SKILL_MARKDOWN_UTF8", "Markdown files must use UTF-8")
    })?;
    let root = parse_skill_document(&root)?;
    require_skill_document_matches(&root, &skill.name, skill.description.as_deref())?;
    let mut stored = Vec::new();
    for entry in entries {
        let artifact = if let Some(content) = entry.content.as_ref() {
            Some(put_artifact(&state, actor.tenant_id, &entry.mime_type, content.clone()).await?)
        } else {
            None
        };
        stored.push((entry, artifact));
    }
    let revision = expected + 1;
    let mut tx = state.pool.begin().await?;
    bump_revision(&mut tx, actor.tenant_id, id, expected).await?;
    sqlx::query("DELETE FROM skill_file_revisions WHERE tenant_id=? AND skill_id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? ORDER BY LENGTH(path) DESC").bind(actor.tenant_id).bind(id).execute(&mut *tx).await?;
    let mut ids = HashMap::new();
    for (entry, _) in &stored {
        ids.insert(entry.path.clone(), Uuid::now_v7());
    }
    for (entry, artifact) in stored {
        let entry_id = ids[&entry.path];
        let parent_id = entry
            .path
            .rsplit_once('/')
            .and_then(|(parent, _)| ids.get(parent))
            .copied();
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
        let entry_type = if entry.content.is_some() {
            "file"
        } else {
            "directory"
        };
        let editable = entry_type == "file" && entry.path.to_ascii_lowercase().ends_with(".md");
        sqlx::query("INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,parent_id,name,path,path_hash,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(entry_id).bind(actor.tenant_id).bind(id).bind(parent_id).bind(name).bind(&entry.path).bind(path_hash(&entry.path)).bind(entry_type)
            .bind(if entry_type == "file" { Some(entry.mime_type.as_str()) } else { None }).bind(artifact.as_ref().map(|value| value.id.as_uuid()))
            .bind(artifact.as_ref().map(|value| value.sha256.as_str())).bind(artifact.as_ref().map_or(0, |value| value.content.len()) as u64).bind(editable)
            .execute(&mut *tx).await?;
        if let Some(artifact) = artifact {
            insert_file_revision(&mut tx, &actor, id, entry_id, revision, &artifact).await?;
        }
    }
    audit(
        &mut tx,
        &actor,
        "skill.workspace_imported",
        "skill",
        id,
        json!({"revision":revision}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_workspace(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(post,path="/api/v1/skills/{id}/entries",request_body=CreateEntryRequest,params(("id"=Uuid,Path)))]
pub async fn create_entry(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateEntryRequest>,
) -> AppResult<(StatusCode, Json<SkillWorkspaceResponse>)> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let name = validate_entry_name(&input.name, &input.entry_type)?;
    if input.entry_type == "file" {
        require_markdown_name(&name)?;
    }
    let parent = parent_path(&state, actor.tenant_id, id, input.parent_id).await?;
    let path = join_path(&parent, &name)?;
    ensure_capacity(&state, actor.tenant_id, id, 0).await?;
    let artifact = if input.entry_type == "file" {
        Some(
            put_artifact(
                &state,
                actor.tenant_id,
                "text/markdown; charset=utf-8",
                format!("# {}\n", name.trim_end_matches(".md")).into_bytes(),
            )
            .await?,
        )
    } else {
        None
    };
    let entry_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    let revision = bump_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await?;
    sqlx::query("INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,parent_id,name,path,path_hash,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)").bind(entry_id).bind(actor.tenant_id).bind(id).bind(input.parent_id).bind(name).bind(&path).bind(path_hash(&path)).bind(&input.entry_type).bind(artifact.as_ref().map(|_|"text/markdown; charset=utf-8")).bind(artifact.as_ref().map(|a|a.id.as_uuid())).bind(artifact.as_ref().map(|a|a.sha256.as_str())).bind(artifact.as_ref().map_or(0,|a|a.content.len()) as u64).bind(artifact.is_some()).execute(&mut *tx).await?;
    if let Some(a) = artifact {
        insert_file_revision(&mut tx, &actor, id, entry_id, revision, &a).await?;
    }
    audit(
        &mut tx,
        &actor,
        "skill.entry_created",
        "skill",
        id,
        json!({"path":path}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_workspace(&state, actor.tenant_id, id).await?),
    ))
}

#[utoipa::path(patch,path="/api/v1/skills/{id}/entries/{entry_id}",request_body=MoveEntryRequest,params(("id"=Uuid,Path),("entry_id"=Uuid,Path)))]
pub async fn move_entry(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, entry_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<MoveEntryRequest>,
) -> AppResult<Json<SkillWorkspaceResponse>> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let row = entry_row(&state, actor.tenant_id, id, entry_id).await?;
    let old: String = row.try_get("path")?;
    if old == "SKILL.md" {
        return Err(AppError::bad_request(
            "SKILL_ROOT_IMMUTABLE",
            "SKILL.md cannot be moved or renamed",
        ));
    }
    let kind: String = row.try_get("entry_type")?;
    let name = validate_entry_name(&input.name, &kind)?;
    if row.try_get::<bool, _>("editable")? {
        require_markdown_name(&name)?;
    }
    let parent = parent_path(&state, actor.tenant_id, id, input.parent_id).await?;
    let new = join_path(&parent, &name)?;
    if kind == "directory" && (new == old || new.starts_with(&format!("{old}/"))) {
        return Err(AppError::bad_request(
            "SKILL_DIRECTORY_CYCLE",
            "Directory cannot be moved into itself",
        ));
    }
    let rewritten = rewrite_links_for_move(&state, actor.tenant_id, id, &old, &new).await?;
    let mut tx = state.pool.begin().await?;
    let revision = bump_revision(&mut tx, actor.tenant_id, id, input.expected_revision).await?;
    sqlx::query("UPDATE skill_workspace_entries SET parent_id=?,name=?,path=?,path_hash=? WHERE tenant_id=? AND skill_id=? AND id=?").bind(input.parent_id).bind(name).bind(&new).bind(path_hash(&new)).bind(actor.tenant_id).bind(id).bind(entry_id).execute(&mut *tx).await?;
    if kind == "directory" {
        sqlx::query("UPDATE skill_workspace_entries SET path_hash=SHA2(CONCAT(?,SUBSTRING(path,?)),256),path=CONCAT(?,SUBSTRING(path,?)) WHERE tenant_id=? AND skill_id=? AND path LIKE ?").bind(&new).bind((old.len()+1) as u64).bind(&new).bind((old.len()+1) as u64).bind(actor.tenant_id).bind(id).bind(format!("{old}/%")).execute(&mut *tx).await?;
    }
    for (markdown_entry, artifact) in rewritten {
        sqlx::query("UPDATE skill_workspace_entries SET artifact_id=?,content_hash=?,size_bytes=? WHERE tenant_id=? AND skill_id=? AND id=?")
            .bind(artifact.id.as_uuid())
            .bind(&artifact.sha256)
            .bind(artifact.content.len() as u64)
            .bind(actor.tenant_id)
            .bind(id)
            .bind(markdown_entry)
            .execute(&mut *tx)
            .await?;
        insert_file_revision(&mut tx, &actor, id, markdown_entry, revision, &artifact).await?;
    }
    audit(
        &mut tx,
        &actor,
        "skill.entry_moved",
        "skill",
        id,
        json!({"from":old,"to":new}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_workspace(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(delete,path="/api/v1/skills/{id}/entries/{entry_id}",params(("id"=Uuid,Path),("entry_id"=Uuid,Path)))]
pub async fn delete_entry(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, entry_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteEntryQuery>,
) -> AppResult<StatusCode> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let row = entry_row(&state, actor.tenant_id, id, entry_id).await?;
    let path: String = row.try_get("path")?;
    if path == "SKILL.md" {
        return Err(AppError::bad_request(
            "SKILL_ROOT_IMMUTABLE",
            "SKILL.md cannot be deleted",
        ));
    }
    let children:i64=sqlx::query_scalar("SELECT COUNT(*) FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? AND parent_id=?").bind(actor.tenant_id).bind(id).bind(entry_id).fetch_one(&state.pool).await?;
    if children > 0 {
        return Err(AppError::unprocessable(
            "SKILL_DIRECTORY_NOT_EMPTY",
            "Directory is not empty",
        ));
    }
    if workspace_references_path(&state, actor.tenant_id, id, &path).await? {
        return Err(AppError::unprocessable(
            "SKILL_FILE_REFERENCED",
            "Remove Markdown references before deleting this file",
        ));
    }
    let mut tx = state.pool.begin().await?;
    bump_revision(&mut tx, actor.tenant_id, id, query.expected_revision).await?;
    sqlx::query("DELETE FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? AND id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        "skill.entry_deleted",
        "skill",
        id,
        json!({"path":path}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get,path="/api/v1/skills/{id}/files/{entry_id}",params(("id"=Uuid,Path),("entry_id"=Uuid,Path)))]
pub async fn get_file(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, entry_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response<Body>> {
    actor.require("skill:view")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let row = entry_row(&state, actor.tenant_id, id, entry_id).await?;
    let artifact: Option<Uuid> = row.try_get("artifact_id")?;
    let mime: Option<String> = row.try_get("mime_type")?;
    let name: String = row.try_get("name")?;
    let value = get_artifact(
        &state,
        actor.tenant_id,
        artifact
            .ok_or_else(|| AppError::bad_request("SKILL_ENTRY_NOT_FILE", "Entry is not a file"))?,
    )
    .await?;
    let mut response = Response::new(Body::from(value.content));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_deref().unwrap_or("application/octet-stream"))
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "sandbox; default-src 'none'; img-src data:; style-src 'unsafe-inline'",
        ),
    );
    let safe_name = name.replace(['\r', '\n', '"'], "_");
    if let Ok(value) = HeaderValue::from_str(&format!("inline; filename=\"{safe_name}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

#[utoipa::path(put,path="/api/v1/skills/{id}/files/{entry_id}",request_body=UpdateMarkdownRequest,params(("id"=Uuid,Path),("entry_id"=Uuid,Path)))]
pub async fn update_markdown(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, entry_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateMarkdownRequest>,
) -> AppResult<Json<SkillFileContentResponse>> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let UpdateMarkdownRequest {
        content,
        description,
        expected_revision,
    } = input;
    let row = entry_row(&state, actor.tenant_id, id, entry_id).await?;
    let editable: bool = row.try_get("editable")?;
    if !editable {
        return Err(AppError::bad_request(
            "SKILL_FILE_READ_ONLY",
            "Only Markdown files can be edited",
        ));
    }
    let path: String = row.try_get("path")?;
    let (content, root_description) = if path == "SKILL.md" {
        let description = validate_skill_description(description.as_deref())?;
        let name: String = sqlx::query_scalar("SELECT name FROM skills WHERE tenant_id=? AND id=?")
            .bind(actor.tenant_id)
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
        (
            render_skill_document(&name, &description, &content)?,
            Some(description),
        )
    } else {
        (content, None)
    };
    if content.len() > MAX_FILE_SIZE {
        return Err(AppError::bad_request(
            "SKILL_FILE_TOO_LARGE",
            "Skill file exceeds 20 MiB",
        ));
    }
    ensure_capacity(
        &state,
        actor.tenant_id,
        id,
        content.len() as i64 - row.try_get::<u64, _>("size_bytes")? as i64,
    )
    .await?;
    let artifact = put_artifact(
        &state,
        actor.tenant_id,
        "text/markdown; charset=utf-8",
        content.into_bytes(),
    )
    .await?;
    let mut tx = state.pool.begin().await?;
    let revision = bump_revision(&mut tx, actor.tenant_id, id, expected_revision).await?;
    sqlx::query("UPDATE skill_workspace_entries SET artifact_id=?,content_hash=?,size_bytes=? WHERE tenant_id=? AND skill_id=? AND id=?").bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).bind(actor.tenant_id).bind(id).bind(entry_id).execute(&mut *tx).await?;
    if let Some(description) = root_description {
        sqlx::query("UPDATE skills SET description=?,version=version+1 WHERE tenant_id=? AND id=?")
            .bind(description)
            .bind(actor.tenant_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    insert_file_revision(&mut tx, &actor, id, entry_id, revision, &artifact).await?;
    audit(
        &mut tx,
        &actor,
        "skill.markdown_updated",
        "skill",
        id,
        json!({"entryId":entry_id,"revision":revision}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(SkillFileContentResponse {
        entry_id,
        content: String::from_utf8(artifact.content).map_err(AppError::internal)?,
        content_hash: artifact.sha256,
        revision,
    }))
}

#[utoipa::path(post,path="/api/v1/skills/{id}/uploads",params(("id"=Uuid,Path)))]
pub async fn upload_file(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<ArtifactUploadResponse>)> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let mut parent_id = None;
    let mut expected = None;
    let mut upload = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("INVALID_MULTIPART", "Upload is invalid"))?
    {
        match field.name() {
            Some("parentId") => {
                let text = field.text().await.map_err(AppError::internal)?;
                if !text.is_empty() {
                    parent_id = Some(Uuid::parse_str(&text).map_err(|_| {
                        AppError::bad_request("INVALID_PARENT", "Parent is invalid")
                    })?);
                }
            }
            Some("expectedRevision") => {
                expected = Some(
                    field
                        .text()
                        .await
                        .map_err(AppError::internal)?
                        .parse::<u64>()
                        .map_err(|_| {
                            AppError::bad_request("INVALID_REVISION", "Revision is invalid")
                        })?,
                )
            }
            Some("file") if upload.is_none() => {
                let name = validate_entry_name(field.file_name().unwrap_or("file"), "file")?;
                let mime = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_owned();
                let bytes = field.bytes().await.map_err(AppError::internal)?;
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(AppError::bad_request(
                        "SKILL_FILE_TOO_LARGE",
                        "Skill file exceeds 20 MiB",
                    ));
                }
                upload = Some((name, mime, bytes.to_vec()));
            }
            _ => {}
        }
    }
    let (name, mime, bytes) = upload
        .ok_or_else(|| AppError::bad_request("FILE_REQUIRED", "Multipart file is required"))?;
    let expected = expected.ok_or_else(|| {
        AppError::bad_request("REVISION_REQUIRED", "Expected revision is required")
    })?;
    ensure_capacity(&state, actor.tenant_id, id, bytes.len() as i64).await?;
    let parent = parent_path(&state, actor.tenant_id, id, parent_id).await?;
    let path = join_path(&parent, &name)?;
    let artifact = put_artifact(&state, actor.tenant_id, &mime, bytes).await?;
    let entry_id = Uuid::now_v7();
    let editable = name.to_ascii_lowercase().ends_with(".md");
    let mut tx = state.pool.begin().await?;
    let revision = bump_revision(&mut tx, actor.tenant_id, id, expected).await?;
    sqlx::query("INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,parent_id,name,path,path_hash,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable) VALUES(?,?,?,?,?,?,?,'file',?,?,?,?,?)").bind(entry_id).bind(actor.tenant_id).bind(id).bind(parent_id).bind(name).bind(&path).bind(path_hash(&path)).bind(&mime).bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).bind(editable).execute(&mut *tx).await?;
    insert_file_revision(&mut tx, &actor, id, entry_id, revision, &artifact).await?;
    audit(
        &mut tx,
        &actor,
        "skill.file_uploaded",
        "skill",
        id,
        json!({"path":path,"sizeBytes":artifact.content.len()}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(ArtifactUploadResponse {
            entry: load_entry(&state, actor.tenant_id, id, entry_id).await?,
            revision,
        }),
    ))
}

#[utoipa::path(get,path="/api/v1/skills/{id}/versions",params(("id"=Uuid,Path)))]
pub async fn list_versions(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<SkillVersionResponse>>> {
    actor.require("skill:view")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let rows=sqlx::query("SELECT id,skill_id,version_number,source_revision,manifest_json,content_hash,created_at,(SELECT COUNT(*) FROM skill_version_files f WHERE f.skill_version_id=sv.id) file_count FROM skill_versions sv WHERE tenant_id=? AND skill_id=? ORDER BY version_number DESC").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let mut values = Vec::new();
    for r in rows {
        values.push(version_from_row(&state, r).await?);
    }
    Ok(Json(values))
}

#[utoipa::path(operation_id="create_skill_version",post,path="/api/v1/skills/{id}/versions",request_body=PublishSkillVersionRequest,params(("id"=Uuid,Path)))]
pub async fn create_version(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<PublishSkillVersionRequest>,
) -> AppResult<(StatusCode, Json<SkillVersionResponse>)> {
    actor.require("skill:manage")?;
    require_resource_visible(&state, &actor, "skill", id).await?;
    let skill = load_skill(&state, actor.tenant_id, id).await?;
    if skill.draft_revision != input.expected_revision {
        return Err(AppError::conflict(
            "SKILL_REVISION_CONFLICT",
            "Skill workspace changed",
        ));
    }
    validate_dependencies(&state, &actor, id, &input.dependencies).await?;
    let workspace = load_workspace(&state, actor.tenant_id, id).await?;
    let files: Vec<_> = workspace
        .entries
        .iter()
        .filter(|e| e.entry_type == "file")
        .cloned()
        .collect();
    if !files.iter().any(|e| e.path == "SKILL.md") {
        return Err(AppError::unprocessable(
            "SKILL_ROOT_REQUIRED",
            "Root SKILL.md is required",
        ));
    }
    let root = files
        .iter()
        .find(|entry| entry.path == "SKILL.md")
        .expect("root SKILL.md checked");
    let root = get_artifact(
        &state,
        actor.tenant_id,
        root.artifact_id.expect("root file artifact"),
    )
    .await?;
    let root = String::from_utf8(root.content).map_err(|_| {
        AppError::unprocessable("SKILL_MARKDOWN_UTF8", "Markdown files must use UTF-8")
    })?;
    let root = parse_skill_document(&root)?;
    require_skill_document_matches(&root, &skill.name, skill.description.as_deref())?;
    let index: HashMap<_, _> = files.iter().map(|e| (e.path.clone(), e.clone())).collect();
    let mut references = Vec::new();
    for file in files.iter().filter(|e| e.editable) {
        let artifact = get_artifact(
            &state,
            actor.tenant_id,
            file.artifact_id.expect("file artifact"),
        )
        .await?;
        let text = String::from_utf8(artifact.content).map_err(|_| {
            AppError::unprocessable("SKILL_MARKDOWN_UTF8", "Markdown files must use UTF-8")
        })?;
        for (target, kind) in markdown_references(&file.path, &text)? {
            let target_entry = index.get(&target).ok_or_else(|| {
                AppError::unprocessable(
                    "SKILL_REFERENCE_MISSING",
                    format!("{} references missing file {target}", file.path),
                )
            })?;
            references.push((
                file.path.clone(),
                target,
                target_entry.content_hash.clone().unwrap_or_default(),
                kind,
            ));
        }
    }
    let manifest = json!({"name":skill.name,"description":skill.description,"sourceRevision":skill.draft_revision});
    let hash=canonical_content_hash(&json!({"manifest":manifest,"files":files.iter().map(|f|json!({"path":f.path,"hash":f.content_hash})).collect::<Vec<_>>(),"dependencies":input.dependencies})).map_err(AppError::internal)?;
    let mut tx = state.pool.begin().await?;
    let next:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(version_number),0)+1 AS UNSIGNED) FROM skill_versions WHERE tenant_id=? AND skill_id=? FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_one(&mut *tx).await?;
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO skill_versions(id,tenant_id,skill_id,version_number,source_revision,manifest_json,content_hash,created_by) VALUES(?,?,?,?,?,?,?,?)").bind(version_id).bind(actor.tenant_id).bind(id).bind(next).bind(skill.draft_revision).bind(&manifest).bind(&hash).bind(actor.user_id).execute(&mut *tx).await?;
    for file in &files {
        sqlx::query("INSERT INTO skill_version_files(id,tenant_id,skill_version_id,path,path_hash,mime_type,artifact_id,content_hash,size_bytes) VALUES(?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(version_id).bind(&file.path).bind(path_hash(&file.path)).bind(file.mime_type.as_deref().unwrap_or("application/octet-stream")).bind(file.artifact_id).bind(file.content_hash.as_deref().unwrap_or("")).bind(file.size_bytes).execute(&mut *tx).await?;
    }
    for (source, target, target_hash, kind) in references {
        sqlx::query("INSERT INTO skill_file_references(id,tenant_id,skill_version_id,source_path,target_path,target_path_hash,target_content_hash,reference_type) VALUES(?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(version_id).bind(source).bind(&target).bind(path_hash(&target)).bind(target_hash).bind(kind).execute(&mut *tx).await?;
    }
    for dep in &input.dependencies {
        sqlx::query("INSERT INTO skill_dependencies(id,tenant_id,skill_version_id,resource_type,resource_id,resource_version_id,operation_key) VALUES(?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(version_id).bind(&dep.resource_type).bind(dep.resource_id).bind(dep.resource_version_id).bind(&dep.operation).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE skills SET status='active',version=version+1 WHERE tenant_id=? AND id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &actor,
        "skill.version_published",
        "skill",
        id,
        json!({"versionId":version_id,"versionNumber":next,"fileCount":files.len()}),
    )
    .await?;
    tx.commit().await?;
    let row=sqlx::query("SELECT id,skill_id,version_number,source_revision,manifest_json,content_hash,created_at,(SELECT COUNT(*) FROM skill_version_files f WHERE f.skill_version_id=sv.id) file_count FROM skill_versions sv WHERE id=?").bind(version_id).fetch_one(&state.pool).await?;
    Ok((
        StatusCode::CREATED,
        Json(version_from_row(&state, row).await?),
    ))
}

async fn put_artifact(
    state: &AppState,
    tenant: Uuid,
    mime: &str,
    content: Vec<u8>,
) -> AppResult<agentx_application::ArtifactRead> {
    let store = state.object_store.clone().ok_or_else(|| {
        AppError::service_unavailable(
            "ARTIFACT_STORE_UNAVAILABLE",
            "Artifact storage is unavailable",
        )
    })?;
    let mut artifacts = MySqlObjectArtifactStore::new(state.pool.clone(), store);
    if let Some(redis) = state.redis.as_ref() {
        artifacts = artifacts.with_quota_admission(
            agentx_infrastructure::quota::QuotaAdmission::new((**redis).clone()),
        );
    }
    artifacts
        .put(ArtifactWrite {
            tenant_id: TenantId::from_uuid(tenant),
            content_type: mime.to_owned(),
            content,
        })
        .await
        .map_err(AppError::internal)
}

#[derive(Debug)]
struct ImportedWorkspaceEntry {
    path: String,
    mime_type: String,
    content: Option<Vec<u8>>,
}

fn parse_workspace_zip(bytes: &[u8]) -> AppResult<Vec<ImportedWorkspaceEntry>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| AppError::bad_request("INVALID_WORKSPACE_ZIP", "Workspace ZIP is invalid"))?;
    let mut entries = HashMap::<String, ImportedWorkspaceEntry>::new();
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| {
            AppError::bad_request("INVALID_WORKSPACE_ZIP", "Workspace ZIP entry is invalid")
        })?;
        let raw = file.name().trim_end_matches('/');
        if raw.is_empty() {
            continue;
        }
        let path = validate_archive_path(raw)?;
        for parent in parent_paths(&path) {
            entries
                .entry(parent.clone())
                .or_insert(ImportedWorkspaceEntry {
                    path: parent,
                    mime_type: String::new(),
                    content: None,
                });
        }
        if file.is_dir() {
            entries
                .entry(path.clone())
                .or_insert(ImportedWorkspaceEntry {
                    path,
                    mime_type: String::new(),
                    content: None,
                });
            continue;
        }
        if file.size() > MAX_FILE_SIZE as u64 {
            return Err(AppError::unprocessable(
                "SKILL_FILE_TOO_LARGE",
                "Skill file exceeds 20 MiB",
            ));
        }
        total_size = total_size.saturating_add(file.size());
        if total_size > MAX_WORKSPACE_SIZE {
            return Err(AppError::unprocessable(
                "SKILL_WORKSPACE_TOO_LARGE",
                "Skill workspace exceeds 100 MiB",
            ));
        }
        let mut content = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut content).map_err(|_| {
            AppError::bad_request(
                "INVALID_WORKSPACE_ZIP",
                "Workspace ZIP entry cannot be read",
            )
        })?;
        if entries
            .insert(
                path.clone(),
                ImportedWorkspaceEntry {
                    mime_type: content_type_for_path(&path),
                    path,
                    content: Some(content),
                },
            )
            .is_some()
        {
            return Err(AppError::bad_request(
                "SKILL_DUPLICATE_PATH",
                "Workspace ZIP contains duplicate paths",
            ));
        }
    }
    if entries.len() > MAX_ENTRIES as usize {
        return Err(AppError::unprocessable(
            "SKILL_ENTRY_LIMIT",
            "Skill workspace contains more than 1000 entries",
        ));
    }
    if !matches!(entries.get("SKILL.md"), Some(entry) if entry.content.is_some()) {
        return Err(AppError::unprocessable(
            "SKILL_ROOT_REQUIRED",
            "Workspace ZIP must contain a root SKILL.md file",
        ));
    }
    let mut values = entries.into_values().collect::<Vec<_>>();
    values.sort_by_key(|entry| {
        (
            entry.path.split('/').count(),
            entry.content.is_some(),
            entry.path.clone(),
        )
    });
    Ok(values)
}

fn build_workspace_zip(files: Vec<(String, String, Option<Vec<u8>>)>) -> AppResult<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, kind, content) in files {
        if kind == "directory" {
            writer
                .add_directory(format!("{path}/"), options)
                .map_err(AppError::internal)?;
        } else {
            writer
                .start_file(path, options)
                .map_err(AppError::internal)?;
            writer
                .write_all(content.as_deref().unwrap_or_default())
                .map_err(AppError::internal)?;
        }
    }
    Ok(writer.finish().map_err(AppError::internal)?.into_inner())
}

fn validate_archive_path(value: &str) -> AppResult<String> {
    if value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(AppError::bad_request(
            "SKILL_PATH_INVALID",
            "Workspace ZIP path is invalid",
        ));
    }
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > MAX_DEPTH
        || parts
            .iter()
            .any(|part| part.is_empty() || matches!(*part, "." | "..") || part.len() > 255)
    {
        return Err(AppError::bad_request(
            "SKILL_PATH_INVALID",
            "Workspace ZIP path is invalid",
        ));
    }
    let path = parts.join("/");
    if path.len() > 2048 {
        return Err(AppError::bad_request(
            "SKILL_PATH_TOO_LONG",
            "Skill path is too long",
        ));
    }
    Ok(path)
}

fn parent_paths(path: &str) -> Vec<String> {
    let mut parents = Vec::new();
    let mut current = path;
    while let Some((parent, _)) = current.rsplit_once('/') {
        parents.push(parent.to_owned());
        current = parent;
    }
    parents.reverse();
    parents
}

fn content_type_for_path(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".md") || lower.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
    .to_owned()
}
async fn get_artifact(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<agentx_application::ArtifactRead> {
    let store = state.object_store.clone().ok_or_else(|| {
        AppError::service_unavailable(
            "ARTIFACT_STORE_UNAVAILABLE",
            "Artifact storage is unavailable",
        )
    })?;
    MySqlObjectArtifactStore::new(state.pool.clone(), store)
        .get(TenantId::from_uuid(tenant), ArtifactId::from_uuid(id))
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::not_found("Artifact"))
}
async fn insert_file_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    actor: &AuthActor,
    skill: Uuid,
    entry: Uuid,
    revision: u64,
    artifact: &agentx_application::ArtifactRead,
) -> AppResult<()> {
    sqlx::query("INSERT INTO skill_file_revisions(id,tenant_id,skill_id,entry_id,workspace_revision,artifact_id,content_hash,size_bytes,created_by) VALUES(?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(skill).bind(entry).bind(revision).bind(artifact.id.as_uuid()).bind(&artifact.sha256).bind(artifact.content.len() as u64).bind(actor.user_id).execute(&mut **tx).await?;
    Ok(())
}
async fn bump_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant: Uuid,
    skill: Uuid,
    expected: u64,
) -> AppResult<u64> {
    let result=sqlx::query("UPDATE skills SET draft_revision=draft_revision+1 WHERE tenant_id=? AND id=? AND draft_revision=?").bind(tenant).bind(skill).bind(expected).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "SKILL_REVISION_CONFLICT",
            "Skill workspace changed",
        ));
    }
    Ok(expected + 1)
}
async fn load_skill(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<SkillResponse> {
    let sql = format!("SELECT {SKILL_COLUMNS} FROM skills s WHERE s.tenant_id=? AND s.id=?");
    let row = sqlx::query(&sql)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Skill"))?;
    skill_from_row(row).map_err(Into::into)
}
async fn load_workspace(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<SkillWorkspaceResponse> {
    let revision: Option<u64> =
        sqlx::query_scalar("SELECT draft_revision FROM skills WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let revision = revision.ok_or_else(|| AppError::not_found("Skill"))?;
    let rows=sqlx::query("SELECT id,parent_id,name,path,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable,updated_at FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? ORDER BY entry_type,path").bind(tenant).bind(id).fetch_all(&state.pool).await?;
    Ok(SkillWorkspaceResponse {
        revision,
        entries: rows
            .into_iter()
            .map(entry_from_row)
            .collect::<Result<_, _>>()?,
    })
}
async fn load_entry(
    state: &AppState,
    tenant: Uuid,
    skill: Uuid,
    id: Uuid,
) -> AppResult<SkillWorkspaceEntry> {
    entry_from_row(entry_row(state, tenant, skill, id).await?).map_err(Into::into)
}
async fn entry_row(
    state: &AppState,
    tenant: Uuid,
    skill: Uuid,
    id: Uuid,
) -> AppResult<sqlx::mysql::MySqlRow> {
    sqlx::query("SELECT id,parent_id,name,path,entry_type,mime_type,artifact_id,content_hash,size_bytes,editable,updated_at FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=? AND id=?").bind(tenant).bind(skill).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Skill entry"))
}
async fn parent_path(
    state: &AppState,
    tenant: Uuid,
    skill: Uuid,
    parent: Option<Uuid>,
) -> AppResult<String> {
    if let Some(parent) = parent {
        let row = entry_row(state, tenant, skill, parent).await?;
        let kind: String = row.try_get("entry_type")?;
        if kind != "directory" {
            return Err(AppError::bad_request(
                "SKILL_PARENT_NOT_DIRECTORY",
                "Parent must be a directory",
            ));
        }
        Ok(row.try_get("path")?)
    } else {
        Ok(String::new())
    }
}
fn join_path(parent: &str, name: &str) -> AppResult<String> {
    let value = if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    };
    if value.split('/').count() > MAX_DEPTH {
        return Err(AppError::bad_request(
            "SKILL_PATH_TOO_DEEP",
            "Skill path exceeds 20 levels",
        ));
    }
    if value.len() > 2048 {
        return Err(AppError::bad_request(
            "SKILL_PATH_TOO_LONG",
            "Skill path is too long",
        ));
    }
    Ok(value)
}
fn path_hash(path: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(path.as_bytes()))
}
fn validate_entry_name(value: &str, kind: &str) -> AppResult<String> {
    let name = value.trim();
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(AppError::bad_request(
            "INVALID_SKILL_ENTRY_NAME",
            "Skill entry name is invalid",
        ));
    }
    if !matches!(kind, "directory" | "file") {
        return Err(AppError::bad_request(
            "INVALID_SKILL_ENTRY_TYPE",
            "Skill entry type is invalid",
        ));
    }
    Ok(name.to_owned())
}
fn require_markdown_name(name: &str) -> AppResult<()> {
    if !name.to_ascii_lowercase().ends_with(".md") {
        return Err(AppError::bad_request(
            "SKILL_MARKDOWN_REQUIRED",
            "Online-created files must use the .md extension",
        ));
    }
    Ok(())
}

fn validate_skill_description(value: Option<&str>) -> AppResult<String> {
    let description = value.unwrap_or_default().trim();
    if description.is_empty() || description.chars().count() > 1000 {
        return Err(AppError::bad_request(
            "SKILL_DESCRIPTION_REQUIRED",
            "Skill description is required and must not exceed 1000 characters",
        ));
    }
    Ok(description.to_owned())
}

fn render_skill_document(name: &str, description: &str, body: &str) -> AppResult<String> {
    let metadata = SkillDocumentMetadata {
        name: name.to_owned(),
        description: validate_skill_description(Some(description))?,
    };
    let yaml = serde_yaml::to_string(&metadata)
        .map_err(AppError::internal)?
        .trim_start_matches("---\n")
        .to_owned();
    Ok(format!(
        "---\n{}---\n\n{}",
        yaml,
        body.trim_start_matches('\n')
    ))
}

fn parse_skill_document(content: &str) -> AppResult<SkillDocument> {
    let normalized = content.replace("\r\n", "\n");
    let document = normalized.strip_prefix("---\n").ok_or_else(|| {
        AppError::unprocessable(
            "SKILL_FRONTMATTER_REQUIRED",
            "SKILL.md must start with YAML frontmatter",
        )
    })?;
    let (yaml, body) = document.split_once("\n---\n").ok_or_else(|| {
        AppError::unprocessable(
            "SKILL_FRONTMATTER_INVALID",
            "SKILL.md frontmatter is not closed",
        )
    })?;
    let metadata: SkillDocumentMetadata = serde_yaml::from_str(yaml).map_err(|_| {
        AppError::unprocessable(
            "SKILL_FRONTMATTER_INVALID",
            "SKILL.md frontmatter must define name and description",
        )
    })?;
    validate_name(&metadata.name, 160)?;
    validate_skill_description(Some(&metadata.description))?;
    Ok(SkillDocument {
        metadata,
        body: body.trim_start_matches('\n').to_owned(),
    })
}

fn require_skill_document_matches(
    document: &SkillDocument,
    name: &str,
    description: Option<&str>,
) -> AppResult<()> {
    if document.metadata.name != name
        || document.metadata.description.trim() != description.unwrap_or_default().trim()
    {
        return Err(AppError::unprocessable(
            "SKILL_FRONTMATTER_MISMATCH",
            "SKILL.md name and description must match the Skill metadata",
        ));
    }
    Ok(())
}

async fn ensure_capacity(state: &AppState, tenant: Uuid, skill: Uuid, delta: i64) -> AppResult<()> {
    let row=sqlx::query("SELECT COUNT(*) entry_count,CAST(COALESCE(SUM(size_bytes),0) AS UNSIGNED) total_size FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=?").bind(tenant).bind(skill).fetch_one(&state.pool).await?;
    if row.try_get::<i64, _>("entry_count")? >= MAX_ENTRIES {
        return Err(AppError::unprocessable(
            "SKILL_ENTRY_LIMIT",
            "Skill workspace contains 1000 entries",
        ));
    }
    let current: u64 = row.try_get("total_size")?;
    let next = i128::from(current) + i128::from(delta);
    if next > i128::from(MAX_WORKSPACE_SIZE) {
        return Err(AppError::unprocessable(
            "SKILL_WORKSPACE_TOO_LARGE",
            "Skill workspace exceeds 100 MiB",
        ));
    }
    Ok(())
}
async fn rewrite_links_for_move(
    state: &AppState,
    tenant: Uuid,
    skill: Uuid,
    old_root: &str,
    new_root: &str,
) -> AppResult<Vec<(Uuid, agentx_application::ArtifactRead)>> {
    let workspace = load_workspace(state, tenant, skill).await?;
    let mut rewritten = Vec::new();
    for entry in workspace.entries.iter().filter(|entry| entry.editable) {
        let artifact = get_artifact(
            state,
            tenant,
            entry.artifact_id.expect("editable file artifact"),
        )
        .await?;
        let content = String::from_utf8(artifact.content).map_err(AppError::internal)?;
        let source_after = moved_path(&entry.path, old_root, new_root);
        if let Some(content) =
            rewrite_markdown_links(&entry.path, &source_after, &content, old_root, new_root)?
        {
            let artifact = put_artifact(
                state,
                tenant,
                "text/markdown; charset=utf-8",
                content.into_bytes(),
            )
            .await?;
            rewritten.push((entry.id, artifact));
        }
    }
    Ok(rewritten)
}

fn rewrite_markdown_links(
    source_before: &str,
    source_after: &str,
    content: &str,
    old_root: &str,
    new_root: &str,
) -> AppResult<Option<String>> {
    let arena = Arena::new();
    let options = Options::default();
    let root = parse_document(&arena, content, &options);
    let before_base = source_before.rsplit_once('/').map_or("", |(base, _)| base);
    let mut changed = false;
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        let url = match &mut data.value {
            NodeValue::Link(link) | NodeValue::Image(link) => &mut link.url,
            _ => continue,
        };
        if url.starts_with('#') || url.contains("://") || url.starts_with("mailto:") {
            continue;
        }
        let (path, suffix) = split_reference_suffix(url);
        let target_before = normalize_relative(before_base, path)?;
        let target_after = moved_path(&target_before, old_root, new_root);
        if source_after == source_before && target_after == target_before {
            continue;
        }
        *url = format!(
            "{}{}",
            relative_reference(source_after, &target_after),
            suffix
        );
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    let mut output = Vec::new();
    format_commonmark(root, &options, &mut output).map_err(AppError::internal)?;
    String::from_utf8(output)
        .map(Some)
        .map_err(AppError::internal)
}

fn moved_path(path: &str, old_root: &str, new_root: &str) -> String {
    if path == old_root {
        new_root.to_owned()
    } else if let Some(suffix) = path.strip_prefix(&format!("{old_root}/")) {
        format!("{new_root}/{suffix}")
    } else {
        path.to_owned()
    }
}

fn split_reference_suffix(value: &str) -> (&str, &str) {
    value
        .char_indices()
        .find(|(_, value)| matches!(value, '#' | '?'))
        .map_or((value, ""), |(index, _)| value.split_at(index))
}

fn relative_reference(source: &str, target: &str) -> String {
    let source_parts = source
        .rsplit_once('/')
        .map_or(Vec::new(), |(base, _)| base.split('/').collect::<Vec<_>>());
    let target_parts = target.split('/').collect::<Vec<_>>();
    let common = source_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; source_parts.len().saturating_sub(common)];
    parts.extend(target_parts[common..].iter().copied());
    if parts.is_empty() {
        target.rsplit('/').next().unwrap_or(target).to_owned()
    } else {
        parts.join("/")
    }
}
async fn workspace_references_path(
    state: &AppState,
    tenant: Uuid,
    skill: Uuid,
    target: &str,
) -> AppResult<bool> {
    let workspace = load_workspace(state, tenant, skill).await?;
    for entry in workspace.entries.iter().filter(|e| e.editable) {
        let content =
            get_artifact(state, tenant, entry.artifact_id.expect("file artifact")).await?;
        let text = String::from_utf8_lossy(&content.content);
        if markdown_references(&entry.path, &text)?
            .iter()
            .any(|(path, _)| path == target)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn markdown_references(source: &str, content: &str) -> AppResult<Vec<(String, String)>> {
    let arena = Arena::new();
    let root = parse_document(&arena, content, &Options::default());
    let base = source.rsplit_once('/').map_or("", |(value, _)| value);
    let mut values = Vec::new();
    for node in root.descendants() {
        let value = node.data.borrow();
        let (url, kind) = match &value.value {
            NodeValue::Link(link) => (link.url.as_str(), "link"),
            NodeValue::Image(link) => (link.url.as_str(), "image"),
            _ => continue,
        };
        if url.starts_with('#') || url.contains("://") || url.starts_with("mailto:") {
            continue;
        }
        let target = normalize_relative(base, url)?;
        values.push((target, kind.to_owned()));
    }
    Ok(values)
}
fn normalize_relative(base: &str, target: &str) -> AppResult<String> {
    let target = target.trim_matches(['<', '>']);
    let joined = if base.is_empty() {
        target.to_owned()
    } else {
        format!("{base}/{target}")
    };
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(AppError::unprocessable(
                        "SKILL_REFERENCE_OUTSIDE",
                        "Skill reference leaves the workspace",
                    ));
                }
            }
            value => parts.push(value),
        }
    }
    Ok(parts.join("/"))
}
async fn validate_dependencies(
    state: &AppState,
    actor: &AuthActor,
    skill: Uuid,
    deps: &[SkillDependencyInput],
) -> AppResult<()> {
    for dep in deps {
        let permission = match dep.resource_type.as_str() {
            "credential" => "credential:view",
            "model" => "model:view",
            "mcp_tool" => "mcp:view",
            "skill" => "skill:view",
            "rag" => "knowledge:view",
            "memory" => "memory:view",
            _ => {
                return Err(AppError::bad_request(
                    "INVALID_SKILL_DEPENDENCY",
                    "Skill dependency type is invalid",
                ));
            }
        };
        if !matches!(
            dep.operation.as_str(),
            "view" | "use" | "read" | "write" | "manage"
        ) {
            return Err(AppError::bad_request(
                "INVALID_SKILL_DEPENDENCY",
                "Skill dependency operation is invalid",
            ));
        }
        actor.require(permission)?;
        require_resource_visible(state, actor, &dep.resource_type, dep.resource_id).await?;
        if dep.resource_type == "skill"
            && (dep.resource_id == skill
                || skill_reaches(state, actor.tenant_id, dep.resource_id, skill).await?)
        {
            return Err(AppError::bad_request(
                "SKILL_DEPENDENCY_CYCLE",
                "Skill dependency creates a cycle",
            ));
        }
    }
    Ok(())
}
async fn skill_reaches(
    state: &AppState,
    tenant: Uuid,
    start: Uuid,
    target: Uuid,
) -> AppResult<bool> {
    let mut queue = VecDeque::from([start]);
    let mut seen = HashSet::new();
    while let Some(skill) = queue.pop_front() {
        if !seen.insert(skill) {
            continue;
        }
        if skill == target {
            return Ok(true);
        }
        let rows=sqlx::query("SELECT sd.resource_id FROM skill_dependencies sd JOIN skill_versions sv ON sv.id=sd.skill_version_id WHERE sd.tenant_id=? AND sv.skill_id=? AND sd.resource_type='skill'").bind(tenant).bind(skill).fetch_all(&state.pool).await?;
        for row in rows {
            queue.push_back(row.try_get("resource_id")?);
        }
    }
    Ok(false)
}
async fn version_from_row(
    state: &AppState,
    r: sqlx::mysql::MySqlRow,
) -> AppResult<SkillVersionResponse> {
    let id: Uuid = r.try_get("id")?;
    let deps=sqlx::query("SELECT resource_type,resource_id,resource_version_id,operation_key FROM skill_dependencies WHERE skill_version_id=? ORDER BY resource_type").bind(id).fetch_all(&state.pool).await?.into_iter().map(|d|Ok(SkillDependencyInput{resource_type:d.try_get("resource_type")?,resource_id:d.try_get("resource_id")?,resource_version_id:d.try_get("resource_version_id")?,operation:d.try_get("operation_key")?})).collect::<Result<_,sqlx::Error>>()?;
    Ok(SkillVersionResponse {
        id,
        skill_id: r.try_get("skill_id")?,
        version_number: r.try_get("version_number")?,
        source_revision: r.try_get("source_revision")?,
        manifest: r.try_get("manifest_json")?,
        content_hash: r.try_get("content_hash")?,
        file_count: r.try_get::<i64, _>("file_count")? as u64,
        dependencies: deps,
        created_at: r.try_get("created_at")?,
    })
}
fn skill_from_row(r: sqlx::mysql::MySqlRow) -> Result<SkillResponse, sqlx::Error> {
    Ok(SkillResponse {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        owner_department_id: r.try_get("owner_department_id")?,
        status: r.try_get("status")?,
        draft_revision: r.try_get("draft_revision")?,
        latest_version: r.try_get("latest_version")?,
        grant_count: r.try_get::<i64, _>("grant_count")? as u64,
        version: r.try_get("version")?,
        updated_at: r.try_get("updated_at")?,
    })
}
fn entry_from_row(r: sqlx::mysql::MySqlRow) -> Result<SkillWorkspaceEntry, sqlx::Error> {
    Ok(SkillWorkspaceEntry {
        id: r.try_get("id")?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        path: r.try_get("path")?,
        entry_type: r.try_get("entry_type")?,
        mime_type: r.try_get("mime_type")?,
        artifact_id: r.try_get("artifact_id")?,
        content_hash: r.try_get("content_hash")?,
        size_bytes: r.try_get("size_bytes")?,
        editable: r.try_get("editable")?,
        updated_at: r.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_zip(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (path, content) in entries {
            if let Some(content) = content {
                writer.start_file(*path, options).expect("start ZIP file");
                writer.write_all(content).expect("write ZIP file");
            } else {
                writer
                    .add_directory(format!("{path}/"), options)
                    .expect("add ZIP directory");
            }
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    #[test]
    fn markdown_links_follow_moved_files_and_sources() {
        let content = "[guide](docs/guide.md)\n![logo](assets/logo.png)\n";
        let rewritten = rewrite_markdown_links("SKILL.md", "SKILL.md", content, "docs", "manual")
            .expect("rewrite links")
            .expect("content changed");
        assert!(rewritten.contains("(manual/guide.md)"));
        assert!(rewritten.contains("(assets/logo.png)"));

        let moved_source = rewrite_markdown_links(
            "docs/guide.md",
            "manual/guide.md",
            "[root](../SKILL.md)\n",
            "docs",
            "manual",
        )
        .expect("rewrite moved source")
        .expect("source-relative link changed");
        assert!(moved_source.contains("(../SKILL.md)"));
    }

    #[test]
    fn workspace_zip_requires_root_skill_markdown() {
        let archive = workspace_zip(&[("README.md", Some(b"# Readme"))]);
        let error = parse_workspace_zip(&archive).expect_err("missing root must fail");
        assert_eq!(error.code, "SKILL_ROOT_REQUIRED");
    }

    #[test]
    fn workspace_zip_rejects_parent_traversal_and_duplicate_paths() {
        let traversal = workspace_zip(&[("../SKILL.md", Some(b"# Unsafe"))]);
        let error = parse_workspace_zip(&traversal).expect_err("Zip Slip must fail");
        assert_eq!(error.code, "SKILL_PATH_INVALID");

        let duplicate = workspace_zip(&[
            ("SKILL.md", Some(b"# One")),
            ("docs/guide.md", Some(b"# Guide")),
            ("docs", Some(b"conflicts with the implicit directory")),
        ]);
        let error = parse_workspace_zip(&duplicate).expect_err("duplicates must fail");
        assert_eq!(error.code, "SKILL_DUPLICATE_PATH");
    }

    #[test]
    fn workspace_zip_accepts_a_valid_workspace() {
        let archive = workspace_zip(&[
            (
                "SKILL.md",
                Some(b"---\nname: Skill\ndescription: Reusable instructions\n---\n\n# Skill\n[Guide](docs/guide.md)"),
            ),
            ("docs", None),
            ("docs/guide.md", Some(b"# Guide")),
        ]);
        let entries = parse_workspace_zip(&archive).expect("valid workspace ZIP");
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|entry| entry.path == "SKILL.md"));
    }

    #[test]
    fn skill_document_round_trips_description_and_body() {
        let content = render_skill_document(
            "browser-helper",
            "Use tools: safely and consistently",
            "# Instructions\n\nOpen the requested page.\n",
        )
        .expect("render Skill document");
        let document = parse_skill_document(&content).expect("parse Skill document");
        assert_eq!(document.metadata.name, "browser-helper");
        assert_eq!(
            document.metadata.description,
            "Use tools: safely and consistently"
        );
        assert_eq!(
            document.body,
            "# Instructions\n\nOpen the requested page.\n"
        );
    }

    #[test]
    fn skill_document_requires_a_description() {
        let error = parse_skill_document("---\nname: helper\ndescription: ''\n---\n\n# Body")
            .expect_err("empty description must fail");
        assert_eq!(error.code, "SKILL_DESCRIPTION_REQUIRED");
    }
}
