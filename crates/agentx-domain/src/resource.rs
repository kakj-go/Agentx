use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Credential,
    Model,
    McpServer,
    McpTool,
    Skill,
    Rag,
    Memory,
    SandboxProfile,
}

impl ResourceType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Model => "model",
            Self::McpServer => "mcp_server",
            Self::McpTool => "mcp_tool",
            Self::Skill => "skill",
            Self::Rag => "rag",
            Self::Memory => "memory",
            Self::SandboxProfile => "sandbox_profile",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOperation {
    View,
    Use,
    Read,
    Write,
    Manage,
}

impl ResourceOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Use => "use",
            Self::Read => "read",
            Self::Write => "write",
            Self::Manage => "manage",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_role: Option<String>,
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_version_id: Option<Uuid>,
    pub operation: ResourceOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceVersionSnapshot {
    pub node_id: String,
    pub reference: ResourceReference,
    pub snapshot_hash: String,
    pub snapshot: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGrant {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub reference: ResourceReference,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingGrant {
    pub node_id: String,
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    pub operation: ResourceOperation,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_by_resource_id: Option<Uuid>,
}
