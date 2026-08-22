use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use agentx_node_protocol::Item;
use agentx_runtime_contracts::WorkerResultStatusV1;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256, Sha512};

use super::{ClaimedWorkerAttempt, WorkerExecution};

pub(super) fn execute(claim: &ClaimedWorkerAttempt) -> WorkerExecution {
    let parameters = &claim.node_parameters;
    let main = claim.inputs.get("main").cloned().unwrap_or_default();
    match claim.node_type.as_str() {
        "stop_and_error" => WorkerExecution::failed(
            parameters
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("WORKFLOW_STOPPED"),
            parameters
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Workflow stopped"),
            false,
        ),
        "set" => map_main_items(claim, main, set_values),
        "filter" => filter_items(claim, main),
        "limit" => limit(main, parameters),
        "sort" => sort(main, parameters),
        "remove_duplicates" => deduplicate(main, parameters),
        "split_out" => split_out_per_item(claim, main),
        "aggregate" => aggregate(main, parameters),
        "rename_fields" => map_main_items(claim, main, rename_fields),
        "json_transform" => map_main_items(claim, main, json_transform),
        "item_generator" => item_generator(parameters),
        "date_time" => map_main_items(claim, main, date_time),
        "base64" => map_main_items(claim, main, base64_value),
        "hash" => map_main_items(claim, main, hash_value),
        "compare_datasets" => compare_datasets(claim, parameters),
        "structured_validator" => structured_validator(main, parameters),
        "if" => if_items(claim, main),
        "switch" => switch_items(claim, main),
        "merge" => merge(claim, parameters),
        "loop_over_items" => output("done", main),
        "error_handler" => {
            if parameters.get("mode").and_then(Value::as_str) == Some("fail") {
                WorkerExecution::failed(
                    "HANDLED_ERROR_RETHROWN",
                    "Error handler was configured to fail",
                    false,
                )
            } else {
                output(
                    "recovered",
                    claim.inputs.values().flatten().cloned().collect(),
                )
            }
        }
        _ => output("main", main),
    }
}

#[cfg(test)]
pub(super) fn execute_single(node_type: &str, parameters: &Value, value: Value) -> WorkerExecution {
    match node_type {
        "stop_and_error" => WorkerExecution::failed(
            parameters
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("WORKFLOW_STOPPED"),
            parameters
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Workflow stopped"),
            false,
        ),
        "set" => map_items(
            vec![Item {
                json: value,
                ..Item::default()
            }],
            |value| set_values(value, parameters),
        ),
        _ => output(
            "main",
            vec![Item {
                json: value,
                ..Item::default()
            }],
        ),
    }
}

fn output(port: &str, items: Vec<Item>) -> WorkerExecution {
    WorkerExecution {
        status: WorkerResultStatusV1::Succeeded,
        outputs: BTreeMap::from([(port.into(), items)]),
        error_code: None,
        error_message: None,
    }
}

#[cfg(test)]
fn map_items(
    items: Vec<Item>,
    transform: impl Fn(Value) -> Result<Value, String>,
) -> WorkerExecution {
    let mut output_items = Vec::with_capacity(items.len());
    for mut item in items {
        match transform(item.json) {
            Ok(value) => {
                item.json = value;
                output_items.push(item);
            }
            Err(error) => {
                return WorkerExecution::failed("BUILTIN_PARAMETER_INVALID", error, false);
            }
        }
    }
    output("main", output_items)
}

fn main_parameter(claim: &ClaimedWorkerAttempt, index: usize) -> &Value {
    let offset = claim
        .inputs
        .iter()
        .take_while(|(port, _)| port.as_str() != "main")
        .map(|(_, items)| items.len())
        .sum::<usize>();
    claim
        .per_item_parameters
        .get(offset + index)
        .unwrap_or(&claim.node_parameters)
}

