use std::collections::{BTreeMap, BTreeSet};

use agentx_application::{RuntimeContext, RuntimeResourceSnapshot, SandboxLease};
use agentx_domain::{
    AttemptId, ExecutionId, NodeExecutionId, TenantId, TraceId, WorkflowId,
    WorkflowServiceIdentityId, WorkflowVersionId,
};
use agentx_infrastructure::{
    OpenSandboxAdapter,
    credential::{CredentialKeyring, PlainSecret},
};
use agentx_runtime_rpc::sandbox_v1::{
    SandboxCredentialHandle, SandboxLease as RpcLease, SandboxScope,
};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct LeaseStore {
    pool: MySqlPool,
    signing_key: SecretString,
    endpoint_keyring: CredentialKeyring,
    credential_keyring: CredentialKeyring,
    max_active_per_tenant: u64,
}

pub struct ValidLease {
    pub context: RuntimeContext,
    pub lease: SandboxLease,
    pub status: String,
}

pub struct ExistingLease {
    pub id: Uuid,
    pub sandbox_id: Option<String>,
    pub expires_at: OffsetDateTime,
    pub status: String,
    pub attempt_id: Uuid,
    pub idempotency_key: String,
}

pub struct ResolvedSandboxCredential {
    pub handle_id: Uuid,
    pub environment_name: String,
    pub secret: PlainSecret,
}

impl LeaseStore {
    pub fn new(
        pool: MySqlPool,
        signing_key: SecretString,
        endpoint_keyring: CredentialKeyring,
        credential_keyring: CredentialKeyring,
        max_active_per_tenant: u64,
    ) -> Self {
        Self {
            pool,
            signing_key,
            endpoint_keyring,
            credential_keyring,
            max_active_per_tenant,
        }
    }

    pub async fn validate_scope(&self, scope: &SandboxScope) -> Result<RuntimeContext> {
        self.load_scope(scope, true).await
    }

    async fn validate_cleanup_scope(&self, scope: &SandboxScope) -> Result<RuntimeContext> {
        self.load_scope(scope, false).await
    }

    async fn load_scope(
        &self,
        scope: &SandboxScope,
        require_active: bool,
    ) -> Result<RuntimeContext> {
        let tenant = parse_uuid(&scope.tenant_id, "tenant_id")?;
        let execution = parse_uuid(&scope.execution_id, "execution_id")?;
        let node = parse_uuid(&scope.node_execution_id, "node_execution_id")?;
        let attempt = parse_uuid(&scope.attempt_id, "attempt_id")?;
        let worker_lease = parse_uuid(&scope.worker_lease_token, "worker_lease_token")?;
        let query = if require_active {
            "SELECT e.workflow_id,e.workflow_version_id,e.trace_id,s.resource_snapshot_json FROM worker_leases wl JOIN node_attempts a ON a.id=wl.node_attempt_id AND a.tenant_id=wl.tenant_id JOIN workflow_executions e ON e.id=a.execution_id AND e.tenant_id=a.tenant_id JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id WHERE wl.tenant_id=? AND wl.node_execution_id=? AND wl.node_attempt_id=? AND wl.lease_token=? AND wl.released_at IS NULL AND wl.expires_at>CURRENT_TIMESTAMP(6) AND a.execution_id=? AND a.status='running'"
        } else {
            "SELECT e.workflow_id,e.workflow_version_id,e.trace_id,s.resource_snapshot_json FROM worker_leases wl JOIN node_attempts a ON a.id=wl.node_attempt_id AND a.tenant_id=wl.tenant_id JOIN workflow_executions e ON e.id=a.execution_id AND e.tenant_id=a.tenant_id JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id WHERE wl.tenant_id=? AND wl.node_execution_id=? AND wl.node_attempt_id=? AND wl.lease_token=? AND a.execution_id=?"
        };
        let row = sqlx::query(query)
            .bind(tenant)
            .bind(node)
            .bind(attempt)
            .bind(worker_lease)
            .bind(execution)
            .fetch_optional(&self.pool)
            .await?
            .context("SANDBOX_SCOPE_LEASE_INVALID")?;
        let workflow_id: Uuid = row.try_get("workflow_id")?;
        let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
        if !scope.workflow_id.is_empty()
            && parse_uuid(&scope.workflow_id, "workflow_id")? != workflow_id
        {
            anyhow::bail!("SANDBOX_SCOPE_WORKFLOW_MISMATCH");
        }
        if !scope.workflow_version_id.is_empty()
            && parse_uuid(&scope.workflow_version_id, "workflow_version_id")? != workflow_version_id
        {
            anyhow::bail!("SANDBOX_SCOPE_WORKFLOW_VERSION_MISMATCH");
        }
        let snapshot: Value = row.try_get("resource_snapshot_json")?;
        let identity = snapshot
            .get("workflowServiceIdentityId")
            .and_then(Value::as_str)
            .context("SANDBOX_SCOPE_IDENTITY_MISSING")
            .and_then(|value| Uuid::parse_str(value).context("SANDBOX_SCOPE_IDENTITY_INVALID"))?;
        let deadline =
            OffsetDateTime::from_unix_timestamp_nanos(scope.deadline_unix_ms as i128 * 1_000_000)
                .unwrap_or_else(|_| OffsetDateTime::now_utc() + time::Duration::minutes(5));
        Ok(RuntimeContext {
            tenant_id: TenantId::from_uuid(tenant),
            workflow_service_identity_id: WorkflowServiceIdentityId::from_uuid(identity),
            workflow_id: WorkflowId::from_uuid(workflow_id),
            workflow_version_id: WorkflowVersionId::from_uuid(workflow_version_id),
            execution_id: ExecutionId::from_uuid(execution),
            node_execution_id: NodeExecutionId::from_uuid(node),
            attempt_id: AttemptId::from_uuid(attempt),
            lease_token: worker_lease,
            trace_id: TraceId::from_uuid(row.try_get("trace_id")?),
            span_id: Uuid::now_v7(),
            deadline,
            cancellation: CancellationToken::new(),
            idempotency_key: scope.request_id.clone(),
            resources: Vec::new(),
        })
    }

