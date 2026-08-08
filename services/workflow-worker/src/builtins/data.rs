use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use agentx_node_protocol::{Item, ItemSource};
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Number, Value, json};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

pub fn execute(task: &RuntimeTask) -> Result<TaskResult> {
    match task.node_type.as_str() {
        "filter" => filter(task),
        "limit" => limit(task),
        "sort" => sort(task),
        "remove_duplicates" => remove_duplicates(task),
        "split_out" => split_out(task),
        "aggregate" => aggregate(task),
        "merge" => merge(task),
        "rename_fields" => rename_fields(task),
        "json_transform" => json_transform(task),
        "no_op" => Ok(completed("main", flatten(task))),
        "item_generator" => item_generator(task),
        "date_time" => date_time(task),
        "compare_datasets" => compare_datasets(task),
        _ => unreachable!(),
    }
}

fn filter(task: &RuntimeTask) -> Result<TaskResult> {
    let condition = task
        .node_parameters
        .get("condition")
        .context("condition is required")?;
    let mut output = Vec::new();
    for (index, item) in flatten(task).into_iter().enumerate() {
        let keep = ExpressionEngine
            .resolve_parameters(condition, &context(task, &item, index))?
            .as_bool()
            .context("filter condition must resolve to boolean")?;
        if keep {
            output.push(item);
        }
    }
    Ok(completed("main", output))
}

fn limit(task: &RuntimeTask) -> Result<TaskResult> {
    let mut items = flatten(task);
    let count = task
        .node_parameters
        .get("maxItems")
        .or_else(|| task.node_parameters.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize;
    if items.len() > count {
        if task.node_parameters.get("keep").and_then(Value::as_str) == Some("last") {
            items.drain(..items.len() - count);
        } else {
            items.truncate(count);
        }
    }
    Ok(completed("main", items))
}

fn sort(task: &RuntimeTask) -> Result<TaskResult> {
    let mut items = flatten(task);
    let fields = task
        .node_parameters
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    items.sort_by(|left, right| {
        for field in &fields {
            let name = field
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let nulls_first = field.get("nulls").and_then(Value::as_str) == Some("first");
            let mut order = compare_values(
                get_path(&left.json, name),
                get_path(&right.json, name),
                nulls_first,
            );
            if field.get("direction").and_then(Value::as_str) == Some("desc") {
                order = order.reverse();
            }
            if order != Ordering::Equal {
                return order;
            }
        }
        Ordering::Equal
    });
    Ok(completed("main", items))
}

fn remove_duplicates(task: &RuntimeTask) -> Result<TaskResult> {
    let items = flatten(task);
    let fields = string_array(task.node_parameters.get("fields"));
    let keep_last = task.node_parameters.get("keep").and_then(Value::as_str) == Some("last");
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    let iter: Box<dyn Iterator<Item = Item>> = if keep_last {
        Box::new(items.into_iter().rev())
    } else {
        Box::new(items.into_iter())
    };
    for item in iter {
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
            output.push(item);
        }
    }
    if keep_last {
        output.reverse();
    }
    Ok(completed("main", output))
}

fn split_out(task: &RuntimeTask) -> Result<TaskResult> {
    let field = required_string(&task.node_parameters, "field")?;
    let mut output = Vec::new();
    for item in flatten(task) {
        let values = get_path(&item.json, field)
            .and_then(Value::as_array)
            .context("split field must be an array")?
            .clone();
        for value in values {
            let mut split = item.clone();
            set_path(&mut split.json, field, value)?;
            output.push(split);
        }
    }
    Ok(completed("main", output))
}

