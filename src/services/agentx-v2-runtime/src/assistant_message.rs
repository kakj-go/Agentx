use serde_json::Value;

use crate::error::{RuntimeError, RuntimeResult};

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::assistant_message_parts;

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
    fn assistant_message_extracts_mapped_artifacts() {
        let mapping = agentx_runtime_contracts::ChatMappingV1 {
            question_input: "prompt".into(),
            file_input: None,
            answer_output: "result".into(),
            answer_files_output: Some("files".into()),
        };
        let first =
            json!({"artifactId":"00000000-0000-0000-0000-000000000001","fileName":"one.txt"});
        let second =
            json!({"artifactId":"00000000-0000-0000-0000-000000000002","fileName":"two.txt"});
        let parts =
            assistant_message_parts(&json!({"result":"done","files":[first,second]}), &mapping)
                .unwrap();
        assert_eq!(parts.len(), 3);
        assert!(parts[1..].iter().all(|part| part.part_type == "file"));
    }
}
