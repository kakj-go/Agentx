use crate::process::redact_embedded_material;
use anyhow::Result;
use clap::ValueEnum;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

pub fn emit(value: &Value, format: OutputFormat) -> Result<()> {
    let value = redact_value(value);
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
        OutputFormat::Text => match &value {
            Value::String(value) => print!("{value}"),
            Value::Object(object) => {
                for (key, value) in object {
                    if value.is_object() || value.is_array() {
                        println!("{key}: {}", serde_json::to_string(value)?);
                    } else if let Some(value) = value.as_str() {
                        println!("{key}: {value}");
                    } else {
                        println!("{key}: {value}");
                    }
                }
            }
            value => println!("{value}"),
        },
    }
    Ok(())
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(redact_embedded_material(value)),
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
                    let sensitive = ["password", "token", "privatekey", "authorization"]
                        .iter()
                        .any(|marker| normalized.ends_with(marker))
                        || normalized == "secret"
                        || normalized.ends_with("secretvalue");
                    (
                        key.clone(),
                        if sensitive && !key.to_ascii_lowercase().ends_with("name") {
                            Value::String("<redacted>".into())
                        } else {
                            redact_value(value)
                        },
                    )
                })
                .collect::<Map<_, _>>(),
        ),
        value => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn successful_output_is_redacted_without_hiding_secret_resource_names() {
        let value = redact_value(&json!({
            "password": "hunter2",
            "controlTlsSecretName": "control-tls",
            "endpoint": "https://user:pass@example.test",
            "nested": {"private_key": "material"}
        }));
        assert_eq!(value["password"], "<redacted>");
        assert_eq!(value["controlTlsSecretName"], "control-tls");
        assert_eq!(value["endpoint"], "https://<redacted>@example.test");
        assert_eq!(value["nested"]["private_key"], "<redacted>");
    }
}
