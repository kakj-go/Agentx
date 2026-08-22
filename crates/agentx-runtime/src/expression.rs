use std::collections::BTreeMap;

use agentx_domain::{
    DynamicValue, ExpressionBinaryOperator, ExpressionFunction, ExpressionNode,
    ExpressionUnaryOperator, MissingValuePolicy, TemplateSegment, ValueCoercion, ValueNamespace,
    ValuePathSegment, ValueSelection, ValueSelector,
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
    pub selector: ValueSelector,
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

    fn resolve_parameter_value(
        &self,
        value: &Value,
        context: &ExpressionContext,
        target_path: &str,
        conversions: &mut Vec<StringConversionRecord>,
    ) -> Result<Option<Value>, ExpressionError> {
        if value.is_object()
            && let Ok(dynamic) = serde_json::from_value::<DynamicValue>(value.clone())
        {
            return self.resolve_dynamic_optional_traced(
                &dynamic,
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
        value: &DynamicValue,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        Ok(self
            .resolve_dynamic_optional(value, context)?
            .unwrap_or(Value::Null))
    }

    /// Resolves a field-level dynamic value while preserving `omit` as the
    /// absence of a JSON member instead of silently converting it to null.
    pub fn resolve_dynamic_optional(
        &self,
        value: &DynamicValue,
        context: &ExpressionContext,
    ) -> Result<Option<Value>, ExpressionError> {
        self.resolve_dynamic_optional_with_conversions(value, context, "value")
            .map(|(value, _)| value)
    }

    pub fn resolve_dynamic_optional_with_conversions(
        &self,
        value: &DynamicValue,
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
        value: &DynamicValue,
        context: &ExpressionContext,
        target_path: &str,
        conversions: &mut Vec<StringConversionRecord>,
    ) -> Result<Option<Value>, ExpressionError> {
        match value {
            DynamicValue::Literal { value } => Ok(Some(value.clone())),
            DynamicValue::Reference {
                selector,
                missing_policy,
                coerce,
            } => {
                let resolved =
                    self.resolve_selector_with_policy(selector, missing_policy, context)?;
                if *coerce != Some(ValueCoercion::String) {
                    return Ok(resolved);
                }
                Ok(resolved.map(|value| {
                    let source_type = json_type(&value).to_owned();
                    let text = value_to_text(value);
                    conversions.push(StringConversionRecord {
                        target_path: target_path.to_owned(),
                        selector: selector.clone(),
                        source_type,
                        mode: "reference".into(),
                        result_bytes: text.len(),
                    });
                    Value::String(text)
                }))
            }
            DynamicValue::Template { segments } => {
                let mut text = String::new();
                for (index, segment) in segments.iter().enumerate() {
                    match segment {
                        TemplateSegment::Text { text: value } => text.push_str(value),
                        TemplateSegment::Reference {
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
                                    selector: selector.clone(),
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
            DynamicValue::Expression { root } => self.evaluate_node(root, context).map(Some),
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
                MissingValuePolicy::Default { value } => {
                    self.resolve_dynamic_optional(value, context)
                }
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

    fn evaluate_node(
        &self,
        node: &ExpressionNode,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        match node {
            ExpressionNode::Literal { value } => Ok(value.clone()),
            ExpressionNode::Reference {
                selector,
                missing_policy,
            } => self
                .resolve_selector_with_policy(selector, missing_policy, context)?
                .ok_or_else(|| {
                    ExpressionError::InvalidOperation(
                        "missingPolicy=omit cannot remove an expression operand".into(),
                    )
                }),
            ExpressionNode::Array { items } => items
                .iter()
                .map(|item| self.evaluate_node(item, context))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            ExpressionNode::Object { fields } => fields
                .iter()
                .map(|(key, value)| {
                    self.evaluate_node(value, context)
                        .map(|value| (key.clone(), value))
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object),
            ExpressionNode::Unary { operator, operand } => {
                let value = self.evaluate_node(operand, context)?;
                match operator {
                    ExpressionUnaryOperator::Not => Ok(Value::Bool(!truthy(&value))),
                    ExpressionUnaryOperator::Negate => json_number(-number(&value)?),
                }
            }
            ExpressionNode::Binary {
                operator,
                left,
                right,
            } => {
                if *operator == ExpressionBinaryOperator::And {
                    let left = self.evaluate_node(left, context)?;
                    return Ok(Value::Bool(
                        truthy(&left) && truthy(&self.evaluate_node(right, context)?),
                    ));
                }
                if *operator == ExpressionBinaryOperator::Or {
                    let left = self.evaluate_node(left, context)?;
                    return Ok(Value::Bool(
                        truthy(&left) || truthy(&self.evaluate_node(right, context)?),
                    ));
                }
                let left = self.evaluate_node(left, context)?;
                let right = self.evaluate_node(right, context)?;
                evaluate_binary(*operator, left, right)
            }
            ExpressionNode::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                if truthy(&self.evaluate_node(condition, context)?) {
                    self.evaluate_node(then_value, context)
                } else {
                    self.evaluate_node(else_value, context)
                }
            }
            ExpressionNode::Call {
                function,
                arguments,
            } => {
                let values = arguments
                    .iter()
                    .map(|value| self.evaluate_node(value, context))
                    .collect::<Result<Vec<_>, _>>()?;
                evaluate_function(*function, &values)
            }
        }
    }
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

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn number(value: &Value) -> Result<f64, ExpressionError> {
    value
        .as_f64()
        .ok_or_else(|| ExpressionError::InvalidOperation("expected number".into()))
}

fn json_number(value: f64) -> Result<Value, ExpressionError> {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| ExpressionError::Result("number is not finite".into()))
}

fn evaluate_binary(
    operator: ExpressionBinaryOperator,
    left: Value,
    right: Value,
) -> Result<Value, ExpressionError> {
    use ExpressionBinaryOperator as Op;
    match operator {
        Op::Eq => Ok(Value::Bool(left == right)),
        Op::Ne => Ok(Value::Bool(left != right)),
        Op::Gt | Op::Gte | Op::Lt | Op::Lte => {
            let (left, right) = (number(&left)?, number(&right)?);
            Ok(Value::Bool(match operator {
                Op::Gt => left > right,
                Op::Gte => left >= right,
                Op::Lt => left < right,
                Op::Lte => left <= right,
                _ => unreachable!(),
            }))
        }
        Op::Add if left.is_string() || right.is_string() => Ok(Value::String(format!(
            "{}{}",
            value_to_text(left),
            value_to_text(right)
        ))),
        Op::Add => json_number(number(&left)? + number(&right)?),
        Op::Subtract => json_number(number(&left)? - number(&right)?),
        Op::Multiply => json_number(number(&left)? * number(&right)?),
        Op::Divide => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(ExpressionError::InvalidOperation("division by zero".into()));
            }
            json_number(number(&left)? / divisor)
        }
        Op::Modulo => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(ExpressionError::InvalidOperation("modulo by zero".into()));
            }
            json_number(number(&left)? % divisor)
        }
        Op::In => Ok(Value::Bool(match right {
            Value::Array(values) => values.contains(&left),
            Value::Object(values) => left.as_str().is_some_and(|key| values.contains_key(key)),
            Value::String(value) => left.as_str().is_some_and(|needle| value.contains(needle)),
            _ => false,
        })),
        Op::And | Op::Or => unreachable!("short-circuit operators are handled by the caller"),
    }
}

fn evaluate_function(
    function: ExpressionFunction,
    values: &[Value],
) -> Result<Value, ExpressionError> {
    use ExpressionFunction as Fn;
    let first = values.first().cloned().unwrap_or(Value::Null);
    match function {
        Fn::Contains => Ok(Value::Bool(values.get(1).is_some_and(
            |needle| match &first {
                Value::String(value) => {
                    needle.as_str().is_some_and(|needle| value.contains(needle))
                }
                Value::Array(value) => value.contains(needle),
                _ => false,
            },
        ))),
        Fn::StartsWith => Ok(Value::Bool(
            first
                .as_str()
                .zip(values.get(1).and_then(Value::as_str))
                .is_some_and(|(value, prefix)| value.starts_with(prefix)),
        )),
        Fn::EndsWith => Ok(Value::Bool(
            first
                .as_str()
                .zip(values.get(1).and_then(Value::as_str))
                .is_some_and(|(value, suffix)| value.ends_with(suffix)),
        )),
        // The visual protocol does not accept an arbitrary executable regex
        // object. Until the typed pattern validator is introduced, matches is
        // a deterministic literal substring match.
        Fn::Matches => {
            let (Some(value), Some(pattern)) =
                (first.as_str(), values.get(1).and_then(Value::as_str))
            else {
                return Ok(Value::Bool(false));
            };
            if pattern.len() > 1_024 {
                return Err(ExpressionError::InvalidOperation(
                    "regex pattern exceeds 1024 bytes".into(),
                ));
            }
            let regex = regex::Regex::new(pattern).map_err(|error| {
                ExpressionError::InvalidOperation(format!("invalid regex pattern: {error}"))
            })?;
            Ok(Value::Bool(regex.is_match(value)))
        }
        Fn::Size => Ok(Value::from(match first {
            Value::String(value) => value.chars().count() as u64,
            Value::Array(value) => value.len() as u64,
            Value::Object(value) => value.len() as u64,
            _ => 0,
        })),
        Fn::String | Fn::Timestamp | Fn::Duration => Ok(Value::String(value_to_text(first))),
        Fn::Int | Fn::Uint => Ok(Value::from(number(&first)? as i64)),
        Fn::Double => json_number(number(&first)?),
        Fn::Max | Fn::Min => {
            let numbers = values.iter().map(number).collect::<Result<Vec<_>, _>>()?;
            let value = numbers
                .into_iter()
                .reduce(|left, right| {
                    if function == Fn::Max {
                        left.max(right)
                    } else {
                        left.min(right)
                    }
                })
                .ok_or_else(|| {
                    ExpressionError::InvalidOperation("min/max requires an argument".into())
                })?;
            json_number(value)
        }
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
    fn reference_string_coercion_uses_canonical_json() {
        let dynamic: DynamicValue = serde_json::from_value(json!({
            "kind":"reference",
            "selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["payload"]},
            "missingPolicy":{"kind":"error"},
            "coerce":"string"
        })).unwrap();
        let (value, conversions) = ExpressionEngine
            .resolve_dynamic_optional_with_conversions(
                &dynamic,
                &ExpressionContext {
                    inputs: json!({"payload":{"z":1,"a":2}}),
                    ..ExpressionContext::default()
                },
                "end.outputs.answer",
            )
            .unwrap();
        assert_eq!(value, Some(json!("{\"a\":2,\"z\":1}")));
        assert_eq!(conversions.len(), 1);
        assert_eq!(conversions[0].target_path, "end.outputs.answer");
        assert_eq!(conversions[0].source_type, "object");
        assert_eq!(conversions[0].result_bytes, 13);
    }

    #[test]
    fn matches_uses_a_validated_linear_time_regex() {
        let dynamic: DynamicValue = serde_json::from_value(json!({
            "kind":"expression",
            "root":{"kind":"call","function":"matches","arguments":[
                {"kind":"literal","value":"invoice-2026"},
                {"kind":"literal","value":"^invoice-[0-9]{4}$"}
            ]}
        }))
        .unwrap();
        assert_eq!(
            ExpressionEngine
                .resolve_dynamic(&dynamic, &ExpressionContext::default())
                .unwrap(),
            json!(true)
        );
    }
}
