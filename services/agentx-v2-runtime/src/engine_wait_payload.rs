use serde_json::Value;

pub(super) fn validate(schema: &Value, payload: &Value) -> Result<(), (&'static str, String)> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| ("INVALID_WAIT_PAYLOAD_SCHEMA", error.to_string()))?;
    validator
        .validate(payload)
        .map_err(|error| ("WAIT_PAYLOAD_SCHEMA_VALIDATION_FAILED", error.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate;

    #[test]
    fn resume_payload_is_validated_against_the_frozen_schema() {
        let schema = json!({
            "type":"object",
            "required":["answer"],
            "properties":{"answer":{"type":"string"}},
            "additionalProperties":false
        });
        assert!(validate(&schema, &json!({"answer":"ok"})).is_ok());
        let error = validate(&schema, &json!({"answer":42})).unwrap_err();
        assert_eq!(error.0, "WAIT_PAYLOAD_SCHEMA_VALIDATION_FAILED");
    }
}
