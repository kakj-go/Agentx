use std::collections::BTreeMap;

use agentx_domain::{ExecutionId, NodeExecutionId, ResourceType, TenantId, WorkflowVersionId};
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
    Agent,
    Model,
    McpTool,
    Skill,
    Rag,
    Memory,
    Sandbox,
}

pub const ALL_RUNTIME_CAPABILITIES: &[&str] = &[
    "builtin",
    "declarative_http",
    "remote_action",
    "agent",
    "model",
    "mcp_tool",
    "skill",
    "rag",
    "memory",
    "sandbox",
];

impl NodeCapability {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::DeclarativeHttp => "declarative_http",
            Self::RemoteAction => "remote_action",
            Self::Agent => "agent",
            Self::Model => "model",
            Self::McpTool => "mcp_tool",
            Self::Skill => "skill",
            Self::Rag => "rag",
            Self::Memory => "memory",
            Self::Sandbox => "sandbox",
        }
    }
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

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingSlot {
    pub name: String,
    pub resource_type: ResourceType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasNodeRole {
    Default,
    Trigger,
    Branch,
    Flow,
    Merge,
    Loop,
    Suspend,
    Approval,
    SubWorkflow,
    Agent,
    Code,
    ErrorHandler,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanvasAppearance {
    pub role: CanvasNodeRole,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeUiSchema {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_selectors: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<CanvasAppearance>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeManifestLocalization {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_port_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_port_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub binding_slot_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_descriptions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_placeholders: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_enum_options: BTreeMap<String, BTreeMap<String, String>>,
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
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub localizations: BTreeMap<String, NodeManifestLocalization>,
    #[serde(default)]
    pub icon_key: String,
    pub execution_style: ExecutionStyle,
    pub capability: NodeCapability,
    pub readiness: ReadinessPolicy,
    pub input_ports: Vec<NodePort>,
    pub output_ports: Vec<NodePort>,
    #[serde(default)]
    pub binding_slots: Vec<BindingSlot>,
    pub parameter_schema: Value,
    #[serde(default)]
    pub output_schema: Value,
    /// Optional schema overrides for individual output ports. The default
    /// output_schema remains the native main-port schema.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_port_schemas: BTreeMap<String, Value>,
    #[serde(default)]
    pub output_cardinality: BTreeMap<String, OutputCardinality>,
    #[serde(default)]
    pub expression_capabilities: ExpressionCapabilities,
    #[serde(default)]
    pub context_read_capability: bool,
    #[serde(default)]
    pub context_write_capability: bool,
    #[serde(default)]
    pub output_projection_schema: Value,
    #[serde(default)]
    pub artifact_output_schema: Value,
    #[serde(default)]
    pub ui_schema: NodeUiSchema,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputCardinality {
    ZeroOrOne,
    ExactlyOne,
    ZeroOrMany,
    #[default]
    Many,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpressionCapabilities {
    #[serde(default)]
    pub namespaces: Vec<String>,
    #[serde(default)]
    pub supports_current: bool,
    #[serde(default)]
    pub supports_first_last: bool,
    #[serde(default)]
    pub supports_all: bool,
    #[serde(default)]
    pub supports_run_selection: bool,
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

    pub fn validate_localizations(&self) -> Result<(), String> {
        let inputs = self
            .input_ports
            .iter()
            .map(|port| port.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let outputs = self
            .output_ports
            .iter()
            .map(|port| port.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let slots = self
            .binding_slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let parameter_properties = parameter_schema_paths(&self.parameter_schema);
        for (locale, localization) in &self.localizations {
            if !matches!(locale.as_str(), "zh-CN" | "en-US") {
                return Err(format!("unsupported manifest locale '{locale}'"));
            }
            for name in localization.input_port_labels.keys() {
                if !inputs.contains(name.as_str()) {
                    return Err(format!(
                        "locale {locale} references unknown input port '{name}'"
                    ));
                }
            }
            for name in localization.output_port_labels.keys() {
                if !outputs.contains(name.as_str()) {
                    return Err(format!(
                        "locale {locale} references unknown output port '{name}'"
                    ));
                }
            }
            for name in localization.binding_slot_labels.keys() {
                if !slots.contains(name.as_str()) {
                    return Err(format!(
                        "locale {locale} references unknown binding slot '{name}'"
                    ));
                }
            }
            for name in localization
                .parameter_labels
                .keys()
                .chain(localization.parameter_descriptions.keys())
                .chain(localization.parameter_placeholders.keys())
                .chain(localization.parameter_enum_options.keys())
            {
                if !parameter_properties.contains_key(name) {
                    return Err(format!(
                        "locale {locale} references unknown parameter '{name}'"
                    ));
                }
            }
            for (name, options) in &localization.parameter_enum_options {
                let declared = parameter_properties
                    .get(name)
                    .and_then(|property| property.get("enum"))
                    .and_then(serde_json::Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .map(serde_json::Value::to_string)
                            .collect::<std::collections::HashSet<_>>()
                    })
                    .unwrap_or_default();
                for option in options.keys() {
                    if !declared.contains(&serde_json::Value::String(option.clone()).to_string())
                        && !declared.contains(option)
                    {
                        return Err(format!(
                            "locale {locale} references unknown enum option '{name}.{option}'"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn parameter_schema_paths(schema: &Value) -> BTreeMap<String, Value> {
    let mut paths = BTreeMap::new();
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            collect_parameter_schema_paths(name, property, &mut paths);
        }
    }
    paths
}

fn collect_parameter_schema_paths(path: &str, schema: &Value, paths: &mut BTreeMap<String, Value>) {
    paths.insert(path.to_owned(), schema.clone());
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            collect_parameter_schema_paths(&format!("{path}.{name}"), property, paths);
        }
    }
    if let Some(items) = schema.get("items") {
        let item_path = format!("{path}[]");
        collect_parameter_schema_paths(&item_path, items, paths);
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
    pub workflow_version_id: Option<WorkflowVersionId>,
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

    #[test]
    fn canvas_role_is_controlled_and_ui_schema_remains_optional() {
        let schema: NodeUiSchema = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(schema.canvas.is_none());
        let appearance: CanvasAppearance =
            serde_json::from_value(serde_json::json!({"role": "sub_workflow"})).unwrap();
        assert_eq!(appearance.role, CanvasNodeRole::SubWorkflow);
        assert!(
            serde_json::from_value::<CanvasAppearance>(serde_json::json!({
                "role": "unknown"
            }))
            .is_err()
        );
    }
}
