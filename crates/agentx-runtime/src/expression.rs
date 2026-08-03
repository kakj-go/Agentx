use std::{collections::BTreeSet, sync::Arc};

use cel_interpreter::extractors::This;
use cel_interpreter::objects::Key;
use cel_interpreter::{Context, FunctionContext, Program, Value as CelValue};
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
        let source = normalize_aliases(source);
        let program =
            Program::compile(&source).map_err(|error| ExpressionError::Parse(error.to_string()))?;
        let allowed = BTreeSet::from([
            "agentx_json",
            "agentx_input",
            "agentx_item_index",
            "agentx_run_index",
        ]);
        for variable in program.references().variables() {
            if !allowed.contains(variable) {
                return Err(ExpressionError::ForbiddenReference(variable.to_string()));
            }
        }
        let allowed_functions = BTreeSet::from([
            "all",
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
            "node",
            "size",
            "startsWith",
            "string",
            "timestamp",
            "uint",
            "agentxAll",
        ]);
        for function in program.references().functions() {
            if !function.starts_with('_') && !allowed_functions.contains(function) {
                return Err(ExpressionError::ForbiddenReference(function.to_string()));
            }
        }
        Ok(())
    }

    pub fn evaluate(
        &self,
        source: &str,
        values: &ExpressionContext,
    ) -> Result<Value, ExpressionError> {
        self.validate(source)?;
        let source = normalize_aliases(source);
        let program =
            Program::compile(&source).map_err(|error| ExpressionError::Parse(error.to_string()))?;
        let mut context = Context::default();
        context
            .add_variable("agentx_json", &values.json)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("agentx_input", &values.input)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("agentx_nodes", &values.linked_nodes)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("agentx_item_index", values.item_index as u64)
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context
            .add_variable("agentx_run_index", u64::from(values.run_index))
            .map_err(|error| ExpressionError::Evaluation(error.to_string()))?;
        context.add_function("node", linked_node);
        context.add_function("agentxAll", linked_all);
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
            Value::String(value) if value.starts_with('=') => self.evaluate(&value[1..], context),
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
}

fn linked_node(
    ftx: &FunctionContext,
    name: Arc<String>,
) -> Result<CelValue, cel_interpreter::ExecutionError> {
    let nodes = ftx.ptx.get_variable("agentx_nodes")?;
    let CelValue::Map(nodes) = nodes else {
        return Err(ftx.error("linked node context is not a map"));
    };
    nodes
        .get(&Key::from(name.as_str()))
        .cloned()
        .ok_or_else(|| ftx.error(format!("linked node '{name}' is unavailable")))
}

fn linked_all(
    ftx: &FunctionContext,
    This(node): This<CelValue>,
    branch: Arc<String>,
    run: i64,
) -> Result<CelValue, cel_interpreter::ExecutionError> {
    let CelValue::Map(node) = node else {
        return Err(ftx.error("node data is not a map"));
    };
    let branch = node
        .get(&Key::from(branch.as_str()))
        .ok_or_else(|| ftx.error(format!("branch '{branch}' is unavailable")))?;
    let CelValue::Map(runs) = branch else {
        return Err(ftx.error("branch data is not a map"));
    };
    runs.get(&Key::from(run.to_string()))
        .cloned()
        .ok_or_else(|| ftx.error(format!("run '{run}' is unavailable")))
}

fn normalize_aliases(source: &str) -> String {
    let aliases = [
        ("$itemIndex", "agentx_item_index"),
        ("$runIndex", "agentx_run_index"),
        ("$input", "agentx_input"),
        ("$json", "agentx_json"),
    ];
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
        if source[index..].starts_with(".all(") {
            output.push_str(".agentxAll(");
            index += ".all(".len();
            continue;
        }
        if let Some((alias, replacement)) = aliases
            .iter()
            .find(|(alias, _)| source[index..].starts_with(alias))
        {
            output.push_str(replacement);
            index += alias.len();
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
    fn evaluates_agentx_aliases_and_linked_runs() {
        let context = ExpressionContext {
            json: json!({"amount": 4}),
            input: json!([{"json":{"amount":4}}]),
            item_index: 2,
            run_index: 3,
            linked_nodes: json!({"Source":{"main":{"0":[{"json":{"value":8}}]}}}),
        };
        let engine = ExpressionEngine;
        assert_eq!(
            engine
                .evaluate("$json.amount + $itemIndex", &context)
                .unwrap(),
            6
        );
        assert_eq!(
            engine
                .evaluate("node('Source').all('main', 0)[0].json.value", &context)
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
                .evaluate("'$json'", &ExpressionContext::default())
                .unwrap(),
            "$json"
        );
    }
}
