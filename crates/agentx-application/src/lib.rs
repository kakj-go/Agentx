use std::time::Duration;

use agentx_domain::{
    ArtifactId, ExecutionId, ExecutionStatus, TenantId, WorkflowId, WorkflowSummary,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    async fn find_summary(&self, id: WorkflowId) -> Result<Option<WorkflowSummary>>;
}

#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    async fn status(&self, id: ExecutionId) -> Result<Option<ExecutionStatus>>;
}

#[derive(Clone, Debug)]
pub struct ArtifactWrite {
    pub tenant_id: TenantId,
    pub content_type: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ArtifactRead {
    pub id: ArtifactId,
    pub content_type: String,
    pub content: Vec<u8>,
    pub sha256: String,
}

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put(&self, artifact: ArtifactWrite) -> Result<ArtifactRead>;
    async fn get(&self, tenant_id: TenantId, id: ArtifactId) -> Result<Option<ArtifactRead>>;
    async fn delete(&self, tenant_id: TenantId, id: ArtifactId) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct OutboxMessage {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: Value,
}

impl OutboxMessage {
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            tenant_id,
            event_type: event_type.into(),
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            payload,
        }
    }
}

#[async_trait]
pub trait Outbox: Send + Sync {
    async fn append(&self, message: OutboxMessage) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct OutboxDelivery {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: Value,
    pub attempt_count: u32,
    pub lease_id: Uuid,
}

#[async_trait]
pub trait OutboxDispatcher: Send + Sync {
    async fn claim(&self, batch_size: u32, lease: Duration) -> Result<Vec<OutboxDelivery>>;
    async fn mark_published(&self, delivery: &OutboxDelivery) -> Result<bool>;
    async fn mark_failed(
        &self,
        delivery: &OutboxDelivery,
        error: &str,
        retry_after: Duration,
    ) -> Result<bool>;
}

#[async_trait]
pub trait TransactionManager: Send + Sync {
    type Transaction: Send;
    async fn begin(&self) -> Result<Self::Transaction>;
    async fn commit(&self, transaction: Self::Transaction) -> Result<()>;
    async fn rollback(&self, transaction: Self::Transaction) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct ExternalIdentity {
    pub provider: String,
    pub subject: String,
    pub preferred_username: Option<String>,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn authenticate(&self, authorization_code: &str) -> Result<ExternalIdentity>;
}
