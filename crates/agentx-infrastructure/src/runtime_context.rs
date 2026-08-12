use std::collections::BTreeMap;

use agentx_domain::{
    ContextDefinition, ContextMergePolicy, ContextScope, ContextWrite, ContextWriteOperation,
};
use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use sqlx::{MySql, Row, Transaction};
use uuid::Uuid;

pub(crate) fn initial_context(definitions: &BTreeMap<String, ContextDefinition>) -> Value {
    Value::Object(
        definitions
            .iter()
            .map(|(name, definition)| (name.clone(), definition.default.clone()))
            .collect(),
    )
}

pub(crate) fn scoped_context(
    definitions: &BTreeMap<String, ContextDefinition>,
    scope: ContextScope,
) -> Value {
    Value::Object(
        definitions
            .iter()
            .filter(|(_, definition)| definition.scope == scope)
            .map(|(name, definition)| (name.clone(), definition.default.clone()))
            .collect(),
    )
}

pub(crate) fn merge_context_overlay(
    current: &mut Value,
    base: &Value,
    next: &Value,
    policy: ContextMergePolicy,
) -> Result<()> {
    match policy {
        ContextMergePolicy::Replace => {
            *current = next.clone();
        }
        ContextMergePolicy::RejectConflict => {
            anyhow::ensure!(
                current == base || current == next,
                "CONTEXT_OVERLAY_CONFLICT"
            );
            *current = next.clone();
        }
        ContextMergePolicy::Append => {
            let base = base
                .as_array()
                .context("SESSION_CONTEXT_OVERLAY_BASE_NOT_ARRAY")?;
            let next = next
                .as_array()
                .context("SESSION_CONTEXT_OVERLAY_VALUE_NOT_ARRAY")?;
            anyhow::ensure!(
                next.len() >= base.len() && next[..base.len()] == base[..],
                "SESSION_CONTEXT_APPEND_OVERLAY_REWROTE_BASE"
            );
            current
                .as_array_mut()
                .context("SESSION_CONTEXT_APPEND_TARGET_NOT_ARRAY")?
                .extend_from_slice(&next[base.len()..]);
        }
        ContextMergePolicy::MergeObject => {
            let base = base
                .as_object()
                .context("SESSION_CONTEXT_OVERLAY_BASE_NOT_OBJECT")?;
            let next = next
                .as_object()
                .context("SESSION_CONTEXT_OVERLAY_VALUE_NOT_OBJECT")?;
            let current = current
                .as_object_mut()
                .context("SESSION_CONTEXT_MERGE_TARGET_NOT_OBJECT")?;
            for (key, value) in next {
                if base.get(key) != Some(value) {
                    current.insert(key.clone(), value.clone());
                }
            }
        }
        ContextMergePolicy::Increment => {
            let delta = next
                .as_f64()
                .context("SESSION_CONTEXT_OVERLAY_VALUE_NOT_NUMBER")?
                - base
                    .as_f64()
                    .context("SESSION_CONTEXT_OVERLAY_BASE_NOT_NUMBER")?;
            let value = current
                .as_f64()
                .context("SESSION_CONTEXT_INCREMENT_TARGET_NOT_NUMBER")?
                + delta;
            *current = json!(value);
        }
    }
    Ok(())
}