fn map_main_items(
    claim: &ClaimedWorkerAttempt,
    items: Vec<Item>,
    transform: impl Fn(Value, &Value) -> Result<Value, String>,
) -> WorkerExecution {
    let mut output_items = Vec::with_capacity(items.len());
    for (index, mut item) in items.into_iter().enumerate() {
        match transform(item.json, main_parameter(claim, index)) {
            Ok(value) => {
                item.json = value;
                output_items.push(item);
            }
            Err(error) => {
                return WorkerExecution::failed("BUILTIN_PARAMETER_INVALID", error, false);
            }
        }
    }
    output("main", output_items)
}

fn filter_items(claim: &ClaimedWorkerAttempt, items: Vec<Item>) -> WorkerExecution {
    output(
        "main",
        items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| {
                main_parameter(claim, index)
                    .get("condition")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    .then_some(item)
            })
            .collect(),
    )
}

fn if_items(claim: &ClaimedWorkerAttempt, items: Vec<Item>) -> WorkerExecution {
    let mut truthy = Vec::new();
    let mut falsy = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if main_parameter(claim, index)
            .get("condition")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            truthy.push(item);
        } else {
            falsy.push(item);
        }
    }
    WorkerExecution {
        status: WorkerResultStatusV1::Succeeded,
        outputs: BTreeMap::from([("true".into(), truthy), ("false".into(), falsy)]),
        error_code: None,
        error_message: None,
    }
}

fn set_values(value: Value, parameters: &Value) -> Result<Value, String> {
    let mut result = if parameters
        .get("keepOnlySet")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Map::new()
    } else {
        value.as_object().cloned().unwrap_or_default()
    };
    if let Some(values) = parameters.get("values").and_then(Value::as_object) {
        result.extend(values.clone());
    }
    Ok(Value::Object(result))
}

fn limit(mut items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let maximum = parameters
        .get("maxItems")
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize;
    if parameters.get("keep").and_then(Value::as_str) == Some("last") && items.len() > maximum {
        items.drain(..items.len() - maximum);
    } else {
        items.truncate(maximum);
    }
    output("main", items)
}

fn sort(mut items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let fields = parameters
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    items.sort_by(|left, right| {
        for field in &fields {
            let path = field
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let direction = field
                .get("direction")
                .and_then(Value::as_str)
                .unwrap_or("asc");
            let nulls = field.get("nulls").and_then(Value::as_str).unwrap_or("last");
            let ordering = compare_json(
                get_path(&left.json, path),
                get_path(&right.json, path),
                nulls,
            );
            if ordering != Ordering::Equal {
                return if direction == "desc" {
                    ordering.reverse()
                } else {
                    ordering
                };
            }
        }
        Ordering::Equal
    });
    output("main", items)
}

