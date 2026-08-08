use std::collections::BTreeMap;

use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub fn execute(task: &RuntimeTask) -> Result<TaskResult> {
    match task.node_type.as_str() {
        "stop_and_error" => stop_with_error(task),
        "structured_validator" => validate(task),
        _ => unreachable!(),
    }
}

fn stop_with_error(task: &RuntimeTask) -> Result<TaskResult> {
    let first = flatten(task).into_iter().next().unwrap_or_default();
    let parameters =
        ExpressionEngine.resolve_parameters(&task.node_parameters, &context(task, &first, 0))?;
    Ok(TaskResult::Failed {
        code: parameters
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("WORKFLOW_STOPPED")
            .into(),
        message: parameters
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Workflow stopped by Stop With Error")
            .into(),
        retryable: false,
    })
}

fn validate(task: &RuntimeTask) -> Result<TaskResult> {
    let schema = task
        .node_parameters
        .get("schema")
        .context("schema is required")?;
    let validator = jsonschema::validator_for(schema).context("invalid JSON Schema")?;
    let fail_on_invalid = task.node_parameters.get("mode").and_then(Value::as_str) == Some("fail");
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for mut item in flatten(task) {
        let errors = validator
            .iter_errors(&item.json)
            .map(|error| {
                json!({
                    "path": error.instance_path.to_string(), "message": error.to_string(),
                })
            })
            .collect::<Vec<_>>();
        if errors.is_empty() {
            valid.push(item);
        } else if fail_on_invalid {
            return Ok(TaskResult::Failed {
                code: "SCHEMA_VALIDATION_FAILED".into(),
                message: errors[0]["message"]
                    .as_str()
                    .unwrap_or("JSON Schema validation failed")
                    .into(),
                retryable: false,
            });
        } else {
            item.metadata
                .insert("validationErrors".into(), Value::Array(errors));
            invalid.push(item);
        }
    }
    Ok(TaskResult::Completed(BTreeMap::from([
        ("valid".into(), valid),
        ("invalid".into(), invalid),
    ])))
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
    }
}
