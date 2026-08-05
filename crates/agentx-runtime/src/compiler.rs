use std::collections::{BTreeMap, BTreeSet, VecDeque};

use agentx_domain::{
    ExecutionOrder, NodeSettings, WorkflowDefinition, WorkflowNode, canonical_content_hash,
    validate_definition,
};
use agentx_node_protocol::{
    ExecutionStyle, NodeCapability, NodeManifestVersion, ReadinessPolicy, SideEffectLevel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{ExpressionEngine, NodeRegistry};

pub const COMPILER_VERSION: &str = "agentx-workflow-3.0.0";

#[derive(Clone, Debug, Default)]
pub struct CompileContext {
    pub current_workflow_version_id: Option<String>,
    pub ancestor_workflow_version_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error)]
#[error("workflow compilation failed with {issues_len} issue(s)")]
pub struct CompileError {
    pub issues: Vec<CompileIssue>,
    issues_len: usize,
}

impl CompileError {
    fn new(issues: Vec<CompileIssue>) -> Self {
        let issues_len = issues.len();
        Self { issues, issues_len }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledWorkflow {
    pub schema_version: String,
    pub compiler_version: String,
    pub canonical_hash: String,
    pub definition_hash: String,
    pub execution_order: ExecutionOrder,
    pub activation_budget: u32,
    pub nodes: Vec<CompiledNode>,
    pub connections: Vec<CompiledConnection>,
    pub start_nodes: Vec<usize>,
    pub strongly_connected_components: Vec<Vec<usize>>,
    pub subworkflow_version_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledNode {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub type_version: u32,
    pub parameters: Value,
    pub settings: NodeSettings,
    pub capability: NodeCapability,
    pub execution_style: ExecutionStyle,
    pub readiness: ReadinessPolicy,
    pub required_input_ports: Vec<String>,
    pub output_ports: Vec<String>,
    pub side_effect_level: SideEffectLevel,
    pub incoming_connections: Vec<usize>,
    pub outgoing_connections: Vec<usize>,
    pub component_index: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledConnection {
    pub index: usize,
    pub id: String,
    pub source_node: usize,
    pub source_port: String,
    pub target_node: usize,
    pub target_port: String,
    pub branch_order: u32,
    pub back_edge: bool,
}

pub struct WorkflowCompiler<'a> {
    registry: &'a NodeRegistry,
    expressions: ExpressionEngine,
}

impl<'a> WorkflowCompiler<'a> {
    #[must_use]
    pub fn new(registry: &'a NodeRegistry) -> Self {
        Self {
            registry,
            expressions: ExpressionEngine,
        }
    }

    pub fn compile(
        &self,
        definition: &WorkflowDefinition,
        context: &CompileContext,
    ) -> Result<CompiledWorkflow, CompileError> {
        let mut issues = validate_definition(definition)
            .into_iter()
            .map(|issue| CompileIssue {
                code: issue.code,
                path: issue.path,
                message: issue.message,
            })
            .collect::<Vec<_>>();
        let enabled = definition
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.disabled)
            .collect::<Vec<_>>();
        let indexes = enabled
            .iter()
            .enumerate()
            .map(|(compiled, (_, node))| (node.id.as_str(), compiled))
            .collect::<BTreeMap<_, _>>();

        let mut manifests: Vec<Option<NodeManifestVersion>> = Vec::with_capacity(enabled.len());
        let mut subworkflows = BTreeSet::new();
        for (definition_index, node) in &enabled {
            let path = format!("nodes[{definition_index}]");
            let manifest = self
                .registry
                .get(&node.node_type, node.type_version)
                .cloned();
            if manifest.is_none() {
                issues.push(CompileIssue {
                    code: "UNKNOWN_NODE_VERSION".into(),
                    path: format!("{path}.typeVersion"),
                    message: format!(
                        "Node {}@{} is not registered",
                        node.node_type, node.type_version
                    ),
                });
            }
            if let Some(manifest) = &manifest {
                validate_binding_slots(*definition_index, node, manifest, &mut issues);
                validate_parameters(*definition_index, node, manifest, &mut issues);
            }
            validate_expressions(
                &self.expressions,
                &node.parameters,
                &format!("{path}.parameters"),
                &mut issues,
            );
            if node.node_type == "sub_workflow" {
                match node
                    .parameters
                    .get("workflowVersionId")
                    .and_then(Value::as_str)
                {
                    Some(version) => {
                        if context.current_workflow_version_id.as_deref() == Some(version)
                            || context.ancestor_workflow_version_ids.contains(version)
                        {
                            issues.push(CompileIssue {
                                code: "RECURSIVE_SUBWORKFLOW".into(),
                                path: format!("{path}.parameters.workflowVersionId"),
                                message: "Sub-workflow recursion is not allowed".into(),
                            });
                        }
                        subworkflows.insert(version.to_owned());
                    }
                    None => issues.push(CompileIssue {
                        code: "SUBWORKFLOW_VERSION_REQUIRED".into(),
                        path: format!("{path}.parameters.workflowVersionId"),
                        message: "Sub-workflow must reference an immutable Workflow Version".into(),
                    }),
                }
            }
            manifests.push(manifest);
        }

        let mut raw_connections = Vec::new();
        for (definition_index, connection) in definition.connections.iter().enumerate() {
            let (Some(&source), Some(&target)) = (
                indexes.get(connection.source_node_id.as_str()),
                indexes.get(connection.target_node_id.as_str()),
            ) else {
                continue;
            };
            if let Some(manifest) = &manifests[source]
                && !port_matches(&manifest.output_ports, &connection.source_handle)
            {
                issues.push(CompileIssue {
                    code: "UNKNOWN_SOURCE_PORT".into(),
                    path: format!("connections[{definition_index}].sourceHandle"),
                    message: format!(
                        "Port '{}' is not declared by {}",
                        connection.source_handle, manifest.node_type
                    ),
                });
            }
            if let Some(manifest) = &manifests[target]
                && !port_matches(&manifest.input_ports, &connection.target_handle)
            {
                issues.push(CompileIssue {
                    code: "UNKNOWN_TARGET_PORT".into(),
                    path: format!("connections[{definition_index}].targetHandle"),
                    message: format!(
                        "Port '{}' is not declared by {}",
                        connection.target_handle, manifest.node_type
                    ),
                });
            }
            raw_connections.push((connection, source, target, connection.order));
        }

        validate_reachability(&enabled, &raw_connections, &mut issues);
        if !issues.is_empty() {
            return Err(CompileError::new(issues));
        }

        let adjacency = adjacency(enabled.len(), &raw_connections);
        let components = strongly_connected_components(&adjacency);
        let mut component_by_node = vec![0; enabled.len()];
        for (component_index, component) in components.iter().enumerate() {
            for node in component {
                component_by_node[*node] = component_index;
            }
        }
        let mut connections = raw_connections
            .iter()
            .enumerate()
            .map(
                |(index, (connection, source, target, branch_order))| CompiledConnection {
                    index,
                    id: connection.id.clone(),
                    source_node: *source,
                    source_port: connection.source_handle.clone(),
                    target_node: *target,
                    target_port: connection.target_handle.clone(),
                    branch_order: *branch_order,
                    back_edge: component_by_node[*source] == component_by_node[*target]
                        && (components[component_by_node[*source]].len() > 1 || source == target),
                },
            )
            .collect::<Vec<_>>();
        connections.sort_by_key(|edge| (edge.source_node, edge.branch_order, edge.index));
        for (index, connection) in connections.iter_mut().enumerate() {
            connection.index = index;
        }

        let mut nodes = enabled
            .iter()
            .enumerate()
            .map(|(index, (_, node))| {
                let manifest = manifests[index].as_ref().expect("manifests validated");
                CompiledNode {
                    index,
                    id: node.id.clone(),
                    name: node.name.clone(),
                    node_type: node.node_type.clone(),
                    type_version: node.type_version,
                    parameters: node.parameters.clone(),
                    settings: node.settings.clone(),
                    capability: manifest.capability.clone(),
                    execution_style: manifest.execution_style.clone(),
                    readiness: manifest.readiness.clone(),
                    required_input_ports: manifest
                        .input_ports
                        .iter()
                        .filter(|port| port.required)
                        .map(|port| port.name.clone())
                        .collect(),
                    output_ports: manifest
                        .output_ports
                        .iter()
                        .map(|port| port.name.clone())
                        .collect(),
                    side_effect_level: manifest.side_effect_level.clone(),
                    incoming_connections: Vec::new(),
                    outgoing_connections: Vec::new(),
                    component_index: component_by_node[index],
                }
            })
            .collect::<Vec<_>>();
        for connection in &connections {
            nodes[connection.source_node]
                .outgoing_connections
                .push(connection.index);
            nodes[connection.target_node]
                .incoming_connections
                .push(connection.index);
        }
        let start_nodes = nodes
            .iter()
            .filter(|node| {
                node.incoming_connections.is_empty()
                    && node.execution_style == ExecutionStyle::Trigger
            })
            .map(|node| node.index)
            .collect::<Vec<_>>();
        let definition_hash = canonical_content_hash(
            &serde_json::to_value(definition).expect("definition serializes"),
        )
        .expect("definition hash serializes");
        let hash_source = serde_json::json!({
            "compilerVersion": COMPILER_VERSION,
            "definitionHash": definition_hash,
            "nodes": &nodes,
            "connections": &connections,
            "components": &components,
        });
        let bytes = serde_json::to_vec(&hash_source).expect("compiled workflow serializes");
        let canonical_hash = format!("sha256:ir-v1:{:x}", Sha256::digest(bytes));
        Ok(CompiledWorkflow {
            schema_version: definition.schema_version.clone(),
            compiler_version: COMPILER_VERSION.into(),
            canonical_hash,
            definition_hash,
            execution_order: definition.settings.execution_order,
            activation_budget: definition.settings.activation_budget,
            nodes,
            connections,
            start_nodes,
            strongly_connected_components: components,
            subworkflow_version_ids: subworkflows.into_iter().collect(),
        })
    }
}

fn validate_binding_slots(
    definition_index: usize,
    node: &agentx_domain::WorkflowNode,
    manifest: &NodeManifestVersion,
    issues: &mut Vec<CompileIssue>,
) {
    for slot in &manifest.binding_slots {
        let count = node
            .resource_references
            .iter()
            .filter(|reference| {
                reference.binding_role.as_deref() == Some(slot.name.as_str())
                    && reference.resource_type == slot.resource_type
            })
            .count();
        if slot.required && count == 0 {
            issues.push(CompileIssue {
                code: "BINDING_REQUIRED".into(),
                path: format!("nodes[{definition_index}].resourceReferences"),
                message: format!("Binding slot '{}' is required", slot.name),
            });
        }
        if !slot.multiple && count > 1 {
            issues.push(CompileIssue {
                code: "BINDING_MULTIPLE_NOT_ALLOWED".into(),
                path: format!("nodes[{definition_index}].resourceReferences"),
                message: format!("Binding slot '{}' accepts only one resource", slot.name),
            });
        }
    }
    for (reference_index, reference) in node.resource_references.iter().enumerate() {
        let Some(role) = reference.binding_role.as_deref() else {
            continue;
        };
        if !manifest
            .binding_slots
            .iter()
            .any(|slot| slot.name == role && slot.resource_type == reference.resource_type)
        {
            issues.push(CompileIssue {
                code: "INVALID_BINDING_SLOT".into(),
                path: format!(
                    "nodes[{definition_index}].resourceReferences[{reference_index}].bindingRole"
                ),
                message: format!("Binding role '{role}' does not accept this resource type"),
            });
        }
    }
}

fn validate_parameters(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    issues: &mut Vec<CompileIssue>,
) {
    let schema = expression_aware_schema(&manifest.parameter_schema, true);
    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            issues.push(CompileIssue {
                code: "INVALID_PARAMETER_SCHEMA".into(),
                path: format!("nodes[{definition_index}].typeVersion"),
                message: format!("Node Manifest parameter schema is invalid: {error}"),
            });
            return;
        }
    };
    let empty = serde_json::json!({});
    let parameters = if node.parameters.is_null() {
        &empty
    } else {
        &node.parameters
    };
    for error in validator.iter_errors(parameters) {
        let suffix = error.instance_path.to_string();
        issues.push(CompileIssue {
            code: "INVALID_NODE_PARAMETERS".into(),
            path: format!(
                "nodes[{definition_index}].parameters{}",
                suffix.replace('/', ".")
            ),
            message: error.to_string(),
        });
    }
}

