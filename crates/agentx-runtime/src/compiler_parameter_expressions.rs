use agentx_domain::{DynamicValue, ValueNamespace};
use serde_json::Value;

use super::{
    CompileIssue, ParameterExpressionContract, collect_dynamic_selectors, parameter_items_schema,
    parameter_property_schema,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_parameter_expression_value_contract(
    parameter_name: &str,
    path: &str,
    value: &Value,
    schema: Option<&Value>,
    inherited: &ParameterExpressionContract,
    issues: &mut Vec<CompileIssue>,
) {
    let contract = parameter_expression_contract(schema, inherited);
    if let Ok(dynamic) = serde_json::from_value::<DynamicValue>(value.clone()) {
        if !matches!(dynamic, DynamicValue::Literal { .. }) && !contract.templatable {
            issues.push(CompileIssue {
                code: "PARAMETER_NOT_TEMPLATABLE".into(),
                path: path.into(),
                message: format!("Parameter '{parameter_name}' does not allow dynamic values"),
            });
        }
        let mut selectors = Vec::new();
        collect_dynamic_selectors(&dynamic, &mut selectors);
        for selector in selectors {
            let namespace = namespace_name(selector.namespace);
            if !contract.allowed_namespaces.is_empty()
                && !contract.allowed_namespaces.contains(namespace)
            {
                issues.push(CompileIssue {
                    code: "EXPRESSION_NAMESPACE_NOT_ALLOWED".into(),
                    path: path.into(),
                    message: format!(
                        "Parameter '{parameter_name}' cannot reference namespace '{namespace}'"
                    ),
                });
            }
        }
        return;
    }
    if value.is_string() {
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
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn namespace_name(namespace: ValueNamespace) -> &'static str {
    match namespace {
        ValueNamespace::Inputs => "inputs",
        ValueNamespace::Outputs => "outputs",
        ValueNamespace::Contexts => "contexts",
        ValueNamespace::Execution => "execution",
        ValueNamespace::Item => "item",
        ValueNamespace::Loop => "loop",
    }
}

fn parameter_expression_contract(
    schema: Option<&Value>,
    inherited: &ParameterExpressionContract,
) -> ParameterExpressionContract {
    let dynamic = schema.and_then(|schema| schema.get("x-agentx-dynamicValue"));
    let templatable = dynamic.is_some() || inherited.templatable;
    let allowed_namespaces = schema
        .and(dynamic)
        .and_then(|dynamic| dynamic.get("allowedNamespaces"))
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