fn rename_fields(task: &RuntimeTask) -> Result<TaskResult> {
    let mappings = task
        .node_parameters
        .get("mappings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let fail_missing = task
        .node_parameters
        .get("missingField")
        .and_then(Value::as_str)
        == Some("error");
    let mut output = Vec::new();
    for mut item in flatten(task) {
        for mapping in &mappings {
            let from = mapping
                .get("from")
                .and_then(Value::as_str)
                .context("rename from is required")?;
            let to = mapping
                .get("to")
                .and_then(Value::as_str)
                .context("rename to is required")?;
            match remove_path(&mut item.json, from) {
                Some(value) => set_path(&mut item.json, to, value)?,
                None if fail_missing => bail!("rename source field {from} is missing"),
                None => {}
            }
        }
        output.push(item);
    }
    Ok(completed("main", output))
}

fn json_transform(task: &RuntimeTask) -> Result<TaskResult> {
    let mut output = Vec::new();
    for (index, mut item) in flatten(task).into_iter().enumerate() {
        let parameters = ExpressionEngine
            .resolve_parameters(&task.node_parameters, &context(task, &item, index))?;
        let field = required_string(&parameters, "field")?;
        let output_field = parameters
            .get("outputField")
            .and_then(Value::as_str)
            .unwrap_or(field);
        let value = get_path(&item.json, field).context("JSON transform field is missing")?;
        let transformed = match parameters
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("parse")
        {
            "parse" => serde_json::from_str(
                value
                    .as_str()
                    .context("JSON Parse input must be a string")?,
            )
            .context("invalid JSON text")?,
            "stringify" => Value::String(serde_json::to_string(value)?),
            other => bail!("unsupported JSON transform operation {other}"),
        };
        set_path(&mut item.json, output_field, transformed)?;
        output.push(item);
    }
    Ok(completed("main", output))
}

fn item_generator(task: &RuntimeTask) -> Result<TaskResult> {
    if let Some(values) = task.node_parameters.get("items").and_then(Value::as_array) {
        return Ok(completed(
            "main",
            values
                .iter()
                .cloned()
                .map(|json| Item {
                    json,
                    ..Item::default()
                })
                .collect(),
        ));
    }
    let start = task
        .node_parameters
        .get("start")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let end = task
        .node_parameters
        .get("end")
        .and_then(Value::as_i64)
        .unwrap_or(10);
    let step = task
        .node_parameters
        .get("step")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    if step == 0 {
        bail!("generator step cannot be zero");
    }
    let field = task
        .node_parameters
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("value");
    let mut output = Vec::new();
    let mut value = start;
    while (step > 0 && value <= end) || (step < 0 && value >= end) {
        output.push(Item {
            json: json!({field: value}),
            ..Item::default()
        });
        if output.len() >= 100_000 {
            bail!("generator exceeds 100000 items");
        }
        value = value
            .checked_add(step)
            .context("generator range overflow")?;
    }
    Ok(completed("main", output))
}

fn date_time(task: &RuntimeTask) -> Result<TaskResult> {
    let mut output = Vec::new();
    for (index, mut item) in flatten(task).into_iter().enumerate() {
        let parameters = ExpressionEngine
            .resolve_parameters(&task.node_parameters, &context(task, &item, index))?;
        let field = required_string(&parameters, "field")?;
        let output_field = parameters
            .get("outputField")
            .and_then(Value::as_str)
            .unwrap_or(field);
        let source = get_path(&item.json, field)
            .and_then(Value::as_str)
            .context("date field must be RFC3339 text")?;
        let date = OffsetDateTime::parse(source, &Rfc3339).context("invalid RFC3339 date")?;
        let value = match parameters
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("format")
        {
            "format" if parameters.get("format").and_then(Value::as_str) == Some("unix") => {
                Value::Number(date.unix_timestamp().into())
            }
            "format" => Value::String(date.format(&Rfc3339)?),
            "add" | "subtract" => {
                let amount = parameters
                    .get("amount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let duration = duration(
                    amount,
                    parameters
                        .get("unit")
                        .and_then(Value::as_str)
                        .unwrap_or("seconds"),
                )?;
                let adjusted =
                    if parameters.get("operation").and_then(Value::as_str) == Some("subtract") {
                        date - duration
                    } else {
                        date + duration
                    };
                Value::String(adjusted.format(&Rfc3339)?)
            }
            "difference" => {
                let other = parameters
                    .get("compareTo")
                    .and_then(Value::as_str)
                    .context("compareTo is required")?;
                let other = OffsetDateTime::parse(other, &Rfc3339)
                    .context("invalid compareTo RFC3339 date")?;
                Value::Number((date - other).whole_seconds().into())
            }
            other => bail!("unsupported date operation {other}"),
        };
        set_path(&mut item.json, output_field, value)?;
        output.push(item);
    }
    Ok(completed("main", output))
}

fn aggregate(task: &RuntimeTask) -> Result<TaskResult> {
    let group_fields = string_array(task.node_parameters.get("groupBy"));
    let operations = task
        .node_parameters
        .get("operations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json!({"operation":"count","outputField":"count"})]);
    let mut groups = BTreeMap::<String, Vec<Item>>::new();
    for item in flatten(task) {
        let key = canonical(&Value::Array(
            group_fields
                .iter()
                .map(|field| get_path(&item.json, field).cloned().unwrap_or(Value::Null))
                .collect(),
        ));
        groups.entry(key).or_default().push(item);
    }
    let mut output = Vec::new();
    for items in groups.into_values() {
        let mut json = Map::new();
        if let Some(first) = items.first() {
            for field in &group_fields {
                set_path_in_map(
                    &mut json,
                    field,
                    get_path(&first.json, field).cloned().unwrap_or(Value::Null),
                )?;
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
            let values = items
                .iter()
                .filter_map(|item| get_path(&item.json, field))
                .cloned()
                .collect::<Vec<_>>();
            let value = aggregate_value(kind, &values, items.len())?;
            set_path_in_map(&mut json, output_field, value)?;
        }
        output.push(combined_item(Value::Object(json), &items, "prefer_right"));
    }
    Ok(completed("main", output))
}

fn merge(task: &RuntimeTask) -> Result<TaskResult> {
    let mode = task
        .node_parameters
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("append");
    if mode == "append" {
        return Ok(completed("main", flatten(task)));
    }
    let inputs = ordered_inputs(task);
    if mode == "combine_by_position" {
        let size = inputs.iter().map(Vec::len).max().unwrap_or(0);
        let output = (0..size)
            .map(|index| {
                let items = inputs
                    .iter()
                    .filter_map(|input| input.get(index))
                    .cloned()
                    .collect::<Vec<_>>();
                combined_item(merge_json(&items, conflict(task)), &items, conflict(task))
            })
            .collect();
        return Ok(completed("main", output));
    }
    let left = task
        .inputs
        .get("left")
        .cloned()
        .or_else(|| inputs.first().cloned())
        .unwrap_or_default();
    let right = task
        .inputs
        .get("right")
        .cloned()
        .or_else(|| inputs.get(1).cloned())
        .unwrap_or_default();
    let left_field = task
        .node_parameters
        .get("leftField")
        .and_then(Value::as_str)
        .unwrap_or("id");
    let right_field = task
        .node_parameters
        .get("rightField")
        .and_then(Value::as_str)
        .unwrap_or(left_field);
    let join = task
        .node_parameters
        .get("joinType")
        .and_then(Value::as_str)
        .unwrap_or("inner");
    let mut right_by_key = BTreeMap::<String, Vec<(usize, &Item)>>::new();
    for (index, item) in right.iter().enumerate() {
        right_by_key
            .entry(canonical(
                get_path(&item.json, right_field).unwrap_or(&Value::Null),
            ))
            .or_default()
            .push((index, item));
    }
    let mut matched_right = BTreeSet::new();
    let mut output = Vec::new();
    for left_item in &left {
        let key = canonical(get_path(&left_item.json, left_field).unwrap_or(&Value::Null));
        if let Some(matches) = right_by_key.get(&key) {
            for (index, right_item) in matches {
                matched_right.insert(*index);
                let pair = vec![left_item.clone(), (*right_item).clone()];
                output.push(combined_item(
                    merge_json(&pair, conflict(task)),
                    &pair,
                    conflict(task),
                ));
            }
        } else if matches!(join, "left" | "full") {
            output.push(left_item.clone());
        }
    }
    if matches!(join, "right" | "full") {
        output.extend(
            right
                .into_iter()
                .enumerate()
                .filter_map(|(index, item)| (!matched_right.contains(&index)).then_some(item)),
        );
    }
    Ok(completed("main", output))
}

fn compare_datasets(task: &RuntimeTask) -> Result<TaskResult> {
    let fields = string_array(task.node_parameters.get("keyFields"));
    let left = task.inputs.get("left").cloned().unwrap_or_default();
    let right = task.inputs.get("right").cloned().unwrap_or_default();
    let key = |item: &Item| {
        if fields.is_empty() {
            canonical(&item.json)
        } else {
            canonical(&Value::Array(
                fields
                    .iter()
                    .map(|field| get_path(&item.json, field).cloned().unwrap_or(Value::Null))
                    .collect(),
            ))
        }
    };
    let right_map = right
        .iter()
        .map(|item| (key(item), item))
        .collect::<BTreeMap<_, _>>();
    let left_map = left
        .iter()
        .map(|item| (key(item), item))
        .collect::<BTreeMap<_, _>>();
    let mut same = Vec::new();
    let mut different = Vec::new();
    let mut left_only = Vec::new();
    let mut right_only = Vec::new();
    for (entry_key, item) in &left_map {
        match right_map.get(entry_key) {
            Some(other) if item.json == other.json => same.push((*item).clone()),
            Some(other) => {
                let pair = vec![(*item).clone(), (*other).clone()];
                different.push(combined_item(
                    json!({"left":item.json,"right":other.json}),
                    &pair,
                    "prefer_right",
                ));
            }
            None => left_only.push((*item).clone()),
        }
    }
    for (entry_key, item) in right_map {
        if !left_map.contains_key(&entry_key) {
            right_only.push(item.clone());
        }
    }
    Ok(TaskResult::Completed(BTreeMap::from([
        ("same".into(), same),
        ("different".into(), different),
        ("left_only".into(), left_only),
        ("right_only".into(), right_only),
    ])))
}

fn aggregate_value(kind: &str, values: &[Value], count: usize) -> Result<Value> {
    Ok(match kind {
        "count" => Value::Number((count as u64).into()),
        "collect" => Value::Array(values.to_vec()),
        "first" => values.first().cloned().unwrap_or(Value::Null),
        "last" => values.last().cloned().unwrap_or(Value::Null),
        "sum" | "avg" | "min" | "max" => {
            let numbers = values
                .iter()
                .map(|value| value.as_f64().context("aggregate value must be numeric"))
                .collect::<Result<Vec<_>>>()?;
            if numbers.is_empty() {
                Value::Null
            } else {
                let number = match kind {
                    "sum" => numbers.iter().sum(),
                    "avg" => numbers.iter().sum::<f64>() / numbers.len() as f64,
                    "min" => numbers.iter().copied().fold(f64::INFINITY, f64::min),
                    _ => numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                };
                Value::Number(Number::from_f64(number).context("aggregate result is not finite")?)
            }
        }
        other => bail!("unsupported aggregate operation {other}"),
    })
}

fn combined_item(json: Value, items: &[Item], conflict: &str) -> Item {
    let mut combined = Item {
        json,
        ..Item::default()
    };
    for item in items {
        for (key, value) in &item.binary {
            if conflict == "prefer_left" {
                combined
                    .binary
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            } else {
                combined.binary.insert(key.clone(), value.clone());
            }
        }
        for (key, value) in &item.metadata {
            if conflict == "prefer_left" {
                combined
                    .metadata
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            } else {
                combined.metadata.insert(key.clone(), value.clone());
            }
        }
        append_lineage(&mut combined.lineage, &item.lineage);
    }
    combined
}

fn merge_json(items: &[Item], conflict: &str) -> Value {
    let mut result = Map::new();
    for (item_index, item) in items.iter().enumerate() {
        if let Some(object) = item.json.as_object() {
            for (key, value) in object {
                if conflict == "prefer_left" {
                    result.entry(key.clone()).or_insert_with(|| value.clone());
                } else if conflict == "suffix" && result.contains_key(key) {
                    result.insert(format!("{key}_{}", item_index + 1), value.clone());
                } else {
                    result.insert(key.clone(), value.clone());
                }
            }
        }
    }
    Value::Object(result)
}

fn append_lineage(target: &mut Vec<ItemSource>, sources: &[ItemSource]) {
    for source in sources {
        if !target.contains(source) {
            target.push(source.clone());
        }
    }
}

fn duration(amount: i64, unit: &str) -> Result<Duration> {
    Ok(match unit {
        "seconds" => Duration::seconds(amount),
        "minutes" => Duration::minutes(amount),
        "hours" => Duration::hours(amount),
        "days" => Duration::days(amount),
        other => bail!("unsupported date unit {other}"),
    })
}

fn ordered_inputs(task: &RuntimeTask) -> Vec<Vec<Item>> {
    task.inputs.values().cloned().collect()
}

fn conflict(task: &RuntimeTask) -> &str {
    task.node_parameters
        .get("conflictStrategy")
        .and_then(Value::as_str)
        .unwrap_or("prefer_right")
}

fn flatten(task: &RuntimeTask) -> Vec<Item> {
    task.inputs.values().flatten().cloned().collect()
}
fn completed(port: &str, items: Vec<Item>) -> TaskResult {
    TaskResult::Completed(BTreeMap::from([(port.into(), items)]))
}
fn canonical(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
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
fn required_string<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("{name} is required"))
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

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, segment| current.get(segment))
}

fn set_path(value: &mut Value, path: &str, next: Value) -> Result<()> {
    let object = value
        .as_object_mut()
        .context("item JSON must be an object")?;
    set_path_in_map(object, path, next)
}

fn set_path_in_map(object: &mut Map<String, Value>, path: &str, next: Value) -> Result<()> {
    let mut root = Value::Object(std::mem::take(object));
    let mut segments = path.split('.').peekable();
    let mut current = &mut root;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current
                .as_object_mut()
                .context("field parent must be an object")?
                .insert(segment.into(), next);
            break;
        }
        current = current
            .as_object_mut()
            .context("field parent must be an object")?
            .entry(segment)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    *object = root
        .as_object_mut()
        .map(std::mem::take)
        .context("item JSON must be an object")?;
    Ok(())
}

fn remove_path(value: &mut Value, path: &str) -> Option<Value> {
    let mut parts = path.rsplitn(2, '.');
    let leaf = parts.next()?;
    let parent = parts.next();
    let object = match parent {
        Some(parent) => get_path_mut(value, parent)?.as_object_mut()?,
        None => value.as_object_mut()?,
    };
    object.remove(leaf)
}

fn get_path_mut<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get_mut(segment)?;
    }
    Some(current)
}

fn compare_values(left: Option<&Value>, right: Option<&Value>, nulls_first: bool) -> Ordering {
    match (left, right) {
        (None | Some(Value::Null), None | Some(Value::Null)) => Ordering::Equal,
        (None | Some(Value::Null), _) => {
            if nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, None | Some(Value::Null)) => {
            if nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(Value::Number(left)), Some(Value::Number(right))) => left
            .as_f64()
            .partial_cmp(&right.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(Value::String(left)), Some(Value::String(right))) => left.cmp(right),
        (Some(Value::Bool(left)), Some(Value::Bool(right))) => left.cmp(right),
        (Some(left), Some(right)) => canonical(left).cmp(&canonical(right)),
    }
}