fn expression_aware_schema(schema: &Value, root: bool) -> Value {
    let mut schema = schema.clone();
    if let Some(object) = schema.as_object_mut() {
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            for property in properties.values_mut() {
                *property = expression_aware_schema(property, false);
            }
        }
        if let Some(items) = object.get_mut("items") {
            *items = expression_aware_schema(items, false);
        }
        if let Some(additional) = object.get_mut("additionalProperties")
            && additional.is_object()
        {
            *additional = expression_aware_schema(additional, false);
        }
    }
    if root {
        schema
    } else {
        serde_json::json!({"anyOf":[schema,{"type":"string","pattern":"^="}]})
    }
}

fn validate_expressions(
    engine: &ExpressionEngine,
    value: &Value,
    path: &str,
    issues: &mut Vec<CompileIssue>,
) {
    match value {
        Value::String(source) if source.starts_with('=') => {
            if let Err(error) = engine.validate(&source[1..]) {
                issues.push(CompileIssue {
                    code: "INVALID_EXPRESSION".into(),
                    path: path.into(),
                    message: error.to_string(),
                });
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_expressions(engine, value, &format!("{path}[{index}]"), issues);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_expressions(engine, value, &format!("{path}.{key}"), issues);
            }
        }
        _ => {}
    }
}

