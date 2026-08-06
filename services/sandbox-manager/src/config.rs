use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use agentx_infrastructure::{
    OpenSandboxAdapter,
    config::{MySqlSettings, SecretProviderMode, secret_provider_mode},
    credential::{CredentialKeyring, CredentialSource, RemoteSecretProvider},
};
use anyhow::{Context, Result};
use secrecy::SecretString;
use url::Url;

#[derive(Clone)]
pub struct ManagerSettings {
    pub mysql: MySqlSettings,
    pub grpc_bind: SocketAddr,
    pub rpc_token: Arc<SecretString>,
    pub lease_signing_key: Arc<SecretString>,
    pub endpoint_keyring: CredentialKeyring,
    pub credential_source: CredentialSource,
    pub adapter: OpenSandboxAdapter,
    pub reaper_interval: Duration,
    pub max_active_per_tenant: u64,
}

#[derive(Clone)]
pub struct OpenSandboxSettings {
    pub adapter: OpenSandboxAdapter,
}

impl OpenSandboxSettings {
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var("AGENTX_OPENSANDBOX_ENDPOINT")
            .context("AGENTX_OPENSANDBOX_ENDPOINT is required")?;
        let api_key = SecretString::from(
            env::var("AGENTX_OPENSANDBOX_API_KEY")
                .context("AGENTX_OPENSANDBOX_API_KEY is required")?,
        );
        let allowed_hosts = comma_list("AGENTX_OPENSANDBOX_ALLOWED_ENDPOINT_HOSTS");
        let allowed_cidrs = comma_list("AGENTX_OPENSANDBOX_ALLOWED_ENDPOINT_CIDRS");
        let secure_access = boolean("AGENTX_OPENSANDBOX_SECURE_ACCESS", true)?;
        let use_server_proxy = boolean("AGENTX_OPENSANDBOX_USE_SERVER_PROXY", true)?;
        let adapter = OpenSandboxAdapter::with_policy(
            Url::parse(&endpoint).context("AGENTX_OPENSANDBOX_ENDPOINT must be a URL")?,
            api_key,
            allowed_hosts,
            allowed_cidrs,
        )
        .map_err(anyhow::Error::new)?
        .with_secure_access(secure_access)
        .with_server_proxy(use_server_proxy);
        Ok(Self { adapter })
    }
}

impl ManagerSettings {
    pub fn from_env() -> Result<Self> {
        let opensandbox = OpenSandboxSettings::from_env()?;
        let endpoint_keyring = CredentialKeyring::from_json(
            env::var("AGENTX_SANDBOX_ENDPOINT_ACTIVE_KEY_ID")
                .or_else(|_| env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID"))
                .context("sandbox endpoint encryption active key is required")?,
            &SecretString::from(
                env::var("AGENTX_SANDBOX_ENDPOINT_KEYS_JSON")
                    .or_else(|_| env::var("AGENTX_CREDENTIAL_KEYS_JSON"))
                    .context("sandbox endpoint encryption keyring is required")?,
            ),
        )?;
        let credential_source = match secret_provider_mode()? {
            SecretProviderMode::VaultKvV2 => {
                CredentialSource::external(Arc::new(RemoteSecretProvider::from_env()?))
            }
            SecretProviderMode::LocalEncrypted => {
                CredentialSource::local(Arc::new(CredentialKeyring::from_json(
                    env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")
                        .context("AGENTX_CREDENTIAL_ACTIVE_KEY_ID is required")?,
                    &SecretString::from(
                        env::var("AGENTX_CREDENTIAL_KEYS_JSON")
                            .context("AGENTX_CREDENTIAL_KEYS_JSON is required")?,
                    ),
                )?))
            }
        };
        let max_active_per_tenant = env::var("AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .context("AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT must be an integer")
            })
            .transpose()?
            .unwrap_or(16);
        anyhow::ensure!(
            (1..=10_000).contains(&max_active_per_tenant),
            "AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT must be between 1 and 10000"
        );
        Ok(Self {
            mysql: MySqlSettings::from_env()?,
            grpc_bind: env::var("AGENTX_SANDBOX_GRPC_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:9091".into())
                .parse()
                .context("AGENTX_SANDBOX_GRPC_BIND_ADDR must be a socket address")?,
            rpc_token: Arc::new(SecretString::from(
                env::var("AGENTX_SANDBOX_RPC_TOKEN")
                    .context("AGENTX_SANDBOX_RPC_TOKEN is required")?,
            )),
            lease_signing_key: Arc::new(SecretString::from(
                env::var("AGENTX_SANDBOX_LEASE_SIGNING_KEY")
                    .context("AGENTX_SANDBOX_LEASE_SIGNING_KEY is required")?,
            )),
            endpoint_keyring,
            credential_source,
            adapter: opensandbox.adapter,
            reaper_interval: Duration::from_secs(
                env::var("AGENTX_SANDBOX_REAPER_INTERVAL_SECONDS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(15),
            ),
            max_active_per_tenant,
        })
    }
}

fn comma_list(name: &str) -> Vec<String> {
    env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn boolean(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Ok(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        Ok(_) => anyhow::bail!("{name} must be true or false"),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
