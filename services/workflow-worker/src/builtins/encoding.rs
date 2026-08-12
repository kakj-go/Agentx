use std::collections::BTreeMap;

use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};

pub fn execute(task: &RuntimeTask) -> Result<TaskResult> {
    let mut output = Vec::new();
    for (index, mut item) in flatten(task).into_iter().enumerate() {
        let parameters = ExpressionEngine
            .resolve_parameters(&task.node_parameters, &context(task, &item, index))?;
        let field = string_parameter(&parameters, "field")?;
        let output_field = parameters
            .get("outputField")
            .and_then(Value::as_str)
            .unwrap_or(field);
        let value = get_path(&item.json, field).context("source field is missing")?;
        let raw = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        let transformed = match task.node_type.as_str() {
            "base64" => match parameters
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("encode")
            {
                "encode" => STANDARD.encode(raw.as_bytes()),
                "decode" => String::from_utf8(
                    STANDARD
                        .decode(raw.trim())
                        .context("invalid Base64 input")?,
                )
                .context("decoded Base64 value is not UTF-8")?,
                other => bail!("unsupported Base64 operation {other}"),
            },
            "hash" => {
                let digest = match parameters
                    .get("algorithm")
                    .and_then(Value::as_str)
                    .unwrap_or("sha256")
                {
                    "sha256" => Sha256::digest(raw.as_bytes()).to_vec(),
                    "sha512" => Sha512::digest(raw.as_bytes()).to_vec(),
                    other => bail!("unsupported hash algorithm {other}"),
                };
                if parameters.get("encoding").and_then(Value::as_str) == Some("base64") {
                    STANDARD.encode(digest)
                } else {
                    digest.iter().map(|byte| format!("{byte:02x}")).collect()
                }
            }
            _ => unreachable!(),
        };
        set_path(&mut item.json, output_field, Value::String(transformed))?;
        output.push(item);
    }
    Ok(completed("main", output))
}

fn flatten(task: &RuntimeTask) -> Vec<Item> {
    task.inputs.values().flatten().cloned().collect()
}

fn context(task: &RuntimeTask, item: &Item, index: usize) -> ExpressionContext {
    ExpressionContext {
        json: item.json.clone(),
        input: serde_json::to_value(flatten(task)).unwrap_or(Value::Null),
        item_index: index,
        run_index: task.run_index,
        linked_nodes: task.linked_nodes.clone(),
        inputs: task.workflow_inputs.clone(),
        outputs: task.linked_nodes.clone(),
        contexts: task.contexts.clone(),
        loop_context: serde_json::json!({"iteration": task.iteration_index, "itemIndex": index}),
        execution: serde_json::json!({
            "executionId": task.execution_id,
            "nodeExecutionId": task.node_execution_id,
            "runIndex": task.run_index,
            "iterationIndex": task.iteration_index,
            "contextVersion": task.context_version,
        }),
    }
}

fn completed(port: &str, items: Vec<Item>) -> TaskResult {
    TaskResult::Completed(BTreeMap::from([(port.into(), items)]))
}

fn string_parameter<'a>(parameters: &'a Value, name: &str) -> Result<&'a str> {
    parameters
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("{name} is required"))
}

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, segment| current.get(segment))
}

fn set_path(value: &mut Value, path: &str, next: Value) -> Result<()> {
    let mut segments = path.split('.').peekable();
    let mut current = value;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current
                .as_object_mut()
                .context("field parent must be an object")?
                .insert(segment.into(), next);
            return Ok(());
        }
        let object = current
            .as_object_mut()
            .context("field parent must be an object")?;
        current = object
            .entry(segment)
            .or_insert_with(|| Value::Object(Default::default()));
    }
    bail!("field path cannot be empty")
}
