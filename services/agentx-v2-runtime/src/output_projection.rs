use std::collections::BTreeMap;

use agentx_domain::DynamicValue;
use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine, StringConversionRecord};
use serde_json::Value;

use crate::error::{RuntimeError, RuntimeResult};

pub(crate) fn apply(
    outputs: &mut BTreeMap<String, Vec<Item>>,
    projection: &Value,
    base: &ExpressionContext,
) -> RuntimeResult<Vec<StringConversionRecord>> {
    let Some(ports) = projection.as_object().filter(|fields| !fields.is_empty()) else {
        return Ok(Vec::new());
    };
    let engine = ExpressionEngine;
    let mut conversions = Vec::new();
    for (port, fields) in ports {
        if port == "error" {
            return Err(invalid_projection("ERROR_PROJECTION_NOT_ALLOWED"));
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
                .ok_or_else(|| invalid_projection("OUTPUT_PROJECTION_ITEM_MUST_BE_OBJECT"))?;
            for (name, definition) in fields {
                let dynamic = definition
                    .get("value")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<DynamicValue>(value).ok())
                    .ok_or_else(|| {
                        invalid_projection(format!("OUTPUT_PROJECTION_VALUE_MISSING:{port}.{name}"))
                    })?;
                let (value, mut field_conversions) = engine
                    .resolve_dynamic_optional_with_conversions(
                        &dynamic,
                        &context,
                        format!("outputProjection.{port}.{name}"),
                    )
                    .map_err(|error| invalid_projection(error.to_string()))?;
                conversions.append(&mut field_conversions);
                let Some(value) = value else {
                    continue;
                };
                if target.contains_key(name) {
                    return Err(invalid_projection(format!(
                        "OUTPUT_PROJECTION_FIELD_CONFLICT:{name}"
                    )));
                }
                target.insert(name.clone(), value);
            }
        }
    }
    Ok(conversions)
}

pub(crate) fn assistant_message_parts(
    output: &Value,
    mapping: &agentx_runtime_contracts::ChatMappingV1,
) -> RuntimeResult<Vec<agentx_runtime_contracts::MessagePartInputV1>> {
    let text = output
        .get(&mapping.answer_output)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "CHAT_ANSWER_OUTPUT_MISSING",
                format!(
                    "Mapped answer output '{}' is missing or is not a string",
                    mapping.answer_output
                ),
            )
        })?;
    let mut parts = vec![agentx_runtime_contracts::MessagePartInputV1 {
        part_type: "text".into(),
        content: Some(Value::String(text.to_owned())),
        artifact_id: None,
    }];
    let Some(field) = &mapping.answer_files_output else {
        return Ok(parts);
    };
    let Some(value) = output.get(field) else {
        return Ok(parts);
    };
    let values = value
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    for reference in values {
        let artifact_id = reference
            .get("artifactId")
            .and_then(Value::as_str)
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(
                    "CHAT_ANSWER_FILE_INVALID",
                    format!(
                        "Mapped answer file output '{field}' contains an invalid Artifact Reference"
                    ),
                )
            })?;
        parts.push(agentx_runtime_contracts::MessagePartInputV1 {
            part_type: "file".into(),
            content: Some(reference),
            artifact_id: Some(artifact_id),
        });
    }
    Ok(parts)
}

fn invalid_projection(message: impl Into<String>) -> RuntimeError {
    RuntimeError::InvalidRequest("OUTPUT_PROJECTION_FAILED", message.into())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentx_node_protocol::Item;
    use agentx_runtime::ExpressionContext;
    use serde_json::json;

    use super::{apply, assistant_message_parts};

    #[test]
    fn assistant_message_uses_the_snapshotted_output_mapping() {
        let mapping = agentx_runtime_contracts::ChatMappingV1 {
            question_input: "prompt".into(),
            file_input: None,
            answer_output: "result".into(),
            answer_files_output: None,
        };
        let parts =
            assistant_message_parts(&json!({"answer":"ignored","result":"mapped"}), &mapping)
                .unwrap();
        assert_eq!(parts[0].part_type, "text");
        assert_eq!(parts[0].content, Some(json!("mapped")));
    }

    #[test]
    fn assistant_message_does_not_guess_answer_fields() {
        let mapping = agentx_runtime_contracts::ChatMappingV1 {
            question_input: "prompt".into(),
            file_input: None,
            answer_output: "result".into(),
            answer_files_output: None,
        };
        assert!(assistant_message_parts(&json!({"answer":"legacy"}), &mapping).is_err());
    }

    #[test]
    fn assistant_message_extracts_single_and_multiple_mapped_artifacts() {
        let mut mapping = agentx_runtime_contracts::ChatMappingV1 {
            question_input: "prompt".into(),
            file_input: None,
            answer_output: "result".into(),
            answer_files_output: Some("files".into()),
        };
        let first =
            json!({"artifactId":"00000000-0000-0000-0000-000000000001","fileName":"one.txt"});
        let second =
            json!({"artifactId":"00000000-0000-0000-0000-000000000002","fileName":"two.txt"});
        let single =
            assistant_message_parts(&json!({"result":"done","files":first.clone()}), &mapping)
                .unwrap();
        assert_eq!(single.len(), 2);
        assert_eq!(single[1].content, Some(first.clone()));

        let multiple =
            assistant_message_parts(&json!({"result":"done","files":[first,second]}), &mapping)
                .unwrap();
        assert_eq!(multiple.len(), 3);
        assert!(multiple[1..].iter().all(|part| part.part_type == "file"));

        mapping.answer_files_output = None;
        assert_eq!(
            assistant_message_parts(&json!({"result":"done"}), &mapping)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn projection_fields_are_available_on_the_current_item() {
        let mut outputs = BTreeMap::from([(
            "main".to_owned(),
            vec![Item {
                json: json!({"stdout":"sandbox-ok"}),
                ..Item::default()
            }],
        )]);
        apply(
            &mut outputs,
            &json!({
                "main": {
                    "summary": {
                        "value":{"kind":"reference","selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["stdout"]},"missingPolicy":{"kind":"error"}},
                        "schema":{"type":"string"},
                        "sensitive":false
                    }
                }
            }),
            &ExpressionContext::default(),
        )
        .unwrap();
        assert_eq!(
            outputs["main"][0].json,
            json!({"stdout":"sandbox-ok","summary":"sandbox-ok"})
        );
    }

    #[test]
    fn projection_reports_string_conversion_records() {
        let mut outputs = BTreeMap::from([(
            "main".to_owned(),
            vec![Item {
                json: json!({"body":{"b":2,"a":1}}),
                ..Item::default()
            }],
        )]);
        let conversions = apply(
            &mut outputs,
            &json!({"main":{"summary":{"value":{"kind":"reference","selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["body"]},"missingPolicy":{"kind":"error"},"coerce":"string"}}}}),
            &ExpressionContext::default(),
        )
        .unwrap();
        assert_eq!(outputs["main"][0].json["summary"], "{\"a\":1,\"b\":2}");
        assert_eq!(conversions[0].target_path, "outputProjection.main.summary");
        assert_eq!(conversions[0].source_type, "object");
    }

    #[test]
    fn projection_does_not_overwrite_provider_fields() {
        let mut outputs = BTreeMap::from([(
            "main".to_owned(),
            vec![Item {
                json: json!({"summary":"provider"}),
                ..Item::default()
            }],
        )]);
        let error = apply(
            &mut outputs,
            &json!({"main":{"summary":{"value":{"kind":"literal","value":"replacement"}}}}),
            &ExpressionContext::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("OUTPUT_PROJECTION_FIELD_CONFLICT")
        );
    }
}
