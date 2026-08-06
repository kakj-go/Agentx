use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use secrecy::SecretString;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretProviderMode {
    LocalEncrypted,
    VaultKvV2,
}

pub fn is_production_environment() -> bool {
    ["AGENTX_ENV", "AGENTX_ENVIRONMENT"]
        .into_iter()
        .any(|name| {
            env::var(name).is_ok_and(|value| value.trim().eq_ignore_ascii_case("production"))
        })
}

pub fn secret_provider_mode() -> Result<SecretProviderMode> {
    validate_secret_provider(
        env::var("AGENTX_SECRET_PROVIDER").ok().as_deref(),
        is_production_environment(),
    )
}

fn validate_secret_provider(
    provider: Option<&str>,
    production: bool,
) -> Result<SecretProviderMode> {
    let provider = match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(provider) => provider,
        None if production => {
            anyhow::bail!("AGENTX_SECRET_PROVIDER=vault_kv_v2 is required in production")
        }
        None => "local_encrypted",
    };
    match provider {
        "vault_kv_v2" => Ok(SecretProviderMode::VaultKvV2),
        "local_encrypted" if !production => Ok(SecretProviderMode::LocalEncrypted),
        "local_encrypted" => {
            anyhow::bail!("AGENTX_SECRET_PROVIDER=vault_kv_v2 is required in production")
        }
        _ => anyhow::bail!("AGENTX_SECRET_PROVIDER must be local_encrypted or vault_kv_v2"),
    }
}

#[derive(Clone)]
pub struct InfrastructureSettings {
    pub mysql: MySqlSettings,
    pub redis: RedisSettings,
    pub clickhouse: ClickHouseSettings,
    pub object_storage: ObjectStorageSettings,
}

#[derive(Clone)]
pub struct RuntimeInfrastructureSettings {
    pub mysql: MySqlSettings,
    pub redis: RedisSettings,
    pub object_storage: ObjectStorageSettings,
}

#[derive(Clone)]
pub struct TraceInfrastructureSettings {
    pub mysql: MySqlSettings,
    pub redis: RedisSettings,
    pub clickhouse: ClickHouseSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MySqlTlsMode {
    Disabled,
    Preferred,
    Required,
    VerifyCa,
    VerifyIdentity,
}

#[derive(Clone)]
pub struct MySqlSettings {
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
pub struct RedisSettings {
    pub url: SecretString,
    pub password: Option<SecretString>,
    pub tls_ca_path: Option<PathBuf>,
    pub tls_client_cert_path: Option<PathBuf>,
    pub tls_client_key_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct ClickHouseSettings {
    pub url: String,
    pub database: String,
    pub username: String,
    pub password: SecretString,
    pub tls_ca_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct ObjectStorageSettings {
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

impl InfrastructureSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            mysql: MySqlSettings::from_env()?,
            redis: RedisSettings::from_env()?,
            clickhouse: ClickHouseSettings::from_env()?,
            object_storage: ObjectStorageSettings::from_env()?,
        })
    }
}

impl RuntimeInfrastructureSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            mysql: MySqlSettings::from_env()?,
            redis: RedisSettings::from_env()?,
            object_storage: ObjectStorageSettings::from_env()?,
        })
    }
}

impl TraceInfrastructureSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            mysql: MySqlSettings::from_env()?,
            redis: RedisSettings::from_env()?,
            clickhouse: ClickHouseSettings::from_env()?,
        })
    }
}

impl MySqlSettings {
    pub fn from_env() -> Result<Self> {
        let tls_mode = match value("AGENTX_MYSQL_TLS_MODE", "preferred")
            .to_ascii_lowercase()
            .as_str()
        {
            "disabled" => MySqlTlsMode::Disabled,
            "preferred" => MySqlTlsMode::Preferred,
            "required" => MySqlTlsMode::Required,
            "verify_ca" => MySqlTlsMode::VerifyCa,
            "verify_identity" => MySqlTlsMode::VerifyIdentity,
            _ => anyhow::bail!(
                "AGENTX_MYSQL_TLS_MODE must be disabled, preferred, required, verify_ca or verify_identity"
            ),
        };
        let settings = Self {
            host: value("AGENTX_MYSQL_HOST", "127.0.0.1"),
            port: parse("AGENTX_MYSQL_PORT", 3306)?,
            database: value("AGENTX_MYSQL_DATABASE", "agentx"),
            username: value("AGENTX_MYSQL_USER", "agentx"),
            password: SecretString::from(required("AGENTX_MYSQL_PASSWORD")?),
            max_connections: parse("AGENTX_MYSQL_MAX_CONNECTIONS", 20)?,
            tls_mode,
            tls_ca_path: path("AGENTX_MYSQL_TLS_CA_PATH"),
            tls_client_cert_path: path("AGENTX_MYSQL_TLS_CLIENT_CERT_PATH"),
            tls_client_key_path: path("AGENTX_MYSQL_TLS_CLIENT_KEY_PATH"),
        };
        validate_pair(
            "AGENTX_MYSQL_TLS_CLIENT_CERT_PATH",
            settings.tls_client_cert_path.as_ref(),
            "AGENTX_MYSQL_TLS_CLIENT_KEY_PATH",
            settings.tls_client_key_path.as_ref(),
        )?;
        Ok(settings)
    }
}

