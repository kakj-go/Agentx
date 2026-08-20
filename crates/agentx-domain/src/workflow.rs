use std::collections::{BTreeMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ResourceOperation, ResourceReference, ResourceType, WorkflowId, WorkflowVersionId};

pub const WORKFLOW_START_NODE_ID: &str = "__start__";
pub const WORKFLOW_END_NODE_ID: &str = "__end__";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDefinition {
    #[schemars(with = "WorkflowSchemaVersion")]
    pub schema_version: String,
    pub start: WorkflowStart,
    pub nodes: Vec<WorkflowNode>,
    pub connections: Vec<WorkflowConnection>,
    pub end: WorkflowEnd,
    #[serde(default)]
    pub settings: WorkflowSettings,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorDocument {
    #[serde(default)]
    pub node_layouts: Vec<NodeLayout>,
    #[serde(default)]
    pub boundary_layouts: Vec<BoundaryLayout>,
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
pub struct BoundaryLayout {
    pub boundary: WorkflowBoundary,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowBoundary {
    Start,
    End,
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
    #[serde(default = "default_annotation_width")]
    pub width: f64,
    #[serde(default = "default_annotation_height")]
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorGroup {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

const fn default_annotation_width() -> f64 {
    240.0
}

const fn default_annotation_height() -> f64 {
    160.0
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
    #[serde(rename = "5.0")]
    V5,
}

/// A persisted dynamic value is always a tagged object.  It deliberately
/// lives beside ordinary JSON rather than overloading strings, so business
/// payloads cannot accidentally become expressions.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum DynamicValue {
    Literal {
        value: Value,
    },
    Reference {
        selector: ValueSelector,
        #[serde(default, rename = "missingPolicy")]
        missing_policy: MissingValuePolicy,
    },
    Template {
        segments: Vec<TemplateSegment>,
    },
    Expression {
        root: ExpressionNode,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueSelector {
    pub namespace: ValueNamespace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(default)]
    pub run: ValueSelection,
    #[serde(default)]
    pub item: ValueSelection,
    #[serde(default)]
    pub path: Vec<ValuePathSegment>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueNamespace {
    Inputs,
    Outputs,
    Contexts,
    Execution,
    Item,
    Loop,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum ValueSelection {
    #[default]
    Current,
    First,
    Last,
    All,
    Index {
        index: u32,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ValuePathSegment {
    Key(String),
    Index(u32),
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum MissingValuePolicy {
    #[default]
    Error,
    Null,
    Default {
        value: Box<DynamicValue>,
    },
    Omit,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum TemplateSegment {
    Text {
        text: String,
    },
    Reference {
        selector: ValueSelector,
        #[serde(default, rename = "missingPolicy")]
        missing_policy: MissingValuePolicy,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum ExpressionNode {
    Literal {
        value: Value,
    },
    Reference {
        selector: ValueSelector,
        #[serde(default, rename = "missingPolicy")]
        missing_policy: MissingValuePolicy,
    },
    Unary {
        operator: ExpressionUnaryOperator,
        operand: Box<ExpressionNode>,
    },
    Binary {
        operator: ExpressionBinaryOperator,
        left: Box<ExpressionNode>,
        right: Box<ExpressionNode>,
    },
    Conditional {
        condition: Box<ExpressionNode>,
        then_value: Box<ExpressionNode>,
        else_value: Box<ExpressionNode>,
    },
    Call {
        function: ExpressionFunction,
        arguments: Vec<ExpressionNode>,
    },
    Array {
        items: Vec<ExpressionNode>,
    },
    Object {
        fields: BTreeMap<String, ExpressionNode>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionBinaryOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    And,
    Or,
    In,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionFunction {
    Contains,
    Double,
    Duration,
    EndsWith,
    Int,
    Matches,
    Max,
    Min,
    Size,
    StartsWith,
    String,
    Timestamp,
    Uint,
}

impl WorkflowDefinition {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: "5.0".to_owned(),
            start: WorkflowStart::default(),
            nodes: Vec::new(),
            connections: Vec::new(),
            end: WorkflowEnd::default(),
            settings: WorkflowSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStart {
    #[serde(default)]
    pub inputs: Value,
    #[serde(default)]
    pub contexts: BTreeMap<String, ContextDefinition>,
}

impl Default for WorkflowStart {
    fn default() -> Self {
        Self {
            inputs: serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
            contexts: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextDefinition {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schema: Value,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub mutable: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub client_writable: bool,
    #[serde(default)]
    pub scope: ContextScope,
    #[serde(default)]
    pub max_size: Option<u64>,
    #[serde(default)]
    pub merge_policy: ContextMergePolicy,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextScope {
    #[default]
    ExecutionTree,
    Session,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMergePolicy {
    #[default]
    Replace,
    Append,
    MergeObject,
    Increment,
    RejectConflict,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowEnd {
    #[serde(default)]
    pub outputs: BTreeMap<String, WorkflowOutput>,
    #[serde(default)]
    pub error: WorkflowErrorEnd,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowErrorEnd {
    #[serde(default)]
    pub strategy: EndErrorStrategy,
    #[serde(default = "default_error_collect_window_ms")]
    #[schemars(range(min = 100, max = 60_000))]
    pub collect_window_ms: u64,
    #[serde(default)]
    pub outputs: BTreeMap<String, WorkflowOutput>,
}

impl Default for WorkflowErrorEnd {
    fn default() -> Self {
        Self {
            strategy: EndErrorStrategy::FailFast,
            collect_window_ms: default_error_collect_window_ms(),
            outputs: BTreeMap::new(),
        }
    }
}

const fn default_error_collect_window_ms() -> u64 {
    5_000
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndErrorStrategy {
    #[default]
    FailFast,
    Collect,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowOutput {
    pub value: DynamicValue,
    #[serde(default)]
    pub schema: Value,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextWrite {
    pub operation: ContextWriteOperation,
    pub path: String,
    pub value: DynamicValue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWriteOperation {
    Set,
    SetIfAbsent,
    Delete,
    Append,
    MergeObject,
    Increment,
    Min,
    Max,
    CompareAndSet,
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
    pub key: String,
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
    pub output_projection: BTreeMap<String, BTreeMap<String, OutputProjectionField>>,
    #[serde(default)]
    pub context_writes: Vec<ContextWrite>,
    #[serde(default)]
    pub resource_references: Vec<ResourceReference>,
    #[serde(default)]
    pub settings: NodeSettings,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputProjectionField {
    pub value: DynamicValue,
    #[serde(default)]
    pub schema: Value,
    #[serde(default)]
    pub sensitive: bool,
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
    if definition.schema_version != "5.0" {
        issue(
            &mut issues,
            "UNSUPPORTED_SCHEMA",
            "schemaVersion",
            "Only schema version 5.0 is supported",
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
    if !definition.start.inputs.is_object() {
        issue(
            &mut issues,
            "INVALID_INPUT_SCHEMA",
            "start.inputs",
            "Start inputs must be a JSON Schema object",
        );
    }
    for (name, context) in &definition.start.contexts {
        if !valid_reference_key(name) {
            issue(
                &mut issues,
                "INVALID_CONTEXT_KEY",
                &format!("start.contexts.{name}"),
                "Context keys must use lowercase ASCII letters, digits and underscores",
            );
        }
        if !context.schema.is_object() {
            issue(
                &mut issues,
                "INVALID_CONTEXT_SCHEMA",
                &format!("start.contexts.{name}.schema"),
                "Context schema must be an object",
            );
        }
        if context.max_size == Some(0) {
            issue(
                &mut issues,
                "INVALID_CONTEXT_MAX_SIZE",
                &format!("start.contexts.{name}.maxSize"),
                "Context maxSize must be greater than zero",
            );
        }
    }
    validate_workflow_outputs("end.outputs", &definition.end.outputs, &mut issues);
    validate_workflow_outputs(
        "end.error.outputs",
        &definition.end.error.outputs,
        &mut issues,
    );
    if !(100..=60_000).contains(&definition.end.error.collect_window_ms) {
        issue(
            &mut issues,
            "INVALID_ERROR_COLLECT_WINDOW",
            "end.error.collectWindowMs",
            "Error collectWindowMs must be between 100 and 60000 milliseconds",
        );
    }
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for (index, node) in definition.nodes.iter().enumerate() {
        if matches!(
            node.id.as_str(),
            WORKFLOW_START_NODE_ID | WORKFLOW_END_NODE_ID
        ) {
            issue(
                &mut issues,
                "RESERVED_NODE_ID",
                &format!("nodes[{index}].id"),
                "Node id is reserved for a Workflow boundary",
            );
        } else if node.id.is_empty() || node.id.len() > 128 {
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
        if !valid_reference_key(&node.key) {
            issue(
                &mut issues,
                "INVALID_NODE_KEY",
                &format!("nodes[{index}].key"),
                "Node key must use lowercase ASCII letters, digits and underscores",
            );
        } else if !keys.insert(node.key.as_str()) {
            issue(
                &mut issues,
                "DUPLICATE_NODE_KEY",
                &format!("nodes[{index}].key"),
                "Node key must be unique within a workflow",
            );
        }
        if matches!(node.node_type.as_str(), "manual_trigger" | "remote_trigger") {
            issue(
                &mut issues,
                "TRIGGER_NODE_REMOVED",
                &format!("nodes[{index}].type"),
                "Trigger nodes were removed in Workflow Definition 5.0; configure a Trigger Binding instead",
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
        if node.settings.max_tries == 0 {
            issue(
                &mut issues,
                "INVALID_MAX_TRIES",
                &format!("nodes[{index}].settings.maxTries"),
                "maxTries must be greater than zero",
            );
        }
        for (port, fields) in &node.output_projection {
            if port.trim().is_empty() {
                issue(
                    &mut issues,
                    "INVALID_PROJECTION_PORT",
                    &format!("nodes[{index}].outputProjection"),
                    "Projection output port is required",
                );
            }
            for (name, field) in fields {
                let path = format!("nodes[{index}].outputProjection.{port}.{name}");
                if !valid_reference_key(name) {
                    issue(
                        &mut issues,
                        "INVALID_PROJECTION_FIELD_KEY",
                        &path,
                        "Projection field keys must use lowercase ASCII letters, digits and underscores",
                    );
                }
                if !field.schema.is_object() {
                    issue(
                        &mut issues,
                        "INVALID_PROJECTION_SCHEMA",
                        &format!("{path}.schema"),
                        "Projection field schema must be an object",
                    );
                }
            }
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
        if !connection_orders.insert((
            connection.source_node_id.as_str(),
            connection.source_handle.as_str(),
            connection.order,
        )) {
            issue(
                &mut issues,
                "DUPLICATE_CONNECTION_ORDER",
                &format!("connections[{index}].order"),
                "Connection order must be unique for each source node and source handle",
            );
        }
        let source_is_start = connection.source_node_id == WORKFLOW_START_NODE_ID;
        let source_is_end = connection.source_node_id == WORKFLOW_END_NODE_ID;
        let target_is_start = connection.target_node_id == WORKFLOW_START_NODE_ID;
        let target_is_end = connection.target_node_id == WORKFLOW_END_NODE_ID;
        if source_is_end || target_is_start {
            issue(
                &mut issues,
                "INVALID_BOUNDARY_DIRECTION",
                &format!("connections[{index}]"),
                "Start can only be a connection source and End can only be a connection target",
            );
            continue;
        }
        if source_is_start && connection.source_handle != "main" {
            issue(
                &mut issues,
                "INVALID_START_PORT",
                &format!("connections[{index}].sourceHandle"),
                "Start only exposes the main output port",
            );
        }
        if target_is_end && !matches!(connection.target_handle.as_str(), "main" | "error") {
            issue(
                &mut issues,
                "INVALID_END_PORT",
                &format!("connections[{index}].targetHandle"),
                "End only accepts main or error input ports",
            );
        }
        if source_is_start && target_is_end && connection.target_handle != "main" {
            issue(
                &mut issues,
                "INVALID_DIRECT_ERROR_CONNECTION",
                &format!("connections[{index}]"),
                "Start can only connect directly to End.main",
            );
        }
        if (!source_is_start && !ids.contains(&connection.source_node_id))
            || (!target_is_end && !ids.contains(&connection.target_node_id))
        {
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

fn validate_workflow_outputs(
    path: &str,
    outputs: &BTreeMap<String, WorkflowOutput>,
    issues: &mut Vec<DefinitionIssue>,
) {
    for (name, output) in outputs {
        if !valid_reference_key(name) {
            issue(
                issues,
                "INVALID_END_OUTPUT_KEY",
                &format!("{path}.{name}"),
                "End output keys must use lowercase ASCII letters, digits and underscores",
            );
        }
        if !output.schema.is_object() {
            issue(
                issues,
                "INVALID_END_OUTPUT_SCHEMA",
                &format!("{path}.{name}.schema"),
                "End output schema must be an object",
            );
        }
    }
}

fn valid_reference_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
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
    let mut boundaries = HashSet::new();
    for (index, layout) in document.boundary_layouts.iter().enumerate() {
        if !boundaries.insert(layout.boundary) {
            issue(
                &mut issues,
                "DUPLICATE_BOUNDARY_LAYOUT",
                &format!("boundaryLayouts[{index}].boundary"),
                "Each Workflow boundary may have only one layout",
            );
        }
        if !layout.x.is_finite() || !layout.y.is_finite() {
            issue(
                &mut issues,
                "INVALID_BOUNDARY_LAYOUT",
                &format!("boundaryLayouts[{index}]"),
                "Boundary layout coordinates must be finite",
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
    let mut annotation_ids = HashSet::new();
    for (index, annotation) in document.annotations.iter().enumerate() {
        if annotation.id.is_empty() || !annotation_ids.insert(annotation.id.as_str()) {
            issue(
                &mut issues,
                "INVALID_ANNOTATION_ID",
                &format!("annotations[{index}].id"),
                "Annotation ids must be present and unique",
            );
        }
        if !annotation.x.is_finite()
            || !annotation.y.is_finite()
            || !annotation.width.is_finite()
            || annotation.width < 150.0
            || !annotation.height.is_finite()
            || annotation.height < 80.0
        {
            issue(
                &mut issues,
                "INVALID_ANNOTATION_LAYOUT",
                &format!("annotations[{index}]"),
                "Annotation coordinates must be finite and dimensions must be at least 150 by 80",
            );
        }
    }
    let mut group_ids = HashSet::new();
    for (index, group) in document.groups.iter().enumerate() {
        if group.id.is_empty() || !group_ids.insert(group.id.as_str()) {
            issue(
                &mut issues,
                "INVALID_GROUP_ID",
                &format!("groups[{index}].id"),
                "Group ids must be present and unique",
            );
        }
        if group.node_ids.is_empty()
            || group
                .node_ids
                .iter()
                .any(|node_id| !node_ids.contains(node_id.as_str()))
        {
            issue(
                &mut issues,
                "INVALID_GROUP_MEMBERS",
                &format!("groups[{index}].nodeIds"),
                "Groups must reference at least one existing workflow node",
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
        Value::Number(number) => canonicalize_number(number),
        other => other.clone(),
    }
}

fn canonicalize_number(number: &serde_json::Number) -> Value {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

    let Some(value) = number.as_f64() else {
        return Value::Number(number.clone());
    };
    if number.is_f64()
        && value.fract() == 0.0
        && (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value)
    {
        return Value::Number((value as i64).into());
    }
    Value::Number(number.clone())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EditorDocument, WorkflowDefinition, canonical_content_hash, validate_definition};

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
    fn canonical_hash_normalizes_json_equivalent_integral_floats() {
        assert_eq!(
            canonical_content_hash(&json!({"x": 120.0, "y": -0.0, "zoom": 1.0})).unwrap(),
            canonical_content_hash(&json!({"x": 120, "y": 0, "zoom": 1})).unwrap()
        );
        assert_ne!(
            canonical_content_hash(&json!({"x": 1.5})).unwrap(),
            canonical_content_hash(&json!({"x": 1})).unwrap()
        );
    }

    #[test]
    fn empty_definition_is_valid() {
        assert!(validate_definition(&WorkflowDefinition::empty()).is_empty());
    }

    #[test]
    fn editor_annotations_and_groups_apply_backward_compatible_defaults() {
        let document: EditorDocument = serde_json::from_value(json!({
            "annotations": [{"id": "note", "text": "Remember", "x": 10, "y": 20}],
            "groups": [{"id": "group", "label": "Main", "nodeIds": ["node"]}]
        }))
        .unwrap();
        assert_eq!(document.annotations[0].width, 240.0);
        assert_eq!(document.annotations[0].height, 160.0);
        assert!(!document.groups[0].collapsed);
    }

    #[test]
    fn rejects_dangling_cycles_and_mismatched_resources() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[
                {"id":"root","key":"root","type":"no_op","typeVersion":1,"name":"Root","outputProjection":{},"contextWrites":[],"resourceReferences":[]},
                {"id":"model","key":"model","type":"model","typeVersion":1,"name":"Model","outputProjection":{},"contextWrites":[],"resourceReferences":[{"resourceType":"mcp_tool","resourceId":"018f47a0-7e9c-7000-8000-000000000001","operation":"use"}]}
            ],
            "connections":[
                {"id":"edge","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"model","targetHandle":"main","order":0},
                {"id":"edge","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"root","targetHandle":"main","order":0},
                {"id":"missing","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"absent","targetHandle":"main","order":1}
            ],
            "end":{"outputs":{}},
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
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[
                {"id":"root","key":"root","type":"no_op","typeVersion":1,"name":"Root","outputProjection":{},"contextWrites":[]},
                {"id":"loop","key":"loop","type":"loop_over_items","typeVersion":1,"name":"Loop","outputProjection":{},"contextWrites":[]}
            ],
            "connections":[
                {"id":"start","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"loop","targetHandle":"main","order":0},
                {"id":"back","sourceNodeId":"loop","sourceHandle":"loop","targetNodeId":"loop","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        })).unwrap();
        assert!(validate_definition(&definition).is_empty());
    }

    #[test]
    fn connection_order_is_scoped_to_the_source_port() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[
                {"id":"source","key":"source","type":"switch","typeVersion":1,"name":"Source","outputProjection":{},"contextWrites":[]},
                {"id":"left","key":"left","type":"set","typeVersion":1,"name":"Left","outputProjection":{},"contextWrites":[]},
                {"id":"right","key":"right","type":"set","typeVersion":1,"name":"Right","outputProjection":{},"contextWrites":[]}
            ],
            "connections":[
                {"id":"left-edge","sourceNodeId":"source","sourceHandle":"case:0","targetNodeId":"left","targetHandle":"main","order":0},
                {"id":"right-edge","sourceNodeId":"source","sourceHandle":"case:1","targetNodeId":"right","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        })).unwrap();
        assert!(validate_definition(&definition).is_empty());

        let mut duplicate = definition;
        duplicate.connections[1].source_handle = "case:0".into();
        assert!(
            validate_definition(&duplicate)
                .iter()
                .any(|issue| issue.code == "DUPLICATE_CONNECTION_ORDER")
        );
    }
}