fn port_matches(ports: &[agentx_node_protocol::NodePort], handle: &str) -> bool {
    ports.iter().any(|port| {
        port.name == handle
            || port.variadic
                && (handle.starts_with(&format!("{}:", port.name))
                    || handle.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn validate_reachability(
    nodes: &[(usize, &agentx_domain::WorkflowNode)],
    connections: &[(&agentx_domain::WorkflowConnection, usize, usize, u32)],
    issues: &mut Vec<CompileIssue>,
) {
    let adjacency = adjacency(nodes.len(), connections);
    let mut reachable = BTreeSet::new();
    let mut queue = nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, node))| node.node_type == "manual_trigger")
        .map(|(index, _)| index)
        .collect::<VecDeque<_>>();
    while let Some(node) = queue.pop_front() {
        if reachable.insert(node) {
            queue.extend(adjacency[node].iter().copied());
        }
    }
    for (compiled_index, (definition_index, node)) in nodes.iter().enumerate() {
        if !reachable.contains(&compiled_index) {
            issues.push(CompileIssue {
                code: "UNREACHABLE_NODE".into(),
                path: format!("nodes[{definition_index}]"),
                message: format!("Enabled node '{}' is unreachable", node.id),
            });
        }
    }
}

fn adjacency(
    node_count: usize,
    connections: &[(&agentx_domain::WorkflowConnection, usize, usize, u32)],
) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); node_count];
    for (_, source, target, _) in connections {
        adjacency[*source].push(*target);
    }
    for targets in &mut adjacency {
        targets.sort_unstable();
        targets.dedup();
    }
    adjacency
}

