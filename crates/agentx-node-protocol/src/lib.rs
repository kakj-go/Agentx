use std::collections::BTreeMap;

use agentx_domain::{ExecutionId, NodeExecutionId, TenantId, WorkflowVersionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

pub const NODE_PROTOCOL_VERSION: &str = "1.0";

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeProtocolVersion {
    #[serde(rename = "1.0")]
    V1,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub json: Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub binary: BTreeMap<String, BinaryReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<ItemSource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BinaryReference {
    pub artifact_handle: String,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemSource {
    pub node_execution_id: NodeExecutionId,
    pub node_id: String,
    pub run_index: u32,
    pub output_index: u32,
    pub item_index: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupedInput {
    pub port: String,
    pub branch_index: u32,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedParameters {
    pub common: Value,
    #[serde(default)]
    pub per_item: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Manual,
    Production,
    Evaluation,
    Partial,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCapability {
    Builtin,
    DeclarativeHttp,
    RemoteAction,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStyle {
    Action,
    Trigger,
    Suspend,
    SubWorkflow,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessPolicy {
    Any,
    All,
    Required,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectLevel {
    None,
    Idempotent,
    Reversible,
    Irreversible,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    Main,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodePort {
    pub name: String,
    pub kind: PortKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub variadic: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeManifestVersion {
    #[schemars(with = "NodeProtocolVersion")]
    pub protocol_version: String,
    pub node_type: String,
    #[schemars(range(min = 1))]
    pub version: u32,
    pub display_name: String,
    pub execution_style: ExecutionStyle,
    pub capability: NodeCapability,
    pub readiness: ReadinessPolicy,
    pub input_ports: Vec<NodePort>,
    pub output_ports: Vec<NodePort>,
    pub parameter_schema: Value,
    #[serde(default)]
    pub ui_schema: Value,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub lifecycle_operations: Vec<LifecycleOperation>,
    #[serde(default)]
    pub credentials: Vec<CredentialRequirement>,
    pub default_timeout_ms: Option<u64>,
    #[serde(default)]
    pub retry_policy: NodeRetryPolicy,
    #[serde(default)]
    pub sandbox_required: bool,
    #[serde(default)]
    pub supports_mock: bool,
    pub side_effect_level: SideEffectLevel,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialRequirement {
    pub credential_type: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRetryPolicy {
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub max_attempts: u16,
    #[serde(default)]
    pub initial_backoff_ms: u64,
    #[serde(default)]
    pub max_backoff_ms: u64,
}

impl NodeManifestVersion {
    #[must_use]
    pub fn key(&self) -> (String, u32) {
        (self.node_type.clone(), self.version)
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeActionRequest {
    #[schemars(with = "NodeProtocolVersion")]
    pub protocol_version: String,
    pub node_type: String,
    #[schemars(range(min = 1))]
    pub node_version: u32,
    pub tenant_id: TenantId,
    pub workflow_version_id: WorkflowVersionId,
    pub execution_id: ExecutionId,
    pub node_execution_id: NodeExecutionId,
    pub attempt_id: Uuid,
    pub run_index: u32,
    pub iteration_index: u32,
    pub mode: ExecutionMode,
    pub inputs: Vec<GroupedInput>,
    pub parameters: ResolvedParameters,
    #[serde(default)]
    pub artifact_handles: Vec<InvocationHandle>,
    #[serde(default)]
    pub credential_handles: Vec<InvocationHandle>,
    pub idempotency_key: String,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
    pub cancellation_url: Option<String>,
    pub trace_context: TraceContext,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationHandle {
    pub handle: String,
    pub broker_url: String,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationResourceRequest {
    pub handle: String,
    pub tenant_id: TenantId,
    pub node_execution_id: NodeExecutionId,
    pub attempt_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum InvocationResourceResponse {
    Credential {
        credential_type: String,
        value: Value,
    },
    Artifact {
        content_type: String,
        content_base64: String,
        sha256: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationCancellationStatus {
    pub cancellation_requested: bool,
    pub lease_valid: bool,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NodeActionResult {
    Completed {
        outputs: Vec<Vec<Item>>,
        #[serde(default)]
        artifacts: Vec<ProducedArtifact>,
    },
    Failed {
        error: NodeProtocolError,
    },
    Suspended {
        resume: ResumeContract,
        #[serde(default)]
        checkpoint: Value,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProducedArtifact {
    pub name: String,
    pub artifact_handle: String,
    pub content_type: Option<String>,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeContract {
    pub kind: ResumeKind,
    #[schemars(with = "Option<String>")]
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub timeout_at: Option<OffsetDateTime>,
    pub allowed_output_ports: Vec<String>,
    pub payload_schema: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeKind {
    Time,
    Webhook,
    Form,
    Approval,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeProtocolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub details: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRequest {
    #[schemars(with = "NodeProtocolVersion")]
    pub protocol_version: String,
    pub provider: String,
    pub operation: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub credential_handles: Vec<InvocationHandle>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderResponse {
    pub options: Vec<ProviderOption>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderOption {
    pub label: String,
    pub value: Value,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleRequest {
    #[schemars(with = "NodeProtocolVersion")]
    pub protocol_version: String,
    pub operation: LifecycleOperation,
    pub node_type: String,
    #[schemars(range(min = 1))]
    pub node_version: u32,
    pub tenant_id: TenantId,
    pub workflow_version_id: WorkflowVersionId,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOperation {
    Activate,
    Deactivate,
    Poll,
    Webhook,
    Suspend,
    Resume,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleResponse {
    pub accepted: bool,
    #[serde(default)]
    pub state: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_result_has_stable_tag() {
        let result = NodeActionResult::Completed {
            outputs: vec![vec![Item {
                json: serde_json::json!({"ok": true}),
                ..Item::default()
            }]],
            artifacts: Vec::new(),
        };
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["outputs"][0][0]["json"]["ok"], true);
    }

    #[test]
    fn item_supports_multiple_sources() {
        let first = ItemSource {
            node_execution_id: NodeExecutionId::new(),
            node_id: "left".into(),
            run_index: 0,
            output_index: 0,
            item_index: 0,
        };
        let mut second = first.clone();
        second.node_execution_id = NodeExecutionId::new();
        second.node_id = "right".into();
        let item = Item {
            json: Value::Null,
            lineage: vec![first, second],
            ..Item::default()
        };
        assert_eq!(item.lineage.len(), 2);
    }
}
