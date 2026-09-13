use std::collections::{BTreeMap, BTreeSet};

use agentx_domain::{
    ContextDefinition, ContextWrite, ExecutionOrder, InputBinding, NodeSettings, ReferenceBinding,
    ResourceOperation, ResourceType, WorkflowEnd, WorkflowStart,
};
use agentx_node_protocol::{
    ExecutionStyle, NodeCapability, OutputCardinality, PluginNodeBinding, PortKind,
    ReadinessPolicy, SideEffectLevel,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledWorkflowV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub contract_version: u32,
    pub schema_version: String,
    pub compiler_version: String,
    pub canonical_hash: String,
    pub definition_hash: String,
    pub execution_order: ExecutionOrder,
    pub activation_budget: u32,
    pub start: WorkflowStart,
    pub contexts: BTreeMap<String, ContextDefinition>,
    pub end: WorkflowEnd,
    /// Exit nodes keyed by node id. Terminal materialization picks the exit
    /// that actually received the delivery.
    #[serde(default)]
    pub exits: BTreeMap<String, CompiledExitV1>,
    /// Enabled Exit ids in their original Definition order. Runtime result
    /// materialization must use this list instead of map-key ordering.
    #[serde(default)]
    pub exit_order: Vec<String>,
    pub nodes: Vec<CompiledNodeV1>,
    pub connections: Vec<CompiledConnectionV1>,
    pub terminal_connections: Vec<CompiledTerminalConnectionV1>,
    /// Exit node id when Start connects straight to an exit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_to_exit: Option<String>,
    pub start_nodes: Vec<usize>,
    pub strongly_connected_components: Vec<Vec<usize>>,
    pub subworkflow_version_ids: Vec<String>,
}

/// Per-exit mapping of global contract fields to dynamic values.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledExitV1 {
    #[serde(default)]
    pub outputs: BTreeMap<String, InputBinding>,
    #[serde(default)]
    pub error_outputs: BTreeMap<String, InputBinding>,
    #[serde(default)]
    pub protected: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledNodeV1 {
    pub index: usize,
    pub id: String,
    pub key: String,
    pub name: String,
    pub node_type: String,
    pub type_version: u32,
    pub parameters: Value,
    pub parameter_schema: Value,
    pub context_writes: Vec<ContextWrite>,
    pub settings: NodeSettings,
    pub capability: NodeCapability,
    pub execution_style: ExecutionStyle,
    pub readiness: ReadinessPolicy,
    pub required_input_ports: Vec<String>,
    pub output_ports: Vec<String>,
    #[serde(default)]
    pub variadic_output_ports: Vec<String>,
    /// True when the node has at least one outgoing `error` edge: a failed
    /// attempt then emits an error item on that branch instead of failing the
    /// workflow ("wiring is the policy").
    #[serde(default)]
    pub routes_error: bool,
    /// Definition id of the container node this node lives in (loop body).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_body: Option<CompiledLoopBodyV1>,
    pub effective_output_contract: EffectiveOutputContractV1,
    pub side_effect_level: SideEffectLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginNodeBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<CompiledAgentNodeV2>,
    pub incoming_connections: Vec<usize>,
    pub outgoing_connections: Vec<usize>,
    pub component_index: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionPolicyModeV2 {
    ApplicationSession,
    Invocation,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledAgentResourceReferenceV2 {
    pub binding_role: String,
    pub resource_type: ResourceType,
    pub resource_id: uuid::Uuid,
    pub resource_version_id: uuid::Uuid,
    pub operation: ResourceOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledAgentAttachmentV2 {
    pub binding_role: String,
    pub resource_type: ResourceType,
    pub resource_id: uuid::Uuid,
    pub resource_version_id: uuid::Uuid,
    pub operation: ResourceOperation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreToolReplayPolicyV2 {
    Safe,
    Never,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedCoreToolV2 {
    pub name: String,
    pub replay_policy: CoreToolReplayPolicyV2,
    pub workspace_sandbox_resource_id: uuid::Uuid,
    pub workspace_sandbox_version_id: uuid::Uuid,
}

/// Container execution contract for `loop_over_items`: the machine fans each
/// input item into one activation round of the body sub-DAG (distinct
/// generation per iteration) and aggregates the body sinks back into an
/// ordered array on the loop node's `main` output.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledLoopBodyV1 {
    pub entries: Vec<usize>,
    pub sinks: Vec<usize>,
    pub output_selector: ReferenceBinding,
    pub parallelism: u32,
    /// terminate | continue | remove
    pub error_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledAgentNodeV2 {
    pub contract_version: String,
    pub session_policy: AgentSessionPolicyModeV2,
    pub model: CompiledAgentResourceReferenceV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_sandbox: Option<CompiledAgentResourceReferenceV2>,
    #[serde(default)]
    pub attachments: Vec<CompiledAgentAttachmentV2>,
    #[serde(default)]
    pub core_tools: Vec<DerivedCoreToolV2>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveOutputContractV1 {
    /// Frozen per-port JSON Schemas after applying node-instance projections.
    pub port_schemas: BTreeMap<String, Value>,
    /// Frozen per-port item cardinality from the published Node Manifest.
    pub cardinalities: BTreeMap<String, OutputCardinality>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledConnectionV1 {
    pub index: usize,
    pub id: String,
    pub source_node: usize,
    pub source_port: String,
    pub source_port_kind: PortKind,
    pub target_node: usize,
    pub target_port: String,
    pub target_port_kind: PortKind,
    pub branch_order: u32,
    pub back_edge: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledTerminalConnectionV1 {
    pub id: String,
    pub source_node: usize,
    pub source_port: String,
    pub target_port: String,
    /// Exit node id this terminal connection delivers through.
    pub target_exit: String,
    pub branch_order: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerCompatibilityV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub protocol_version: u32,
    pub ir_versions: BTreeSet<u32>,
    pub compiler_versions: BTreeSet<String>,
    pub capabilities: BTreeSet<String>,
    pub manifest_versions: BTreeSet<String>,
}

pub type CompiledWorkflow = CompiledWorkflowV1;
pub type CompiledNode = CompiledNodeV1;
pub type CompiledConnection = CompiledConnectionV1;
pub type CompiledTerminalConnection = CompiledTerminalConnectionV1;
pub type CompiledExit = CompiledExitV1;
