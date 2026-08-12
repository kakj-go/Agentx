use agentx_application::{ResourceAuthorizer, RuntimeContext, RuntimeError, RuntimeResult};
use agentx_domain::{MissingGrant, ResourceReference, TenantId, WorkflowServiceIdentity};
use anyhow::Result;
use async_trait::async_trait;
use sqlx::MySqlPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct MySqlResourceAuthorizer {
    pool: MySqlPool,
}

impl MySqlResourceAuthorizer {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn authorize_context(
        &self,
        context: &RuntimeContext,
        reference: &ResourceReference,
    ) -> RuntimeResult<()> {
        let identity = WorkflowServiceIdentity {
            id: context.workflow_service_identity_id,
            tenant_id: context.tenant_id,
            workflow_id: context.workflow_id,
            status: "active".into(),
        };
        if context.resource(reference).is_none() {
            return Err(RuntimeError::new(
                "RESOURCE_SNAPSHOT_MISSING",
                "The requested resource is not present in the Execution snapshot",
            ));
        }
        let mut tx = self.pool.begin().await.map_err(|error| {
            RuntimeError::new("RESOURCE_STATUS_CHECK_FAILED", error.to_string()).retryable(true)
        })?;
        let active = resource_is_active(
            &mut tx,
            context.tenant_id.as_uuid(),
            reference.resource_type.as_str(),
            reference.resource_id,
            reference.resource_version_id,
        )
        .await
        .map_err(|error| {
            RuntimeError::new("RESOURCE_STATUS_CHECK_FAILED", error.to_string()).retryable(true)
        })?;
        tx.rollback().await.map_err(|error| {
            RuntimeError::new("RESOURCE_STATUS_CHECK_FAILED", error.to_string()).retryable(true)
        })?;
        if !active {
            return Err(RuntimeError::new(
                "RESOURCE_UNAVAILABLE",
                "The requested resource or its fixed version is disabled or unavailable",
            ));
        }
        if !self
            .authorize(context.tenant_id, &identity, reference)
            .await
            .map_err(|error| {
                RuntimeError::new("RESOURCE_AUTHORIZATION_FAILED", error.to_string())
                    .retryable(true)
            })?
        {
            return Err(RuntimeError::new(
                "RESOURCE_GRANT_MISSING",
                "The Workflow Service Identity is no longer authorized for this resource",
            ));
        }
        Ok(())
    }
}

pub async fn resource_is_active(
    transaction: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant_id: Uuid,
    resource_type: &str,
    resource_id: Uuid,
    version_id: Option<Uuid>,
) -> Result<bool> {
    let active = match resource_type {
        "credential" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credentials WHERE tenant_id=? AND id=? AND status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?,
        "model" => if let Some(version_id) = version_id {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_aliases a JOIN model_alias_deployment_history h ON h.alias_id=a.id JOIN model_deployments d ON d.id=h.deployment_id WHERE a.tenant_id=? AND a.id=? AND a.status='active' AND d.id=? AND d.status='active')").bind(tenant_id).bind(resource_id).bind(version_id).fetch_one(&mut **transaction).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.id=? AND a.status='active' AND d.status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?
        },
        "mcp_server" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE tenant_id=? AND id=? AND status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?,
        "mcp_tool" => if let Some(version_id) = version_id {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_tool_versions tv JOIN mcp_tools t ON t.id=tv.tool_id JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE tv.tenant_id=? AND tv.id=? AND tv.tool_id=? AND t.availability='available' AND s.status='active' AND p.enabled=TRUE)").bind(tenant_id).bind(version_id).bind(resource_id).fetch_one(&mut **transaction).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_tools t JOIN mcp_servers s ON s.id=t.server_id JOIN mcp_tool_policies p ON p.tool_id=t.id AND p.tenant_id=t.tenant_id WHERE t.tenant_id=? AND t.id=? AND t.availability='available' AND s.status='active' AND p.enabled=TRUE)").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?
        },
        "skill" => if let Some(version_id) = version_id {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skill_versions sv JOIN skills s ON s.id=sv.skill_id WHERE sv.tenant_id=? AND sv.id=? AND sv.skill_id=? AND s.status='active')").bind(tenant_id).bind(version_id).bind(resource_id).fetch_one(&mut **transaction).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM skills s JOIN skill_versions sv ON sv.skill_id=s.id WHERE s.tenant_id=? AND s.id=? AND s.status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?
        },
        "rag" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND r.id=? AND r.status='active' AND c.status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?,
        "memory" => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND n.id=? AND n.status='active' AND c.status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?,
        "sandbox_profile" => if let Some(version_id) = version_id {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profile_versions v JOIN sandbox_profiles p ON p.id=v.profile_id WHERE v.tenant_id=? AND v.id=? AND v.profile_id=? AND p.status='active')").bind(tenant_id).bind(version_id).bind(resource_id).fetch_one(&mut **transaction).await?
        } else {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profiles WHERE tenant_id=? AND id=? AND status='active')").bind(tenant_id).bind(resource_id).fetch_one(&mut **transaction).await?
        },
        _ => false,
    };
    Ok(active)
}

#[async_trait]
impl ResourceAuthorizer for MySqlResourceAuthorizer {
    async fn authorize(
        &self,
        tenant_id: TenantId,
        identity: &WorkflowServiceIdentity,
        reference: &ResourceReference,
    ) -> Result<bool> {
        if identity.tenant_id != tenant_id || identity.status != "active" {
            return Ok(false);
        }
        let value: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_service_identities i JOIN resource_grants g ON g.tenant_id=i.tenant_id AND g.subject_type='workflow_service_identity' AND g.subject_id=i.id WHERE i.tenant_id=? AND i.id=? AND i.workflow_id=? AND i.status='active' AND g.resource_type=? AND g.resource_id=? AND g.operation_key IN (?, 'manage'))")
            .bind(tenant_id.as_uuid()).bind(identity.id.as_uuid()).bind(identity.workflow_id.as_uuid())
            .bind(reference.resource_type.as_str()).bind(reference.resource_id).bind(reference.operation.as_str())
            .fetch_one(&self.pool).await?;
        Ok(value)
    }

    async fn missing_dependencies(
        &self,
        _tenant_id: TenantId,
        _identity: &WorkflowServiceIdentity,
        _reference: &ResourceReference,
    ) -> Result<Vec<MissingGrant>> {
        Ok(Vec::new())
    }
}
