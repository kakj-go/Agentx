use std::collections::BTreeMap;

use agentx_domain::DynamicValue;
use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine};
use serde_json::Value;

use crate::error::{RuntimeError, RuntimeResult};

pub(crate) fn apply(
    outputs: &mut BTreeMap<String, Vec<Item>>,
    projection: &Value,
    base: &ExpressionContext,
) -> RuntimeResult<()> {
    let Some(ports) = projection.as_object().filter(|fields| !fields.is_empty()) else {
        return Ok(());
    };
    let engine = ExpressionEngine;
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
                let Some(value) = engine
                    .resolve_dynamic_optional(&dynamic, &context)
                    .map_err(|error| invalid_projection(error.to_string()))?
                else {
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
    Ok(())
}

pub(crate) fn assistant_message_content(output: &Value) -> (&'static str, Value) {
    if let Some(text) = output.as_str() {
        return ("text", Value::String(text.to_owned()));
    }
    for field in ["message", "answer", "text", "finalAnswer"] {
        if let Some(text) = output.get(field).and_then(Value::as_str) {
            return ("text", Value::String(text.to_owned()));
        }
    }
    ("json", output.clone())
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

    use super::{apply, assistant_message_content};

    #[test]
    fn assistant_message_uses_chat_text_from_end_output() {
        for output in [
            json!("plain"),
            json!({"message":"message text"}),
            json!({"answer":"answer text"}),
            json!({"text":"text field"}),
            json!({"finalAnswer":"agent answer"}),
        ] {
            let (part_type, content) = assistant_message_content(&output);
            assert_eq!(part_type, "text");
            assert!(content.is_string());
        }
        assert_eq!(
            assistant_message_content(&json!({"answer":"answer text"})).1,
            json!("answer text")
        );
    }

    #[test]
    fn assistant_message_preserves_structured_end_output_without_chat_text() {
        let output = json!({"count":2,"items":[1,2]});
        let (part_type, content) = assistant_message_content(&output);
        assert_eq!(part_type, "json");
        assert_eq!(content, output);
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
