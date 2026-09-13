use std::collections::BTreeSet;

use agentx_node_protocol::PortKind;
use serde_json::Value;

use super::CompileIssue;

#[derive(Clone, Default)]
pub(super) struct ParameterBindingContract {
    pub(super) accepted_kinds: BTreeSet<String>,
    pub(super) allowed_namespaces: BTreeSet<String>,
}

pub(super) fn parameter_property_schema<'a>(schema: &'a Value, name: &str) -> Option<&'a Value> {
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

pub(super) fn parameter_items_schema(schema: &Value) -> Option<&Value> {
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

pub(super) fn json_schemas_compatible(expected: &Value, actual: &Value) -> bool {
    let expected_types = schema_types(expected);
    let actual_types = schema_types(actual);
    if expected_types.is_empty() {
        return true;
    }
    if actual_types.is_empty()
        || !actual_types.iter().all(|actual| {
            expected_types
                .iter()
                .any(|expected| json_types_compatible(expected, actual))
        })
    {
        return false;
    }
    if let (Some(expected_format), Some(actual_format)) = (
        expected.get("format").and_then(Value::as_str),
        actual.get("format").and_then(Value::as_str),
    ) && expected_format != actual_format
    {
        return false;
    }
    if actual_types.contains(&"array")
        && let Some(expected_items) = expected.get("items")
        && let Some(actual_items) = actual.get("items")
        && !json_schemas_compatible(expected_items, actual_items)
    {
        return false;
    }
    if actual_types.contains(&"object")
        && let Some(expected_properties) = expected.get("properties").and_then(Value::as_object)
    {
        let actual_properties = actual.get("properties").and_then(Value::as_object);
        for required in expected
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let Some(expected_property) = expected_properties.get(required) else {
                continue;
            };
            let Some(actual_property) = actual_properties.and_then(|values| values.get(required))
            else {
                return false;
            };
            if !json_schemas_compatible(expected_property, actual_property) {
                return false;
            }
        }
    }
    true
}

pub(super) fn schema_types(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(value)) => vec![value.as_str()],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn binding_aware_schema(schema: &Value, root: bool) -> Value {
    binding_aware_schema_inner(schema, root, None)
}

fn binding_aware_schema_inner(
    schema: &Value,
    root: bool,
    inherited_binding: Option<&Value>,
) -> Value {
    let mut schema = schema.clone();
    let own_binding = schema.get("x-agentx-binding").cloned();
    let binding = own_binding.as_ref().or(inherited_binding);
    let propagate = binding.filter(|binding| {
        own_binding.is_none()
            || binding
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    });
    if let Some(object) = schema.as_object_mut() {
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(variants) = object.get_mut(keyword).and_then(Value::as_array_mut) {
                for variant in variants {
                    *variant = binding_aware_schema_inner(variant, false, propagate);
                }
            }
        }
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            for property in properties.values_mut() {
                *property = binding_aware_schema_inner(property, false, propagate);
            }
        }
        if let Some(items) = object.get_mut("items") {
            *items = binding_aware_schema_inner(items, false, propagate);
        }
        if let Some(additional) = object.get_mut("additionalProperties")
            && additional.is_object()
        {
            *additional = binding_aware_schema_inner(additional, false, propagate);
        }
    }
    if root {
        return schema;
    }
    let accepted_kinds = binding
        .and_then(|binding| binding.get("acceptedKinds"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let reference = serde_json::json!({
        "type":"object",
        "required":["kind","selector"],
        "properties":{"kind":{"const":"reference"},"selector":{"type":"object"},"missingPolicy":{"type":"object"}},
        "additionalProperties":false
    });
    let literal = serde_json::json!({
        "type":"object",
        "required":["kind","value"],
        "properties":{"kind":{"const":"literal"},"value":schema.clone()},
        "additionalProperties":false
    });
    let template = serde_json::json!({
        "type":"object",
        "required":["kind","segments"],
        "properties":{"kind":{"const":"template"},"segments":{"type":"array"}},
        "additionalProperties":false
    });
    let array = serde_json::json!({
        "type":"object",
        "required":["kind","items"],
        "properties":{"kind":{"const":"array"},"items":{"type":"array"}},
        "additionalProperties":false
    });
    let object = serde_json::json!({
        "type":"object",
        "required":["kind","fields"],
        "properties":{"kind":{"const":"object"},"fields":{"type":"object"}},
        "additionalProperties":false
    });
    let mut variants = Vec::new();
    for (kind, variant) in [
        ("literal", literal),
        ("reference", reference),
        ("template", template),
        ("array", array),
        ("object", object),
    ] {
        if accepted_kinds.contains(kind) {
            variants.push(variant);
        }
    }
    match variants.len() {
        0 => schema,
        1 => variants.pop().expect("one binding variant"),
        _ => serde_json::json!({"anyOf":variants}),
    }
}

pub(super) fn validate_legacy_expressions(
    value: &Value,
    path: &str,
    issues: &mut Vec<CompileIssue>,
) {
    match value {
        Value::String(source) if source.contains("${{") => {
            issues.push(CompileIssue {
                code: "LEGACY_EXPRESSION_NOT_SUPPORTED".into(),
                path: path.into(),
                message: "Workflow 5.0 requires a structured dynamic value".into(),
            });
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_legacy_expressions(value, &format!("{path}[{index}]"), issues);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_legacy_expressions(value, &format!("{path}.{key}"), issues);
            }
        }
        _ => {}
    }
}

pub(super) fn is_subworkflow_type(node_type: &str) -> bool {
    node_type == "sub_workflow"
}

pub(super) fn port_matches(ports: &[agentx_node_protocol::NodePort], handle: &str) -> bool {
    ports.iter().any(|port| {
        port.name == handle
            || port.variadic
                && (handle.starts_with(&format!("{}:", port.name))
                    || handle.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

pub(super) fn port_kind(
    ports: &[agentx_node_protocol::NodePort],
    handle: &str,
) -> Option<PortKind> {
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
