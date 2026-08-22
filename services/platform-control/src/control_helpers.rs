use std::str::FromStr;

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api_error::{ApiError, ApiResult};

pub fn required_name(value: &str) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 160 {
        return Err(ApiError::bad_request(
            "INVALID_NAME",
            "Name must contain between 1 and 160 characters",
        ));
    }
    Ok(value.to_owned())
}

pub fn random_url_token(size: usize) -> String {
    let mut value = vec![0_u8; size];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}

pub fn generate_api_key() -> (Uuid, String, String, [u8; 32]) {
    let id = Uuid::now_v7();
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let secret = format!("axk_{}", URL_SAFE_NO_PAD.encode(bytes));
    let prefix = secret.chars().take(12).collect();
    (
        id,
        secret.clone(),
        prefix,
        Sha256::digest(secret.as_bytes()).into(),
    )
}

pub fn validate_schedule(cron_expression: &str, timezone: &str, policy: &str) -> ApiResult<()> {
    if !matches!(policy, "skip" | "fire_once") {
        return Err(ApiError::bad_request(
            "INVALID_MISFIRE_POLICY",
            "Misfire Policy must be skip or fire_once",
        ));
    }
    chrono_tz::Tz::from_str(timezone).map_err(|_| {
        ApiError::bad_request("INVALID_TIMEZONE", "Timezone must be an IANA timezone")
    })?;
    let normalized = match cron_expression.split_whitespace().count() {
        5 => format!("0 {cron_expression}"),
        6 | 7 => cron_expression.to_owned(),
        _ => {
            return Err(ApiError::bad_request(
                "INVALID_CRON",
                "Cron expression must contain five, six, or seven fields",
            ));
        }
    };
    cron::Schedule::from_str(&normalized)
        .map_err(|_| ApiError::bad_request("INVALID_CRON", "Cron expression is invalid"))?;
    Ok(())
}

pub fn parse_action_id(value: &str, action: Option<&str>, code: &'static str) -> ApiResult<Uuid> {
    let id = match action {
        Some(action) => value.strip_suffix(&format!(":{action}")),
        None if !value.contains(':') => Some(value),
        None => None,
    }
    .ok_or_else(|| ApiError::bad_request(code, "Application action path is invalid"))?;
    Uuid::parse_str(id)
        .map_err(|_| ApiError::bad_request(code, "Application action path is invalid"))
}

pub fn payload_uuid(payload: &Value, key: &str) -> Result<Uuid> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .with_context(|| format!("Admission Outbox {key}"))
}

pub fn payload_strings(payload: &Value, key: &str) -> Result<Vec<String>> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .with_context(|| format!("Admission Outbox {key}"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .with_context(|| format!("Admission Outbox {key} item"))
        })
        .collect()
}

pub fn payload_uuids(payload: &Value, key: &str) -> Result<Vec<Uuid>> {
    payload_strings(payload, key)?
        .into_iter()
        .map(|value| {
            Uuid::parse_str(&value).with_context(|| format!("Admission Outbox {key} UUID item"))
        })
        .collect()
}

pub async fn allocate_activation_sequence(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant_id: Uuid,
    application_id: Uuid,
) -> ApiResult<u64> {
    let current: u64 = sqlx::query_scalar("SELECT runtime_activation_sequence FROM applications WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(tenant_id).bind(application_id).fetch_optional(&mut **tx).await?
        .ok_or_else(|| ApiError::not_found("Application"))?;
    let next = current
        .checked_add(1)
        .ok_or_else(|| ApiError::internal("Runtime activation sequence exhausted"))?;
    sqlx::query("UPDATE applications SET runtime_activation_sequence=? WHERE tenant_id=? AND id=?")
        .bind(next)
        .bind(tenant_id)
        .bind(application_id)
        .execute(&mut **tx)
        .await?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_control_input() {
        assert!(required_name("trigger").is_ok());
        assert!(required_name(" ").is_err());
        assert!(validate_schedule("0 9 * * 1", "Asia/Shanghai", "fire_once").is_ok());
        assert!(validate_schedule("invalid", "Asia/Shanghai", "skip").is_err());
    }

    #[test]
    fn action_ids_preserve_browser_api_paths() {
        let id = Uuid::now_v7();
        assert_eq!(
            parse_action_id(&id.to_string(), None, "INVALID").unwrap(),
            id
        );
        assert_eq!(
            parse_action_id(&format!("{id}:retry"), Some("retry"), "INVALID").unwrap(),
            id
        );
        assert!(parse_action_id(&format!("{id}:retry"), None, "INVALID").is_err());
    }
}
