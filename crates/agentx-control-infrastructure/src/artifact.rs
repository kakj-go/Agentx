use std::sync::Arc;

use agentx_application::{ArtifactRead, ArtifactStore, ArtifactWrite};
use agentx_domain::{ArtifactId, TenantId};
use anyhow::{Context, Result};
use async_trait::async_trait;
use object_store::{ObjectStore, path::Path};
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

pub struct MySqlControlArtifactStore {
    pool: MySqlPool,
    objects: Arc<dyn ObjectStore>,
}

impl MySqlControlArtifactStore {
    #[must_use]
    pub fn new(pool: MySqlPool, objects: Arc<dyn ObjectStore>) -> Self {
        Self { pool, objects }
    }
}

#[async_trait]
impl ArtifactStore for MySqlControlArtifactStore {
    async fn put(&self, artifact: ArtifactWrite) -> Result<ArtifactRead> {
        let id = ArtifactId::new();
        let sha256 = format!("{:x}", Sha256::digest(&artifact.content));
        let key = format!("{}/{id}", artifact.tenant_id);
        if let Err(error) = self
            .objects
            .put(&Path::from(key.clone()), artifact.content.clone().into())
            .await
        {
            return Err(error).context("failed to write Control artifact object");
        }

        let mut transaction = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO artifacts (id, tenant_id, content_type, size_bytes, sha256, storage_key) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_uuid())
        .bind(artifact.tenant_id.as_uuid())
        .bind(&artifact.content_type)
        .bind(i64::try_from(artifact.content.len())?)
        .bind(&sha256)
        .bind(&key)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = inserted {
            transaction.rollback().await?;
            let _ = self.objects.delete(&Path::from(key)).await;
            return Err(error).context("failed to persist Control artifact metadata");
        }
        transaction.commit().await?;
        Ok(ArtifactRead {
            id,
            content_type: artifact.content_type,
            content: artifact.content,
            sha256,
        })
    }

    async fn get(&self, tenant_id: TenantId, id: ArtifactId) -> Result<Option<ArtifactRead>> {
        let row = sqlx::query(
            "SELECT content_type, sha256, storage_key FROM artifacts WHERE tenant_id=? AND id=? AND deleted_at IS NULL",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .context("failed to query Control artifact metadata")?;
        let Some(row) = row else { return Ok(None) };
        let key: String = row.try_get("storage_key")?;
        let content = self
            .objects
            .get(&Path::from(key))
            .await
            .context("failed to read Control artifact object")?
            .bytes()
            .await
            .context("failed to collect Control artifact bytes")?
            .to_vec();
        Ok(Some(ArtifactRead {
            id,
            content_type: row.try_get("content_type")?,
            content,
            sha256: row.try_get("sha256")?,
        }))
    }

    async fn delete(&self, tenant_id: TenantId, id: ArtifactId) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT storage_key FROM artifacts WHERE tenant_id=? AND id=? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = row {
            anyhow::ensure!(
                artifact_reference_reason(&mut transaction, tenant_id.as_uuid(), id.as_uuid())
                    .await?
                    .is_none(),
                "ARTIFACT_REFERENCED"
            );
            let key: String = row.try_get("storage_key")?;
            self.objects.delete(&Path::from(key)).await?;
            sqlx::query(
                "UPDATE artifacts SET deleted_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=?",
            )
            .bind(tenant_id.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

async fn artifact_reference_reason(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    artifact_id: Uuid,
) -> Result<Option<String>> {
    let row = sqlx::query("SELECT CASE
        WHEN EXISTS (SELECT 1 FROM artifact_references r WHERE r.tenant_id=? AND r.artifact_id=? AND (r.retention_until IS NULL OR r.retention_until>UTC_TIMESTAMP(6))) THEN 'registered_reference'
        WHEN EXISTS (SELECT 1 FROM workflow_debug_overlays r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'debug_overlay'
        WHEN EXISTS (SELECT 1 FROM skill_workspace_entries r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_workspace'
        WHEN EXISTS (SELECT 1 FROM skill_file_revisions r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_revision'
        WHEN EXISTS (SELECT 1 FROM skill_version_files r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_version'
        ELSE NULL END AS reason")
        .bind(tenant_id).bind(artifact_id)
        .bind(tenant_id).bind(artifact_id)
        .bind(tenant_id).bind(artifact_id)
        .bind(tenant_id).bind(artifact_id)
        .bind(tenant_id).bind(artifact_id)
        .fetch_one(&mut **transaction).await?;
    Ok(row.try_get("reason")?)
}
