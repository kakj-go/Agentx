use std::time::Duration;

use agentx_domain::{
    ApprovalTaskId, ArtifactId, DatasetVersionId, EvaluationProfileVersionId, EvaluationRunId,
    ExecutionId, ExecutionStatus, InvocationId, MissingGrant, NotificationId, ResourceOperation,
    ResourceReference, ResourceType, ResourceVersionSnapshot, SessionId, TenantId, TraceEvent,
    UserId, WorkflowDefinition, WorkflowId, WorkflowServiceIdentity, WorkflowSummary,
    WorkflowVersionId,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroize;

pub mod runtime;
pub use runtime::*;

#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    async fn find_summary(&self, id: WorkflowId) -> Result<Option<WorkflowSummary>>;
}

#[async_trait]
pub trait WorkflowControlRepository: Send + Sync {
    async fn definition(
        &self,
        tenant_id: TenantId,
        workflow_id: WorkflowId,
    ) -> Result<Option<WorkflowDefinition>>;
    async fn service_identity(
        &self,
        tenant_id: TenantId,
        workflow_id: WorkflowId,
    ) -> Result<Option<WorkflowServiceIdentity>>;
    async fn version_snapshots(
        &self,
        tenant_id: TenantId,
        version_id: WorkflowVersionId,
    ) -> Result<Vec<ResourceVersionSnapshot>>;
}

#[async_trait]
pub trait PublishValidator: Send + Sync {
    async fn validate(
        &self,
        tenant_id: TenantId,
        workflow_id: WorkflowId,
        definition: &WorkflowDefinition,
    ) -> Result<Vec<MissingGrant>>;
    async fn snapshot(
        &self,
        tenant_id: TenantId,
        workflow_id: WorkflowId,
        definition: &WorkflowDefinition,
    ) -> Result<Vec<ResourceVersionSnapshot>>;
}

#[async_trait]
pub trait ResourceAuthorizer: Send + Sync {
    async fn authorize(
        &self,
        tenant_id: TenantId,
        identity: &WorkflowServiceIdentity,
        reference: &ResourceReference,
    ) -> Result<bool>;
    async fn missing_dependencies(
        &self,
        tenant_id: TenantId,
        identity: &WorkflowServiceIdentity,
        reference: &ResourceReference,
    ) -> Result<Vec<MissingGrant>>;
}

pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    #[must_use]
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct ResolvedCredential {
    pub credential_type: String,
    pub secret: SecretMaterial,
}

#[async_trait]
pub trait CredentialResolver: Send + Sync {
    async fn resolve(&self, tenant_id: TenantId, credential_id: Uuid)
    -> Result<ResolvedCredential>;
    async fn resolve_version(
        &self,
        tenant_id: TenantId,
        credential_id: Uuid,
        version: u64,
    ) -> Result<ResolvedCredential>;
}

#[derive(Clone, Debug)]
pub struct ConnectionTestResult {
    pub status: String,
    pub latency_ms: Option<u64>,
    pub error_code: Option<String>,
}

