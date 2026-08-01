use agentx_domain::{ExecutionId, ExecutionStatus, WorkflowId, WorkflowSummary};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    async fn find_summary(&self, id: WorkflowId) -> Result<Option<WorkflowSummary>>;
}

#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    async fn status(&self, id: ExecutionId) -> Result<Option<ExecutionStatus>>;
}
