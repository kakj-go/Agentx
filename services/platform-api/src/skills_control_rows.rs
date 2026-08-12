use sqlx::Row;
use uuid::Uuid;

use crate::{error::AppResult, state::AppState};

use super::{SkillDependencyInput, SkillResponse, SkillVersionResponse, SkillWorkspaceEntry};

pub(super) async fn version_from_row(
    state: &AppState,
    row: sqlx::mysql::MySqlRow,
) -> AppResult<SkillVersionResponse> {
    let id: Uuid = row.try_get("id")?;
    let dependencies = sqlx::query(
        "SELECT resource_type,resource_id,resource_version_id,operation_key \
         FROM skill_dependencies WHERE skill_version_id=? ORDER BY resource_type",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|dependency| {
        Ok(SkillDependencyInput {
            resource_type: dependency.try_get("resource_type")?,
            resource_id: dependency.try_get("resource_id")?,
            resource_version_id: dependency.try_get("resource_version_id")?,
            operation: dependency.try_get("operation_key")?,
        })
    })
    .collect::<Result<_, sqlx::Error>>()?;

    Ok(SkillVersionResponse {
        id,
        skill_id: row.try_get("skill_id")?,
        version_number: row.try_get("version_number")?,
        source_revision: row.try_get("source_revision")?,
        manifest: row.try_get("manifest_json")?,
        content_hash: row.try_get("content_hash")?,
        file_count: row.try_get::<i64, _>("file_count")? as u64,
        dependencies,
        created_at: row.try_get("created_at")?,
    })
}

pub(super) fn skill_from_row(row: sqlx::mysql::MySqlRow) -> Result<SkillResponse, sqlx::Error> {
    Ok(SkillResponse {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        alias: row.try_get("alias")?,
        description: row.try_get("description")?,
        owner_department_id: row.try_get("owner_department_id")?,
        status: row.try_get("status")?,
        draft_revision: row.try_get("draft_revision")?,
        latest_version: row.try_get("latest_version")?,
        grant_count: row.try_get::<i64, _>("grant_count")? as u64,
        version: row.try_get("version")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn entry_from_row(
    row: sqlx::mysql::MySqlRow,
) -> Result<SkillWorkspaceEntry, sqlx::Error> {
    Ok(SkillWorkspaceEntry {
        id: row.try_get("id")?,
        parent_id: row.try_get("parent_id")?,
        name: row.try_get("name")?,
        path: row.try_get("path")?,
        entry_type: row.try_get("entry_type")?,
        mime_type: row.try_get("mime_type")?,
        artifact_id: row.try_get("artifact_id")?,
        content_hash: row.try_get("content_hash")?,
        size_bytes: row.try_get("size_bytes")?,
        editable: row.try_get("editable")?,
        updated_at: row.try_get("updated_at")?,
    })
}
