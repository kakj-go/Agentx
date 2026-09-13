use agentx_domain::{InputBinding, WorkflowDefinition, WorkflowNode};
use agentx_node_protocol::NodeManifestVersion;
use serde_json::Value;

use super::{CompileIssue, infer_json_schema, json_schemas_compatible, value_binding_schema};

pub(super) fn normalized_node_parameters(
    node: &WorkflowNode,
    _manifest: &NodeManifestVersion,
) -> Value {
    let mut parameters = node.parameters.clone();
    if node.node_type != "code" {
        return parameters;
    }
    if let Some(object) = parameters.as_object_mut() {
        let schema = object
            .get("outputExample")
            .map(infer_json_schema)
            .unwrap_or_else(|| serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}));
        object.insert("outputSchema".to_owned(), schema);
    }
    parameters
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_parameter_reference_types(
    value: &Value,
    schema: &Value,
    path: &str,
    definition: &WorkflowDefinition,
    nodes: &[(usize, &WorkflowNode)],
    manifests: &[Option<NodeManifestVersion>],
    issues: &mut Vec<CompileIssue>,
) {
    if let Ok(binding) = serde_json::from_value::<InputBinding>(value.clone()) {
        let actual = value_binding_schema(&binding, definition, nodes, manifests);
        if let Some(actual) = actual
            && !json_schemas_compatible(schema, &actual)
            && !schemas_runtime_coercible(schema, &actual)
        {
            issues.push(CompileIssue {
                code: "PARAMETER_REFERENCE_TYPE_MISMATCH".into(),
                path: path.into(),
                message: format!(
                    "Parameter reference for {path} requires {}, but the source is {}",
                    schema.get("type").unwrap_or(&Value::Null),
                    actual.get("type").unwrap_or(&Value::Null)
                ),
            });
        }
        return;
    }
    if let (Some(object), Some(properties)) = (
        value.as_object(),
        schema.get("properties").and_then(Value::as_object),
    ) {
        for (name, child) in object {
            if let Some(child_schema) = properties.get(name) {
                validate_parameter_reference_types(
                    child,
                    child_schema,
                    &format!("{path}.{name}"),
                    definition,
                    nodes,
                    manifests,
                    issues,
                );
            }
        }
    } else if let (Some(items), Some(item_schema)) = (value.as_array(), schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            validate_parameter_reference_types(
                item,
                item_schema,
                &format!("{path}[{index}]"),
                definition,
                nodes,
                manifests,
                issues,
            );
        }
    }
}

fn schemas_runtime_coercible(expected: &Value, actual: &Value) -> bool {
    let expected = schema_types(expected);
    let actual = schema_types(actual);
    expected.is_empty()
        || actual.is_empty()
        || actual.contains(&"string")
        || expected.contains(&"string")
}

fn schema_types(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn normalized_context_writes(
    node: &WorkflowNode,
    _definition: &WorkflowDefinition,
) -> Vec<agentx_domain::ContextWrite> {
    node.context_writes.clone()
}
