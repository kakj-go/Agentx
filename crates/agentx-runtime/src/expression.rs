use std::collections::BTreeMap;

use agentx_domain::{
    ConditionOperator, InputBinding, InputTemplateSegment, MissingValuePolicy, ReferenceBinding,
    ValueNamespace, ValuePathSegment, ValueSelection, ValueSelector,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Default)]
pub struct ExpressionContext {
    pub json: Value,
    pub input: Value,
    pub item_index: usize,
    pub run_index: u32,
    /// `{ "node name": { "branch": { "run index": [items] } } }`
    pub linked_nodes: Value,
    pub inputs: Value,
    pub outputs: Value,
    pub contexts: Value,
    pub execution: Value,
    pub loop_context: Value,
    /// Stable Definition node id -> display/runtime node key.
    pub output_node_keys: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum ExpressionError {
    #[error("expression result cannot be represented as JSON: {0}")]
    Result(String),
    #[error("dynamic value reference is missing: {0}")]
    MissingReference(String),
    #[error("dynamic value operation is invalid: {0}")]
    InvalidOperation(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StringConversionRecord {
    pub target_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<ValueSelector>,
    pub source_type: String,
    pub mode: String,
    pub result_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ExpressionEngine;

impl ExpressionEngine {
    pub fn resolve_parameters(
        &self,
        parameters: &Value,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        self.resolve_parameters_with_conversions(parameters, context)
            .map(|(value, _)| value)
    }

    pub fn resolve_parameters_with_conversions(
        &self,
        parameters: &Value,
        context: &ExpressionContext,
    ) -> Result<(Value, Vec<StringConversionRecord>), ExpressionError> {
        let mut conversions = Vec::new();
        let value = self
            .resolve_parameter_value(parameters, context, "parameters", &mut conversions)?
            .unwrap_or(Value::Null);
        Ok((value, conversions))
    }

    pub fn resolve_parameters_with_schema(
        &self,
        parameters: &Value,
        schema: &Value,
        context: &ExpressionContext,
    ) -> Result<(Value, Vec<StringConversionRecord>), ExpressionError> {
        let (value, mut conversions) =
            self.resolve_parameters_with_conversions(parameters, context)?;
        let value = coerce_value_to_schema(value, schema, "parameters", &mut conversions)?;
        Ok((value, conversions))
    }

    pub fn resolve_input_with_schema(
        &self,
        binding: &InputBinding,
        schema: &Value,
        context: &ExpressionContext,
        target_path: impl Into<String>,
    ) -> Result<(Option<Value>, Vec<StringConversionRecord>), ExpressionError> {
        let target_path = target_path.into();
        let (value, mut conversions) =
            self.resolve_dynamic_optional_with_conversions(binding, context, target_path.clone())?;
        let value = value
            .map(|value| coerce_value_to_schema(value, schema, &target_path, &mut conversions))
            .transpose()?;
        Ok((value, conversions))
    }

    fn resolve_parameter_value(
        &self,
        value: &Value,
        context: &ExpressionContext,
        target_path: &str,
        conversions: &mut Vec<StringConversionRecord>,
    ) -> Result<Option<Value>, ExpressionError> {
        if value.is_object()
            && let Ok(binding) = serde_json::from_value::<InputBinding>(value.clone())
        {
            return self.resolve_dynamic_optional_traced(
                &binding,
                context,
                target_path,
                conversions,
            );
        }
        match value {
            Value::Array(values) => {
                let mut resolved = Vec::with_capacity(values.len());
                for (index, value) in values.iter().enumerate() {
                    if let Some(value) = self.resolve_parameter_value(
                        value,
                        context,
                        &format!("{target_path}[{index}]"),
                        conversions,
                    )? {
                        resolved.push(value);
                    }
                }
                Ok(Some(Value::Array(resolved)))
            }
            Value::Object(values) => {
                let mut resolved = serde_json::Map::new();
                for (key, value) in values {
                    if let Some(value) = self.resolve_parameter_value(
                        value,
                        context,
                        &format!("{target_path}.{key}"),
                        conversions,
                    )? {
                        resolved.insert(key.clone(), value);
                    }
                }
                Ok(Some(Value::Object(resolved)))
            }
            value => Ok(Some(value.clone())),
        }
    }

    pub fn resolve_dynamic(
        &self,
        value: &InputBinding,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        Ok(self
            .resolve_dynamic_optional(value, context)?
            .unwrap_or(Value::Null))
    }

    pub fn resolve_reference(
        &self,
        value: &ReferenceBinding,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        let ReferenceBinding::Reference {
            selector,
            missing_policy,
        } = value;
        Ok(self
            .resolve_selector_with_policy(selector, missing_policy, context)?
            .unwrap_or(Value::Null))
    }

    /// Resolves a field-level dynamic value while preserving `omit` as the
    /// absence of a JSON member instead of silently converting it to null.
    pub fn resolve_dynamic_optional(
        &self,
        value: &InputBinding,
        context: &ExpressionContext,
    ) -> Result<Option<Value>, ExpressionError> {
        self.resolve_dynamic_optional_with_conversions(value, context, "value")
            .map(|(value, _)| value)
    }

    pub fn resolve_dynamic_optional_with_conversions(
        &self,
        value: &InputBinding,
        context: &ExpressionContext,
        target_path: impl Into<String>,
    ) -> Result<(Option<Value>, Vec<StringConversionRecord>), ExpressionError> {
        let mut conversions = Vec::new();
        let target_path = target_path.into();
        let value =
            self.resolve_dynamic_optional_traced(value, context, &target_path, &mut conversions)?;
        Ok((value, conversions))
    }

    fn resolve_dynamic_optional_traced(
        &self,
        value: &InputBinding,
        context: &ExpressionContext,
        target_path: &str,
        conversions: &mut Vec<StringConversionRecord>,
    ) -> Result<Option<Value>, ExpressionError> {
        match value {
            InputBinding::Literal { value } => Ok(Some(value.clone())),
            InputBinding::Reference {
                selector,
                missing_policy,
            } => self.resolve_selector_with_policy(selector, missing_policy, context),
            InputBinding::Template { segments } => {
                let mut text = String::new();
                for (index, segment) in segments.iter().enumerate() {
                    match segment {
                        InputTemplateSegment::Text { text: value } => text.push_str(value),
                        InputTemplateSegment::Reference {
                            selector,
                            missing_policy,
                        } => {
                            if let Some(value) = self.resolve_selector_with_policy(
                                selector,
                                missing_policy,
                                context,
                            )? {
                                let source_type = json_type(&value).to_owned();
                                let converted = value_to_text(value);
                                conversions.push(StringConversionRecord {
                                    target_path: format!("{target_path}.segments[{index}]"),
                                    selector: Some(selector.clone()),
                                    source_type,
                                    mode: "template".into(),
                                    result_bytes: converted.len(),
                                });
                                text.push_str(&converted);
                            }
                        }
                    }
                }
                Ok(Some(Value::String(text)))
            }
            InputBinding::Array { items } => {
                let mut resolved = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    if let Some(value) = self.resolve_dynamic_optional_traced(
                        item,
                        context,
                        &format!("{target_path}[{index}]"),
                        conversions,
                    )? {
                        resolved.push(value);
                    }
                }
                Ok(Some(Value::Array(resolved)))
            }
            InputBinding::Object { fields } => {
                let mut resolved = serde_json::Map::new();
                for (name, field) in fields {
                    if let Some(value) = self.resolve_dynamic_optional_traced(
                        field,
                        context,
                        &format!("{target_path}.{name}"),
                        conversions,
                    )? {
                        resolved.insert(name.clone(), value);
                    }
                }
                Ok(Some(Value::Object(resolved)))
            }
        }
    }

    fn resolve_selector_with_policy(
        &self,
        selector: &ValueSelector,
        policy: &MissingValuePolicy,
        context: &ExpressionContext,
    ) -> Result<Option<Value>, ExpressionError> {
        match self.resolve_selector(selector, context) {
            Some(value) if !value.is_null() => Ok(Some(value)),
            _ => match policy {
                MissingValuePolicy::Error => {
                    Err(ExpressionError::MissingReference(format!("{:?}", selector)))
                }
                MissingValuePolicy::Null => Ok(Some(Value::Null)),
                MissingValuePolicy::Omit => Ok(None),
            },
        }
    }

    fn resolve_selector(
        &self,
        selector: &ValueSelector,
        context: &ExpressionContext,
    ) -> Option<Value> {
        let mut current = match selector.namespace {
            ValueNamespace::Inputs => context.inputs.clone(),
            ValueNamespace::Contexts => context.contexts.clone(),
            ValueNamespace::Execution => context.execution.clone(),
            ValueNamespace::Item => context.json.clone(),
            ValueNamespace::Loop => context.loop_context.clone(),
            ValueNamespace::Outputs => {
                let source_id = selector.source_node_id.as_ref()?;
                let key = context.output_node_keys.get(source_id).unwrap_or(source_id);
                let node = context.outputs.get(key)?;
                let port = selector.port.as_deref().unwrap_or("main");
                let port_value = match &selector.run {
                    ValueSelection::Index { index } => {
                        node.get("runs")?.get(index.to_string())?.get(port)?
                    }
                    _ => node.get(port)?,
                };
                let selected = match (&selector.run, &selector.item) {
                    (ValueSelection::Index { .. }, ValueSelection::All) => port_value.clone(),
                    (ValueSelection::Index { .. }, selection) => {
                        select_array_item(port_value, selection)?
                    }
                    (_, ValueSelection::Current) => port_value.get("current")?.clone(),
                    (_, ValueSelection::First) => port_value.get("first")?.clone(),
                    (_, ValueSelection::Last) => port_value.get("last")?.clone(),
                    (_, ValueSelection::All) => port_value.get("all")?.clone(),
                    (_, ValueSelection::Index { index }) => {
                        port_value.get("all")?.get(*index as usize)?.clone()
                    }
                };
                if matches!(selector.item, ValueSelection::All) {
                    selected
                } else {
                    selected.get("json").cloned().unwrap_or(selected)
                }
            }
        };
        for segment in &selector.path {
            current = match segment {
                ValuePathSegment::Key(key) => current.get(key)?.clone(),
                ValuePathSegment::Index(index) => current.get(*index as usize)?.clone(),
            };
        }
        Some(current)
    }

    pub fn evaluate_condition(
        &self,
        operator: ConditionOperator,
        left: &Value,
        right: Option<&Value>,
    ) -> Result<bool, ExpressionError> {
        evaluate_condition(operator, left, right)
    }
}

fn coerce_value_to_schema(
    mut value: Value,
    schema: &Value,
    path: &str,
    conversions: &mut Vec<StringConversionRecord>,
) -> Result<Value, ExpressionError> {
    let types = match schema.get("type") {
        Some(Value::String(value)) => vec![value.as_str()],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if types.is_empty() {
        return Ok(value);
    }
    let current = json_type(&value);
    let directly_compatible = types
        .iter()
        .any(|kind| *kind == current || (*kind == "number" && current == "integer"));
    let target = if directly_compatible {
        current
    } else {
        types
            .iter()
            .copied()
            .find(|kind| *kind != "null")
            .unwrap_or(types[0])
    };
    if !directly_compatible {
        let source_type = current.to_owned();
        value = match target {
            "string" => Value::String(value_to_text(value)),
            "integer" => Value::Number(
                integer_number(&value)
                    .ok_or_else(|| conversion_error(path, &source_type, target))?,
            ),
            "number" => Value::Number(
                value
                    .as_str()
                    .and_then(|text| text.parse::<f64>().ok())
                    .and_then(serde_json::Number::from_f64)
                    .ok_or_else(|| conversion_error(path, &source_type, target))?,
            ),
            "boolean" => Value::Bool(
                match value.as_str().map(str::to_ascii_lowercase).as_deref() {
                    Some("true") => true,
                    Some("false") => false,
                    _ => return Err(conversion_error(path, &source_type, target)),
                },
            ),
            "object" | "array" => {
                let parsed = value
                    .as_str()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .ok_or_else(|| conversion_error(path, &source_type, target))?;
                if (target == "object" && !parsed.is_object())
                    || (target == "array" && !parsed.is_array())
                {
                    return Err(conversion_error(path, &source_type, target));
                }
                parsed
            }
            "null" if value.as_str() == Some("null") => Value::Null,
            _ => return Err(conversion_error(path, &source_type, target)),
        };
        conversions.push(StringConversionRecord {
            target_path: path.into(),
            selector: None,
            source_type,
            mode: format!("schema:{target}"),
            result_bytes: serde_json::to_vec(&value).map_or(0, |bytes| bytes.len()),
        });
    }
    match (&mut value, target) {
        (Value::Object(fields), "object") => {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, field) in fields {
                    if let Some(field_schema) = properties.get(name) {
                        *field = coerce_value_to_schema(
                            std::mem::take(field),
                            field_schema,
                            &format!("{path}.{name}"),
                            conversions,
                        )?;
                    }
                }
            }
        }
        (Value::Array(items), "array") => {
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter_mut().enumerate() {
                    *item = coerce_value_to_schema(
                        std::mem::take(item),
                        item_schema,
                        &format!("{path}[{index}]"),
                        conversions,
                    )?;
                }
            }
        }
        _ => {}
    }
    Ok(value)
}

fn integer_number(value: &Value) -> Option<serde_json::Number> {
    if let Some(value) = value.as_i64() {
        return Some(value.into());
    }
    if let Some(value) = value.as_u64() {
        return Some(value.into());
    }
    if let Some(value) = value.as_f64()
        && value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        return Some((value as i64).into());
    }
    let text = value.as_str()?;
    text.parse::<i64>()
        .map(serde_json::Number::from)
        .or_else(|_| text.parse::<u64>().map(serde_json::Number::from))
        .ok()
}