pub(crate) async fn load_output_namespace(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> Result<Value> {
    let rows = sqlx::query("SELECT node_key,run_index,output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND output_json IS NOT NULL ORDER BY run_index,id")
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_all(&mut **transaction)
        .await?;
    let mut namespace = Map::new();
    for row in rows {
        merge_output_namespace(
            &mut namespace,
            &row.try_get::<String, _>("node_key")?,
            row.try_get("run_index")?,
            &row.try_get::<Value, _>("output_json")?,
        );
    }
    Ok(Value::Object(namespace))
}

pub(crate) fn merge_output_namespace(
    namespace: &mut Map<String, Value>,
    key: &str,
    run: u32,
    outputs: &Value,
) {
    let node = namespace
        .entry(key.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("node output namespace must be an object");
    node.entry("runs")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("node runs namespace must be an object")
        .insert(run.to_string(), outputs.clone());
    let Some(ports) = outputs.as_object() else {
        return;
    };
    for (port, value) in ports {
        let items = value.as_array().cloned().unwrap_or_default();
        let first = items.first().cloned().unwrap_or(Value::Null);
        let last = items.last().cloned().unwrap_or(Value::Null);
        node.insert(
            port.clone(),
            json!({"current":first,"first":first,"last":last,"all":items,"runIndex":run}),
        );
        if port == "main"
            && let Some(fields) = first.get("json").and_then(Value::as_object)
        {
            for (field, value) in fields {
                if !matches!(field.as_str(), "runs" | "main" | "error") {
                    node.insert(field.clone(), value.clone());
                }
            }
        }
    }
}

pub(crate) fn apply_context_writes(
    contexts: &mut Value,
    writes: &[ContextWrite],
    definitions: &BTreeMap<String, ContextDefinition>,
    base: &ExpressionContext,
) -> Result<()> {
    let engine = ExpressionEngine;
    let mut evaluation = base.clone();
    for write in writes {
        let root = write.path.split('.').next().unwrap_or_default();
        let definition = definitions
            .get(root)
            .with_context(|| format!("CONTEXT_PATH_UNDECLARED: {}", write.path))?;
        anyhow::ensure!(definition.mutable, "CONTEXT_READ_ONLY: {}", write.path);
        evaluation.contexts = contexts.clone();
        let resolved = engine.resolve_parameters(&write.value, &evaluation)?;
        apply_context_operation(contexts, &write.path, write.operation, resolved)?;
        let current = contexts.get(root).unwrap_or(&Value::Null);
        let validator = jsonschema::validator_for(&definition.schema)
            .with_context(|| format!("Context schema '{root}' is invalid"))?;
        validator
            .validate(current)
            .map_err(|error| anyhow::anyhow!("CONTEXT_VALUE_INVALID: {root}: {error}"))?;
        if let Some(max_size) = definition.max_size {
            anyhow::ensure!(
                serde_json::to_vec(current)?.len() as u64 <= max_size,
                "CONTEXT_VALUE_TOO_LARGE: {root}"
            );
        }
    }
    Ok(())
}

pub(crate) fn apply_output_projection(
    outputs: &mut BTreeMap<String, Vec<Item>>,
    projection: &Value,
    base: &ExpressionContext,
) -> Result<()> {
    let Some(ports) = projection.as_object().filter(|fields| !fields.is_empty()) else {
        return Ok(());
    };
    let engine = ExpressionEngine;
    for (port, fields) in ports {
        if port == "error" {
            anyhow::bail!("ERROR_PROJECTION_NOT_ALLOWED");
        }
        let Some(fields) = fields.as_object().filter(|fields| !fields.is_empty()) else {
            continue;
        };
        let Some(items) = outputs.get_mut(port) else {
            continue;
        };
        for (index, item) in items.iter_mut().enumerate() {
            let context = ExpressionContext {
                json: item.json.clone(),
                item_index: index,
                ..base.clone()
            };
            let target = item
                .json
                .as_object_mut()
                .context("OUTPUT_PROJECTION_ITEM_MUST_BE_OBJECT")?;
            for (name, definition) in fields {
                let expression = definition
                    .get("expression")
                    .and_then(Value::as_str)
                    .with_context(|| {
                        format!("OUTPUT_PROJECTION_EXPRESSION_MISSING:{port}.{name}")
                    })?;
                let value =
                    engine.resolve_parameters(&Value::String(expression.to_owned()), &context)?;
                anyhow::ensure!(
                    !target.contains_key(name),
                    "OUTPUT_PROJECTION_FIELD_CONFLICT:{name}"
                );
                target.insert(name.clone(), value);
            }
        }
    }
    Ok(())
}

fn apply_context_operation(
    contexts: &mut Value,
    path: &str,
    operation: ContextWriteOperation,
    value: Value,
) -> Result<()> {
    match operation {
        ContextWriteOperation::Set => set_json_path(contexts, path, value),
        ContextWriteOperation::SetIfAbsent => {
            if get_json_path(contexts, path).is_none() {
                set_json_path(contexts, path, value)?;
            }
            Ok(())
        }
        ContextWriteOperation::Delete => delete_json_path(contexts, path),
        ContextWriteOperation::Append => {
            let mut values = get_json_path(contexts, path)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            match value {
                Value::Array(next) => values.extend(next),
                next => values.push(next),
            }
            set_json_path(contexts, path, Value::Array(values))
        }
        ContextWriteOperation::MergeObject => {
            let mut current = get_json_path(contexts, path)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            current.extend(
                value
                    .as_object()
                    .context("CONTEXT_MERGE_REQUIRES_OBJECT")?
                    .clone(),
            );
            set_json_path(contexts, path, Value::Object(current))
        }
        ContextWriteOperation::Increment => {
            let current = get_json_path(contexts, path)
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let delta = value
                .as_f64()
                .context("CONTEXT_INCREMENT_REQUIRES_NUMBER")?;
            set_json_path(contexts, path, json!(current + delta))
        }
        ContextWriteOperation::Min | ContextWriteOperation::Max => {
            let next = value.as_f64().context("CONTEXT_REDUCER_REQUIRES_NUMBER")?;
            let current = get_json_path(contexts, path).and_then(Value::as_f64);
            let reduced = current.map_or(next, |current| {
                if operation == ContextWriteOperation::Min {
                    current.min(next)
                } else {
                    current.max(next)
                }
            });
            set_json_path(contexts, path, json!(reduced))
        }
        ContextWriteOperation::CompareAndSet => {
            let expected = value
                .get("expected")
                .context("CONTEXT_COMPARE_AND_SET_EXPECTED_REQUIRED")?;
            anyhow::ensure!(
                get_json_path(contexts, path).unwrap_or(&Value::Null) == expected,
                "CONTEXT_COMPARE_AND_SET_CONFLICT: {path}"
            );
            set_json_path(
                contexts,
                path,
                value.get("value").cloned().unwrap_or(Value::Null),
            )
        }
    }
}

fn get_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, segment| current.get(segment))
}