fn compare_json(left: Option<&Value>, right: Option<&Value>, nulls: &str) -> Ordering {
    match (
        left.filter(|value| !value.is_null()),
        right.filter(|value| !value.is_null()),
    ) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => {
            if nulls == "first" {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (Some(_), None) => {
            if nulls == "first" {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(Value::Number(left)), Some(Value::Number(right))) => left
            .as_f64()
            .partial_cmp(&right.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(left), Some(right)) => canonical(left).cmp(&canonical(right)),
    }
}

fn deduplicate(items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let fields = string_array(parameters.get("fields"));
    let keep_last = parameters.get("keep").and_then(Value::as_str) == Some("last");
    let mut seen = BTreeSet::new();
    let mut retained = Vec::new();
    for item in if keep_last {
        items.into_iter().rev().collect()
    } else {
        items
    } {
        let key = if fields.is_empty() {
            canonical(&item.json)
        } else {
            canonical(&Value::Array(
                fields
                    .iter()
                    .map(|field| get_path(&item.json, field).cloned().unwrap_or(Value::Null))
                    .collect(),
            ))
        };
        if seen.insert(key) {
            retained.push(item);
        }
    }
    if keep_last {
        retained.reverse();
    }
    output("main", retained)
}

#[cfg(test)]
fn split_out(items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut result = Vec::new();
    for item in items {
        let Some(values) = get_path(&item.json, field).and_then(Value::as_array) else {
            return WorkerExecution::failed(
                "BUILTIN_FIELD_INVALID",
                format!("{field} is not an array"),
                false,
            );
        };
        for value in values {
            let mut next = item.clone();
            if let Err(error) = set_path(&mut next.json, field, value.clone()) {
                return WorkerExecution::failed("BUILTIN_FIELD_INVALID", error, false);
            }
            result.push(next);
        }
    }
    output("main", result)
}

fn split_out_per_item(claim: &ClaimedWorkerAttempt, items: Vec<Item>) -> WorkerExecution {
    let mut result = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let parameters = main_parameter(claim, index);
        let field = parameters
            .get("field")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(values) = get_path(&item.json, field).and_then(Value::as_array) else {
            return WorkerExecution::failed(
                "BUILTIN_FIELD_INVALID",
                format!("{field} is not an array"),
                false,
            );
        };
        for value in values {
            let mut next = item.clone();
            if let Err(error) = set_path(&mut next.json, field, value.clone()) {
                return WorkerExecution::failed("BUILTIN_FIELD_INVALID", error, false);
            }
            result.push(next);
        }
    }
    output("main", result)
}

fn aggregate(items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let group_fields = string_array(parameters.get("groupBy"));
    let mut groups: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for item in &items {
        let key = canonical(&Value::Array(
            group_fields
                .iter()
                .map(|field| get_path(&item.json, field).cloned().unwrap_or(Value::Null))
                .collect(),
        ));
        groups.entry(key).or_default().push(item);
    }
    let operations = parameters
        .get("operations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut result = Vec::new();
    for group in groups.values() {
        let mut value = Map::new();
        for field in &group_fields {
            if let Some(item_value) = get_path(&group[0].json, field) {
                value.insert(field.clone(), item_value.clone());
            }
        }
        for operation in &operations {
            let kind = operation
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("count");
            let field = operation
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let output_field = operation
                .get("outputField")
                .and_then(Value::as_str)
                .unwrap_or(kind);
            let values = group
                .iter()
                .filter_map(|item| get_path(&item.json, field).cloned())
                .collect::<Vec<_>>();
            let aggregate = match kind {
                "count" => json!(group.len()),
                "first" => values.first().cloned().unwrap_or(Value::Null),
                "last" => values.last().cloned().unwrap_or(Value::Null),
                "collect" => Value::Array(values),
                "sum" | "avg" | "min" | "max" => numeric_aggregate(kind, &values),
                _ => Value::Null,
            };
            value.insert(output_field.into(), aggregate);
        }
        result.push(Item {
            json: Value::Object(value),
            ..Item::default()
        });
    }
    output("main", result)
}

fn numeric_aggregate(kind: &str, values: &[Value]) -> Value {
    let numbers = values.iter().filter_map(Value::as_f64).collect::<Vec<_>>();
    if numbers.is_empty() {
        return Value::Null;
    }
    let value = match kind {
        "sum" => numbers.iter().sum(),
        "avg" => numbers.iter().sum::<f64>() / numbers.len() as f64,
        "min" => numbers.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => 0.0,
    };
    json!(value)
}

fn rename_fields(mut value: Value, parameters: &Value) -> Result<Value, String> {
    for mapping in parameters
        .get("mappings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let from = mapping
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let to = mapping
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match remove_path(&mut value, from) {
            Some(field) => set_path(&mut value, to, field)?,
            None if parameters.get("missingField").and_then(Value::as_str) == Some("error") => {
                return Err(format!("Field {from} is missing"));
            }
            None => {}
        }
    }
    Ok(value)
}

fn json_transform(mut value: Value, parameters: &Value) -> Result<Value, String> {
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output_field = parameters
        .get("outputField")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(field);
    let source = get_path(&value, field)
        .cloned()
        .ok_or_else(|| format!("Field {field} is missing"))?;
    let transformed = if parameters
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("parse")
        == "stringify"
    {
        Value::String(canonical(&source))
    } else {
        serde_json::from_str(
            source
                .as_str()
                .ok_or_else(|| format!("Field {field} is not a string"))?,
        )
        .map_err(|error| error.to_string())?
    };
    set_path(&mut value, output_field, transformed)?;
    Ok(value)
}

fn item_generator(parameters: &Value) -> WorkerExecution {
    if let Some(items) = parameters.get("items").and_then(Value::as_array) {
        return output(
            "main",
            items
                .iter()
                .cloned()
                .map(|json| Item {
                    json,
                    ..Item::default()
                })
                .collect(),
        );
    }
    let start = parameters.get("start").and_then(Value::as_i64).unwrap_or(1);
    let end = parameters.get("end").and_then(Value::as_i64).unwrap_or(10);
    let step = parameters.get("step").and_then(Value::as_i64).unwrap_or(1);
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("value");
    if step == 0 {
        return WorkerExecution::failed("BUILTIN_PARAMETER_INVALID", "step cannot be zero", false);
    }
    let mut items = Vec::new();
    let mut current = start;
    while (step > 0 && current <= end) || (step < 0 && current >= end) {
        items.push(Item {
            json: json!({field:current}),
            ..Item::default()
        });
        current += step;
        if items.len() > 100_000 {
            return WorkerExecution::failed(
                "BUILTIN_LIMIT_EXCEEDED",
                "item generator exceeds 100000 items",
                false,
            );
        }
    }
    output("main", items)
}

fn date_time(mut value: Value, parameters: &Value) -> Result<Value, String> {
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output_field = parameters
        .get("outputField")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(field);
    let source = get_path(&value, field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Field {field} is not a date string"))?;
    let date = DateTime::parse_from_rfc3339(source)
        .map_err(|error| error.to_string())?
        .with_timezone(&Utc);
    let unit = parameters
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("seconds");
    let amount = parameters
        .get("amount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let duration = match unit {
        "minutes" => Duration::minutes(amount),
        "hours" => Duration::hours(amount),
        "days" => Duration::days(amount),
        _ => Duration::seconds(amount),
    };
    let result = match parameters
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("format")
    {
        "add" => Value::String((date + duration).to_rfc3339()),
        "subtract" => Value::String((date - duration).to_rfc3339()),
        "difference" => {
            let other = parameters
                .get("compareTo")
                .and_then(Value::as_str)
                .ok_or("compareTo is required")?;
            let other = DateTime::parse_from_rfc3339(other)
                .map_err(|error| error.to_string())?
                .with_timezone(&Utc);
            json!((date - other).num_seconds())
        }
        _ if parameters.get("format").and_then(Value::as_str) == Some("unix") => {
            json!(date.timestamp())
        }
        _ => Value::String(date.to_rfc3339()),
    };
    set_path(&mut value, output_field, result)?;
    Ok(value)
}

fn base64_value(mut value: Value, parameters: &Value) -> Result<Value, String> {
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output_field = parameters
        .get("outputField")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(field);
    let source = get_path(&value, field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Field {field} is not a string"))?;
    let result = if parameters.get("operation").and_then(Value::as_str) == Some("decode") {
        String::from_utf8(STANDARD.decode(source).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?
    } else {
        STANDARD.encode(source)
    };
    set_path(&mut value, output_field, Value::String(result))?;
    Ok(value)
}

fn hash_value(mut value: Value, parameters: &Value) -> Result<Value, String> {
    let field = parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output_field = parameters
        .get("outputField")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(field);
    let source = get_path(&value, field)
        .cloned()
        .ok_or_else(|| format!("Field {field} is missing"))?;
    let bytes = canonical(&source);
    let digest = if parameters.get("algorithm").and_then(Value::as_str) == Some("sha512") {
        Sha512::digest(bytes).to_vec()
    } else {
        Sha256::digest(bytes).to_vec()
    };
    let encoded = if parameters.get("encoding").and_then(Value::as_str) == Some("base64") {
        STANDARD.encode(digest)
    } else {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    };
    set_path(&mut value, output_field, Value::String(encoded))?;
    Ok(value)
}

fn compare_datasets(claim: &ClaimedWorkerAttempt, parameters: &Value) -> WorkerExecution {
    let left = claim.inputs.get("left").cloned().unwrap_or_default();
    let right = claim.inputs.get("right").cloned().unwrap_or_default();
    let keys = string_array(parameters.get("keyFields"));
    let key = |item: &Item| {
        if keys.is_empty() {
            canonical(&item.json)
        } else {
            canonical(&Value::Array(
                keys.iter()
                    .map(|field| get_path(&item.json, field).cloned().unwrap_or(Value::Null))
                    .collect(),
            ))
        }
    };
    let left_map = left
        .iter()
        .map(|item| (key(item), item))
        .collect::<BTreeMap<_, _>>();
    let right_map = right
        .iter()
        .map(|item| (key(item), item))
        .collect::<BTreeMap<_, _>>();
    let mut outputs = BTreeMap::new();
    for name in ["same", "different", "left_only", "right_only"] {
        outputs.insert(name.into(), Vec::new());
    }
    for (key, item) in &left_map {
        match right_map.get(key) {
            None => outputs.get_mut("left_only").unwrap().push((*item).clone()),
            Some(other) if item.json == other.json => {
                outputs.get_mut("same").unwrap().push((*item).clone())
            }
            Some(other) => outputs.get_mut("different").unwrap().push(Item {
                json: json!({"left":item.json,"right":other.json}),
                ..Item::default()
            }),
        }
    }
    for (key, item) in &right_map {
        if !left_map.contains_key(key) {
            outputs.get_mut("right_only").unwrap().push((*item).clone());
        }
    }
    WorkerExecution {
        status: WorkerResultStatusV1::Succeeded,
        outputs,
        error_code: None,
        error_message: None,
    }
}

fn structured_validator(items: Vec<Item>, parameters: &Value) -> WorkerExecution {
    let schema = parameters
        .get("schema")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let validator = match jsonschema::validator_for(&schema) {
        Ok(value) => value,
        Err(error) => {
            return WorkerExecution::failed("VALIDATOR_SCHEMA_INVALID", error.to_string(), false);
        }
    };
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for item in items {
        if let Err(error) = validator.validate(&item.json) {
            let error = error.to_string();
            if parameters.get("mode").and_then(Value::as_str) == Some("fail") {
                return WorkerExecution::failed("STRUCTURED_VALIDATION_FAILED", error, false);
            }
            let mut item = item;
            item.json = json!({"value":item.json,"validationError":error});
            invalid.push(item);
        } else {
            valid.push(item);
        }
    }
    WorkerExecution {
        status: WorkerResultStatusV1::Succeeded,
        outputs: BTreeMap::from([("valid".into(), valid), ("invalid".into(), invalid)]),
        error_code: None,
        error_message: None,
    }
}

fn switch_items(claim: &ClaimedWorkerAttempt, items: Vec<Item>) -> WorkerExecution {
    let mut case = Vec::new();
    let mut fallback = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let parameters = main_parameter(claim, index);
        let matches = parameters
            .get("rules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|rule| {
                rule.get("condition")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        if matches == 0 {
            fallback.push(item);
        } else {
            let repeats = if parameters
                .get("sendToAllMatches")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                matches
            } else {
                1
            };
            case.extend((0..repeats).map(|_| item.clone()));
        }
    }
    WorkerExecution {
        status: WorkerResultStatusV1::Succeeded,
        outputs: BTreeMap::from([("case".into(), case), ("fallback".into(), fallback)]),
        error_code: None,
        error_message: None,
    }
}

fn merge(claim: &ClaimedWorkerAttempt, parameters: &Value) -> WorkerExecution {
    let mode = parameters
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("append");
    if mode == "append" {
        return output("main", claim.inputs.values().flatten().cloned().collect());
    }
    let left = claim
        .inputs
        .get("left")
        .cloned()
        .or_else(|| claim.inputs.get("main:0").cloned())
        .unwrap_or_default();
    let right = claim
        .inputs
        .get("right")
        .cloned()
        .or_else(|| claim.inputs.get("main:1").cloned())
        .unwrap_or_default();
    if mode == "combine_by_position" {
        let length = left.len().max(right.len());
        let items = (0..length)
            .filter_map(|index| combine_items(left.get(index), right.get(index), parameters))
            .collect();
        return output("main", items);
    }
    let left_field = parameters
        .get("leftField")
        .and_then(Value::as_str)
        .unwrap_or("id");
    let right_field = parameters
        .get("rightField")
        .and_then(Value::as_str)
        .unwrap_or("id");
    let left_map = left
        .iter()
        .filter_map(|item| get_path(&item.json, left_field).map(|value| (canonical(value), item)))
        .collect::<BTreeMap<_, _>>();
    let right_map = right
        .iter()
        .filter_map(|item| get_path(&item.json, right_field).map(|value| (canonical(value), item)))
        .collect::<BTreeMap<_, _>>();
    let join = parameters
        .get("joinType")
        .and_then(Value::as_str)
        .unwrap_or("inner");
    let keys = left_map
        .keys()
        .chain(right_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let items = keys
        .into_iter()
        .filter_map(|key| {
            let left = left_map.get(&key).copied();
            let right = right_map.get(&key).copied();
            let included = match join {
                "left" => left.is_some(),
                "right" => right.is_some(),
                "full" => true,
                _ => left.is_some() && right.is_some(),
            };
            included
                .then(|| combine_items(left, right, parameters))
                .flatten()
        })
        .collect();
    output("main", items)
}

fn combine_items(left: Option<&Item>, right: Option<&Item>, parameters: &Value) -> Option<Item> {
    let mut result = left.cloned().or_else(|| right.cloned())?;
    let (Some(target), Some(source)) = (
        result.json.as_object_mut(),
        right.and_then(|item| item.json.as_object()),
    ) else {
        return Some(result);
    };
    match parameters
        .get("conflictStrategy")
        .and_then(Value::as_str)
        .unwrap_or("prefer_right")
    {
        "prefer_left" => {
            for (name, value) in source {
                target.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        "suffix" => {
            for (name, value) in source {
                if let Some(left_value) = target.remove(name) {
                    if left_value == *value {
                        target.insert(name.clone(), left_value);
                    } else {
                        target.insert(format!("{name}_left"), left_value);
                        target.insert(format!("{name}_right"), value.clone());
                    }
                } else {
                    target.insert(name.clone(), value.clone());
                }
            }
        }
        _ => target.extend(source.clone()),
    }
    Some(result)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
fn canonical(value: &Value) -> String {
    agentx_runtime_contracts::canonical_bytes(value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}
fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(value, |current, segment| current.get(segment))
}
fn set_path(value: &mut Value, path: &str, field: Value) -> Result<(), String> {
    let segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let Some((last, parents)) = segments.split_last() else {
        return Err("field path cannot be empty".into());
    };
    let mut current = value;
    for segment in parents {
        if !current.is_object() {
            *current = json!({});
        }
        current = current
            .as_object_mut()
            .unwrap()
            .entry((*segment).to_owned())
            .or_insert_with(|| json!({}));
    }
    if !current.is_object() {
        *current = json!({});
    }
    current
        .as_object_mut()
        .unwrap()
        .insert((*last).to_owned(), field);
    Ok(())
}
fn remove_path(value: &mut Value, path: &str) -> Option<Value> {
    let segments = path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let (last, parents) = segments.split_last()?;
    let mut current = value;
    for segment in parents {
        current = current.get_mut(*segment)?;
    }
    current.as_object_mut()?.remove(*last)
}

#[cfg(test)]
mod tests {
    use agentx_node_protocol::Item;
    use serde_json::{Value, json};

    use super::{
        aggregate, base64_value, combine_items, date_time, deduplicate, hash_value, item_generator,
        json_transform, limit, rename_fields, sort, split_out, structured_validator,
    };

    fn items(values: Vec<Value>) -> Vec<Item> {
        values
            .into_iter()
            .map(|json| Item {
                json,
                ..Item::default()
            })
            .collect()
    }

    #[test]
    fn collection_builtins_consume_sort_limit_deduplicate_and_split_parameters() {
        let sorted = sort(
            items(vec![json!({"id":2}), json!({"id":1}), json!({"id":1})]),
            &json!({"fields":[{"field":"id","direction":"asc","nulls":"last"}]}),
        );
        let deduplicated = deduplicate(
            sorted.outputs["main"].clone(),
            &json!({"fields":["id"],"keep":"first"}),
        );
        let limited = limit(
            deduplicated.outputs["main"].clone(),
            &json!({"maxItems":1,"keep":"last"}),
        );
        assert_eq!(limited.outputs["main"][0].json, json!({"id":2}));

        let split = split_out(
            items(vec![json!({"values":["a","b"]})]),
            &json!({"field":"values"}),
        );
        assert_eq!(split.outputs["main"].len(), 2);
        assert_eq!(split.outputs["main"][1].json, json!({"values":"b"}));

        let merged = combine_items(
            Some(&items(vec![json!({"id":2,"value":"left"})])[0]),
            Some(&items(vec![json!({"id":2,"value":"right"})])[0]),
            &json!({"conflictStrategy":"suffix"}),
        )
        .unwrap();
        assert_eq!(
            merged.json,
            json!({"id":2,"value_left":"left","value_right":"right"})
        );
    }

    #[test]
    fn transform_builtins_consume_mapping_json_encoding_hash_and_time_parameters() {
        let renamed = rename_fields(
            json!({"old":{"value":"hello"}}),
            &json!({"mappings":[{"from":"old.value","to":"new.value"}],"missingField":"error"}),
        )
        .unwrap();
        let stringified = json_transform(
            json!({"payload":{"b":2,"a":1}}),
            &json!({"operation":"stringify","field":"payload","outputField":"text"}),
        )
        .unwrap();
        let encoded = base64_value(
            json!({"text":"hello"}),
            &json!({"operation":"encode","field":"text","outputField":"encoded"}),
        )
        .unwrap();
        let hashed = hash_value(
            json!({"text":"hello"}),
            &json!({"algorithm":"sha256","encoding":"hex","field":"text","outputField":"digest"}),
        )
        .unwrap();
        let dated = date_time(
            json!({"at":"2026-08-20T10:00:00Z"}),
            &json!({"operation":"add","field":"at","amount":2,"unit":"hours","outputField":"later","format":"rfc3339"}),
        )
        .unwrap();
        assert_eq!(renamed, json!({"old":{},"new":{"value":"hello"}}));
        assert_eq!(stringified["text"], "{\"a\":1,\"b\":2}");
        assert_eq!(encoded["encoded"], "aGVsbG8=");
        assert_eq!(hashed["digest"].as_str().unwrap().len(), 64);
        assert_eq!(dated["later"], "2026-08-20T12:00:00+00:00");
    }

    #[test]
    fn aggregate_generator_and_validator_parameters_change_outputs() {
        let aggregated = aggregate(
            items(vec![
                json!({"team":"a","score":2}),
                json!({"team":"a","score":4}),
            ]),
            &json!({"groupBy":["team"],"operations":[{"operation":"avg","field":"score","outputField":"average"}]}),
        );
        assert_eq!(
            aggregated.outputs["main"][0].json,
            json!({"team":"a","average":3.0})
        );
        let generated = item_generator(&json!({"start":2,"end":6,"step":2,"field":"n"}));
        assert_eq!(generated.outputs["main"].len(), 3);
        assert_eq!(generated.outputs["main"][2].json, json!({"n":6}));
        let validated = structured_validator(
            items(vec![json!({"name":"ok"}), json!({"name":3})]),
            &json!({"schema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]},"mode":"route"}),
        );
        assert_eq!(validated.outputs["valid"].len(), 1);
        assert_eq!(validated.outputs["invalid"].len(), 1);
    }
}
