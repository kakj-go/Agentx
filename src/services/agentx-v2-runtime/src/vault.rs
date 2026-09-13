use std::{env, time::Duration};

use agentx_runtime_contracts::VaultSecretReferenceV1;
use anyhow::{Context, Result};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Clone)]
pub struct RuntimeVault {
    endpoint: String,
    token: SecretString,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct VaultResponse {
    data: VaultData,
}

#[derive(Deserialize)]
struct VaultData {
    data: serde_json::Map<String, serde_json::Value>,
}

impl RuntimeVault {
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var("AGENTX_RUNTIME_VAULT_ENDPOINT")
            .context("AGENTX_RUNTIME_VAULT_ENDPOINT is required")?;
        let token = SecretString::from(
            env::var("AGENTX_RUNTIME_VAULT_TOKEN")
                .context("AGENTX_RUNTIME_VAULT_TOKEN is required")?,
        );
        let client =
            agentx_service_kit::reqwest_client_builder_with_ca("AGENTX_RUNTIME_VAULT_TLS_CA_PATH")?
                .timeout(Duration::from_secs(5))
                .build()?;
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            token,
            client,
        })
    }

    pub async fn read(&self, reference: &VaultSecretReferenceV1) -> RuntimeResult<Vec<u8>> {
        let url = format!(
            "{}/v1/{}/data/{}?version={}",
            self.endpoint,
            reference.mount.trim_matches('/'),
            reference.path.trim_matches('/'),
            reference.version
        );
        let response = self
            .client
            .get(url)
            .header("X-Vault-Token", self.token.expose_secret())
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, "Runtime Vault request failed");
                RuntimeError::SecretUnavailable
            })?;
        if response.status() != StatusCode::OK {
            tracing::warn!(status=%response.status(), "Runtime Vault secret is unavailable");
            return Err(RuntimeError::SecretUnavailable);
        }
        let payload = response
            .json::<VaultResponse>()
            .await
            .map_err(|_| RuntimeError::SecretUnavailable)?;
        let value = payload
            .data
            .data
            .get(&reference.key)
            .ok_or(RuntimeError::SecretUnavailable)?;
        match value {
            serde_json::Value::String(value) => Ok(value.as_bytes().to_vec()),
            value => serde_json::to_vec(value).map_err(|_| RuntimeError::SecretUnavailable),
        }
    }
}
