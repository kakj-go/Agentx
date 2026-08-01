use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    Main,
    Error,
    AiModel,
    AiTool,
    AiMemory,
    AiRetriever,
    AiOutputParser,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphConnection {
    pub source_node_id: String,
    pub source_port: String,
    pub target_node_id: String,
    pub target_port: String,
    pub kind: ConnectionKind,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledWorkflow {
    pub node_ids: Vec<String>,
    pub connections: Vec<GraphConnection>,
}