impl RedisSettings {
    pub fn from_env() -> Result<Self> {
        let settings = Self {
            url: SecretString::from(value("AGENTX_REDIS_URL", "redis://127.0.0.1:6379/")),
            password: optional("AGENTX_REDIS_PASSWORD").map(SecretString::from),
            tls_ca_path: path("AGENTX_REDIS_TLS_CA_PATH"),
            tls_client_cert_path: path("AGENTX_REDIS_TLS_CLIENT_CERT_PATH"),
            tls_client_key_path: path("AGENTX_REDIS_TLS_CLIENT_KEY_PATH"),
        };
        validate_pair(
            "AGENTX_REDIS_TLS_CLIENT_CERT_PATH",
            settings.tls_client_cert_path.as_ref(),
            "AGENTX_REDIS_TLS_CLIENT_KEY_PATH",
            settings.tls_client_key_path.as_ref(),
        )?;
        Ok(settings)
    }
}

impl ClickHouseSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            url: value("AGENTX_CLICKHOUSE_URL", "http://127.0.0.1:8123"),
            database: value("AGENTX_CLICKHOUSE_DATABASE", "agentx"),
            username: value("AGENTX_CLICKHOUSE_USER", "agentx"),
            password: SecretString::from(value("AGENTX_CLICKHOUSE_PASSWORD", "")),
            tls_ca_path: path("AGENTX_CLICKHOUSE_TLS_CA_PATH"),
        })
    }
}

impl ObjectStorageSettings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            endpoint: value("AGENTX_S3_ENDPOINT", "http://127.0.0.1:9000"),
            bucket: value("AGENTX_S3_BUCKET", "agentx"),
            region: value("AGENTX_S3_REGION", "us-east-1"),
            access_key: SecretString::from(value_with_alias(
                "AGENTX_S3_ACCESS_KEY",
                "AGENTX_MINIO_USER",
                "",
            )),
            secret_key: SecretString::from(value_with_alias(
                "AGENTX_S3_SECRET_KEY",
                "AGENTX_MINIO_PASSWORD",
                "",
            )),
            session_token: optional("AGENTX_S3_SESSION_TOKEN").map(SecretString::from),
            allow_http: parse_bool("AGENTX_S3_ALLOW_HTTP", true)?,
            path_style: parse_bool("AGENTX_S3_PATH_STYLE", true)?,
            tls_ca_path: path("AGENTX_S3_TLS_CA_PATH"),
        })
    }
}

fn validate_pair<T>(
    left_name: &str,
    left: Option<&T>,
    right_name: &str,
    right: Option<&T>,
) -> Result<()> {
    anyhow::ensure!(
        left.is_some() == right.is_some(),
        "{left_name} and {right_name} must be configured together"
    );
    Ok(())
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("required environment variable {name} is missing"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn path(name: &str) -> Option<PathBuf> {
    optional(name).map(PathBuf::from)
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn value_with_alias(name: &str, alias: &str, default: &str) -> String {
    env::var(name)
        .or_else(|_| env::var(alias))
        .unwrap_or_else(|_| default.to_owned())
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

#[cfg(test)]
mod tests {
    use super::{MySqlTlsMode, SecretProviderMode, validate_pair, validate_secret_provider};

    #[test]
    fn client_certificates_are_a_pair() {
        assert!(validate_pair("cert", Some(&"cert"), "key", Some(&"key")).is_ok());
        assert!(validate_pair("cert", None::<&&str>, "key", None::<&&str>).is_ok());
        assert!(validate_pair("cert", Some(&"cert"), "key", None::<&&str>).is_err());
    }

    #[test]
    fn mysql_tls_mode_is_comparable() {
        assert_eq!(MySqlTlsMode::VerifyIdentity, MySqlTlsMode::VerifyIdentity);
    }

    #[test]
    fn production_requires_vault_secret_provider() {
        assert!(validate_secret_provider(None, true).is_err());
        assert!(validate_secret_provider(Some("local_encrypted"), true).is_err());
        assert_eq!(
            validate_secret_provider(Some("vault_kv_v2"), true).unwrap(),
            SecretProviderMode::VaultKvV2
        );
    }

    #[test]
    fn non_production_defaults_to_local_secret_provider() {
        assert_eq!(
            validate_secret_provider(None, false).unwrap(),
            SecretProviderMode::LocalEncrypted
        );
        assert!(validate_secret_provider(Some("unknown"), false).is_err());
    }
}
