use std::collections::HashMap;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Context, Result, bail};
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

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use secrecy::SecretString;

    use super::{CredentialKeyring, PlainSecret};

    #[test]
    fn encrypts_with_random_nonce_and_authenticates_context() {
        let json = format!(r#"{{"keys":{{"v1":"{}"}}}}"#, STANDARD.encode([7_u8; 32]));
        let keyring =
            CredentialKeyring::from_json("v1".to_owned(), &SecretString::from(json)).unwrap();
        let first = keyring
            .encrypt(&PlainSecret::new(b"secret".to_vec()), b"tenant/a/1")
            .unwrap();
        let second = keyring
            .encrypt(&PlainSecret::new(b"secret".to_vec()), b"tenant/a/1")
            .unwrap();
        assert_ne!(first.nonce, second.nonce);
        assert_eq!(
            keyring
                .decrypt(
                    &first.key_id,
                    &first.nonce,
                    &first.ciphertext,
                    b"tenant/a/1"
                )
                .unwrap()
                .expose(),
            b"secret"
        );
        assert!(
            keyring
                .decrypt(
                    &first.key_id,
                    &first.nonce,
                    &first.ciphertext,
                    b"tenant/a/2"
                )
                .is_err()
        );
    }
}
