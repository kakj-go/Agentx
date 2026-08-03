use std::{collections::BTreeSet, sync::Arc};

use agentx_application::ArtifactStore;
use agentx_domain::{ArtifactId, ResourceReference, ResourceType, TenantId};
use agentx_node_protocol::{
    InvocationCancellationStatus, InvocationHandle, InvocationResourceRequest,
    InvocationResourceResponse, Item,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::credential::CredentialKeyring;

#[derive(Clone)]
pub struct InvocationBroker {
    pool: MySqlPool,
    keyring: Arc<CredentialKeyring>,
    artifacts: Arc<dyn ArtifactStore>,
    base_url: String,
    maximum_ttl: Duration,
}

#[derive(Clone, Debug)]
pub struct InvocationScope {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_token: Uuid,
    pub deadline: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct IssuedInvocation {
    pub artifact_handles: Vec<InvocationHandle>,
    pub credential_handles: Vec<InvocationHandle>,
    pub cancellation_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InvocationBrokerError {
    #[error("INVOCATION_HANDLE_INVALID")]
    Invalid,
    #[error("INVOCATION_HANDLE_EXPIRED")]
    Expired,
    #[error("INVOCATION_HANDLE_REPLAYED")]
    Replayed,
    #[error("INVOCATION_LEASE_INVALID")]
    LeaseInvalid,
    #[error("INVOCATION_RESOURCE_UNAVAILABLE")]
    ResourceUnavailable(#[source] anyhow::Error),
}

impl InvocationBroker {
    #[must_use]
    pub fn new(
        pool: MySqlPool,
        keyring: Arc<CredentialKeyring>,
        artifacts: Arc<dyn ArtifactStore>,
        base_url: String,
        maximum_ttl_seconds: i64,
    ) -> Self {
        Self {
            pool,
            keyring,
            artifacts,
            base_url: base_url.trim_end_matches('/').to_owned(),
            maximum_ttl: Duration::seconds(maximum_ttl_seconds.clamp(5, 900)),
        }
    }

    pub async fn issue<'a>(
        &self,
        scope: &InvocationScope,
        resource_references: &[ResourceReference],
        inputs: impl Iterator<Item = &'a Item>,
    ) -> Result<IssuedInvocation, InvocationBrokerError> {
        let now = OffsetDateTime::now_utc();
        let expires_at = scope.deadline.min(now + self.maximum_ttl);
        if expires_at <= now {
            return Err(InvocationBrokerError::Expired);
        }
        self.ensure_scope_lease(scope).await?;

        let credential_ids = resource_references
            .iter()
            .filter(|reference| reference.resource_type == ResourceType::Credential)
            .map(|reference| reference.resource_id)
            .collect::<BTreeSet<_>>();
        let artifact_ids = inputs
            .flat_map(|item| item.binary.values())
            .filter_map(|binary| Uuid::parse_str(&binary.artifact_handle).ok())
            .collect::<BTreeSet<_>>();
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let mut credential_handles = Vec::with_capacity(credential_ids.len());
        for resource_id in credential_ids {
            let version = sqlx::query_scalar::<_, u64>(
                "SELECT current_secret_version FROM credentials WHERE tenant_id=? AND id=? AND status='active'",
            )
            .bind(scope.tenant_id)
            .bind(resource_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(unavailable)?
            .ok_or(InvocationBrokerError::Invalid)?;
            credential_handles.push(
                insert_handle(
                    &mut transaction,
                    scope,
                    "credential",
                    Some(resource_id),
                    Some(version),
                    expires_at,
                    &self.base_url,
                )
                .await?,
            );
        }
        let mut artifact_handles = Vec::with_capacity(artifact_ids.len());
        for resource_id in artifact_ids {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM artifacts WHERE tenant_id=? AND id=? AND deleted_at IS NULL)",
            )
            .bind(scope.tenant_id)
            .bind(resource_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(unavailable)?;
            if !exists {
                return Err(InvocationBrokerError::Invalid);
            }
            artifact_handles.push(
                insert_handle(
                    &mut transaction,
                    scope,
                    "artifact",
                    Some(resource_id),
                    None,
                    expires_at,
                    &self.base_url,
                )
                .await?,
            );
        }
        let cancellation = insert_handle(
            &mut transaction,
            scope,
            "cancellation",
            None,
            None,
            expires_at,
            &self.base_url,
        )
        .await?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(IssuedInvocation {
            artifact_handles,
            credential_handles,
            cancellation_url: format!(
                "{}/agentx/runtime/v1/invocations/cancellation/{}",
                self.base_url, cancellation.handle
            ),
        })
    }

    pub async fn resolve(
        &self,
        request: &InvocationResourceRequest,
    ) -> Result<InvocationResourceResponse, InvocationBrokerError> {
        let handle_hash = token_hash(&request.handle);
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let row = sqlx::query(
            "SELECT h.handle_kind,h.resource_id,h.resource_version,h.expires_at,h.consumed_at,h.lease_token,a.status attempt_status,l.expires_at lease_expires,l.released_at,e.cancellation_requested_at,e.status execution_status FROM node_invocation_handles h JOIN node_attempts a ON a.id=h.attempt_id AND a.tenant_id=h.tenant_id JOIN workflow_executions e ON e.id=h.execution_id AND e.tenant_id=h.tenant_id LEFT JOIN worker_leases l ON l.node_attempt_id=h.attempt_id AND l.lease_token=h.lease_token WHERE h.token_hash=? AND h.tenant_id=? AND h.node_execution_id=? AND h.attempt_id=? FOR UPDATE",
        )
        .bind(&handle_hash)
        .bind(request.tenant_id.as_uuid())
        .bind(request.node_execution_id.as_uuid())
        .bind(request.attempt_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
        .ok_or(InvocationBrokerError::Invalid)?;
        let now = OffsetDateTime::now_utc();
        if row
            .try_get::<OffsetDateTime, _>("expires_at")
            .map_err(unavailable)?
            <= now
        {
            return Err(InvocationBrokerError::Expired);
        }
        if row
            .try_get::<Option<OffsetDateTime>, _>("consumed_at")
            .map_err(unavailable)?
            .is_some()
        {
            return Err(InvocationBrokerError::Replayed);
        }
        ensure_active_lease(&row, now)?;
        let kind: String = row.try_get("handle_kind").map_err(unavailable)?;
        let resource_id: Uuid = row
            .try_get::<Option<Uuid>, _>("resource_id")
            .map_err(unavailable)?
            .ok_or(InvocationBrokerError::Invalid)?;
        let response = match kind.as_str() {
            "credential" => {
                let version = row
                    .try_get::<Option<u64>, _>("resource_version")
                    .map_err(unavailable)?
                    .ok_or(InvocationBrokerError::Invalid)?;
                let secret = sqlx::query("SELECT c.credential_type,s.key_id,s.nonce,s.ciphertext FROM credentials c JOIN credential_secret_versions s ON s.credential_id=c.id AND s.tenant_id=c.tenant_id WHERE c.tenant_id=? AND c.id=? AND c.status='active' AND s.version_number=?")
                    .bind(request.tenant_id.as_uuid()).bind(resource_id).bind(version)
                    .fetch_optional(&mut *transaction).await.map_err(unavailable)?
                    .ok_or(InvocationBrokerError::Invalid)?;
                let aad = format!("{}/{}/{}", request.tenant_id, resource_id, version);
                let plaintext = self
                    .keyring
                    .decrypt(
                        &secret.try_get::<String, _>("key_id").map_err(unavailable)?,
                        &secret.try_get::<Vec<u8>, _>("nonce").map_err(unavailable)?,
                        &secret
                            .try_get::<Vec<u8>, _>("ciphertext")
                            .map_err(unavailable)?,
                        aad.as_bytes(),
                    )
                    .map_err(InvocationBrokerError::ResourceUnavailable)?;
                let value = serde_json::from_slice::<Value>(plaintext.expose())
                    .map_err(|error| InvocationBrokerError::ResourceUnavailable(error.into()))?;
                InvocationResourceResponse::Credential {
                    credential_type: secret.try_get("credential_type").map_err(unavailable)?,
                    value,
                }
            }
            "artifact" => {
                let artifact = self
                    .artifacts
                    .get(
                        TenantId::from_uuid(request.tenant_id.as_uuid()),
                        ArtifactId::from_uuid(resource_id),
                    )
                    .await
                    .map_err(InvocationBrokerError::ResourceUnavailable)?
                    .ok_or(InvocationBrokerError::Invalid)?;
                InvocationResourceResponse::Artifact {
                    content_type: artifact.content_type,
                    content_base64: STANDARD.encode(artifact.content),
                    sha256: artifact.sha256,
                }
            }
            _ => return Err(InvocationBrokerError::Invalid),
        };
        sqlx::query("UPDATE node_invocation_handles SET consumed_at=CURRENT_TIMESTAMP(6) WHERE token_hash=? AND consumed_at IS NULL")
            .bind(handle_hash).execute(&mut *transaction).await.map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(response)
    }

    pub async fn cancellation_status(
        &self,
        token: &str,
    ) -> Result<InvocationCancellationStatus, InvocationBrokerError> {
        let row = sqlx::query(
            "SELECT h.expires_at,a.status attempt_status,l.expires_at lease_expires,l.released_at,e.cancellation_requested_at,e.status execution_status FROM node_invocation_handles h JOIN node_attempts a ON a.id=h.attempt_id AND a.tenant_id=h.tenant_id JOIN workflow_executions e ON e.id=h.execution_id AND e.tenant_id=h.tenant_id LEFT JOIN worker_leases l ON l.node_attempt_id=h.attempt_id AND l.lease_token=h.lease_token WHERE h.token_hash=? AND h.handle_kind='cancellation'",
        )
        .bind(token_hash(token))
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?
        .ok_or(InvocationBrokerError::Invalid)?;
        let expires_at: OffsetDateTime = row.try_get("expires_at").map_err(unavailable)?;
        if expires_at <= OffsetDateTime::now_utc() {
            return Err(InvocationBrokerError::Expired);
        }
        let lease_valid = active_lease(&row, OffsetDateTime::now_utc())?;
        let cancellation_requested = row
            .try_get::<Option<OffsetDateTime>, _>("cancellation_requested_at")
            .map_err(unavailable)?
            .is_some()
            || matches!(
                row.try_get::<String, _>("execution_status")
                    .map_err(unavailable)?
                    .as_str(),
                "cancelled" | "failed" | "timed_out" | "succeeded"
            )
            || !lease_valid;
        Ok(InvocationCancellationStatus {
            cancellation_requested,
            lease_valid,
            expires_at,
        })
    }

    async fn ensure_scope_lease(
        &self,
        scope: &InvocationScope,
    ) -> Result<(), InvocationBrokerError> {
        let valid = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM node_attempts a JOIN worker_leases l ON l.node_attempt_id=a.id AND l.lease_token=a.lease_token JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.execution_id=? AND a.node_execution_id=? AND a.id=? AND a.lease_token=? AND a.status='running' AND n.status='running' AND e.cancellation_requested_at IS NULL AND e.status='running' AND l.released_at IS NULL AND l.expires_at>CURRENT_TIMESTAMP(6))",
        )
        .bind(scope.tenant_id)
        .bind(scope.execution_id)
        .bind(scope.node_execution_id)
        .bind(scope.attempt_id)
        .bind(scope.lease_token)
        .fetch_one(&self.pool)
        .await
        .map_err(unavailable)?;
        if valid {
            Ok(())
        } else {
            Err(InvocationBrokerError::LeaseInvalid)
        }
    }
}

async fn insert_handle(
    transaction: &mut sqlx::Transaction<'_, sqlx::MySql>,
    scope: &InvocationScope,
    kind: &str,
    resource_id: Option<Uuid>,
    resource_version: Option<u64>,
    expires_at: OffsetDateTime,
    base_url: &str,
) -> Result<InvocationHandle, InvocationBrokerError> {
    let token = Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO node_invocation_handles(id,tenant_id,execution_id,node_execution_id,attempt_id,lease_token,token_hash,handle_kind,resource_id,resource_version,expires_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.execution_id).bind(scope.node_execution_id)
        .bind(scope.attempt_id).bind(scope.lease_token).bind(token_hash(&token)).bind(kind)
        .bind(resource_id).bind(resource_version).bind(expires_at)
        .execute(&mut **transaction).await.map_err(unavailable)?;
    Ok(InvocationHandle {
        handle: token,
        broker_url: format!("{base_url}/agentx/runtime/v1/invocation-resources/resolve"),
        expires_at,
    })
}

fn ensure_active_lease(
    row: &sqlx::mysql::MySqlRow,
    now: OffsetDateTime,
) -> Result<(), InvocationBrokerError> {
    if active_lease(row, now)? {
        Ok(())
    } else {
        Err(InvocationBrokerError::LeaseInvalid)
    }
}

fn active_lease(
    row: &sqlx::mysql::MySqlRow,
    now: OffsetDateTime,
) -> Result<bool, InvocationBrokerError> {
    Ok(row
        .try_get::<String, _>("attempt_status")
        .map_err(unavailable)?
        == "running"
        && row
            .try_get::<Option<OffsetDateTime>, _>("released_at")
            .map_err(unavailable)?
            .is_none()
        && row
            .try_get::<Option<OffsetDateTime>, _>("lease_expires")
            .map_err(unavailable)?
            .is_some_and(|value| value > now)
        && row
            .try_get::<Option<OffsetDateTime>, _>("cancellation_requested_at")
            .map_err(unavailable)?
            .is_none()
        && row
            .try_get::<String, _>("execution_status")
            .map_err(unavailable)?
            == "running")
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn unavailable(error: impl Into<anyhow::Error>) -> InvocationBrokerError {
    InvocationBrokerError::ResourceUnavailable(error.into())
}