    pub async fn existing(
        &self,
        context: &RuntimeContext,
        idempotency_key: &str,
    ) -> Result<Option<ExistingLease>> {
        let row = sqlx::query("SELECT id,sandbox_id,expires_at,status,attempt_id,idempotency_key FROM sandbox_leases WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND attempt_id=? AND worker_lease_token=? AND idempotency_key=?")
            .bind(context.tenant_id.as_uuid()).bind(context.execution_id.as_uuid()).bind(context.node_execution_id.as_uuid()).bind(context.attempt_id.as_uuid()).bind(context.lease_token).bind(idempotency_key).fetch_optional(&self.pool).await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(ExistingLease {
            id: row.try_get("id")?,
            sandbox_id: row.try_get("sandbox_id")?,
            expires_at: row.try_get("expires_at")?,
            status: row.try_get("status")?,
            attempt_id: row.try_get("attempt_id")?,
            idempotency_key: row.try_get("idempotency_key")?,
        }))
    }

    pub async fn insert_creating(
        &self,
        context: &RuntimeContext,
        profile_version: Uuid,
        idempotency_key: &str,
        expires_at: OffsetDateTime,
    ) -> Result<Uuid> {
        let id = Uuid::now_v7();
        let token = self.lease_token(id);
        let token_hash = hash(&token);
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT id FROM tenants WHERE id=? FOR UPDATE")
            .bind(context.tenant_id.as_uuid())
            .fetch_optional(&mut *transaction)
            .await?
            .context("SANDBOX_TENANT_NOT_FOUND")?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sandbox_leases WHERE tenant_id=? AND status IN ('creating','ready','running','interrupting','terminating','orphaned')")
            .bind(context.tenant_id.as_uuid())
            .fetch_one(&mut *transaction)
            .await?;
        if u64::try_from(active)? >= self.max_active_per_tenant {
            anyhow::bail!("SANDBOX_TENANT_CONCURRENCY_EXCEEDED");
        }
        sqlx::query("INSERT INTO sandbox_leases(id,tenant_id,execution_id,node_execution_id,attempt_id,worker_lease_token,lease_token_hash,profile_version_id,idempotency_key,status,expires_at) VALUES(?,?,?,?,?,?,?,?,?,'creating',?)")
            .bind(id).bind(context.tenant_id.as_uuid()).bind(context.execution_id.as_uuid()).bind(context.node_execution_id.as_uuid()).bind(context.attempt_id.as_uuid()).bind(context.lease_token).bind(token_hash).bind(profile_version).bind(idempotency_key).bind(expires_at).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(id)
    }

    pub async fn bind_credentials(
        &self,
        context: &RuntimeContext,
        lease_id: Uuid,
        credentials: &[SandboxCredentialHandle],
    ) -> Result<()> {
        let mut names = BTreeSet::new();
        let mut transaction = self.pool.begin().await?;
        for credential in credentials {
            let resource_id = parse_uuid(&credential.resource_id, "credential_resource_id")?;
            if !valid_environment_name(&credential.environment_name)
                || !names.insert(credential.environment_name.clone())
            {
                anyhow::bail!("SANDBOX_CREDENTIAL_ENVIRONMENT_INVALID");
            }
            let changed=sqlx::query("UPDATE node_invocation_handles SET sandbox_lease_id=?,scope_json=JSON_SET(scope_json,'$.sandboxEnvironment',?) WHERE token_hash=? AND tenant_id=? AND execution_id=? AND node_execution_id=? AND attempt_id=? AND lease_token=? AND handle_kind='credential' AND resource_id=? AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP(6) AND (sandbox_lease_id IS NULL OR sandbox_lease_id=?)")
                .bind(lease_id).bind(&credential.environment_name).bind(hash(&credential.handle)).bind(context.tenant_id.as_uuid()).bind(context.execution_id.as_uuid()).bind(context.node_execution_id.as_uuid()).bind(context.attempt_id.as_uuid()).bind(context.lease_token).bind(resource_id).bind(lease_id).execute(&mut *transaction).await?.rows_affected();
            if changed != 1 {
                anyhow::bail!("SANDBOX_CREDENTIAL_HANDLE_INVALID");
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn resolve_credentials(
        &self,
        context: &RuntimeContext,
        lease_id: Uuid,
    ) -> Result<Vec<ResolvedSandboxCredential>> {
        let mut transaction = self.pool.begin().await?;
        let rows=sqlx::query("SELECT h.id,h.resource_id,h.resource_version,h.scope_json,h.expires_at,h.consumed_at,h.revoked_at,v.key_id,v.nonce,v.ciphertext FROM node_invocation_handles h JOIN credentials c ON c.tenant_id=h.tenant_id AND c.id=h.resource_id AND c.status='active' JOIN credential_secret_versions v ON v.tenant_id=h.tenant_id AND v.credential_id=h.resource_id AND v.version_number=h.resource_version WHERE h.sandbox_lease_id=? AND h.tenant_id=? AND h.execution_id=? AND h.node_execution_id=? AND h.attempt_id=? AND h.lease_token=? AND h.handle_kind='credential' FOR UPDATE")
            .bind(lease_id).bind(context.tenant_id.as_uuid()).bind(context.execution_id.as_uuid()).bind(context.node_execution_id.as_uuid()).bind(context.attempt_id.as_uuid()).bind(context.lease_token).fetch_all(&mut *transaction).await?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            validate_credential_handle_state(
                row.try_get("expires_at")?,
                row.try_get("consumed_at")?,
                row.try_get("revoked_at")?,
                OffsetDateTime::now_utc(),
            )?;
            let id: Uuid = row.try_get("id")?;
            let resource_id: Uuid = row.try_get("resource_id")?;
            let version: u64 = row.try_get("resource_version")?;
            let scope: Value = row.try_get("scope_json")?;
            let environment_name = scope
                .get("sandboxEnvironment")
                .and_then(Value::as_str)
                .filter(|value| valid_environment_name(value))
                .context("SANDBOX_CREDENTIAL_ENVIRONMENT_INVALID")?
                .to_owned();
            let aad = format!("{}/{resource_id}/{version}", context.tenant_id.as_uuid());
            let secret = self.credential_keyring.decrypt(
                &row.try_get::<String, _>("key_id")?,
                &row.try_get::<Vec<u8>, _>("nonce")?,
                &row.try_get::<Vec<u8>, _>("ciphertext")?,
                aad.as_bytes(),
            )?;
            sqlx::query("UPDATE node_invocation_handles SET consumed_at=CURRENT_TIMESTAMP(6) WHERE id=? AND consumed_at IS NULL AND revoked_at IS NULL").bind(id).execute(&mut *transaction).await?;
            result.push(ResolvedSandboxCredential {
                handle_id: id,
                environment_name,
                secret,
            });
        }
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn revoke_credentials(&self, lease_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE node_invocation_handles SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE sandbox_lease_id=? AND handle_kind='credential'").bind(lease_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn mark_ready(
        &self,
        id: Uuid,
        sandbox_id: &str,
        expires_at: OffsetDateTime,
        endpoint_document: &Value,
    ) -> Result<RpcLease> {
        let aad = format!("sandbox-endpoint/{id}");
        let encrypted = self.endpoint_keyring.encrypt(
            &PlainSecret::new(serde_json::to_vec(endpoint_document)?),
            aad.as_bytes(),
        )?;
        sqlx::query("UPDATE sandbox_leases SET sandbox_id=?,status='ready',expires_at=?,endpoint_auth_key_id=?,endpoint_auth_nonce=?,endpoint_auth_ciphertext=?,heartbeat_at=CURRENT_TIMESTAMP(6),last_error=NULL WHERE id=? AND status IN ('creating','orphaned')")
            .bind(sandbox_id).bind(expires_at).bind(encrypted.key_id).bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(id).execute(&self.pool).await?;
        Ok(self.rpc_lease(id, sandbox_id.to_owned(), expires_at))
    }

    pub async fn mark_failed(&self, id: Uuid, error: &str, orphaned: bool) -> Result<bool> {
        let result = sqlx::query("UPDATE sandbox_leases SET status=?,last_error=?,heartbeat_at=CURRENT_TIMESTAMP(6) WHERE id=? AND status<>'terminated'")
            .bind(if orphaned{"orphaned"}else{"failed"}).bind(truncate(error,1000)).bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn validate_lease(&self, scope: &SandboxScope, rpc: &RpcLease) -> Result<ValidLease> {
        let context = self.validate_scope(scope).await?;
        self.validate_bound_lease(context, rpc, false).await
    }

    pub async fn validate_cleanup_lease(
        &self,
        scope: &SandboxScope,
        rpc: &RpcLease,
    ) -> Result<ValidLease> {
        let context = self.validate_cleanup_scope(scope).await?;
        self.validate_bound_lease(context, rpc, true).await
    }

    async fn validate_bound_lease(
        &self,
        context: RuntimeContext,
        rpc: &RpcLease,
        cleanup: bool,
    ) -> Result<ValidLease> {
        let id = parse_uuid(&rpc.lease_id, "lease_id")?;
        let row=sqlx::query("SELECT sandbox_id,expires_at,status,lease_token_hash FROM sandbox_leases WHERE id=? AND tenant_id=? AND execution_id=? AND node_execution_id=? AND attempt_id=? AND worker_lease_token=?")
            .bind(id).bind(context.tenant_id.as_uuid()).bind(context.execution_id.as_uuid()).bind(context.node_execution_id.as_uuid()).bind(context.attempt_id.as_uuid()).bind(context.lease_token).fetch_optional(&self.pool).await?.context("SANDBOX_LEASE_NOT_FOUND")?;
        let expected_hash: String = row.try_get("lease_token_hash")?;
        if hash(&rpc.lease_token) != expected_hash || rpc.lease_token != self.lease_token(id) {
            anyhow::bail!("SANDBOX_LEASE_TOKEN_INVALID");
        }
        let status: String = row.try_get("status")?;
        if !lease_status_allowed(&status, cleanup) {
            anyhow::bail!("SANDBOX_LEASE_NOT_ACTIVE:{status}");
        }
        let sandbox_id: String = row.try_get("sandbox_id")?;
        if sandbox_id != rpc.sandbox_id {
            anyhow::bail!("SANDBOX_LEASE_SANDBOX_MISMATCH");
        }
        let expires_at: OffsetDateTime = row.try_get("expires_at")?;
        if !cleanup && expires_at <= OffsetDateTime::now_utc() {
            anyhow::bail!("SANDBOX_LEASE_EXPIRED");
        }
        if status != "terminated" {
            sqlx::query("UPDATE sandbox_leases SET heartbeat_at=CURRENT_TIMESTAMP(6) WHERE id=?")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(ValidLease {
            context,
            lease: SandboxLease {
                lease_id: id,
                sandbox_id,
                lease_token: rpc.lease_token.clone(),
                expires_at,
            },
            status,
        })
    }

    pub async fn set_status(&self, id: Uuid, status: &str) -> Result<()> {
        sqlx::query(
            "UPDATE sandbox_leases SET status=?,heartbeat_at=CURRENT_TIMESTAMP(6) WHERE id=? AND status NOT IN ('terminated','failed')",
        )
        .bind(status)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn terminate(&self, id: Uuid) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("UPDATE sandbox_leases SET status='terminated',terminated_at=CURRENT_TIMESTAMP(6),heartbeat_at=CURRENT_TIMESTAMP(6),endpoint_auth_key_id=NULL,endpoint_auth_nonce=NULL,endpoint_auth_ciphertext=NULL WHERE id=?").bind(id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE node_invocation_handles SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE sandbox_lease_id=? AND handle_kind='credential'").bind(id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn reap_once(&self, adapter: &OpenSandboxAdapter) -> Result<u64> {
        let rows=sqlx::query("SELECT id,sandbox_id,attempt_id,idempotency_key,status FROM sandbox_leases WHERE (status IN ('ready','running','interrupting','orphaned') AND expires_at<=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 5 SECOND)) OR (status='terminating' AND heartbeat_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 30 SECOND)) OR (status='creating' AND heartbeat_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 30 SECOND)) LIMIT 100").fetch_all(&self.pool).await?;
        let mut count = 0;
        for row in rows {
            let id: Uuid = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            let mut sandbox_id: Option<String> = row.try_get("sandbox_id")?;
            if status == "creating" && sandbox_id.is_none() {
                let attempt: Uuid = row.try_get("attempt_id")?;
                let idempotency_key: String = row.try_get("idempotency_key")?;
                let matches = adapter
                    .list_by_labels(&reconciliation_labels(attempt, &idempotency_key))
                    .await
                    .unwrap_or_default();
                if matches.len() == 1 {
                    sandbox_id = matches.into_iter().next();
                }
            }
            if let Some(sandbox_id) = sandbox_id {
                let context = dummy_context();
                let lease = SandboxLease {
                    lease_id: id,
                    sandbox_id,
                    lease_token: String::new(),
                    expires_at: OffsetDateTime::now_utc(),
                };
                match agentx_application::SandboxRuntime::terminate(adapter, &context, &lease).await
                {
                    Ok(()) => {
                        self.terminate(id).await?;
                        count += 1
                    }
                    Err(error) => {
                        sqlx::query("UPDATE sandbox_leases SET status='orphaned',termination_attempts=termination_attempts+1,last_error=? WHERE id=?").bind(truncate(&error.to_string(),1000)).bind(id).execute(&self.pool).await?;
                    }
                }
            } else {
                self.mark_failed(id, "No OpenSandbox instance could be reconciled", true)
                    .await?;
            }
        }
        Ok(count)
    }

    pub fn parse_profile(value: &str) -> Result<RuntimeResourceSnapshot> {
        serde_json::from_str(value).context("SANDBOX_PROFILE_SNAPSHOT_INVALID")
    }
    fn rpc_lease(&self, id: Uuid, sandbox_id: String, expires_at: OffsetDateTime) -> RpcLease {
        RpcLease {
            lease_id: id.to_string(),
            sandbox_id,
            lease_token: self.lease_token(id),
            expires_at_unix_ms: (expires_at.unix_timestamp_nanos() / 1_000_000) as i64,
        }
    }
    fn lease_token(&self, id: Uuid) -> String {
        let mut mac = HmacSha256::new_from_slice(self.signing_key.expose_secret().as_bytes())
            .expect("HMAC accepts arbitrary key length");
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

impl ExistingLease {
    pub fn active_rpc(&self, store: &LeaseStore) -> Result<Option<RpcLease>> {
        if !matches!(self.status.as_str(), "ready" | "running") {
            return Ok(None);
        }
        let sandbox_id = self
            .sandbox_id
            .clone()
            .context("SANDBOX_LEASE_SANDBOX_MISSING")?;
        Ok(Some(store.rpc_lease(self.id, sandbox_id, self.expires_at)))
    }
}

pub fn reconciliation_labels(attempt_id: Uuid, idempotency_key: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("agentx-attempt".into(), attempt_id.to_string()),
        ("agentx-request".into(), hash(idempotency_key)),
    ])
}

fn parse_uuid(value: &str, name: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("SANDBOX_SCOPE_{name}_INVALID"))
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn valid_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_uppercase())
        && chars.all(|value| value == '_' || value.is_ascii_uppercase() || value.is_ascii_digit())
}
fn validate_credential_handle_state(
    expires_at: OffsetDateTime,
    consumed_at: Option<OffsetDateTime>,
    revoked_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> Result<()> {
    if expires_at <= now {
        anyhow::bail!("SANDBOX_CREDENTIAL_HANDLE_EXPIRED");
    }
    if consumed_at.is_some() || revoked_at.is_some() {
        anyhow::bail!("SANDBOX_CREDENTIAL_HANDLE_REPLAYED");
    }
    Ok(())
}
fn lease_status_allowed(status: &str, cleanup: bool) -> bool {
    matches!(status, "ready" | "running" | "interrupting")
        || cleanup && matches!(status, "terminating" | "orphaned" | "failed" | "terminated")
}
fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
fn dummy_context() -> RuntimeContext {
    RuntimeContext {
        tenant_id: TenantId::from_uuid(Uuid::nil()),
        workflow_service_identity_id: WorkflowServiceIdentityId::from_uuid(Uuid::nil()),
        workflow_id: WorkflowId::from_uuid(Uuid::nil()),
        workflow_version_id: WorkflowVersionId::from_uuid(Uuid::nil()),
        execution_id: ExecutionId::from_uuid(Uuid::nil()),
        node_execution_id: NodeExecutionId::from_uuid(Uuid::nil()),
        attempt_id: AttemptId::from_uuid(Uuid::nil()),
        lease_token: Uuid::nil(),
        trace_id: TraceId::from_uuid(Uuid::nil()),
        span_id: Uuid::nil(),
        deadline: OffsetDateTime::now_utc() + time::Duration::minutes(1),
        cancellation: CancellationToken::new(),
        idempotency_key: String::new(),
        resources: Vec::new(),
    }
}

pub fn runtime_status(error: impl std::fmt::Display) -> tonic::Status {
    let message = error.to_string();
    if message.contains("SANDBOX_TENANT_CONCURRENCY_EXCEEDED") {
        tonic::Status::resource_exhausted(message)
    } else if message.contains("INVALID")
        || message.contains("MISMATCH")
        || message.contains("EXPIRED")
        || message.contains("REPLAYED")
    {
        tonic::Status::failed_precondition(message)
    } else if message.contains("NOT_FOUND") {
        tonic::Status::not_found(message)
    } else {
        tonic::Status::internal(message)
    }
}

pub fn profile_version(snapshot: &RuntimeResourceSnapshot) -> Result<Uuid> {
    snapshot
        .snapshot
        .get("profileVersionId")
        .and_then(Value::as_str)
        .context("SANDBOX_PROFILE_VERSION_MISSING")
        .and_then(|value| Uuid::parse_str(value).context("SANDBOX_PROFILE_VERSION_INVALID"))
}
pub fn profile_timeout(snapshot: &RuntimeResourceSnapshot) -> Result<u64> {
    snapshot
        .snapshot
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .context("SANDBOX_PROFILE_TIMEOUT_MISSING")
}

#[cfg(test)]
mod tests {
    use super::{lease_status_allowed, validate_credential_handle_state};
    use time::{Duration, OffsetDateTime};

    #[test]
    fn cleanup_accepts_terminal_and_expired_lease_states_only_on_cleanup_path() {
        for status in ["ready", "running", "interrupting"] {
            assert!(lease_status_allowed(status, false));
            assert!(lease_status_allowed(status, true));
        }
        for status in ["terminating", "orphaned", "failed", "terminated"] {
            assert!(!lease_status_allowed(status, false));
            assert!(lease_status_allowed(status, true));
        }
        for status in ["creating", "unknown"] {
            assert!(!lease_status_allowed(status, false));
            assert!(!lease_status_allowed(status, true));
        }
    }

    #[test]
    fn credential_handles_expire_and_cannot_be_replayed() {
        let now = OffsetDateTime::now_utc();
        assert!(
            validate_credential_handle_state(now + Duration::minutes(1), None, None, now).is_ok()
        );
        assert_eq!(
            validate_credential_handle_state(now, None, None, now)
                .unwrap_err()
                .to_string(),
            "SANDBOX_CREDENTIAL_HANDLE_EXPIRED"
        );
        assert_eq!(
            validate_credential_handle_state(now + Duration::minutes(1), Some(now), None, now,)
                .unwrap_err()
                .to_string(),
            "SANDBOX_CREDENTIAL_HANDLE_REPLAYED"
        );
        assert_eq!(
            validate_credential_handle_state(now + Duration::minutes(1), None, Some(now), now,)
                .unwrap_err()
                .to_string(),
            "SANDBOX_CREDENTIAL_HANDLE_REPLAYED"
        );
    }
}
