use std::collections::BTreeMap;

use agentx_domain::{DynamicValue, ValueCoercion, WorkflowDefinition, WorkflowNode};
use agentx_node_protocol::NodeManifestVersion;
use serde_json::Value;

use super::{
    CompileIssue, json_schema_at_path, json_types_compatible, reference_json_type,
    structured_selector_reference,
};

pub(super) fn normalized_node_parameters(
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
) -> Value {
    let mut parameters = node.parameters.clone();
    annotate_string_coercions(&mut parameters, &manifest.parameter_schema);
    if node.node_type != "code" {
        return parameters;
    }
    let egress_mode = parameters
        .get("egressMode")
        .or_else(|| parameters.pointer("/networkPolicy/egressMode"))
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_owned();
    if let Some(object) = parameters.as_object_mut() {
        object.insert(
            "networkPolicy".to_owned(),
            serde_json::json!({"defaultAction":"deny","egressMode":egress_mode}),
        );
    }
    parameters
}

pub(super) fn normalized_output_projection(
    node: &WorkflowNode,
) -> BTreeMap<String, BTreeMap<String, agentx_domain::OutputProjectionField>> {
    let mut projection = node.output_projection.clone();
    for field in projection
        .values_mut()
        .flat_map(|fields| fields.values_mut())
    {
        if field.schema.get("type").and_then(Value::as_str) == Some("string")
            && let DynamicValue::Reference { coerce, .. } = &mut field.value
        {
            *coerce = Some(ValueCoercion::String);
        }
    }
    projection
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
    if let Ok(DynamicValue::Reference { selector, .. }) = serde_json::from_value(value.clone()) {
        let expected = schema.get("type").and_then(Value::as_str);
        let actual = structured_selector_reference(&selector, nodes)
            .and_then(|reference| reference_json_type(&reference, definition, nodes, manifests));
        if expected.is_some_and(|expected| expected != "string") && actual.is_none() {
            issues.push(CompileIssue {
                code: "PARAMETER_REFERENCE_TYPE_UNKNOWN".into(),
                path: path.into(),
                message: format!(
                    "Parameter reference for {} requires a concrete {} value",
                    path,
                    expected.unwrap_or("value")
                ),
            });
        } else if let (Some(expected), Some(actual)) = (expected, actual)
            && expected != "string"
            && !json_types_compatible(expected, &actual)
        {
            issues.push(CompileIssue {
                code: "PARAMETER_REFERENCE_TYPE_MISMATCH".into(),
                path: path.into(),
                message: format!(
                    "Parameter reference for {path} requires {expected}, but the source is {actual}"
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

fn annotate_string_coercions(value: &mut Value, schema: &Value) {
    if schema.get("type").and_then(Value::as_str) == Some("string")
        && value.get("kind").and_then(Value::as_str) == Some("reference")
    {
        if let Some(object) = value.as_object_mut() {
            object.insert("coerce".into(), Value::String("string".into()));
        }
        return;
    }
    if let (Some(object), Some(properties)) = (
        value.as_object_mut(),
        schema.get("properties").and_then(Value::as_object),
    ) {
        for (name, child) in object {
            if let Some(child_schema) = properties.get(name) {
                annotate_string_coercions(child, child_schema);
            }
        }
    } else if let (Some(items), Some(item_schema)) = (value.as_array_mut(), schema.get("items")) {
        for item in items {
            annotate_string_coercions(item, item_schema);
        }
    }
}

pub(super) fn normalized_workflow_end(
    definition: &WorkflowDefinition,
) -> agentx_domain::WorkflowEnd {
    let mut end = definition.end.clone();
    for output in end.outputs.values_mut() {
        if output.schema.get("type").and_then(Value::as_str) == Some("string")
            && let DynamicValue::Reference { coerce, .. } = &mut output.value
        {
            *coerce = Some(ValueCoercion::String);
        }
    }
    end
}

pub(super) fn normalized_context_writes(
    node: &WorkflowNode,
    definition: &WorkflowDefinition,
) -> Vec<agentx_domain::ContextWrite> {
    let mut writes = node.context_writes.clone();
    for write in &mut writes {
        let segments = write.path.split('.').collect::<Vec<_>>();
        let Some(context) = segments
            .first()
            .and_then(|root| definition.start.contexts.get(*root))
        else {
            continue;
        };
        let nested = segments[1..]
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect::<Vec<_>>();
        if json_schema_at_path(&context.schema, &nested)
            .and_then(|schema| schema.get("type"))
            .and_then(Value::as_str)
            == Some("string")
            && let DynamicValue::Reference { coerce, .. } = &mut write.value
        {
            *coerce = Some(ValueCoercion::String);
        }
    }
    writes
}
