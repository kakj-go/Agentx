use std::collections::BTreeSet;

use cel_interpreter::{Context, Program};
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
}

#[derive(Debug, Error)]
pub enum ExpressionError {
    #[error("expression parse failed: {0}")]
    Parse(String),
    #[error("expression uses forbidden reference '{0}'")]
    ForbiddenReference(String),
    #[error("expression evaluation failed: {0}")]
    Evaluation(String),
    #[error("expression result cannot be represented as JSON: {0}")]
    Result(String),
}

#[derive(Clone, Debug, Default)]
pub struct ExpressionEngine;

impl ExpressionEngine {
    pub fn validate(&self, source: &str) -> Result<(), ExpressionError> {
        let source = normalize_selectors(strip_template(source));
        let program =
            Program::compile(&source).map_err(|error| ExpressionError::Parse(error.to_string()))?;
        let allowed =
            BTreeSet::from(["inputs", "outputs", "contexts", "execution", "item", "loop"]);
        for variable in program.references().variables() {
            if !allowed.contains(variable) {
                return Err(ExpressionError::ForbiddenReference(variable.to_string()));
            }
        }
        let allowed_functions = BTreeSet::from([
            "contains",
            "double",
            "duration",
            "endsWith",
            "getDate",
            "getDayOfMonth",
            "getDayOfWeek",
            "getDayOfYear",
            "getFullYear",
            "getHours",
            "getMilliseconds",
            "getMinutes",
            "getMonth",
            "getSeconds",
            "int",
            "matches",
            "max",
            "min",
            "size",
            "startsWith",
            "string",
            "timestamp",
            "uint",
        ]);
        for function in program.references().functions() {
            if !function.starts_with('_') && !allowed_functions.contains(function) {
                return Err(ExpressionError::ForbiddenReference(function.to_string()));
            }
        }
        Ok(())
    }

    pub fn validate_template(&self, source: &str) -> Result<(), ExpressionError> {
        let mut cursor = 0;
        while let Some(start_offset) = source[cursor..].find("${{") {
            let start = cursor + start_offset;
            let Some(end_offset) = source[start + 3..].find("}}") else {
                return Err(ExpressionError::Parse(
                    "template expression is not closed".into(),
                ));
            };
            let end = start + 3 + end_offset + 2;
            self.validate(&source[start..end])?;
            cursor = end;
        }
        Ok(())
    }

    pub fn evaluate(
        &self,
        source: &str,
        values: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        self.validate(source)?;
        let source = normalize_selectors(strip_template(source));
        let program =
            Program::compile(&source).map_err(|error| ExpressionError::Parse(error.to_string()))?;
        let mut context = Context::default();
        context
            .add_variable("inputs", &values.inputs)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("outputs", &values.outputs)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("contexts", &values.contexts)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("execution", &values.execution)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        let item = match &values.json {
            Value::Object(fields) => {
                let mut item = fields.clone();
                item.insert("json".into(), values.json.clone());
                Value::Object(item)
            }
            value => serde_json::json!({"json": value}),
        };
        context
            .add_variable("item", &item)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("loop", &values.loop_context)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        program
            .execute(&context)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?
            .json()
            .map_err(|error| ExpressionError::Result(error.to_string()))
    }

