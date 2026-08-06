use std::sync::Arc;

use agentx_application::{ArtifactRead, ArtifactStore, ArtifactWrite};
use agentx_domain::{ArtifactId, TenantId};
use anyhow::{Context, Result};
use async_trait::async_trait;
use object_store::{ObjectStore, path::Path};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};

use crate::quota::QuotaAdmission;

pub struct MySqlObjectArtifactStore {
    pool: MySqlPool,
    objects: Arc<dyn ObjectStore>,
    quota_admission: Option<QuotaAdmission>,
}

impl MySqlObjectArtifactStore {
    #[must_use]
    pub fn new(pool: MySqlPool, objects: Arc<dyn ObjectStore>) -> Self {
        Self {
            pool,
            objects,
            quota_admission: None,
        }
    }

    #[must_use]
    pub fn with_quota_admission(mut self, admission: QuotaAdmission) -> Self {
        self.quota_admission = Some(admission);
        self
    }
}

#[async_trait]
impl ArtifactStore for MySqlObjectArtifactStore {
    async fn put(&self, artifact: ArtifactWrite) -> Result<ArtifactRead> {
        let id = ArtifactId::new();
        let size_bytes = u64::try_from(artifact.content.len())?;
        crate::quota::reserve_artifact_with_admission(
            &self.pool,
            artifact.tenant_id.as_uuid(),
            id.as_uuid(),
            size_bytes,
            self.quota_admission.as_ref(),
        )
        .await?;
        let sha256 = format!("{:x}", Sha256::digest(&artifact.content));
        let key = format!("{}/{id}", artifact.tenant_id);
        let result = self
            .objects
            .put(&Path::from(key.clone()), artifact.content.clone().into())
            .await;
        if let Err(error) = result {
            crate::quota::release_artifact_reservation_with_admission(
                &self.pool,
                artifact.tenant_id.as_uuid(),
                id.as_uuid(),
                self.quota_admission.as_ref(),
            )
            .await?;
            return Err(error).context("failed to write artifact object");
        }

        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
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

        if let Err(error) = result {
            transaction.rollback().await?;
            let _ = self.objects.delete(&Path::from(key)).await;
            crate::quota::release_artifact_reservation_with_admission(
                &self.pool,
                artifact.tenant_id.as_uuid(),
                id.as_uuid(),
                self.quota_admission.as_ref(),
            )
            .await?;
            return Err(error).context("failed to persist artifact metadata");
        }
        if let Err(error) = crate::quota::settle_artifact_with_admission(
            &mut transaction,
            artifact.tenant_id.as_uuid(),
            id.as_uuid(),
            size_bytes,
            self.quota_admission.as_ref(),
        )
        .await
        {
            transaction.rollback().await?;
            let _ = self.objects.delete(&Path::from(key)).await;
            crate::quota::release_artifact_reservation_with_admission(
                &self.pool,
                artifact.tenant_id.as_uuid(),
                id.as_uuid(),
                self.quota_admission.as_ref(),
            )
            .await?;
            return Err(error).context("failed to settle artifact quota");
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
            "SELECT content_type, sha256, storage_key FROM artifacts WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .context("failed to query artifact metadata")?;
        let Some(row) = row else { return Ok(None) };
        let key: String = row.try_get("storage_key")?;
        let content = self
            .objects
            .get(&Path::from(key))
            .await
            .context("failed to read artifact object")?
            .bytes()
            .await
            .context("failed to collect artifact bytes")?
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
            "SELECT storage_key,size_bytes FROM artifacts WHERE tenant_id = ? AND id = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = row {
            anyhow::ensure!(
                crate::retention::reference_reason(
                    &mut transaction,
                    tenant_id.as_uuid(),
                    id.as_uuid()
                )
                .await?
                .is_none(),
                "ARTIFACT_REFERENCED"
            );
            let key: String = row.try_get("storage_key")?;
            let size_bytes: u64 = row.try_get("size_bytes")?;
            self.objects.delete(&Path::from(key)).await?;
            sqlx::query("UPDATE artifacts SET deleted_at = CURRENT_TIMESTAMP(6) WHERE tenant_id = ? AND id = ?")
                .bind(tenant_id.as_uuid())
                .bind(id.as_uuid())
                .execute(&mut *transaction)
                .await?;
            crate::quota::record_artifact_deletion(
                &mut transaction,
                tenant_id.as_uuid(),
                id.as_uuid(),
                size_bytes,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
