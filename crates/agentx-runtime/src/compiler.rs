use std::collections::{BTreeMap, BTreeSet, VecDeque};

use agentx_domain::{
    ContextDefinition, ContextWriteOperation, DynamicValue, ExpressionNode, MissingValuePolicy,
    TemplateSegment, ValueNamespace, ValuePathSegment, ValueSelection, ValueSelector,
    WORKFLOW_END_NODE_ID, WORKFLOW_START_NODE_ID, WorkflowDefinition, WorkflowNode, WorkflowOutput,
    canonical_content_hash, validate_definition,
};
use agentx_node_protocol::{NodeManifestVersion, OutputCardinality, PortKind};
use agentx_runtime_contracts::{
    CompiledConnection, CompiledNode, CompiledTerminalConnection, CompiledWorkflow,
    IR_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{ExpressionEngine, NodeRegistry};

#[path = "compiler_agent.rs"]
mod compiler_agent;
#[path = "compiler_normalization.rs"]
mod normalization;
#[path = "compiler_parameter_expressions.rs"]
mod parameter_expressions;
use compiler_agent::{compile_agent_node, validate_binding_slots};
use normalization::{
    normalized_context_writes, normalized_node_parameters, normalized_output_projection,
    normalized_workflow_end, validate_parameter_reference_types,
};

pub const COMPILER_VERSION: &str = "agentx-workflow-5.0.1";

#[derive(Clone, Debug, Default)]
pub struct CompileContext {
    pub current_workflow_version_id: Option<String>,
    pub ancestor_workflow_version_ids: BTreeSet<String>,
    /// Immutable display/tool names resolved by the publisher for conflict checks.
    pub resource_tool_names: BTreeMap<uuid::Uuid, String>,
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
        crate::schema_contract::validate_workflow_contract_schemas(definition, &mut issues);
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
                validate_binding_slots(*definition_index, node, manifest, context, &mut issues);
                validate_parameters(*definition_index, node, manifest, &mut issues);
                validate_context_write_capability(
                    *definition_index,
                    node,
                    manifest,
                    definition,
                    &mut issues,
                );
                validate_composite_context_contract(
                    *definition_index,
                    node,
                    manifest,
                    definition,
                    &mut issues,
                );
            }
            validate_expressions(
                &self.expressions,
                &node.parameters,
                &format!("{path}.parameters"),
                &mut issues,
            );
            let projection_value = serde_json::to_value(&node.output_projection)
                .expect("output projection serializes");
            validate_expressions(
                &self.expressions,
                &projection_value,
                &format!("{path}.outputProjection"),
                &mut issues,
            );
            for (write_index, write) in node.context_writes.iter().enumerate() {
                let value =
                    serde_json::to_value(&write.value).expect("dynamic context value serializes");
                validate_expressions(
                    &self.expressions,
                    &value,
                    &format!("{path}.contextWrites[{write_index}].value"),
                    &mut issues,
                );
            }
            if is_subworkflow_type(&node.node_type) {
                match node
                    .parameters
                    .get("workflowVersionId")
                    .and_then(Value::as_str)
                {
                    Some(version) => {
                        if let Some(expected) = node.node_type.strip_prefix("workflow.")
                            && uuid::Uuid::parse_str(version)
                                .map(|version| version.simple().to_string())
                                .ok()
                                .as_deref()
                                != Some(expected)
                        {
                            issues.push(CompileIssue {
                                code: "COMPOSITE_VERSION_MISMATCH".into(),
                                path: format!("{path}.parameters.workflowVersionId"),
                                message: "Composite node type and immutable Workflow Version do not match".into(),
                            });
                        }
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
        let mut raw_start_connections = Vec::new();
        let mut raw_terminal_connections = Vec::new();
        let mut start_to_end = false;
        for (definition_index, connection) in definition.connections.iter().enumerate() {
            if connection.source_node_id == WORKFLOW_START_NODE_ID {
                if connection.target_node_id == WORKFLOW_END_NODE_ID {
                    start_to_end = connection.target_handle == "main";
                    continue;
                }
                let Some(&target) = indexes.get(connection.target_node_id.as_str()) else {
                    continue;
                };
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
                raw_start_connections.push((connection, target));
                continue;
            }
            if connection.target_node_id == WORKFLOW_END_NODE_ID {
                let Some(&source) = indexes.get(connection.source_node_id.as_str()) else {
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
                if let Some(manifest) = &manifests[source]
                    && let Some(kind) = port_kind(&manifest.output_ports, &connection.source_handle)
                    && ((connection.target_handle == "main" && kind != PortKind::Main)
                        || (connection.target_handle == "error" && kind != PortKind::Error))
                {
                    issues.push(CompileIssue {
                        code: "BOUNDARY_PORT_KIND_MISMATCH".into(),
                        path: format!("connections[{definition_index}]"),
                        message: "End.main accepts Main output and End.error accepts Error output"
                            .into(),
                    });
                }
                raw_terminal_connections.push((connection, source));
                continue;
            }
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
            if let (Some(source_manifest), Some(target_manifest)) =
                (&manifests[source], &manifests[target])
                && let (Some(source_kind), Some(target_kind)) = (
                    port_kind(&source_manifest.output_ports, &connection.source_handle),
                    port_kind(&target_manifest.input_ports, &connection.target_handle),
                )
                && source_kind != target_kind
            {
                issues.push(CompileIssue {
                    code: "PORT_KIND_MISMATCH".into(),
                    path: format!("connections[{definition_index}]"),
                    message: format!(
                        "Connection {} links incompatible {:?} and {:?} ports",
                        connection.id, source_kind, target_kind
                    ),
                });
            }
            raw_connections.push((connection, source, target, connection.order));
        }

        let graph = adjacency(enabled.len(), &raw_connections);
        for (target, (definition_index, node)) in enabled.iter().enumerate() {
            if let Some(manifest) = manifests[target].as_ref() {
                validate_parameter_expression_contracts(
                    *definition_index,
                    node,
                    manifest,
                    &mut issues,
                );
                validate_output_projection(*definition_index, node, manifest, &mut issues);
                validate_parameter_reference_types(
                    &node.parameters,
                    &manifest.parameter_schema,
                    &format!("nodes[{definition_index}].parameters"),
                    definition,
                    &enabled,
                    &manifests,
                    &mut issues,
                );
            }
            validate_reference_paths(
                &node.parameters,
                &format!("nodes[{definition_index}].parameters"),
                Some(target),
                ReferenceUsage::Parameter,
                definition,
                &enabled,
                &manifests,
                &graph,
                &mut issues,
            );
            let projection_value = serde_json::to_value(&node.output_projection)
                .expect("output projection serializes");
            validate_reference_paths(
                &projection_value,
                &format!("nodes[{definition_index}].outputProjection"),
                Some(target),
                ReferenceUsage::OutputProjection,
                definition,
                &enabled,
                &manifests,
                &graph,
                &mut issues,
            );
            for (write_index, write) in node.context_writes.iter().enumerate() {
                let value =
                    serde_json::to_value(&write.value).expect("dynamic context value serializes");
                validate_reference_paths(
                    &value,
                    &format!("nodes[{definition_index}].contextWrites[{write_index}].value"),
                    Some(target),
                    ReferenceUsage::ContextWrite,
                    definition,
                    &enabled,
                    &manifests,
                    &graph,
                    &mut issues,
                );
            }
        }
        for (name, output) in &definition.end.outputs {
            let value = serde_json::to_value(&output.value).expect("dynamic end value serializes");
            validate_expressions(
                &self.expressions,
                &value,
                &format!("end.outputs.{name}.value"),
                &mut issues,
            );
            validate_reference_paths(
                &value,
                &format!("end.outputs.{name}.value"),
                None,
                ReferenceUsage::EndOutput {
                    required: output.required,
                },
                definition,
                &enabled,
                &manifests,
                &graph,
                &mut issues,
            );
            validate_end_output_contract(
                &format!("end.outputs.{name}.value"),
                output,
                definition,
                &enabled,
                &manifests,
                &mut issues,
            );
        }

        for (name, output) in &definition.end.error.outputs {
            let value =
                serde_json::to_value(&output.value).expect("dynamic end error value serializes");
            validate_expressions(
                &self.expressions,
                &value,
                &format!("end.error.outputs.{name}.value"),
                &mut issues,
            );
            validate_reference_paths(
                &value,
                &format!("end.error.outputs.{name}.value"),
                None,
                ReferenceUsage::EndErrorOutput {
                    required: output.required,
                },
                definition,
                &enabled,
                &manifests,
                &graph,
                &mut issues,
            );
            validate_end_output_contract(
                &format!("end.error.outputs.{name}.value"),
                output,
                definition,
                &enabled,
                &manifests,
                &mut issues,
            );
        }

        let start_nodes = raw_start_connections
            .iter()
            .map(|(_, target)| *target)
            .collect::<BTreeSet<_>>();
        let end_main_nodes = raw_terminal_connections
            .iter()
            .filter(|(connection, _)| connection.target_handle == "main")
            .map(|(_, source)| *source)
            .collect::<BTreeSet<_>>();
        validate_reachability(
            &enabled,
            &raw_connections,
            &manifests,
            &start_nodes,
            &end_main_nodes,
            start_to_end,
            &mut issues,
        );
        if !issues.is_empty() {
            return Err(CompileError::new(issues));
        }

        let adjacency = graph;
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
                    source_port_kind: port_kind(
                        &manifests[*source]
                            .as_ref()
                            .expect("manifest validated")
                            .output_ports,
                        &connection.source_handle,
                    )
                    .expect("source port validated"),
                    target_node: *target,
                    target_port: connection.target_handle.clone(),
                    target_port_kind: port_kind(
                        &manifests[*target]
                            .as_ref()
                            .expect("manifest validated")
                            .input_ports,
                        &connection.target_handle,
                    )
                    .expect("target port validated"),
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
                    key: node.key.clone(),
                    name: node.name.clone(),
                    node_type: node.node_type.clone(),
                    type_version: node.type_version,
                    parameters: normalized_node_parameters(node, manifest),
                    output_projection: serde_json::to_value(normalized_output_projection(node))
                        .expect("output projection serializes"),
                    context_writes: normalized_context_writes(node, definition),
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
                    effective_output_contract:
                        agentx_runtime_contracts::EffectiveOutputContractV1 {
                            port_schemas: manifest
                                .output_ports
                                .iter()
                                .map(|port| {
                                    (
                                        port.name.clone(),
                                        merged_output_schema(node, manifest, &port.name),
                                    )
                                })
                                .collect(),
                            cardinalities: manifest.output_cardinality.clone(),
                        },
                    side_effect_level: manifest.side_effect_level.clone(),
                    agent: compile_agent_node(node, manifest),
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
        let start_nodes = start_nodes.into_iter().collect::<Vec<_>>();
        let terminal_connections = raw_terminal_connections
            .into_iter()
            .map(|(connection, source_node)| CompiledTerminalConnection {
                id: connection.id.clone(),
                source_node,
                source_port: connection.source_handle.clone(),
                target_port: connection.target_handle.clone(),
                branch_order: connection.order,
            })
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
            "terminalConnections": &terminal_connections,
            "startNodes": &start_nodes,
            "startToEnd": start_to_end,
            "components": &components,
        });
        let bytes = serde_json::to_vec(&hash_source).expect("compiled workflow serializes");
        let canonical_hash = format!("sha256:ir-v1:{:x}", Sha256::digest(bytes));
        Ok(CompiledWorkflow {
            contract_version: IR_SCHEMA_VERSION,
            schema_version: definition.schema_version.clone(),
            compiler_version: COMPILER_VERSION.into(),
            canonical_hash,
            definition_hash,
            execution_order: definition.settings.execution_order,
            activation_budget: definition.settings.activation_budget,
            start: definition.start.clone(),
            contexts: definition.start.contexts.clone(),
            end: normalized_workflow_end(definition),
            nodes,
            connections,
            terminal_connections,
            start_to_end,
            start_nodes,
            strongly_connected_components: components,
            subworkflow_version_ids: subworkflows.into_iter().collect(),
        })
    }
}

fn validate_output_projection(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    issues: &mut Vec<CompileIssue>,
) {
    let native_fields = manifest
        .output_schema
        .get("properties")
        .and_then(Value::as_object);
    for (port, fields) in &node.output_projection {
        if !port_matches(&manifest.output_ports, port) {
            issues.push(CompileIssue {
                code: "UNKNOWN_PROJECTION_PORT".into(),
                path: format!("nodes[{definition_index}].outputProjection.{port}"),
                message: format!("Projection port '{port}' is not declared by the node Manifest"),
            });
            continue;
        }
        let kind = port_kind(&manifest.output_ports, port);
        if kind == Some(PortKind::Error) {
            issues.push(CompileIssue {
                code: "ERROR_PROJECTION_NOT_ALLOWED".into(),
                path: format!("nodes[{definition_index}].outputProjection.{port}"),
                message: "Output Projection cannot modify an Error Item".into(),
            });
        }
        for name in fields.keys() {
            if native_fields.is_some_and(|native| native.contains_key(name)) {
                issues.push(CompileIssue {
                    code: "PROJECTION_FIELD_CONFLICT".into(),
                    path: format!("nodes[{definition_index}].outputProjection.{port}.{name}"),
                    message: format!(
                        "Projection field '{name}' conflicts with a native Manifest output field"
                    ),
                });
            }
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

fn validate_context_write_capability(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    definition: &WorkflowDefinition,
    issues: &mut Vec<CompileIssue>,
) {
    if !node.context_writes.is_empty() && !manifest.context_write_capability {
        issues.push(CompileIssue {
            code: "CONTEXT_WRITE_NOT_SUPPORTED".into(),
            path: format!("nodes[{definition_index}].contextWrites"),
            message: format!(
                "Node type '{}' cannot write Workflow Context",
                node.node_type
            ),
        });
    }
    for (write_index, write) in node.context_writes.iter().enumerate() {
        let segments = write.path.split('.').collect::<Vec<_>>();
        let root = segments.first().copied().unwrap_or_default();
        match definition.start.contexts.get(root) {
            None => issues.push(CompileIssue {
                code: "UNKNOWN_CONTEXT_WRITE".into(),
                path: format!("nodes[{definition_index}].contextWrites[{write_index}].path"),
                message: format!("Context '{}' is not declared by Start", root),
            }),
            Some(context) if !context.mutable => issues.push(CompileIssue {
                code: "CONTEXT_READ_ONLY".into(),
                path: format!("nodes[{definition_index}].contextWrites[{write_index}].path"),
                message: format!("Context '{}' is immutable", root),
            }),
            Some(context) => {
                let nested = segments[1..]
                    .iter()
                    .map(|segment| (*segment).to_owned())
                    .collect::<Vec<_>>();
                let Some(target_schema) = json_schema_at_path(&context.schema, &nested) else {
                    issues.push(CompileIssue {
                        code: "UNKNOWN_CONTEXT_WRITE_PATH".into(),
                        path: format!(
                            "nodes[{definition_index}].contextWrites[{write_index}].path"
                        ),
                        message: format!("Context write path '{}' is not declared", write.path),
                    });
                    continue;
                };
                let target_type = target_schema.get("type").and_then(Value::as_str);
                let compatible = match write.operation {
                    ContextWriteOperation::Append => target_type == Some("array"),
                    ContextWriteOperation::MergeObject => target_type == Some("object"),
                    ContextWriteOperation::Increment
                    | ContextWriteOperation::Min
                    | ContextWriteOperation::Max => {
                        matches!(target_type, Some("number" | "integer"))
                    }
                    _ => true,
                };
                if !compatible {
                    issues.push(CompileIssue {
                        code: "CONTEXT_WRITE_OPERATION_TYPE_MISMATCH".into(),
                        path: format!(
                            "nodes[{definition_index}].contextWrites[{write_index}].operation"
                        ),
                        message: format!(
                            "Context write operation is not valid for '{}' ({})",
                            write.path,
                            target_type.unwrap_or("unknown")
                        ),
                    });
                }
            }
        }
    }
}

fn validate_composite_context_contract(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    definition: &WorkflowDefinition,
    issues: &mut Vec<CompileIssue>,
) {
    if !node.node_type.starts_with("workflow.") {
        return;
    }
    let Some(contract) = manifest.parameter_schema.get("x-agentx-contextContract") else {
        issues.push(CompileIssue {
            code: "COMPOSITE_CONTEXT_CONTRACT_MISSING".into(),
            path: format!("nodes[{definition_index}].typeVersion"),
            message: "Composite Manifest does not contain its Context contract".into(),
        });
        return;
    };
    let Ok(child_contexts) =
        serde_json::from_value::<BTreeMap<String, ContextDefinition>>(contract.clone())
    else {
        issues.push(CompileIssue {
            code: "COMPOSITE_CONTEXT_CONTRACT_INVALID".into(),
            path: format!("nodes[{definition_index}].typeVersion"),
            message: "Composite Manifest Context contract is invalid".into(),
        });
        return;
    };
    for (name, child) in child_contexts {
        if !child.mutable {
            continue;
        }
        let Some(parent) = definition.start.contexts.get(&name) else {
            issues.push(CompileIssue {
                code: "COMPOSITE_MUTABLE_CONTEXT_UNDECLARED".into(),
                path: format!("nodes[{definition_index}].typeVersion"),
                message: format!(
                    "Mutable child Context '{name}' must be declared by the parent Workflow"
                ),
            });
            continue;
        };
        if !parent.mutable
            || parent.scope != child.scope
            || parent.merge_policy != child.merge_policy
            || parent.schema != child.schema
        {
            issues.push(CompileIssue {
                code: "COMPOSITE_CONTEXT_CONTRACT_MISMATCH".into(),
                path: format!("nodes[{definition_index}].typeVersion"),
                message: format!(
                    "Parent Context '{name}' must match the mutable child Context schema, scope and merge policy"
                ),
            });
        }
    }
}

fn validate_parameter_expression_contracts(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    issues: &mut Vec<CompileIssue>,
) {
    let Some(properties) = manifest
        .parameter_schema
        .get("properties")
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(parameters) = node.parameters.as_object() else {
        return;
    };
    for (name, value) in parameters {
        let Some(schema) = properties.get(name) else {
            continue;
        };
        let path = format!("nodes[{definition_index}].parameters.{name}");
        parameter_expressions::validate_parameter_expression_value_contract(
            name,
            &path,
            value,
            Some(schema),
            &ParameterExpressionContract::default(),
            issues,
        );
    }
}

#[derive(Clone, Default)]
struct ParameterExpressionContract {
    templatable: bool,
    allowed_namespaces: BTreeSet<String>,
}

fn parameter_property_schema<'a>(schema: &'a Value, name: &str) -> Option<&'a Value> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(name))
        .or_else(|| {
            ["allOf", "anyOf", "oneOf"].into_iter().find_map(|keyword| {
                schema
                    .get(keyword)
                    .and_then(Value::as_array)
                    .and_then(|variants| {
                        variants
                            .iter()
                            .find_map(|variant| parameter_property_schema(variant, name))
                    })
            })
        })
        .or_else(|| {
            schema
                .get("additionalProperties")
                .filter(|additional| additional.is_object())
        })
}

fn parameter_items_schema(schema: &Value) -> Option<&Value> {
    schema.get("items").or_else(|| {
        ["allOf", "anyOf", "oneOf"].into_iter().find_map(|keyword| {
            schema
                .get(keyword)
                .and_then(Value::as_array)
                .and_then(|variants| variants.iter().find_map(parameter_items_schema))
        })
    })
}

fn json_types_compatible(expected: &str, actual: &str) -> bool {
    expected == actual
        || matches!(
            (expected, actual),
            ("number", "integer") | ("integer", "number")
        )
}

fn expression_aware_schema(schema: &Value, root: bool) -> Value {
    let mut schema = schema.clone();
    if let Some(object) = schema.as_object_mut() {
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(variants) = object.get_mut(keyword).and_then(Value::as_array_mut) {
                for variant in variants {
                    *variant = expression_aware_schema(variant, false);
                }
            }
        }
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
        serde_json::json!({"anyOf":[schema,{"type":"object","required":["kind"],"properties":{"kind":{"enum":["literal","reference","template","expression"]}}}]})
    }
}

fn validate_expressions(
    engine: &ExpressionEngine,
    value: &Value,
    path: &str,
    issues: &mut Vec<CompileIssue>,
) {
    match value {
        Value::String(source) if source.contains("${{") => {
            let _ = engine;
            issues.push(CompileIssue {
                code: "LEGACY_EXPRESSION_NOT_SUPPORTED".into(),
                path: path.into(),
                message: "Workflow 5.0 requires a structured dynamic value".into(),
            });
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

fn is_subworkflow_type(node_type: &str) -> bool {
    node_type == "sub_workflow" || node_type.starts_with("workflow.")
}

#[derive(Clone, Copy)]
enum ReferenceUsage {
    Parameter,
    OutputProjection,
    ContextWrite,
    EndOutput { required: bool },
    EndErrorOutput { required: bool },
}

#[allow(clippy::too_many_arguments)]
fn validate_reference_paths(
    value: &Value,
    path: &str,
    target: Option<usize>,
    usage: ReferenceUsage,
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    manifests: &[Option<NodeManifestVersion>],
    graph: &[Vec<usize>],
    issues: &mut Vec<CompileIssue>,
) {
    if value.is_object()
        && let Ok(dynamic) = serde_json::from_value::<DynamicValue>(value.clone())
    {
        let mut selectors = Vec::new();
        collect_dynamic_selectors(&dynamic, &mut selectors);
        for selector in selectors {
            if let Some(reference) = structured_selector_reference(selector, nodes) {
                validate_reference_path(
                    &reference, path, target, usage, definition, nodes, manifests, graph, issues,
                );
            } else {
                issues.push(CompileIssue {
                    code: "OUTPUT_REFERENCE_NOT_FOUND".into(),
                    path: path.into(),
                    message: "Structured selector references an unknown source node".into(),
                });
            }
        }
        return;
    }
    match value {
        Value::String(_) => {}
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_reference_paths(
                    value,
                    &format!("{path}[{index}]"),
                    target,
                    usage,
                    definition,
                    nodes,
                    manifests,
                    graph,
                    issues,
                );
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_reference_paths(
                    value,
                    &format!("{path}.{key}"),
                    target,
                    usage,
                    definition,
                    nodes,
                    manifests,
                    graph,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn collect_dynamic_selectors<'a>(value: &'a DynamicValue, selectors: &mut Vec<&'a ValueSelector>) {
    match value {
        DynamicValue::Literal { .. } => {}
        DynamicValue::Reference {
            selector,
            missing_policy,
            ..
        } => {
            selectors.push(selector);
            collect_missing_policy_selectors(missing_policy, selectors);
        }
        DynamicValue::Template { segments } => {
            for segment in segments {
                if let TemplateSegment::Reference {
                    selector,
                    missing_policy,
                } = segment
                {
                    selectors.push(selector);
                    collect_missing_policy_selectors(missing_policy, selectors);
                }
            }
        }
        DynamicValue::Expression { root } => collect_expression_selectors(root, selectors),
    }
}

fn collect_missing_policy_selectors<'a>(
    policy: &'a MissingValuePolicy,
    selectors: &mut Vec<&'a ValueSelector>,
) {
    if let MissingValuePolicy::Default { value } = policy {
        collect_dynamic_selectors(value, selectors);
    }
}

fn collect_expression_selectors<'a>(
    node: &'a ExpressionNode,
    selectors: &mut Vec<&'a ValueSelector>,
) {
    match node {
        ExpressionNode::Literal { .. } => {}
        ExpressionNode::Reference {
            selector,
            missing_policy,
        } => {
            selectors.push(selector);
            collect_missing_policy_selectors(missing_policy, selectors);
        }
        ExpressionNode::Unary { operand, .. } => collect_expression_selectors(operand, selectors),
        ExpressionNode::Binary { left, right, .. } => {
            collect_expression_selectors(left, selectors);
            collect_expression_selectors(right, selectors);
        }
        ExpressionNode::Conditional {
            condition,
            then_value,
            else_value,
        } => {
            collect_expression_selectors(condition, selectors);
            collect_expression_selectors(then_value, selectors);
            collect_expression_selectors(else_value, selectors);
        }
        ExpressionNode::Call { arguments, .. } | ExpressionNode::Array { items: arguments } => {
            for argument in arguments {
                collect_expression_selectors(argument, selectors);
            }
        }
        ExpressionNode::Object { fields } => {
            for value in fields.values() {
                collect_expression_selectors(value, selectors);
            }
        }
    }
}

fn structured_selector_reference(
    selector: &ValueSelector,
    nodes: &[(usize, &WorkflowNode)],
) -> Option<Vec<String>> {
    let root = match selector.namespace {
        ValueNamespace::Inputs => "inputs",
        ValueNamespace::Outputs => "outputs",
        ValueNamespace::Contexts => "contexts",
        ValueNamespace::Execution => "execution",
        ValueNamespace::Item => "item",
        ValueNamespace::Loop => "loop",
    };
    let mut reference = vec![root.to_owned()];
    if selector.namespace == ValueNamespace::Outputs {
        let id = selector.source_node_id.as_deref()?;
        let node = nodes.iter().find(|(_, node)| node.id == id)?.1;
        reference.push(node.key.clone());
        if let ValueSelection::Index { index } = selector.run {
            reference.extend([
                "runs".into(),
                index.to_string(),
                selector.port.clone().unwrap_or_else(|| "main".into()),
            ]);
            reference.push(match selector.item {
                ValueSelection::Index { index } => index.to_string(),
                _ => "0".into(),
            });
            reference.push("json".into());
        } else {
            reference.push(selector.port.clone().unwrap_or_else(|| "main".into()));
            match selector.item {
                ValueSelection::Current => reference.extend(["current".into(), "json".into()]),
                ValueSelection::First => reference.extend(["first".into(), "json".into()]),
                ValueSelection::Last => reference.extend(["last".into(), "json".into()]),
                ValueSelection::All => reference.push("all".into()),
                ValueSelection::Index { index } => {
                    reference.extend(["all".into(), index.to_string(), "json".into()])
                }
            }
        }
    } else if selector.namespace == ValueNamespace::Item {
        reference.push("json".into());
    }
    reference.extend(selector.path.iter().map(|segment| match segment {
        ValuePathSegment::Key(key) => key.clone(),
        ValuePathSegment::Index(index) => index.to_string(),
    }));
    Some(reference)
}

#[allow(clippy::too_many_arguments)]
fn validate_reference_path(
    reference: &[String],
    path: &str,
    target: Option<usize>,
    usage: ReferenceUsage,
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    manifests: &[Option<NodeManifestVersion>],
    graph: &[Vec<usize>],
    issues: &mut Vec<CompileIssue>,
) {
    let Some(root) = reference.first().map(String::as_str) else {
        return;
    };
    match root {
        "inputs" => {
            if reference.get(1).is_none()
                || !json_schema_has_path(&definition.start.inputs, &reference[1..])
            {
                reference_issue(issues, "UNKNOWN_INPUT_REFERENCE", path, reference);
            } else if matches!(
                usage,
                ReferenceUsage::OutputProjection
                    | ReferenceUsage::EndOutput { .. }
                    | ReferenceUsage::EndErrorOutput { .. }
            ) && json_schema_path_is_sensitive(&definition.start.inputs, &reference[1..])
            {
                reference_issue(issues, "SENSITIVE_INPUT_EXPOSURE", path, reference);
            }
        }
        "contexts" => {
            let Some(name) = reference.get(1) else {
                reference_issue(issues, "UNKNOWN_CONTEXT_REFERENCE", path, reference);
                return;
            };
            let Some(context) = definition.start.contexts.get(name) else {
                reference_issue(issues, "UNKNOWN_CONTEXT_REFERENCE", path, reference);
                return;
            };
            if let Some(target) = target
                && manifests[target]
                    .as_ref()
                    .is_some_and(|manifest| !manifest.context_read_capability)
            {
                reference_issue(issues, "CONTEXT_READ_NOT_SUPPORTED", path, reference);
            }
            if context.sensitive
                && matches!(
                    usage,
                    ReferenceUsage::OutputProjection
                        | ReferenceUsage::EndOutput { .. }
                        | ReferenceUsage::EndErrorOutput { .. }
                )
            {
                reference_issue(issues, "SENSITIVE_CONTEXT_EXPOSURE", path, reference);
            }
            if reference.len() > 2 && !json_schema_has_path(&context.schema, &reference[2..]) {
                reference_issue(issues, "UNKNOWN_CONTEXT_REFERENCE", path, reference);
            }
        }
        "outputs" => {
            let Some(key) = reference.get(1) else {
                reference_issue(issues, "UNKNOWN_OUTPUT_REFERENCE", path, reference);
                return;
            };
            let Some((source, (_, source_node))) = nodes
                .iter()
                .enumerate()
                .find(|(_, (_, node))| &node.key == key)
            else {
                reference_issue(issues, "UNKNOWN_OUTPUT_REFERENCE", path, reference);
                return;
            };
            if (target == Some(source) && !matches!(usage, ReferenceUsage::ContextWrite))
                || target.is_some_and(|target| !is_reachable(source, target, graph))
            {
                reference_issue(issues, "OUTPUT_NOT_PREDECESSOR", path, reference);
                return;
            }
            if matches!(usage, ReferenceUsage::EndErrorOutput { .. })
                && !is_common_error_predecessor(source, definition, nodes, graph)
            {
                reference_issue(
                    issues,
                    "ERROR_OUTPUT_NOT_COMMON_PREDECESSOR",
                    path,
                    reference,
                );
                return;
            }
            let Some(port) = reference.get(2) else {
                reference_issue(issues, "OUTPUT_PORT_REQUIRED", path, reference);
                return;
            };
            let Some(manifest) = manifests[source].as_ref() else {
                return;
            };
            let output_schema = merged_output_schema(source_node, manifest, port);
            if port == "runs" {
                if !manifest.expression_capabilities.supports_run_selection {
                    reference_issue(issues, "OUTPUT_RUN_SELECTOR_NOT_SUPPORTED", path, reference);
                    return;
                }
                let Some(run_index) = reference.get(3) else {
                    reference_issue(issues, "OUTPUT_RUN_INDEX_REQUIRED", path, reference);
                    return;
                };
                if run_index.parse::<u32>().is_err() {
                    reference_issue(issues, "OUTPUT_RUN_INDEX_INVALID", path, reference);
                    return;
                }
                let Some(run_port) = reference.get(4) else {
                    reference_issue(issues, "OUTPUT_PORT_REQUIRED", path, reference);
                    return;
                };
                if !port_matches(&manifest.output_ports, run_port) {
                    reference_issue(issues, "UNKNOWN_OUTPUT_PORT", path, reference);
                    return;
                }
                let Some(item_index) = reference.get(5) else {
                    reference_issue(issues, "OUTPUT_ITEM_INDEX_REQUIRED", path, reference);
                    return;
                };
                if item_index.parse::<u32>().is_err() {
                    reference_issue(issues, "OUTPUT_ITEM_INDEX_INVALID", path, reference);
                    return;
                }
                if reference.get(6).map(String::as_str) != Some("json") {
                    reference_issue(issues, "OUTPUT_ITEM_JSON_REQUIRED", path, reference);
                    return;
                }
                if reference.len() > 7
                    && output_schema.get("properties").is_some()
                    && !json_schema_has_path(&output_schema, &reference[7..])
                {
                    unknown_output_field_issue(issues, path, reference, manifest, &reference[7..]);
                }
                if output_reference_is_sensitive(&output_schema, reference)
                    && usage_exposes_value(usage)
                {
                    reference_issue(issues, "SENSITIVE_OUTPUT_EXPOSURE", path, reference);
                }
                if matches!(
                    usage,
                    ReferenceUsage::EndOutput { required: true }
                        | ReferenceUsage::EndErrorOutput { required: true }
                ) {
                    reference_issue(issues, "REQUIRED_OUTPUT_MAY_BE_EMPTY", path, reference);
                }
                return;
            }
            if !port_matches(&manifest.output_ports, port) {
                reference_issue(issues, "UNKNOWN_OUTPUT_PORT", path, reference);
                return;
            }
            let cardinality = manifest
                .output_cardinality
                .get(port)
                .copied()
                .unwrap_or_default();
            let selector = reference.get(3).map(String::as_str);
            if !matches!(selector, Some("current" | "first" | "last" | "all")) {
                reference_issue(issues, "OUTPUT_SELECTOR_REQUIRED", path, reference);
                return;
            }
            if let Some(selector) = selector {
                let supported = match selector {
                    "current" => manifest.expression_capabilities.supports_current,
                    "first" | "last" => manifest.expression_capabilities.supports_first_last,
                    "all" => manifest.expression_capabilities.supports_all,
                    _ => false,
                };
                if !supported {
                    reference_issue(issues, "OUTPUT_SELECTOR_NOT_SUPPORTED", path, reference);
                    return;
                }
                let field_offset = if selector == "all" {
                    if reference.len() == 4 {
                        4
                    } else {
                        let Some(item_index) = reference.get(4) else {
                            return;
                        };
                        if item_index.parse::<u32>().is_err() {
                            reference_issue(issues, "OUTPUT_ITEM_INDEX_INVALID", path, reference);
                            return;
                        }
                        if reference.get(5).map(String::as_str) != Some("json") {
                            reference_issue(issues, "OUTPUT_ITEM_JSON_REQUIRED", path, reference);
                            return;
                        }
                        6
                    }
                } else {
                    if reference.get(4).map(String::as_str) != Some("json") {
                        reference_issue(issues, "OUTPUT_ITEM_JSON_REQUIRED", path, reference);
                        return;
                    }
                    5
                };
                if reference.len() > field_offset
                    && output_schema.get("properties").is_some()
                    && !json_schema_allows_path(&output_schema, &reference[field_offset..])
                {
                    unknown_output_field_issue(
                        issues,
                        path,
                        reference,
                        manifest,
                        &reference[field_offset..],
                    );
                }
                if output_reference_is_sensitive(&output_schema, reference)
                    && usage_exposes_value(usage)
                {
                    reference_issue(issues, "SENSITIVE_OUTPUT_EXPOSURE", path, reference);
                }
                if matches!(
                    usage,
                    ReferenceUsage::EndOutput { required: true }
                        | ReferenceUsage::EndErrorOutput { required: true }
                ) && selector != "all"
                    && matches!(
                        cardinality,
                        OutputCardinality::ZeroOrOne | OutputCardinality::ZeroOrMany
                    )
                {
                    reference_issue(issues, "REQUIRED_OUTPUT_MAY_BE_EMPTY", path, reference);
                }
            }
            if let Some(target) = target
                && !(target == source && matches!(usage, ReferenceUsage::ContextWrite))
                && is_reachable(target, source, graph)
            {
                reference_issue(issues, "EXPRESSION_DEPENDENCY_CYCLE", path, reference);
            }
            let _ = source_node;
        }
        "loop" => {
            if !matches!(
                reference.get(1).map(String::as_str),
                Some("iteration" | "itemIndex")
            ) {
                reference_issue(issues, "UNKNOWN_LOOP_REFERENCE", path, reference);
                return;
            }
            let Some(target) = target else {
                reference_issue(issues, "LOOP_REFERENCE_OUTSIDE_ITERATION", path, reference);
                return;
            };
            if !nodes.iter().enumerate().any(|(loop_index, (_, node))| {
                node.node_type == "loop_over_items"
                    && is_reachable(loop_index, target, graph)
                    && is_reachable(target, loop_index, graph)
            }) {
                reference_issue(issues, "LOOP_REFERENCE_OUTSIDE_ITERATION", path, reference);
            }
        }
        "item" if matches!(usage, ReferenceUsage::EndErrorOutput { .. }) => {
            const ERROR_FIELDS: &[&str] = &[
                "code",
                "message",
                "details",
                "sourceNodeId",
                "nodeExecutionId",
                "retryable",
            ];
            if reference.get(1).map(String::as_str) != Some("json")
                || reference
                    .get(2)
                    .is_some_and(|field| !ERROR_FIELDS.contains(&field.as_str()))
            {
                reference_issue(issues, "UNKNOWN_ERROR_ITEM_REFERENCE", path, reference);
            }
        }
        "item" if matches!(usage, ReferenceUsage::EndOutput { .. }) => {
            reference_issue(issues, "ITEM_NOT_AVAILABLE_AT_SUCCESS_END", path, reference);
        }
        _ => {}
    }
}

fn usage_exposes_value(usage: ReferenceUsage) -> bool {
    matches!(
        usage,
        ReferenceUsage::OutputProjection
            | ReferenceUsage::EndOutput { .. }
            | ReferenceUsage::EndErrorOutput { .. }
    )
}

fn is_common_error_predecessor(
    source: usize,
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    graph: &[Vec<usize>],
) -> bool {
    let error_sources = definition
        .connections
        .iter()
        .filter(|connection| {
            connection.target_node_id == WORKFLOW_END_NODE_ID && connection.target_handle == "error"
        })
        .filter_map(|connection| {
            nodes
                .iter()
                .position(|(_, node)| node.id == connection.source_node_id)
        })
        .collect::<Vec<_>>();
    error_sources.is_empty()
        || error_sources
            .into_iter()
            .all(|error_source| source == error_source || is_reachable(source, error_source, graph))
}

fn json_schema_has_path(schema: &Value, path: &[String]) -> bool {
    json_schema_at_path(schema, path).is_some()
}

fn json_schema_allows_path(schema: &Value, path: &[String]) -> bool {
    let mut current = schema;
    for segment in path {
        if segment.parse::<usize>().is_ok() {
            let Some(items) = current.get("items") else {
                return current.get("type").and_then(Value::as_str) != Some("array");
            };
            current = items;
            continue;
        }
        if let Some(child) = current
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get(segment))
        {
            current = child;
            continue;
        }
        match current.get("additionalProperties") {
            Some(Value::Bool(false)) => return false,
            Some(child) if child.is_object() => current = child,
            _ => return true,
        }
    }
    true
}

fn json_schema_path_is_sensitive(schema: &Value, path: &[String]) -> bool {
    let mut current = schema;
    if schema_is_sensitive(current) {
        return true;
    }
    for segment in path {
        let Some(next) = json_schema_child(current, segment) else {
            return false;
        };
        current = next;
        if schema_is_sensitive(current) {
            return true;
        }
    }
    false
}

fn schema_is_sensitive(schema: &Value) -> bool {
    schema
        .get("sensitive")
        .or_else(|| schema.get("x-sensitive"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn output_reference_is_sensitive(output_schema: &Value, reference: &[String]) -> bool {
    let fields = if reference.get(2).map(String::as_str) == Some("runs") {
        reference
            .get(6)
            .is_some_and(|value| value == "json")
            .then_some(&reference[7..])
    } else {
        match reference.get(3).map(String::as_str) {
            Some("all") if reference.get(5).is_some_and(|value| value == "json") => {
                Some(&reference[6..])
            }
            Some("current" | "first" | "last")
                if reference.get(4).is_some_and(|value| value == "json") =>
            {
                Some(&reference[5..])
            }
            _ => None,
        }
    };
    fields.is_some_and(|fields| json_schema_path_is_sensitive(output_schema, fields))
}

fn merged_output_schema(node: &WorkflowNode, manifest: &NodeManifestVersion, port: &str) -> Value {
    let mut schema = manifest
        .output_port_schemas
        .get(port)
        .cloned()
        .unwrap_or_else(|| manifest.output_schema.clone());
    let Some(schema_object) = schema.as_object_mut() else {
        return schema;
    };
    if !schema_object.contains_key("properties") {
        schema_object.insert("properties".into(), Value::Object(Default::default()));
    }
    let properties = schema_object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("object output schema properties");
    if let Some(projection) = node.output_projection.get(port) {
        for (name, field) in projection {
            let mut field_schema = field.schema.clone();
            if field.sensitive {
                if let Some(schema) = field_schema.as_object_mut() {
                    schema.insert("x-sensitive".into(), Value::Bool(true));
                }
            }
            properties.insert(name.clone(), field_schema);
        }
    }
    schema
}

fn reference_json_type(
    reference: &[String],
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    manifests: &[Option<NodeManifestVersion>],
) -> Option<String> {
    match reference.first()?.as_str() {
        "inputs" => json_schema_at_path(&definition.start.inputs, &reference[1..])?
            .get("type")?
            .as_str()
            .map(str::to_owned),
        "contexts" => {
            let context = definition.start.contexts.get(reference.get(1)?)?;
            json_schema_at_path(&context.schema, &reference[2..])?
                .get("type")?
                .as_str()
                .map(str::to_owned)
        }
        "outputs" => {
            let key = reference.get(1)?;
            let source = nodes.iter().position(|(_, node)| node.key == *key)?;
            let manifest = manifests.get(source)?.as_ref()?;
            let node = nodes.get(source)?.1;
            let port = reference.get(2).map(String::as_str).unwrap_or("main");
            let output_schema = merged_output_schema(node, manifest, port);
            if reference.get(2).map(String::as_str) == Some("runs") {
                if reference.get(6).is_some_and(|value| value == "json") {
                    return json_schema_at_path(&output_schema, &reference[7..])
                        .and_then(|schema| schema.get("type"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                return None;
            }
            let selector = reference.get(3).map(String::as_str);
            if selector == Some("all") {
                return Some("array".into());
            }
            if reference.get(4).is_some_and(|value| value == "json") {
                return json_schema_at_path(&output_schema, &reference[5..])
                    .and_then(|schema| schema.get("type"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            None
        }
        "item" => match reference.get(2).map(String::as_str) {
            Some("retryable") => Some("boolean".into()),
            Some("details") => Some("object".into()),
            Some("code" | "message" | "sourceNodeId" | "nodeExecutionId") => Some("string".into()),
            _ => None,
        },
        _ => None,
    }
}

fn validate_end_output_contract(
    path: &str,
    output: &WorkflowOutput,
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    manifests: &[Option<NodeManifestVersion>],
    issues: &mut Vec<CompileIssue>,
) {
    let DynamicValue::Reference { selector, .. } = &output.value else {
        return;
    };
    let Some(reference) = structured_selector_reference(selector, nodes) else {
        return;
    };
    let Some(expected) = output.schema.get("type").and_then(Value::as_str) else {
        return;
    };
    let actual = reference_json_type(&reference, definition, nodes, manifests);
    if expected != "string" && actual.as_deref().is_none() {
        issues.push(CompileIssue {
            code: "END_OUTPUT_TYPE_UNKNOWN".into(),
            path: path.into(),
            message: format!(
                "End output at {path} declares {expected}, but the referenced value has no concrete type"
            ),
        });
    } else if expected != "string"
        && actual
            .as_deref()
            .is_some_and(|actual| !json_types_compatible(expected, actual))
    {
        issues.push(CompileIssue {
            code: "END_OUTPUT_TYPE_MISMATCH".into(),
            path: path.into(),
            message: format!(
                "End output at {path} declares {expected}, but the referenced value is {}",
                actual.as_deref().unwrap_or("unknown")
            ),
        });
    }
}

fn json_schema_at_path<'a>(schema: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = schema;
    for segment in path {
        current = json_schema_child(current, segment)?;
    }
    Some(current)
}

fn json_schema_child<'a>(schema: &'a Value, segment: &str) -> Option<&'a Value> {
    if segment.parse::<usize>().is_ok() {
        return schema.get("items");
    }
    schema
        .get("properties")
        .and_then(|properties| properties.get(segment))
        .or_else(|| {
            schema
                .get("additionalProperties")
                .filter(|additional| additional.is_object())
        })
}

fn is_reachable(source: usize, target: usize, graph: &[Vec<usize>]) -> bool {
    let mut seen = vec![false; graph.len()];
    let mut pending = vec![source];
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if seen[node] {
            continue;
        }
        seen[node] = true;
        pending.extend(graph[node].iter().copied());
    }
    false
}

fn reference_issue(issues: &mut Vec<CompileIssue>, code: &str, path: &str, reference: &[String]) {
    issues.push(CompileIssue { code: code.into(), path: path.into(), message: format!("Invalid structured reference '{}'; references must resolve through the Workflow 5.0 contract", reference.join(".")) });
}

fn unknown_output_field_issue(
    issues: &mut Vec<CompileIssue>,
    path: &str,
    reference: &[String],
    manifest: &NodeManifestVersion,
    fields: &[String],
) {
    let missing = fields.first().map(String::as_str).unwrap_or("unknown");
    let message = if matches!(manifest.node_type.as_str(), "model" | "agent")
        && matches!(
            missing,
            "message"
                | "messages"
                | "toolCalls"
                | "iterations"
                | "artifacts"
                | "providerRawResponse"
        ) {
        format!("AI output field '{missing}' no longer exists; reselect the stable 'text' field")
    } else {
        format!("Output field '{missing}' does not exist in the frozen Manifest contract")
    };
    issues.push(CompileIssue {
        code: "UNKNOWN_OUTPUT_FIELD".into(),
        path: path.into(),
        message: format!("{message} (reference '{}')", reference.join(".")),
    });
}

fn port_matches(ports: &[agentx_node_protocol::NodePort], handle: &str) -> bool {
    ports.iter().any(|port| {
        port.name == handle
            || port.variadic
                && (handle.starts_with(&format!("{}:", port.name))
                    || handle.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn port_kind(ports: &[agentx_node_protocol::NodePort], handle: &str) -> Option<PortKind> {
    ports
        .iter()
        .find(|port| {
            port.name == handle
                || port.variadic
                    && (handle.starts_with(&format!("{}:", port.name))
                        || handle.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .map(|port| port.kind.clone())
}

fn validate_reachability(
    nodes: &[(usize, &agentx_domain::WorkflowNode)],
    connections: &[(&agentx_domain::WorkflowConnection, usize, usize, u32)],
    manifests: &[Option<NodeManifestVersion>],
    start_nodes: &BTreeSet<usize>,
    end_main_nodes: &BTreeSet<usize>,
    start_to_end: bool,
    issues: &mut Vec<CompileIssue>,
) {
    let adjacency = adjacency(nodes.len(), connections);
    if start_nodes.is_empty() && !start_to_end {
        issues.push(CompileIssue {
            code: "START_REQUIRED".into(),
            path: "connections".into(),
            message: "Start.main must have an explicit connection".into(),
        });
    }
    if end_main_nodes.is_empty() && !start_to_end {
        issues.push(CompileIssue {
            code: "END_MAIN_REQUIRED".into(),
            path: "connections".into(),
            message: "End.main must have at least one explicit connection".into(),
        });
    }
    let mut reachable = BTreeSet::new();
    let mut queue = start_nodes.iter().copied().collect::<VecDeque<_>>();
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
    let mut reverse = vec![Vec::new(); nodes.len()];
    for (connection, source, target, _) in connections {
        if connection.source_handle == "error" {
            continue;
        }
        reverse[*target].push(*source);
    }
    let mut reaches_end = BTreeSet::new();
    let mut queue = end_main_nodes.iter().copied().collect::<VecDeque<_>>();
    while let Some(node) = queue.pop_front() {
        if reaches_end.insert(node) {
            queue.extend(reverse[node].iter().copied());
        }
    }
    for (compiled_index, (definition_index, node)) in nodes.iter().enumerate() {
        let has_main_output = manifests
            .get(compiled_index)
            .and_then(Option::as_ref)
            .is_some_and(|manifest| {
                manifest
                    .output_ports
                    .iter()
                    .any(|port| port.kind == PortKind::Main)
            });
        if reachable.contains(&compiled_index)
            && has_main_output
            && !reaches_end.contains(&compiled_index)
        {
            issues.push(CompileIssue {
                code: "MAIN_PATH_DOES_NOT_REACH_END".into(),
                path: format!("nodes[{definition_index}]"),
                message: format!("Enabled node '{}' cannot reach End.main", node.id),
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
#[path = "compiler_tests.rs"]
mod tests;
