use serde_json::Value;

use super::{
    CompileIssue, ParameterExpressionContext, ParameterExpressionContract, exact_reference,
    expression_reference_paths, json_types_compatible, output_reference_may_be_empty,
    parameter_items_schema, parameter_property_schema, reference_json_type, schema_accepts_null,
    schema_requires,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_parameter_expression_value_contract(
    parameter_name: &str,
    path: &str,
    value: &Value,
    schema: Option<&Value>,
    inherited: &ParameterExpressionContract,
    required: bool,
    context: &ParameterExpressionContext<'_>,
    issues: &mut Vec<CompileIssue>,
) {
    let contract = parameter_expression_contract(schema, inherited);
    if let Value::String(source) = value {
        let references = expression_reference_paths(source);
        if references.is_empty() {
            return;
        }
        if !contract.templatable {
            issues.push(CompileIssue {
                code: "PARAMETER_NOT_TEMPLATABLE".into(),
                path: path.into(),
                message: format!("Parameter '{parameter_name}' does not allow expressions"),
            });
            return;
        }
        for reference in &references {
            if let Some(root) = reference.first()
                && !contract.allowed_namespaces.is_empty()
                && !contract.allowed_namespaces.contains(root)
            {
                issues.push(CompileIssue {
                    code: "EXPRESSION_NAMESPACE_NOT_ALLOWED".into(),
                    path: path.into(),
                    message: format!(
                        "Parameter '{parameter_name}' cannot reference namespace '{root}'"
                    ),
                });
            }
        }
        let expected = schema
            .and_then(|schema| schema.get("expectedType").and_then(Value::as_str))
            .or_else(|| schema.and_then(|schema| schema.get("type").and_then(Value::as_str)));
        if let (Some(expected), Some(reference)) = (
            expected.filter(|value| *value != "any"),
            exact_reference(value),
        ) && let Some(actual) = reference_json_type(
            &reference,
            context.definition,
            context.nodes,
            context.manifests,
        ) && !json_types_compatible(expected, &actual)
        {
            issues.push(CompileIssue {
                code: "EXPRESSION_TYPE_MISMATCH".into(),
                path: path.into(),
                message: format!(
                    "Parameter '{parameter_name}' expects {expected}, but the referenced value is {actual}"
                ),
            });
        }
        if required
            && schema.is_some_and(|schema| !schema_accepts_null(schema))
            && exact_reference(value).is_some_and(|reference| {
                output_reference_may_be_empty(&reference, context.nodes, context.manifests)
            })
        {
            issues.push(CompileIssue {
                code: "REQUIRED_PARAMETER_MAY_BE_EMPTY".into(),
                path: path.into(),
                message: format!(
                    "Required parameter '{parameter_name}' references an output selector that may be empty"
                ),
            });
        }
        return;
    }

    match value {
        Value::Array(values) => {
            let item_schema = schema.and_then(parameter_items_schema);
            for (index, item) in values.iter().enumerate() {
                validate_parameter_expression_value_contract(
                    parameter_name,
                    &format!("{path}[{index}]"),
                    item,
                    item_schema,
                    &contract,
                    false,
                    context,
                    issues,
                );
            }
        }
        Value::Object(values) => {
            for (name, child) in values {
                let child_schema =
                    schema.and_then(|schema| parameter_property_schema(schema, name));
                validate_parameter_expression_value_contract(
                    parameter_name,
                    &format!("{path}.{name}"),
                    child,
                    child_schema,
                    &contract,
                    schema.is_some_and(|schema| schema_requires(schema, name)),
                    context,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn parameter_expression_contract(
    schema: Option<&Value>,
    inherited: &ParameterExpressionContract,
) -> ParameterExpressionContract {
    let templatable = schema
        .and_then(|schema| schema.get("templatable"))
        .and_then(Value::as_bool)
        .unwrap_or(inherited.templatable);
    let allowed_namespaces = schema
        .and_then(|schema| schema.get("allowedNamespaces"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_else(|| inherited.allowed_namespaces.clone());
    ParameterExpressionContract {
        templatable,
        allowed_namespaces,
    }
}