    pub fn resolve_parameters(
        &self,
        parameters: &Value,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        match parameters {
            Value::String(value) if is_template(value) => self.resolve_template(value, context),
            Value::Array(values) => values
                .iter()
                .map(|value| self.resolve_parameters(value, context))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(values) => values
                .iter()
                .map(|(key, value)| {
                    self.resolve_parameters(value, context)
                        .map(|value| (key.clone(), value))
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object),
            value => Ok(value.clone()),
        }
    }

    fn resolve_template(
        &self,
        source: &str,
        context: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        let trimmed = source.trim();
        if trimmed.starts_with("${{")
            && trimmed.ends_with("}}")
            && trimmed[..trimmed.len() - 2].matches("${{").count() == 1
        {
            return self.evaluate(trimmed, context);
        }
        let mut output = String::with_capacity(source.len());
        let mut cursor = 0;
        while let Some(start) = source[cursor..].find("${{") {
            let start = cursor + start;
            output.push_str(&source[cursor..start]);
            let Some(end_offset) = source[start + 3..].find("}}") else {
                return Err(ExpressionError::Parse(
                    "template expression is not closed".into(),
                ));
            };
            let end = start + 3 + end_offset + 2;
            let value = self.evaluate(&source[start..end], context)?;
            output.push_str(&value_to_text(value));
            cursor = end;
        }
        output.push_str(&source[cursor..]);
        Ok(Value::String(output))
    }
}

fn is_template(value: &str) -> bool {
    value.contains("${{")
}

fn strip_template(source: &str) -> &str {
    let source = source.trim();
    source
        .strip_prefix("${{")
        .and_then(|value| value.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(source)
}

fn value_to_text(value: Value) -> String {
    match value {
        Value::String(value) => value,
        other => serde_json::to_string(&other).unwrap_or_default(),
    }
}

fn normalize_selectors(source: &str) -> String {
    let selectors = ["current", "first", "last", "all"];
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    let bytes = source.as_bytes();
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            output.push(byte as char);
            if byte == b'\\' && index + 1 < bytes.len() {
                index += 1;
                output.push(bytes[index] as char);
            } else if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            output.push(byte as char);
            index += 1;
            continue;
        }
        if let Some(selector) = selectors
            .iter()
            .find(|selector| source[index..].starts_with(&format!(".{selector}()")))
        {
            output.push('.');
            output.push_str(selector);
            index += selector.len() + 3;
        } else {
            output.push(byte as char);
            index += 1;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn evaluates_workflow_namespaces_and_selectors() {
        let context = ExpressionContext {
            json: json!({"amount": 4}),
            input: json!([{"json":{"amount":4}}]),
            item_index: 2,
            run_index: 3,
            linked_nodes: json!({"Source":{"main":{"0":[{"json":{"value":8}}]}}}),
            inputs: json!({"amount": 4}),
            outputs: json!({"Source":{"main":{"all":[{"json":{"value":8}}]}}}),
            contexts: json!({"region":"cn"}),
            execution: json!({}),
            loop_context: json!({"iteration": 2}),
        };
        let engine = ExpressionEngine;
        assert_eq!(engine.evaluate("${{ item.amount }}", &context).unwrap(), 4);
        assert_eq!(
            engine
                .evaluate("${{ item.json.amount }}", &context)
                .unwrap(),
            4
        );
        assert_eq!(
            engine.evaluate("${{ inputs.amount }}", &context).unwrap(),
            4
        );
        assert_eq!(
            engine.evaluate("${{ contexts.region }}", &context).unwrap(),
            "cn"
        );
        assert_eq!(
            engine
                .evaluate("${{ outputs.Source.main.all()[0].json.value }}", &context)
                .unwrap(),
            8
        );
    }

    #[test]
    fn rejects_undeclared_variables_and_does_not_replace_strings() {
        let engine = ExpressionEngine;
        assert!(matches!(
            engine.validate("secret.token"),
            Err(ExpressionError::ForbiddenReference(_))
        ));
        assert_eq!(
            engine
                .evaluate("'inputs.token'", &ExpressionContext::default())
                .unwrap(),
            "inputs.token"
        );
    }

    #[test]
    fn validates_and_resolves_expressions_embedded_in_text() {
        let engine = ExpressionEngine;
        let context = ExpressionContext {
            inputs: json!({"amount": 4}),
            contexts: json!({"region":"cn"}),
            ..ExpressionContext::default()
        };
        engine
            .validate_template("v1:${{ inputs.amount }}-${{ contexts.region }}")
            .unwrap();
        assert_eq!(
            engine
                .resolve_parameters(
                    &json!({"answer":"v1:${{ inputs.amount }}-${{ contexts.region }}"}),
                    &context,
                )
                .unwrap(),
            json!({"answer":"v1:4-cn"})
        );
        assert!(matches!(
            engine.validate_template("${{ inputs.amount"),
            Err(ExpressionError::Parse(_))
        ));
    }
}
