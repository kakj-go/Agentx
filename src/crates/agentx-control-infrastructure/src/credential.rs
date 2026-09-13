use std::{collections::HashMap, env};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use zeroize::Zeroize;

pub struct PlainSecret(Vec<u8>);

impl PlainSecret {
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for PlainSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct EncryptedSecret {
    pub algorithm: &'static str,
    pub key_id: String,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

#[derive(Clone)]
pub struct CredentialKeyring {
    active_key_id: String,
    keys: HashMap<String, [u8; 32]>,
}

#[derive(Deserialize)]
struct KeyringDocument {
    keys: HashMap<String, String>,
}

impl CredentialKeyring {
    pub fn from_json(active_key_id: String, value: &SecretString) -> Result<Self> {
        if active_key_id.trim().is_empty() {
            bail!("AGENTX_CREDENTIAL_ACTIVE_KEY_ID cannot be empty");
        }
        let document: KeyringDocument = serde_json::from_str(value.expose_secret())
            .context("AGENTX_CREDENTIAL_KEYS_JSON must be a JSON object containing keys")?;
        let mut keys = HashMap::new();
        for (key_id, encoded) in document.keys {
            let decoded = STANDARD
                .decode(encoded)
                .with_context(|| format!("credential key {key_id} is not valid base64"))?;
            let key: [u8; 32] = decoded
                .try_into()
                .map_err(|_| anyhow::anyhow!("credential key {key_id} must decode to 32 bytes"))?;
            keys.insert(key_id, key);
        }
        if !keys.contains_key(&active_key_id) {
            bail!("active credential key id does not exist in keyring");
        }
        Ok(Self {
            active_key_id,
            keys,
        })
    }

    pub fn encrypt(&self, plaintext: &PlainSecret, aad: &[u8]) -> Result<EncryptedSecret> {
        let key = self
            .keys
            .get(&self.active_key_id)
            .context("active key missing")?;
        let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256 key length was validated");
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.expose(),
                    aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
        Ok(EncryptedSecret {
            algorithm: "AES-256-GCM",
            key_id: self.active_key_id.clone(),
            nonce,
            ciphertext,
        })
    }

    pub fn decrypt(
        &self,
        key_id: &str,
        nonce: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<PlainSecret> {
        let key = self
            .keys
            .get(key_id)
            .context("credential key is not available")?;
        if nonce.len() != 12 {
            bail!("credential nonce length is invalid");
        }
        let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256 key length was validated");
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("credential decryption failed"))?;
        Ok(PlainSecret::new(plaintext))
    }
}

#[async_trait]
pub trait SecretProvider: Send + Sync {
    async fn read(&self, secret_ref: &str, version: Option<u64>) -> Result<PlainSecret>;
    async fn write(&self, secret_ref: &str, value: &PlainSecret) -> Result<u64>;
    async fn destroy(&self, secret_ref: &str, version: u64) -> Result<()>;
}

#[derive(Clone)]
pub struct VaultSecretProvider {
    client: reqwest::Client,
    address: String,
    mount: String,
    token: SecretString,
}

impl VaultSecretProvider {
    pub fn from_env() -> Result<Self> {
        let address = env::var("AGENTX_VAULT_ADDR")
            .map_err(|_| anyhow::anyhow!("AGENTX_VAULT_ADDR is required for Vault"))?;
        let token = env::var("AGENTX_VAULT_TOKEN")
            .map_err(|_| anyhow::anyhow!("AGENTX_VAULT_TOKEN is required for Vault"))?;
        let mount = env::var("AGENTX_VAULT_KV_MOUNT").unwrap_or_else(|_| "secret".into());
        anyhow::ensure!(!address.trim().is_empty(), "Vault address cannot be empty");
        anyhow::ensure!(!mount.contains('/'), "Vault KV mount cannot contain slash");
        Ok(Self::new(
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            address,
            mount,
            SecretString::from(token),
        ))
    }

    #[must_use]
    pub fn new(
        client: reqwest::Client,
        address: impl Into<String>,
        mount: impl Into<String>,
        token: SecretString,
    ) -> Self {
        Self {
            client,
            address: address.into().trim_end_matches('/').to_owned(),
            mount: mount.into(),
            token,
        }
    }

    fn data_url(&self, secret_ref: &str) -> Result<String> {
        validate_secret_ref(secret_ref)?;
        Ok(format!(
            "{}/v1/{}/data/{}",
            self.address,
            self.mount,
            secret_ref.trim_start_matches('/')
        ))
    }

    fn destroy_url(&self, secret_ref: &str) -> Result<String> {
        validate_secret_ref(secret_ref)?;
        Ok(format!(
            "{}/v1/{}/destroy/{}",
            self.address,
            self.mount,
            secret_ref.trim_start_matches('/')
        ))
    }
}

#[async_trait]
impl SecretProvider for VaultSecretProvider {
    async fn read(&self, secret_ref: &str, version: Option<u64>) -> Result<PlainSecret> {
        let mut request = self
            .client
            .get(self.data_url(secret_ref)?)
            .header("X-Vault-Token", self.token.expose_secret());
        if let Some(version) = version {
            request = request.query(&[("version", version)]);
        }
        let response = request.send().await.context("Vault read request failed")?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .context("Vault read response is invalid JSON")?;
        if !status.is_success() {
            bail!("Vault read failed with HTTP {status}");
        }
        let encoded = body
            .get("data")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("value"))
            .and_then(serde_json::Value::as_str)
            .context("Vault response does not contain data.value")?;
        Ok(PlainSecret::new(
            STANDARD
                .decode(encoded)
                .context("Vault secret value is not base64")?,
        ))
    }

    async fn write(&self, secret_ref: &str, value: &PlainSecret) -> Result<u64> {
        let response = self
            .client
            .post(self.data_url(secret_ref)?)
            .header("X-Vault-Token", self.token.expose_secret())
            .json(&serde_json::json!({"data": {"value": STANDARD.encode(value.expose())}}))
            .send()
            .await
            .context("Vault write request failed")?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .context("Vault write response is invalid JSON")?;
        if !status.is_success() {
            bail!("Vault write failed with HTTP {status}");
        }
        body.get("data")
            .and_then(|value| value.get("version"))
            .and_then(serde_json::Value::as_u64)
            .context("Vault write response does not contain data.version")
    }

    async fn destroy(&self, secret_ref: &str, version: u64) -> Result<()> {
        let response = self
            .client
            .post(self.destroy_url(secret_ref)?)
            .header("X-Vault-Token", self.token.expose_secret())
            .json(&serde_json::json!({"versions": [version]}))
            .send()
            .await
            .context("Vault destroy request failed")?;
        anyhow::ensure!(
            response.status().is_success(),
            "Vault destroy failed with HTTP {}",
            response.status()
        );
        Ok(())
    }
}

fn validate_secret_ref(value: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "Vault secret reference is empty");
    anyhow::ensure!(
        !value.contains(".."),
        "Vault secret reference cannot contain '..'"
    );
    anyhow::ensure!(
        value
            .chars()
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, '/' | '-' | '_' | '.')),
        "Vault secret reference contains an invalid character"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CredentialKeyring, PlainSecret};
    use secrecy::SecretString;

    #[test]
    fn keyring_round_trip_is_aad_bound() {
        let keyring = CredentialKeyring::from_json(
            "v1".into(),
            &SecretString::from(format!(
                r#"{{"keys":{{"v1":"{}"}}}}"#,
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [7_u8; 32])
            )),
        )
        .unwrap();
        let encrypted = keyring
            .encrypt(&PlainSecret::new(b"secret".to_vec()), b"tenant/1")
            .unwrap();
        assert!(
            keyring
                .decrypt(
                    &encrypted.key_id,
                    &encrypted.nonce,
                    &encrypted.ciphertext,
                    b"tenant/2"
                )
                .is_err()
        );
        assert_eq!(
            keyring
                .decrypt(
                    &encrypted.key_id,
                    &encrypted.nonce,
                    &encrypted.ciphertext,
                    b"tenant/1"
                )
                .unwrap()
                .expose(),
            b"secret"
        );
    }
}
