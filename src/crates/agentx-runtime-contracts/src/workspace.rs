use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceIdentityV1 {
    pub tenant_id: Uuid,
    pub application_id: Option<Uuid>,
    pub session_key: String,
    pub stable_agent_node_key: String,
    pub sandbox_profile_version_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceLeaseV1 {
    pub api_version: u32,
    pub workspace_id: Uuid,
    pub lease_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub status: WorkspaceLeaseStatusV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLeaseStatusV1 {
    Acquiring,
    Active,
    Releasing,
    Released,
    Expired,
    UnknownOutcome,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolEffectRequestV1 {
    pub api_version: u32,
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub workspace_id: Uuid,
    pub lease_id: Uuid,
    pub operation_id: String,
    pub effect_id: String,
    pub idempotency_key: String,
    pub tool_name: String,
    pub arguments: Value,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolEffectResponseV1 {
    pub api_version: u32,
    pub workspace_id: Uuid,
    pub lease_id: Uuid,
    pub operation_id: String,
    pub effect_id: String,
    pub status: String,
    #[serde(default)]
    pub structured_result: Value,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    pub exit_code: Option<i64>,
    pub content_hash: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceAcquireRequestV1 {
    pub api_version: u32,
    pub identity: WorkspaceIdentityV1,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub profile: Value,
    pub idempotency_key: String,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceAcquireResponseV1 {
    pub api_version: u32,
    pub identity: WorkspaceIdentityV1,
    pub lease: WorkspaceLeaseV1,
    pub provider_sandbox_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceReleaseRequestV1 {
    pub api_version: u32,
    pub identity: WorkspaceIdentityV1,
    pub workspace_id: Uuid,
    pub lease_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub destroy: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceReleaseResponseV1 {
    pub api_version: u32,
    pub released: bool,
    pub destroyed: bool,
}
