use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionVersionPolicyV1 {
    Pinned,
    FollowDeployment,
    ManualUpgrade,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationRuntimePolicyV1 {
    pub session_version_policy: SessionVersionPolicyV1,
    pub synchronous_wait_seconds: u32,
    pub maximum_json_bytes: u64,
    pub maximum_multipart_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VaultSecretReferenceV1 {
    pub mount: String,
    pub path: String,
    pub key: String,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMisfirePolicyV1 {
    Skip,
    FireOnce,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOperationV1 {
    Activate,
    Deactivate,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeTriggerConfigurationV1 {
    Webhook {
        public_id: String,
        secret: VaultSecretReferenceV1,
    },
    Schedule {
        cron_expression: String,
        timezone: String,
        misfire_policy: ScheduleMisfirePolicyV1,
        grace_seconds: u32,
        input: Value,
    },
    Poll {
        interval_seconds: u32,
        provider_endpoint: String,
        input: Value,
    },
    Lifecycle {
        operation: LifecycleOperationV1,
        provider_endpoint: String,
        input: Value,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTriggerSpecV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub trigger_id: Uuid,
    pub application_id: Uuid,
    pub node_id: String,
    pub revision: u64,
    pub configuration_hash: crate::ContentHash,
    pub enabled: bool,
    pub configuration: RuntimeTriggerConfigurationV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeUserAdmissionV1 {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub token_version: u64,
    pub enabled: bool,
    pub tenant_query_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeUserApplicationGrantV1 {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub application_id: Uuid,
    pub grant_version: u64,
    pub can_invoke: bool,
    pub can_query: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeUserWorkflowGrantV1 {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub workflow_id: Uuid,
    pub grant_version: u64,
    pub can_query: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionUpgradeCommandV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub tenant_id: Uuid,
    pub session_id: Uuid,
    pub application_id: Uuid,
    pub expected_session_version: u64,
    pub target_bundle_id: Uuid,
    pub actor_user_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionUpgradeReceiptV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub session_id: Uuid,
    pub bundle_id: Uuid,
    pub session_version: u64,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSearchRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub tenant_id: Uuid,
    pub application_id: Option<Uuid>,
    pub statuses: Vec<String>,
    pub after: Option<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSummaryV1 {
    pub session_id: Uuid,
    pub application_id: Uuid,
    pub application_deployment_id: Uuid,
    pub workflow_version_id: Option<Uuid>,
    pub bundle_id: Option<Uuid>,
    pub version_policy: SessionVersionPolicyV1,
    pub external_user_id: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub version: u64,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSearchPageV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub items: Vec<SessionSummaryV1>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDetailV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub summary: SessionSummaryV1,
    pub external_user_id: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSessionRequestV1 {
    pub external_user_id: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionResponseV1 {
    pub id: Uuid,
    pub application_id: Uuid,
    pub application_deployment_id: Uuid,
    pub workflow_version_id: Option<Uuid>,
    pub bundle_id: Option<Uuid>,
    pub version_policy: SessionVersionPolicyV1,
    pub external_user_id: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub version: u64,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationRequestV1 {
    pub input: Value,
    pub session_id: Option<Uuid>,
    pub response_mode: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationResponseV1 {
    pub id: Uuid,
    pub application_id: Uuid,
    pub session_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub bundle_id: Uuid,
    pub admission_epoch: u64,
    pub status: String,
    pub outputs: Option<Value>,
    pub error: Option<Value>,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessagePartInputV1 {
    pub part_type: String,
    pub content: Option<Value>,
    pub artifact_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageRequestV1 {
    pub parts: Vec<MessagePartInputV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageResponseV1 {
    pub id: Uuid,
    pub invocation_id: Option<Uuid>,
    pub sequence: u64,
    pub role: String,
    pub parts: Vec<MessagePartInputV1>,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactUploadResponseV1 {
    pub artifact_id: Uuid,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitResumeRequestV1 {
    pub output_port: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandAcceptedV1 {
    pub accepted: bool,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayErrorV1 {
    pub code: String,
    pub message: String,
}
