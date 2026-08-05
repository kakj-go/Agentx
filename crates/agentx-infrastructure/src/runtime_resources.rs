use agentx_application::{ResourceAuthorizer, RuntimeContext, RuntimeError, RuntimeResult};
use agentx_domain::{MissingGrant, ResourceReference, TenantId, WorkflowServiceIdentity};
use anyhow::Result;
use async_trait::async_trait;
use sqlx::MySqlPool;

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
