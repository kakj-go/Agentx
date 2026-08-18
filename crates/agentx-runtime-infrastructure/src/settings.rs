use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use secrecy::SecretString;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MySqlTlsMode {
    Disabled,
    Preferred,
    Required,
    VerifyCa,
    VerifyIdentity,
}

#[derive(Clone)]
pub struct RuntimeMySqlSettings {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: SecretString,
    pub max_connections: u32,
    pub tls_mode: MySqlTlsMode,
    pub tls_ca_path: Option<PathBuf>,
    pub tls_client_cert_path: Option<PathBuf>,
    pub tls_client_key_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct RuntimeRedisSettings {
    pub url: SecretString,
    pub password: Option<SecretString>,
    pub tls_ca_path: Option<PathBuf>,
    pub tls_client_cert_path: Option<PathBuf>,
    pub tls_client_key_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct RuntimeObjectStorageSettings {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: SecretString,
    pub secret_key: SecretString,
    pub session_token: Option<SecretString>,
    pub allow_http: bool,
    pub path_style: bool,
    pub tls_ca_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct RuntimeInfrastructureSettings {
    pub mysql: RuntimeMySqlSettings,
    pub redis: RuntimeRedisSettings,
    pub object_storage: RuntimeObjectStorageSettings,
}

impl RuntimeInfrastructureSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            mysql: RuntimeMySqlSettings::from_env()?,
            redis: RuntimeRedisSettings::from_env()?,
            object_storage: RuntimeObjectStorageSettings::from_env()?,
        })
    }
}

impl RuntimeMySqlSettings {
    pub fn from_env() -> Result<Self> {
        let tls_mode = match value("AGENTX_RUNTIME_MYSQL_TLS_MODE", "preferred").as_str() {
            "disabled" => MySqlTlsMode::Disabled,
            "preferred" => MySqlTlsMode::Preferred,
            "required" => MySqlTlsMode::Required,
            "verify_ca" => MySqlTlsMode::VerifyCa,
            "verify_identity" => MySqlTlsMode::VerifyIdentity,
            _ => anyhow::bail!("AGENTX_RUNTIME_MYSQL_TLS_MODE is invalid"),
        };
        let settings = Self {
            host: value("AGENTX_RUNTIME_MYSQL_HOST", "127.0.0.1"),
            port: parse("AGENTX_RUNTIME_MYSQL_PORT", 3306)?,
            database: value("AGENTX_RUNTIME_MYSQL_DATABASE", "agentx_runtime"),
            username: value("AGENTX_RUNTIME_MYSQL_USER", "agentx_runtime"),
            password: SecretString::from(required("AGENTX_RUNTIME_MYSQL_PASSWORD")?),
            max_connections: parse("AGENTX_RUNTIME_MYSQL_MAX_CONNECTIONS", 20)?,
            tls_mode,
            tls_ca_path: path("AGENTX_RUNTIME_MYSQL_TLS_CA_PATH"),
            tls_client_cert_path: path("AGENTX_RUNTIME_MYSQL_TLS_CLIENT_CERT_PATH"),
            tls_client_key_path: path("AGENTX_RUNTIME_MYSQL_TLS_CLIENT_KEY_PATH"),
        };
        validate_pair(
            settings.tls_client_cert_path.as_ref(),
            settings.tls_client_key_path.as_ref(),
            "Runtime MySQL client certificate and key",
        )?;
        Ok(settings)
    }
}

impl RuntimeRedisSettings {
    pub fn from_env() -> Result<Self> {
        let settings = Self {
            url: SecretString::from(value("AGENTX_RUNTIME_REDIS_URL", "redis://127.0.0.1:6379/")),
            password: optional("AGENTX_RUNTIME_REDIS_PASSWORD").map(SecretString::from),
            tls_ca_path: path("AGENTX_RUNTIME_REDIS_TLS_CA_PATH"),
            tls_client_cert_path: path("AGENTX_RUNTIME_REDIS_TLS_CLIENT_CERT_PATH"),
            tls_client_key_path: path("AGENTX_RUNTIME_REDIS_TLS_CLIENT_KEY_PATH"),
        };
        validate_pair(
            settings.tls_client_cert_path.as_ref(),
            settings.tls_client_key_path.as_ref(),
            "Runtime Redis client certificate and key",
        )?;
        Ok(settings)
    }
}

impl RuntimeObjectStorageSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            endpoint: value("AGENTX_RUNTIME_S3_ENDPOINT", "http://127.0.0.1:9000"),
            bucket: value("AGENTX_RUNTIME_S3_BUCKET", "agentx-runtime"),
            region: value("AGENTX_RUNTIME_S3_REGION", "us-east-1"),
            access_key: SecretString::from(value("AGENTX_RUNTIME_S3_ACCESS_KEY", "")),
            secret_key: SecretString::from(value("AGENTX_RUNTIME_S3_SECRET_KEY", "")),
            session_token: optional("AGENTX_RUNTIME_S3_SESSION_TOKEN").map(SecretString::from),
            allow_http: parse_bool("AGENTX_RUNTIME_S3_ALLOW_HTTP", true)?,
            path_style: parse_bool("AGENTX_RUNTIME_S3_PATH_STYLE", true)?,
            tls_ca_path: path("AGENTX_RUNTIME_S3_TLS_CA_PATH"),
        })
    }
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("required environment variable {name} is missing"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
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
    match env::var(name) {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => anyhow::bail!("{name} must be true or false"),
        },
        Err(_) => Ok(default),
    }
}

fn path(name: &str) -> Option<PathBuf> {
    optional(name).map(PathBuf::from)
}

fn validate_pair<T>(left: Option<&T>, right: Option<&T>, label: &str) -> Result<()> {
    anyhow::ensure!(
        left.is_some() == right.is_some(),
        "{label} must be configured together"
    );
    Ok(())
}
