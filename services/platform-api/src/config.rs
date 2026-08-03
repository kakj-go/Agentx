use std::env;

use anyhow::{Context, Result};
use ipnet::IpNet;
use secrecy::SecretString;

#[derive(Clone)]
pub struct AuthSettings {
    pub signing_secret: SecretString,
    pub issuer: String,
    pub audience: String,
    pub access_ttl_seconds: i64,
    pub refresh_ttl_seconds: i64,
    pub change_password_ttl_seconds: i64,
    pub cookie_secure: bool,
    pub login_max_failures: u32,
    pub login_failure_window_seconds: i64,
    pub login_lock_seconds: i64,
}

#[derive(Clone)]
pub struct CredentialSettings {
    pub active_key_id: String,
    pub keys_json: SecretString,
}

impl CredentialSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            active_key_id: env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")
                .context("AGENTX_CREDENTIAL_ACTIVE_KEY_ID is required")?,
            keys_json: SecretString::from(
                env::var("AGENTX_CREDENTIAL_KEYS_JSON")
                    .context("AGENTX_CREDENTIAL_KEYS_JSON is required")?,
            ),
        })
    }
}

#[derive(Clone)]
pub struct ConnectionSettings {
    pub timeout_seconds: u64,
    pub max_concurrency: usize,
    pub allow_private_networks: bool,
    pub allowed_hosts: Vec<String>,
    pub allowed_cidrs: Vec<IpNet>,
}

impl ConnectionSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            timeout_seconds: parse("AGENTX_CONNECTION_TEST_TIMEOUT_SECONDS", 10_u64)?,
            max_concurrency: parse("AGENTX_CONNECTION_TEST_MAX_CONCURRENCY", 4_usize)?,
            allow_private_networks: parse_bool(
                "AGENTX_CONNECTION_ALLOW_PRIVATE_NETWORKS",
                value("AGENTX_ENV", "production").eq_ignore_ascii_case("local"),
            )?,
            allowed_hosts: value("AGENTX_CONNECTION_ALLOWED_HOSTS", "")
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_lowercase)
                .collect(),
            allowed_cidrs: value("AGENTX_CONNECTION_ALLOWED_CIDRS", "")
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value.parse::<IpNet>().with_context(|| {
                        format!("AGENTX_CONNECTION_ALLOWED_CIDRS contains invalid CIDR {value}")
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl AuthSettings {
    pub fn from_env() -> Result<Self> {
        let signing_secret = env::var("AGENTX_JWT_SIGNING_SECRET")
            .context("AGENTX_JWT_SIGNING_SECRET is required")?;
        if signing_secret.len() < 32 {
            anyhow::bail!("AGENTX_JWT_SIGNING_SECRET must contain at least 32 characters");
        }
        let cookie_secure = parse_bool("AGENTX_COOKIE_SECURE", true)?;
        if value("AGENTX_ENV", "production").eq_ignore_ascii_case("production") && !cookie_secure {
            anyhow::bail!("AGENTX_COOKIE_SECURE must be true in production");
        }
        let login_max_failures = parse("AGENTX_LOGIN_MAX_FAILURES", 5)?;
        let login_failure_window_seconds = parse("AGENTX_LOGIN_FAILURE_WINDOW_SECONDS", 900)?;
        let login_lock_seconds = parse("AGENTX_LOGIN_LOCK_SECONDS", 900)?;
        if login_max_failures == 0 || login_failure_window_seconds <= 0 || login_lock_seconds <= 0 {
            anyhow::bail!("login rate-limit values must be greater than zero");
        }
        Ok(Self {
            signing_secret: SecretString::from(signing_secret),
            issuer: value("AGENTX_JWT_ISSUER", "agentx-platform"),
            audience: value("AGENTX_JWT_AUDIENCE", "agentx-web"),
            access_ttl_seconds: parse("AGENTX_ACCESS_TOKEN_TTL_SECONDS", 900)?,
            refresh_ttl_seconds: parse("AGENTX_REFRESH_TOKEN_TTL_SECONDS", 604_800)?,
            change_password_ttl_seconds: parse("AGENTX_CHANGE_PASSWORD_TOKEN_TTL_SECONDS", 600)?,
            cookie_secure,
            login_max_failures,
            login_failure_window_seconds,
            login_lock_seconds,
        })
    }
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}
fn parse<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    env::var(name).map_or(Ok(default), |value| {
        value.parse().with_context(|| format!("{name} is invalid"))
    })
}
fn parse_bool(name: &str, default: bool) -> Result<bool> {
    env::var(name).map_or(Ok(default), |value| {
        match value.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => anyhow::bail!("{name} must be true or false"),
        }
    })
}
