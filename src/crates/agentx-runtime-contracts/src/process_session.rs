use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionIdentityV1 {
    pub tenant_id: Uuid,
    pub agent_run_id: Uuid,
    pub mcp_server_version_id: Uuid,
    pub sandbox_profile_version_id: Uuid,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSessionStatusV1 {
    Acquiring,
    Starting,
    Running,
    Interrupting,
    Terminating,
    Exited,
    Failed,
    UnknownOutcome,
    Expired,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionLeaseV1 {
    pub process_session_id: Uuid,
    pub lease_id: Uuid,
    pub worker_id: Uuid,
    pub attempt_id: Uuid,
    pub fencing_token: u64,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub status: ProcessSessionStatusV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessEnvironmentCredentialV1 {
    pub name: String,
    pub credential: crate::VaultSecretReferenceV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionStartRequestV1 {
    pub api_version: u32,
    pub identity: ProcessSessionIdentityV1,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub operation_id: String,
    pub effect_id: String,
    pub idempotency_key: String,
    pub command: String,
    pub args: Vec<String>,
    pub environment_credentials: Vec<ProcessEnvironmentCredentialV1>,
    pub profile: crate::RuntimeResourceBindingV1,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionStartResponseV1 {
    pub api_version: u32,
    pub identity: ProcessSessionIdentityV1,
    pub lease: ProcessSessionLeaseV1,
    pub provider_sandbox_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionProofV1 {
    pub api_version: u32,
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub agent_run_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub process_session_id: Uuid,
    pub lease_id: Uuid,
    pub operation_id: String,
    pub effect_id: String,
    pub idempotency_key: String,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionWriteRequestV1 {
    #[serde(flatten)]
    pub proof: ProcessSessionProofV1,
    pub frame: Value,
    pub replay_policy: ProcessSessionReplayPolicyV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSessionReplayPolicyV1 {
    Safe,
    IdempotencyRequired,
    Never,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionReadRequestV1 {
    #[serde(flatten)]
    pub proof: ProcessSessionProofV1,
    pub after_sequence: u64,
    pub maximum_frames: u32,
    pub wait_millis: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionFrameV1 {
    pub sequence: u64,
    pub stream: String,
    pub payload: Value,
    pub artifact_id: Option<Uuid>,
    pub truncated: bool,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionFramesResponseV1 {
    pub api_version: u32,
    pub process_session_id: Uuid,
    pub status: ProcessSessionStatusV1,
    pub frames: Vec<ProcessSessionFrameV1>,
    pub next_sequence: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionControlRequestV1 {
    #[serde(flatten)]
    pub proof: ProcessSessionProofV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSessionControlResponseV1 {
    pub api_version: u32,
    pub process_session_id: Uuid,
    pub status: ProcessSessionStatusV1,
    pub exit_code: Option<i64>,
}