#[async_trait]
pub trait ConnectionTester: Send + Sync {
    async fn test(
        &self,
        tenant_id: TenantId,
        resource_type: ResourceType,
        resource_id: Uuid,
        operation: ResourceOperation,
    ) -> Result<ConnectionTestResult>;
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

#[derive(Clone, Debug)]
pub struct RequestExecution {
    pub tenant_id: TenantId,
    pub invocation_id: Option<InvocationId>,
    pub session_id: Option<SessionId>,
    pub workflow_version_id: WorkflowVersionId,
    pub requested_by: Option<UserId>,
    pub trigger_type: String,
    pub input: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AcceptedExecution {
    pub execution_id: ExecutionId,
    pub status: ExecutionStatus,
}

#[derive(Clone, Debug)]
pub struct ForkExecutionCommand {
    pub tenant_id: TenantId,
    pub source_execution_id: ExecutionId,
    pub checkpoint_id: agentx_domain::CheckpointId,
    pub mode: String,
    pub node_id: Option<String>,
    pub input_overrides: Value,
    pub side_effect_decisions: Value,
    pub actor_user_id: UserId,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ResumeExecutionCommand {
    pub tenant_id: TenantId,
    pub execution_id: ExecutionId,
    pub node_execution_id: agentx_domain::NodeExecutionId,
    pub resume_token: String,
    pub output_port: String,
    pub payload: Value,
    pub idempotency_key: String,
    pub actor_user_id: Option<UserId>,
}

#[derive(Clone, Debug)]
pub struct SideEffectConfirmationCommand {
    pub tenant_id: TenantId,
    pub execution_id: ExecutionId,
    pub node_execution_id: agentx_domain::NodeExecutionId,
    pub checkpoint_id: Option<agentx_domain::CheckpointId>,
    pub decision: String,
    pub actor_user_id: UserId,
    pub idempotency_key: String,
}

#[async_trait]
pub trait ExecutionRuntime: Send + Sync {
    async fn request_execution(&self, request: RequestExecution) -> Result<AcceptedExecution>;
    async fn get_execution(
        &self,
        tenant_id: TenantId,
        id: ExecutionId,
    ) -> Result<Option<ExecutionStatus>>;
    async fn cancel_execution(&self, tenant_id: TenantId, id: ExecutionId) -> Result<()>;
    async fn fork_execution(&self, _command: ForkExecutionCommand) -> Result<AcceptedExecution> {
        anyhow::bail!("runtime fork is unavailable")
    }
    async fn resume_execution(&self, _command: ResumeExecutionCommand) -> Result<bool> {
        anyhow::bail!("runtime resume is unavailable")
    }
    async fn confirm_side_effect(&self, _command: SideEffectConfirmationCommand) -> Result<bool> {
        anyhow::bail!("runtime side-effect confirmation is unavailable")
    }
}

#[derive(Clone, Debug)]
pub struct RequestEvaluation {
    pub tenant_id: TenantId,
    pub run_id: EvaluationRunId,
    pub workflow_version_id: WorkflowVersionId,
    pub dataset_version_id: DatasetVersionId,
    pub evaluation_profile_version_id: EvaluationProfileVersionId,
}

#[async_trait]
pub trait EvaluationRuntime: Send + Sync {
    async fn start(&self, request: RequestEvaluation) -> Result<()>;
    async fn cancel(&self, tenant_id: TenantId, run_id: EvaluationRunId) -> Result<()>;
}

#[async_trait]
pub trait ScheduleTrigger: Send + Sync {
    async fn trigger(
        &self,
        tenant_id: TenantId,
        schedule_id: Uuid,
        idempotency_key: &str,
    ) -> Result<()>;
}

#[async_trait]
pub trait ApprovalTaskPort: Send + Sync {
    async fn create_task(
        &self,
        tenant_id: TenantId,
        task_id: ApprovalTaskId,
        payload: Value,
    ) -> Result<()>;
}

#[async_trait]
pub trait ApprovalResumePort: Send + Sync {
    async fn resume(
        &self,
        tenant_id: TenantId,
        task_id: ApprovalTaskId,
        decision: &str,
        input: Value,
    ) -> Result<()>;
}

#[async_trait]
pub trait NotificationPublisher: Send + Sync {
    async fn publish(
        &self,
        tenant_id: TenantId,
        notification_id: NotificationId,
        payload: Value,
    ) -> Result<()>;
}

#[async_trait]
pub trait InvocationEventPublisher: Send + Sync {
    async fn publish(
        &self,
        tenant_id: TenantId,
        invocation_id: InvocationId,
        event: Value,
    ) -> Result<()>;
}

#[async_trait]
pub trait TraceSink: Send + Sync {
    async fn append(&self, event: TraceEvent) -> Result<()>;
}

#[async_trait]
pub trait ExecutionQuery: Send + Sync {
    async fn status(
        &self,
        tenant_id: TenantId,
        execution_id: ExecutionId,
    ) -> Result<Option<ExecutionStatus>>;
}

#[async_trait]
pub trait ExecutionProjectionPort: Send + Sync {
    async fn create(&self, summary: &agentx_domain::ExecutionSummary) -> Result<()>;
}

#[async_trait]
pub trait RuntimeStatusProvider: Send + Sync {
    async fn snapshot(&self, tenant_id: TenantId) -> Result<Value>;
}
