use agentx_domain::{ExecutionId, NodeExecutionId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub json: Value,
    #[serde(default)]
    pub binary: Value,
    #[serde(default)]
    pub paired_item: Option<PairedItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedItem {
    pub item_index: usize,
    pub input_index: usize,
}

#[derive(Clone, Debug)]
pub struct NodeContext {
    pub tenant_id: TenantId,
    pub execution_id: ExecutionId,
    pub node_execution_id: NodeExecutionId,
    pub run_index: u32,
    pub iteration_index: u32,
}

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("node configuration is invalid: {0}")]
    InvalidConfiguration(String),
    #[error("node execution failed: {0}")]
    Execution(String),
}

#[async_trait]
pub trait NodeRunner: Send + Sync {
    async fn execute(
        &self,
        context: &NodeContext,
        input: Vec<Item>,
    ) -> Result<Vec<Vec<Item>>, NodeError>;
}
