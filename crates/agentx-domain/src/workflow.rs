use std::collections::{BTreeMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ResourceOperation, ResourceReference, ResourceType, WorkflowId, WorkflowVersionId};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDefinition {
    #[schemars(with = "WorkflowSchemaVersion")]
    pub schema_version: String,
    pub nodes: Vec<WorkflowNode>,
    pub connections: Vec<WorkflowConnection>,
    #[serde(default)]
    pub settings: WorkflowSettings,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorDocument {
    #[serde(default)]
    pub node_layouts: Vec<NodeLayout>,
    #[serde(default)]
    pub binding_layouts: Vec<BindingLayout>,
    #[serde(default)]
    pub edges: Vec<EditorEdge>,
    #[serde(default)]
    pub binding_edges: Vec<BindingEdge>,
    #[serde(default)]
    pub annotations: Vec<EditorAnnotation>,
    #[serde(default)]
    pub groups: Vec<EditorGroup>,
    #[serde(default)]
    pub viewport: EditorViewport,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeLayout {
    pub node_id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingLayout {
    pub binding_id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorEdge {
    pub edge_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_position: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingEdge {
    pub edge_id: String,
    pub source_binding_id: String,
    pub target_node_id: String,
    pub target_slot: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorAnnotation {
    pub id: String,
    pub text: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorGroup {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorViewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for EditorViewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ExecutionSource {
    Version {
        version_id: WorkflowVersionId,
    },
    DraftRevision {
        workflow_id: WorkflowId,
        revision: u64,
    },
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugPlan {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub included_node_ids: Vec<String>,
    #[serde(default)]
    pub skipped_node_ids: Vec<String>,
    #[serde(default)]
    pub input_source: Option<Value>,
    #[serde(default)]
    pub side_effect_decisions: Value,
    #[serde(default)]
    pub overlay_hash: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowSchemaVersion {
    #[serde(rename = "3.0")]
    V3,
}

impl WorkflowDefinition {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: "3.0".to_owned(),
            nodes: vec![WorkflowNode {
                id: "manual-trigger".to_owned(),
                node_type: "manual_trigger".to_owned(),
                type_version: 1,
                name: "Manual Trigger".to_owned(),
                disabled: false,
                parameters: Value::Object(Default::default()),
                resource_references: Vec::new(),
                settings: NodeSettings::default(),
            }],
            connections: Vec::new(),
            settings: WorkflowSettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOrder {
    #[default]
    Deterministic,
    Parallel,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowSettings {
    #[serde(default)]
    pub execution_order: ExecutionOrder,
    #[serde(default = "default_activation_budget")]
    #[schemars(range(min = 1))]
    pub activation_budget: u32,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            execution_order: ExecutionOrder::Deterministic,
            activation_budget: default_activation_budget(),
            timeout_ms: None,
        }
    }
}

const fn default_activation_budget() -> u32 {
    10_000
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeErrorPolicy {
    #[default]
    StopWorkflow,
    ContinueRegularOutput,
    ContinueErrorOutput,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeSettings {
    #[serde(default)]
    pub execute_once: bool,
    #[serde(default)]
    pub always_output_data: bool,
    #[serde(default)]
    pub retry_on_fail: bool,
    #[serde(default = "default_max_tries")]
    pub max_tries: u16,
    #[serde(default)]
    pub wait_between_tries_ms: u64,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub on_error: NodeErrorPolicy,
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            execute_once: false,
            always_output_data: false,
            retry_on_fail: false,
            max_tries: default_max_tries(),
            wait_between_tries_ms: 0,
            timeout_ms: None,
            on_error: NodeErrorPolicy::StopWorkflow,
        }
    }
}

const fn default_max_tries() -> u16 {
    1
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[schemars(range(min = 1))]
    pub type_version: u32,
    pub name: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub resource_references: Vec<ResourceReference>,
    #[serde(default)]
    pub settings: NodeSettings,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowConnection {
    pub id: String,
    pub source_node_id: String,
    pub source_handle: String,
    pub target_node_id: String,
    pub target_handle: String,
    #[schemars(range(min = 0))]
    pub order: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[must_use]
pub fn validate_definition(definition: &WorkflowDefinition) -> Vec<DefinitionIssue> {
    let mut issues = Vec::new();
    if definition.schema_version != "3.0" {
        issue(
            &mut issues,
            "UNSUPPORTED_SCHEMA",
            "schemaVersion",
            "Only schema version 3.0 is supported",
        );
    }
    if definition.settings.activation_budget == 0 {
        issue(
            &mut issues,
            "INVALID_ACTIVATION_BUDGET",
            "settings.activationBudget",
            "Activation budget must be greater than zero",
        );
    }
    let mut ids = HashSet::new();
    for (index, node) in definition.nodes.iter().enumerate() {
        if node.id.is_empty() || node.id.len() > 128 {
            issue(
                &mut issues,
                "INVALID_NODE_ID",
                &format!("nodes[{index}].id"),
                "Node id must contain 1 to 128 characters",
            );
        } else if !ids.insert(node.id.clone()) {
            issue(
                &mut issues,
                "DUPLICATE_NODE_ID",
                &format!("nodes[{index}].id"),
                "Node id must be unique",
            );
        }
        if node.node_type.is_empty()
            || node.node_type.len() > 128
            || !node.node_type.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            })
        {
            issue(
                &mut issues,
                "INVALID_NODE_TYPE",
                &format!("nodes[{index}].type"),
                "Node type must use lowercase ASCII letters, digits, '.', '_' or '-'",
            );
        }
        if node.name.trim().is_empty() || node.name.chars().count() > 160 {
            issue(
                &mut issues,
                "INVALID_NODE_NAME",
                &format!("nodes[{index}].name"),
                "Node name must contain 1 to 160 characters",
            );
        }
        if node.type_version == 0 {
            issue(
                &mut issues,
                "UNSUPPORTED_NODE_VERSION",
                &format!("nodes[{index}].typeVersion"),
                "Node type version must be greater than zero",
            );
        }
        if is_resource_node(&node.node_type) && node.resource_references.is_empty() {
            issue(
                &mut issues,
                "RESOURCE_REQUIRED",
                &format!("nodes[{index}].resourceReferences"),
                "Resource nodes require at least one resource reference",
            );
        }
        if node.settings.max_tries == 0 {
            issue(
                &mut issues,
                "INVALID_MAX_TRIES",
                &format!("nodes[{index}].settings.maxTries"),
                "maxTries must be greater than zero",
            );
        }
        for (reference_index, reference) in node.resource_references.iter().enumerate() {
            if is_resource_node(&node.node_type)
                && !reference_matches_node(&node.node_type, reference)
            {
                issue(
                    &mut issues,
                    "INVALID_RESOURCE_REFERENCE",
                    &format!("nodes[{index}].resourceReferences[{reference_index}]"),
                    "Resource type or operation does not match the node type",
                );
            }
        }
    }
    let mut connection_ids = HashSet::new();
    let mut connection_orders = HashSet::new();
    for (index, connection) in definition.connections.iter().enumerate() {
        if connection.id.is_empty() || !connection_ids.insert(connection.id.as_str()) {
            issue(
                &mut issues,
                "INVALID_CONNECTION_ID",
                &format!("connections[{index}].id"),
                "Connection id must be present and unique",
            );
        }
        if connection.source_handle.is_empty() || connection.target_handle.is_empty() {
            issue(
                &mut issues,
                "INVALID_CONNECTION_HANDLE",
                &format!("connections[{index}]"),
                "Connection handles are required",
            );
        }
        if !connection_orders.insert((connection.source_node_id.as_str(), connection.order)) {
            issue(
                &mut issues,
                "DUPLICATE_CONNECTION_ORDER",
                &format!("connections[{index}].order"),
                "Connection order must be unique for each source node",
            );
        }
        if !ids.contains(&connection.source_node_id) || !ids.contains(&connection.target_node_id) {
            issue(
                &mut issues,
                "DANGLING_CONNECTION",
                &format!("connections[{index}]"),
                "Connection references a missing node",
            );
            continue;
        }
    }
    issues
}

#[must_use]
pub fn validate_editor_document(
    definition: &WorkflowDefinition,
    document: &EditorDocument,
) -> Vec<DefinitionIssue> {
    let mut issues = Vec::new();
    let node_ids = definition
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<HashSet<_>>();
    let binding_ids = definition
        .nodes
        .iter()
        .flat_map(|node| node.resource_references.iter())
        .filter_map(|reference| reference.binding_id.as_deref())
        .collect::<HashSet<_>>();
    let mut layout_nodes = HashSet::new();
    for (index, layout) in document.node_layouts.iter().enumerate() {
        if !node_ids.contains(layout.node_id.as_str()) {
            issue(
                &mut issues,
                "DANGLING_EDITOR_NODE",
                &format!("nodeLayouts[{index}].nodeId"),
                "Node layout references a missing workflow node",
            );
        }
        if !layout_nodes.insert(layout.node_id.as_str()) {
            issue(
                &mut issues,
                "DUPLICATE_NODE_LAYOUT",
                &format!("nodeLayouts[{index}].nodeId"),
                "Each workflow node may have only one layout",
            );
        }
        if !layout.x.is_finite()
            || !layout.y.is_finite()
            || layout
                .width
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || layout
                .height
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            issue(
                &mut issues,
                "INVALID_NODE_LAYOUT",
                &format!("nodeLayouts[{index}]"),
                "Node layout coordinates and dimensions must be finite and positive",
            );
        }
    }
    let mut layout_bindings = HashSet::new();
    for (index, layout) in document.binding_layouts.iter().enumerate() {
        if !binding_ids.contains(layout.binding_id.as_str()) {
            issue(
                &mut issues,
                "DANGLING_BINDING_LAYOUT",
                &format!("bindingLayouts[{index}].bindingId"),
                "Binding layout references a missing resource binding",
            );
        }
        if !layout_bindings.insert(layout.binding_id.as_str()) {
            issue(
                &mut issues,
                "DUPLICATE_BINDING_LAYOUT",
                &format!("bindingLayouts[{index}].bindingId"),
                "Each resource binding may have only one layout",
            );
        }
    }
    for (index, edge) in document.binding_edges.iter().enumerate() {
        if !binding_ids.contains(edge.source_binding_id.as_str()) {
            issue(
                &mut issues,
                "DANGLING_BINDING_EDGE",
                &format!("bindingEdges[{index}].sourceBindingId"),
                "Binding edge references a missing resource binding",
            );
        }
        if !node_ids.contains(edge.target_node_id.as_str()) {
            issue(
                &mut issues,
                "DANGLING_BINDING_TARGET",
                &format!("bindingEdges[{index}].targetNodeId"),
                "Binding edge references a missing workflow node",
            );
        }
    }
    if !document.viewport.x.is_finite()
        || !document.viewport.y.is_finite()
        || !document.viewport.zoom.is_finite()
        || document.viewport.zoom <= 0.0
    {
        issue(
            &mut issues,
            "INVALID_VIEWPORT",
            "viewport",
            "Viewport coordinates must be finite and zoom must be positive",
        );
    }
    issues
}

fn is_resource_node(node_type: &str) -> bool {
    matches!(
        node_type,
        "model" | "mcp_tool" | "skill" | "rag" | "memory" | "agent" | "code"
    )
}

fn reference_matches_node(node_type: &str, reference: &ResourceReference) -> bool {
    match node_type {
        "manual_trigger" => false,
        "model" => {
            reference.resource_type == ResourceType::Model
                && reference.operation == ResourceOperation::Use
        }
        "mcp_tool" => {
            reference.resource_type == ResourceType::McpTool
                && reference.operation == ResourceOperation::Use
        }
        "skill" => {
            reference.resource_type == ResourceType::Skill
                && reference.operation == ResourceOperation::Use
        }
        "rag" => {
            reference.resource_type == ResourceType::Rag
                && matches!(
                    reference.operation,
                    ResourceOperation::Read | ResourceOperation::Write
                )
        }
        "agent" => matches!(
            reference.resource_type,
            ResourceType::Model
                | ResourceType::McpTool
                | ResourceType::Skill
                | ResourceType::Rag
                | ResourceType::Memory
                | ResourceType::Credential
        ),
        "code" => matches!(
            (reference.resource_type, reference.operation),
            (ResourceType::SandboxProfile, ResourceOperation::Use)
                | (ResourceType::Credential, ResourceOperation::Use)
        ),
        "memory" => {
            reference.resource_type == ResourceType::Memory
                && matches!(
                    reference.operation,
                    ResourceOperation::Read | ResourceOperation::Write
                )
        }
        _ => false,
    }
}

fn issue(issues: &mut Vec<DefinitionIssue>, code: &str, path: &str, message: &str) {
    issues.push(DefinitionIssue {
        code: code.to_owned(),
        path: path.to_owned(),
        message: message.to_owned(),
    });
}

pub fn canonical_content_hash(value: &Value) -> Result<String, serde_json::Error> {
    let canonical = canonicalize(value);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(format!("sha256:v1:{:x}", Sha256::digest(bytes)))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{WorkflowDefinition, canonical_content_hash, validate_definition};

    #[test]
    fn canonical_hash_ignores_object_order_but_keeps_array_order() {
        assert_eq!(
            canonical_content_hash(&json!({"b": 2, "a": 1})).unwrap(),
            canonical_content_hash(&json!({"a": 1, "b": 2})).unwrap()
        );
        assert_ne!(
            canonical_content_hash(&json!([1, 2])).unwrap(),
            canonical_content_hash(&json!([2, 1])).unwrap()
        );
    }

    #[test]
    fn empty_definition_is_valid() {
        assert!(validate_definition(&WorkflowDefinition::empty()).is_empty());
    }

    #[test]
    fn rejects_dangling_cycles_and_mismatched_resources() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"3.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger","resourceReferences":[]},
                {"id":"model","type":"model","typeVersion":1,"name":"Model","resourceReferences":[{"resourceType":"mcp_tool","resourceId":"018f47a0-7e9c-7000-8000-000000000001","operation":"use"}]}
            ],
            "connections":[
                {"id":"edge","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"model","targetHandle":"main","order":0},
                {"id":"edge","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"trigger","targetHandle":"main","order":0},
                {"id":"missing","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"absent","targetHandle":"main","order":1}
            ],
            "settings":{}
        })).unwrap();
        let codes: Vec<_> = validate_definition(&definition)
            .into_iter()
            .map(|issue| issue.code)
            .collect();
        assert!(codes.contains(&"INVALID_RESOURCE_REFERENCE".to_owned()));
        assert!(codes.contains(&"INVALID_CONNECTION_ID".to_owned()));
        assert!(codes.contains(&"DANGLING_CONNECTION".to_owned()));
        assert!(!codes.contains(&"CYCLE_NOT_ALLOWED".to_owned()));
    }

    #[test]
    fn accepts_controlled_cycles() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"3.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger",},
                {"id":"loop","type":"set","typeVersion":1,"name":"Loop",}
            ],
            "connections":[
                {"id":"start","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"loop","targetHandle":"main","order":0},
                {"id":"back","sourceNodeId":"loop","sourceHandle":"loop","targetNodeId":"loop","targetHandle":"main","order":0}
            ]
        })).unwrap();
        assert!(validate_definition(&definition).is_empty());
    }
}