fn strongly_connected_components(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    fn visit(node: usize, graph: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        for target in &graph[node] {
            visit(*target, graph, seen, order);
        }
        order.push(node);
    }
    fn collect(node: usize, graph: &[Vec<usize>], seen: &mut [bool], component: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        component.push(node);
        for target in &graph[node] {
            collect(*target, graph, seen, component);
        }
    }
    let mut order = Vec::new();
    let mut seen = vec![false; adjacency.len()];
    for node in 0..adjacency.len() {
        visit(node, adjacency, &mut seen, &mut order);
    }
    let mut reverse = vec![Vec::new(); adjacency.len()];
    for (source, targets) in adjacency.iter().enumerate() {
        for target in targets {
            reverse[*target].push(source);
        }
    }
    let mut components = Vec::new();
    seen.fill(false);
    for node in order.into_iter().rev() {
        if !seen[node] {
            let mut component = Vec::new();
            collect(node, &reverse, &mut seen, &mut component);
            component.sort_unstable();
            components.push(component);
        }
    }
    components.sort_by_key(|component| component[0]);
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> WorkflowDefinition {
        serde_json::from_value(serde_json::json!({
            "schemaVersion":"3.0",
            "settings":{"activationBudget":20,"executionOrder":"deterministic"},
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger",},
                {"id":"if","type":"if","typeVersion":1,"name":"IF","parameters":{"condition":"=$json.ok"}},
                {"id":"merge","type":"merge","typeVersion":1,"name":"Merge",},
                {"id":"loop","type":"loop_over_items","typeVersion":1,"name":"Loop",}
            ],
            "connections":[
                {"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"if","targetHandle":"main","order":0},
                {"id":"b","sourceNodeId":"if","sourceHandle":"true","targetNodeId":"merge","targetHandle":"main:0","order":0},
                {"id":"c","sourceNodeId":"if","sourceHandle":"false","targetNodeId":"merge","targetHandle":"main:1","order":1},
                {"id":"d","sourceNodeId":"merge","sourceHandle":"main","targetNodeId":"loop","targetHandle":"main","order":0},
                {"id":"e","sourceNodeId":"loop","sourceHandle":"loop","targetNodeId":"merge","targetHandle":"main:2","order":0}
            ]
        })).unwrap()
    }

    #[test]
    fn compilation_is_deterministic_and_marks_cycles() {
        let registry = NodeRegistry::m4_defaults();
        let compiler = WorkflowCompiler::new(&registry);
        let first = compiler
            .compile(&fixture(), &CompileContext::default())
            .unwrap();
        let second = compiler
            .compile(&fixture(), &CompileContext::default())
            .unwrap();
        assert_eq!(first.canonical_hash, second.canonical_hash);
        assert!(
            first
                .connections
                .iter()
                .any(|connection| connection.back_edge)
        );
        assert_eq!(first.connections[1].branch_order, 0);
        assert_eq!(first.connections[2].branch_order, 1);
    }

    #[test]
    fn rejects_unknown_ports_and_recursive_subworkflows() {
        let registry = NodeRegistry::m4_defaults();
        let compiler = WorkflowCompiler::new(&registry);
        let mut definition = fixture();
        definition.connections[0].source_handle = "missing".into();
        definition.nodes.push(serde_json::from_value(serde_json::json!({
            "id":"sub","type":"sub_workflow","typeVersion":1,"name":"Sub","parameters":{"workflowVersionId":"version-a"}
        })).unwrap());
        definition.connections.push(serde_json::from_value(serde_json::json!({
            "id":"sub-edge","sourceNodeId":"loop","sourceHandle":"done","targetNodeId":"sub","targetHandle":"main","order":1
        })).unwrap());
        let error = compiler
            .compile(
                &definition,
                &CompileContext {
                    current_workflow_version_id: Some("version-a".into()),
                    ancestor_workflow_version_ids: BTreeSet::new(),
                },
            )
            .unwrap_err();
        let codes = error
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"UNKNOWN_SOURCE_PORT"));
        assert!(codes.contains(&"RECURSIVE_SUBWORKFLOW"));
    }

    #[test]
    fn validates_literal_parameters_and_accepts_deferred_expressions() {
        let registry = NodeRegistry::m5_defaults();
        let compiler = WorkflowCompiler::new(&registry);
        let mut definition = fixture();
        definition.nodes[3].parameters = serde_json::json!({"batchSize":0});
        let error = compiler
            .compile(&definition, &CompileContext::default())
            .unwrap_err();
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "INVALID_NODE_PARAMETERS"
                    && issue.path == "nodes[3].parameters.batchSize")
        );

        definition.nodes[3].parameters = serde_json::json!({"batchSize":"=$json.batch"});
        assert!(
            compiler
                .compile(&definition, &CompileContext::default())
                .is_ok()
        );
    }
}