fn conversion_error(path: &str, source: &str, target: &str) -> ExpressionError {
    ExpressionError::InvalidOperation(format!("{path} cannot convert {source} to {target}"))
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::String(value) => value,
        other => agentx_runtime_contracts::canonical_bytes(&other)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default(),
    }
}

fn select_array_item(value: &Value, selection: &ValueSelection) -> Option<Value> {
    let items = value.as_array()?;
    match selection {
        ValueSelection::Current | ValueSelection::First => items.first().cloned(),
        ValueSelection::Last => items.last().cloned(),
        ValueSelection::Index { index } => items.get(*index as usize).cloned(),
        ValueSelection::All => Some(value.clone()),
    }
}

fn number(value: &Value) -> Result<f64, ExpressionError> {
    value
        .as_f64()
        .ok_or_else(|| ExpressionError::InvalidOperation("expected number".into()))
}

fn evaluate_condition(
    operator: ConditionOperator,
    left: &Value,
    right: Option<&Value>,
) -> Result<bool, ExpressionError> {
    use ConditionOperator as Op;
    match operator {
        Op::Eq => Ok(right.is_some_and(|right| left == right)),
        Op::Ne => Ok(right.is_none_or(|right| left != right)),
        Op::Gt | Op::Gte | Op::Lt | Op::Lte => {
            let right = right.ok_or_else(|| {
                ExpressionError::InvalidOperation("comparison requires a right operand".into())
            })?;
            let (left, right) = (number(left)?, number(right)?);
            Ok(match operator {
                Op::Gt => left > right,
                Op::Gte => left >= right,
                Op::Lt => left < right,
                Op::Lte => left <= right,
                _ => unreachable!(),
            })
        }
        Op::In => Ok(match right.unwrap_or(&Value::Null) {
            Value::Array(values) => values.contains(left),
            Value::Object(values) => left.as_str().is_some_and(|key| values.contains_key(key)),
            Value::String(value) => left.as_str().is_some_and(|needle| value.contains(needle)),
            _ => false,
        }),
        Op::Contains | Op::NotContains => {
            let contains = right.is_some_and(|needle| match left {
                Value::String(value) => {
                    needle.as_str().is_some_and(|needle| value.contains(needle))
                }
                Value::Array(values) => values.contains(needle),
                _ => false,
            });
            Ok(if operator == Op::NotContains {
                !contains
            } else {
                contains
            })
        }
        Op::StartsWith => Ok(left
            .as_str()
            .zip(right.and_then(Value::as_str))
            .is_some_and(|(value, prefix)| value.starts_with(prefix))),
        Op::EndsWith => Ok(left
            .as_str()
            .zip(right.and_then(Value::as_str))
            .is_some_and(|(value, suffix)| value.ends_with(suffix))),
        Op::Matches => {
            let (Some(value), Some(pattern)) = (left.as_str(), right.and_then(Value::as_str))
            else {
                return Ok(false);
            };
            if pattern.len() > 1_024 {
                return Err(ExpressionError::InvalidOperation(
                    "regex pattern exceeds 1024 bytes".into(),
                ));
            }
            let regex = regex::Regex::new(pattern).map_err(|error| {
                ExpressionError::InvalidOperation(format!("invalid regex pattern: {error}"))
            })?;
            Ok(regex.is_match(value))
        }
        Op::IsEmpty => Ok(is_empty(left)),
        Op::IsNotEmpty => Ok(!is_empty(left)),
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn omit_removes_object_fields_and_template_segments() {
        let engine = ExpressionEngine;
        let parameters = json!({
            "kept":{"kind":"literal","value":"yes"},
            "omitted":{"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["missing"]},"missingPolicy":{"kind":"omit"}},
            "template":{"kind":"template","segments":[
                {"kind":"text","text":"prefix"},
                {"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["missing"]},"missingPolicy":{"kind":"omit"}}
            ]}
        });

        assert_eq!(
            engine
                .resolve_parameters(&parameters, &ExpressionContext::default())
                .unwrap(),
            json!({"kept":"yes","template":"prefix"})
        );
    }

    #[test]
    fn text_template_uses_canonical_json_for_variable_segments() {
        let parameters = json!({"text":{"kind":"template","segments":[
            {"kind":"text","text":"payload="},
            {"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["payload"]},"missingPolicy":{"kind":"error"}}
        ]}});
        let (value, conversions) = ExpressionEngine
            .resolve_parameters_with_conversions(
                &parameters,
                &ExpressionContext {
                    inputs: json!({"payload":{"z":1,"a":2}}),
                    ..ExpressionContext::default()
                },
            )
            .unwrap();
        assert_eq!(value, json!({"text":"payload={\"a\":2,\"z\":1}"}));
        assert_eq!(conversions.len(), 1);
        assert_eq!(conversions[0].target_path, "parameters.text.segments[1]");
        assert_eq!(conversions[0].source_type, "object");
        assert_eq!(conversions[0].result_bytes, 13);
    }

    #[test]
    fn recursive_input_bindings_preserve_typed_references_inside_json() {
        let parameters = json!({"payload":{"kind":"array","items":[
            {"kind":"literal","value":"a"},
            {"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["count"]},"missingPolicy":{"kind":"error"}},
            {"kind":"object","fields":{"label":{"kind":"template","segments":[
                {"kind":"text","text":"item-"},
                {"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["count"]},"missingPolicy":{"kind":"error"}}
            ]}}}
        ]}});
        let value = ExpressionEngine
            .resolve_parameters(
                &parameters,
                &ExpressionContext {
                    inputs: json!({"count":7}),
                    ..ExpressionContext::default()
                },
            )
            .unwrap();
        assert_eq!(value, json!({"payload":["a",7,{"label":"item-7"}]}));
    }

    #[test]
    fn schema_directed_conversion_parses_strings_or_reports_the_field() {
        let parameters = json!({
            "count":{"kind":"literal","value":"12"},
            "payload":{"kind":"literal","value":"{\"ok\":true}"}
        });
        let schema = json!({"type":"object","properties":{
            "count":{"type":"integer"},
            "payload":{"type":"object","properties":{"ok":{"type":"boolean"}}}
        }});
        let (value, conversions) = ExpressionEngine
            .resolve_parameters_with_schema(&parameters, &schema, &ExpressionContext::default())
            .unwrap();
        assert_eq!(value, json!({"count":12,"payload":{"ok":true}}));
        assert_eq!(conversions.len(), 2);

        let error = ExpressionEngine
            .resolve_parameters_with_schema(
                &json!({"count":{"kind":"literal","value":"twelve"}}),
                &json!({"type":"object","properties":{"count":{"type":"integer"}}}),
                &ExpressionContext::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("parameters.count"));
    }

    #[test]
    fn integer_schema_accepts_integral_json_numbers_and_rejects_fractions() {
        let schema = json!({"type":"object","properties":{"budget":{"type":"integer"}}});
        let (value, conversions) = ExpressionEngine
            .resolve_parameters_with_schema(
                &json!({"budget":{"kind":"literal","value":1_000_000.0}}),
                &schema,
                &ExpressionContext::default(),
            )
            .unwrap();
        assert_eq!(value, json!({"budget":1_000_000}));
        assert_eq!(conversions[0].source_type, "number");

        let error = ExpressionEngine
            .resolve_parameters_with_schema(
                &json!({"budget":{"kind":"literal","value":1.5}}),
                &schema,
                &ExpressionContext::default(),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot convert number to integer")
        );
    }

    #[test]
    fn matches_uses_a_validated_linear_time_regex() {
        assert!(
            ExpressionEngine
                .evaluate_condition(
                    ConditionOperator::Matches,
                    &json!("invoice-2026"),
                    Some(&json!("^invoice-[0-9]{4}$")),
                )
                .unwrap()
        );
    }
}
