use std::env;

use anyhow::{Context, Result};
use secrecy::SecretString;

#[derive(Clone)]
pub struct InfrastructureSettings {
    pub mysql: MySqlSettings,
    pub redis: RedisSettings,
    pub clickhouse: ClickHouseSettings,
    pub object_storage: ObjectStorageSettings,
}

#[derive(Clone)]
pub struct MySqlSettings {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: SecretString,
    pub max_connections: u32,
}

#[derive(Clone)]
pub struct RedisSettings {
    pub url: SecretString,
}

#[derive(Clone)]
pub struct ClickHouseSettings {
    pub url: String,
    pub database: String,
    pub username: String,
    pub password: SecretString,
}

#[derive(Clone)]
pub struct ObjectStorageSettings {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: SecretString,
    pub secret_key: SecretString,
    pub allow_http: bool,
}

impl InfrastructureSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            mysql: MySqlSettings {
                host: value("AGENTX_MYSQL_HOST", "127.0.0.1"),
                port: parse("AGENTX_MYSQL_PORT", 3306)?,
                database: value("AGENTX_MYSQL_DATABASE", "agentx"),
                username: value("AGENTX_MYSQL_USER", "agentx"),
                password: SecretString::from(required("AGENTX_MYSQL_PASSWORD")?),
                max_connections: parse("AGENTX_MYSQL_MAX_CONNECTIONS", 20)?,
            },
            redis: RedisSettings {
                url: SecretString::from(value("AGENTX_REDIS_URL", "redis://127.0.0.1:6379/")),
            },
            clickhouse: ClickHouseSettings {
                url: value("AGENTX_CLICKHOUSE_URL", "http://127.0.0.1:8123"),
                database: value("AGENTX_CLICKHOUSE_DATABASE", "agentx"),
                username: value("AGENTX_CLICKHOUSE_USER", "agentx"),
                password: SecretString::from(value("AGENTX_CLICKHOUSE_PASSWORD", "")),
            },
            object_storage: ObjectStorageSettings {
                endpoint: value("AGENTX_S3_ENDPOINT", "http://127.0.0.1:9000"),
                bucket: value("AGENTX_S3_BUCKET", "agentx"),
                region: value("AGENTX_S3_REGION", "us-east-1"),
                access_key: SecretString::from(value("AGENTX_MINIO_USER", "")),
                secret_key: SecretString::from(value("AGENTX_MINIO_PASSWORD", "")),
                allow_http: parse_bool("AGENTX_S3_ALLOW_HTTP", true)?,
            },
        })
    }
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("required environment variable {name} is missing"))
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value
            .parse::<T>()
            .with_context(|| format!("{name} has an invalid value")),
        Err(_) => Ok(default),
    }
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