fn set_json_path(value: &mut Value, path: &str, next: Value) -> Result<()> {
    let mut segments = path.split('.').peekable();
    let mut current = value;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current
                .as_object_mut()
                .context("CONTEXT_PARENT_NOT_OBJECT")?
                .insert(segment.to_owned(), next);
            return Ok(());
        }
        current = current
            .as_object_mut()
            .context("CONTEXT_PARENT_NOT_OBJECT")?
            .entry(segment.to_owned())
            .or_insert_with(|| json!({}));
    }
    anyhow::bail!("CONTEXT_PATH_EMPTY")
}

fn delete_json_path(value: &mut Value, path: &str) -> Result<()> {
    let mut segments = path.split('.').peekable();
    let mut current = value;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current
                .as_object_mut()
                .context("CONTEXT_PARENT_NOT_OBJECT")?
                .remove(segment);
            return Ok(());
        }
        current = current.get_mut(segment).context("CONTEXT_PATH_NOT_FOUND")?;
    }
    anyhow::bail!("CONTEXT_PATH_EMPTY")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_output_namespace_supports_end_output_expression() {
        let mut namespace = Map::new();
        merge_output_namespace(
            &mut namespace,
            "approval",
            0,
            &json!({
                "approved": [{
                    "json": {
                        "decision": "approved",
                        "actorUserId": "user-1"
                    }
                }]
            }),
        );

        let resolved = ExpressionEngine
            .evaluate(
                "${{ outputs.approval.approved.current.json }}",
                &ExpressionContext {
                    outputs: Value::Object(namespace),
                    ..ExpressionContext::default()
                },
            )
            .expect("approval output expression resolves");

        assert_eq!(resolved["decision"], "approved");
    }

    #[test]
    fn context_overlay_reducers_merge_against_the_child_base() {
        let mut appended = json!(["parent", "parallel"]);
        merge_context_overlay(
            &mut appended,
            &json!(["parent"]),
            &json!(["parent", "child"]),
            ContextMergePolicy::Append,
        )
        .unwrap();
        assert_eq!(appended, json!(["parent", "parallel", "child"]));

        let mut incremented = json!(12.0);
        merge_context_overlay(
            &mut incremented,
            &json!(10.0),
            &json!(13.0),
            ContextMergePolicy::Increment,
        )
        .unwrap();
        assert_eq!(incremented, json!(15.0));
    }

    #[test]
    fn reject_conflict_overlay_detects_parallel_changes() {
        let mut current = json!({"value":"parallel"});
        let error = merge_context_overlay(
            &mut current,
            &json!({"value":"base"}),
            &json!({"value":"child"}),
            ContextMergePolicy::RejectConflict,
        )
        .unwrap_err();
        assert!(error.to_string().contains("CONTEXT_OVERLAY_CONFLICT"));
    }

    #[test]
    fn output_projection_inserts_evaluated_values_without_metadata() {
        let mut outputs = BTreeMap::from([(
            "main".to_owned(),
            vec![Item {
                json: json!({"stdout": "ok"}),
                ..Item::default()
            }],
        )]);
        let projection = json!({
            "main": {
                "summary": {
                    "expression": "${{ item.json.stdout }}",
                    "schema": {"type": "string"},
                    "sensitive": false
                }
            }
        });

        apply_output_projection(&mut outputs, &projection, &ExpressionContext::default())
            .expect("projection should resolve");

        assert_eq!(
            outputs["main"][0].json,
            json!({"stdout": "ok", "summary": "ok"})
        );
    }
}
