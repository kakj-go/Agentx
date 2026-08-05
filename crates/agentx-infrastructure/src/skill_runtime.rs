use std::{collections::BTreeSet, sync::Arc};

use agentx_application::{
    ArtifactStore, RuntimeContext, RuntimeError, RuntimeResult, SkillBundle, SkillFile,
    SkillRuntime,
};
use agentx_domain::{ArtifactId, ResourceReference};
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime_resources::MySqlResourceAuthorizer;

#[derive(Clone)]
pub struct SnapshotSkillRuntime {
    authorizer: MySqlResourceAuthorizer,
    artifacts: Arc<dyn ArtifactStore>,
}

impl SnapshotSkillRuntime {
    #[must_use]
    pub fn new(authorizer: MySqlResourceAuthorizer, artifacts: Arc<dyn ArtifactStore>) -> Self {
        Self {
            authorizer,
            artifacts,
        }
    }
}

#[async_trait]
impl SkillRuntime for SnapshotSkillRuntime {
    async fn load(
        &self,
        context: &RuntimeContext,
        resource: ResourceReference,
    ) -> RuntimeResult<SkillBundle> {
        self.authorizer
            .authorize_context(context, &resource)
            .await?;
        let entry = context.resource(&resource).ok_or_else(|| {
            RuntimeError::new("RESOURCE_SNAPSHOT_MISSING", "Skill snapshot is missing")
        })?;
        let files = entry
            .snapshot
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                RuntimeError::new("SKILL_SNAPSHOT_INVALID", "Skill file snapshot is missing")
            })?;
        let mut result = Vec::with_capacity(files.len());
        let mut paths = BTreeSet::new();
        let mut instructions = entry
            .snapshot
            .get("manifest")
            .and_then(|v| v.get("instructions"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        for file in files {
            let id = ArtifactId::from_uuid(
                uuid::Uuid::parse_str(file.get("artifactId").and_then(Value::as_str).ok_or_else(
                    || RuntimeError::new("SKILL_SNAPSHOT_INVALID", "Skill Artifact ID is missing"),
                )?)
                .map_err(|_| {
                    RuntimeError::new("SKILL_SNAPSHOT_INVALID", "Skill Artifact ID is invalid")
                })?,
            );
            let artifact = self
                .artifacts
                .get(context.tenant_id, id)
                .await
                .map_err(|e| {
                    RuntimeError::new("SKILL_ARTIFACT_UNAVAILABLE", e.to_string()).retryable(true)
                })?
                .ok_or_else(|| {
                    RuntimeError::new("SKILL_ARTIFACT_MISSING", "Skill Artifact is missing")
                })?;
            let expected = file
                .get("contentHash")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_start_matches("sha256:");
            if artifact.sha256 != expected {
                return Err(RuntimeError::new(
                    "SKILL_CONTENT_HASH_MISMATCH",
                    "Skill file content does not match its published snapshot",
                ));
            }
            let path = file.get("path").and_then(Value::as_str).ok_or_else(|| {
                RuntimeError::new("SKILL_SNAPSHOT_INVALID", "Skill file path is missing")
            })?;
            validate_skill_path(path)?;
            if !paths.insert(path.to_owned()) {
                return Err(RuntimeError::new(
                    "SKILL_SNAPSHOT_INVALID",
                    "Skill snapshot contains duplicate file paths",
                ));
            }
            if instructions.is_none()
                && (path.eq_ignore_ascii_case("SKILL.md") || path.ends_with("/SKILL.md"))
            {
                instructions = Some(String::from_utf8(artifact.content.clone()).map_err(|_| {
                    RuntimeError::new("SKILL_CONTENT_INVALID", "SKILL.md is not UTF-8")
                })?);
            }
            result.push(SkillFile {
                path: path.to_owned(),
                artifact_id: id,
                content_hash: artifact.sha256,
                mime_type: file
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("application/octet-stream")
                    .to_owned(),
            });
        }
        let dependencies = context
            .resources
            .iter()
            .filter(|candidate| {
                candidate.node_id == entry.node_id
                    && candidate.reference.resource_id != resource.resource_id
            })
            .cloned()
            .collect::<Vec<_>>();
        for dependency in &dependencies {
            self.authorizer
                .authorize_context(context, &dependency.reference)
                .await?;
        }
        Ok(SkillBundle {
            instructions: instructions.ok_or_else(|| {
                RuntimeError::new(
                    "SKILL_INSTRUCTIONS_MISSING",
                    "Skill has no instructions or SKILL.md",
                )
            })?,
            files: result,
            dependencies,
        })
    }
}

fn validate_skill_path(path: &str) -> RuntimeResult<()> {
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(RuntimeError::new(
            "SKILL_PATH_FORBIDDEN",
            "Skill file path must be a normalized relative workspace path",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_skill_path;

    #[test]
    fn skill_paths_stay_inside_the_published_workspace() {
        assert!(validate_skill_path("SKILL.md").is_ok());
        assert!(validate_skill_path("assets/reference.json").is_ok());
        assert!(validate_skill_path("../secret").is_err());
        assert!(validate_skill_path("assets//secret").is_err());
        assert!(validate_skill_path("C:\\secret").is_err());
        assert!(validate_skill_path("/workspace/secret").is_err());
    }
}
