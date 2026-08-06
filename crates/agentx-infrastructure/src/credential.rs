use std::{collections::HashMap, env, sync::Arc};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use agentx_application::{CredentialResolver, ResolvedCredential, SecretMaterial};
use agentx_domain::TenantId;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::{MySqlPool, Row};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
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

    async fn read_scoped(
        &self,
        secret_ref: &str,
        version: Option<u64>,
        _scope: &SecretReadScope,
    ) -> Result<PlainSecret> {
        self.read(secret_ref, version).await
    }

    async fn write(&self, secret_ref: &str, value: &PlainSecret) -> Result<u64>;

    async fn destroy(&self, secret_ref: &str, version: u64) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct SecretReadScope {
    pub tenant_id: Uuid,
    pub credential_id: Uuid,
    pub credential_version: u64,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub lease_token: Uuid,
    pub deadline: OffsetDateTime,
    pub handle: Option<String>,
    pub handle_id: Option<Uuid>,
    /// When false, the caller owns the Handle lock and consumes it after the
    /// secret-dependent operation succeeds.
    pub consume_handle: bool,
}

#[derive(Clone)]
pub struct VaultSecretProvider {
    client: reqwest::Client,
    address: String,
    mount: String,
    token: SecretString,
}

#[derive(Clone)]
pub struct RemoteSecretProvider {
    client: reqwest::Client,
    endpoint: String,
    token: SecretString,
    cache: Arc<std::sync::Mutex<HashMap<String, CachedSecret>>>,
}

struct CachedSecret {
    value: PlainSecret,
    expires_at: OffsetDateTime,
}

impl RemoteSecretProvider {
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var("AGENTX_CREDENTIAL_BROKER_URL")
            .map_err(|_| anyhow::anyhow!("AGENTX_CREDENTIAL_BROKER_URL is required"))?;
        let token = env::var("AGENTX_CREDENTIAL_BROKER_TOKEN")
            .map_err(|_| anyhow::anyhow!("AGENTX_CREDENTIAL_BROKER_TOKEN is required"))?;
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            token: SecretString::from(token),
            cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        })
    }

    fn cache_key(scope: &SecretReadScope) -> Result<String> {
        match (&scope.handle, scope.handle_id) {
            (Some(handle), None) => Ok(format!("token:{handle}")),
            (None, Some(handle_id)) => Ok(format!("id:{handle_id}")),
            _ => bail!("credential scope must select exactly one Handle"),
        }
    }
}

#[async_trait]
impl SecretProvider for RemoteSecretProvider {
    async fn read(&self, _secret_ref: &str, _version: Option<u64>) -> Result<PlainSecret> {
        bail!("credential broker requires a Runtime Attempt scope")
    }

    async fn read_scoped(
        &self,
        secret_ref: &str,
        version: Option<u64>,
        scope: &SecretReadScope,
    ) -> Result<PlainSecret> {
        let now = OffsetDateTime::now_utc();
        let cache_key = Self::cache_key(scope)?;
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| anyhow::anyhow!("credential cache lock is poisoned"))?;
            cache.retain(|_, cached| cached.expires_at > now);
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(PlainSecret::new(cached.value.expose().to_vec()));
            }
        }
        let response = self
            .client
            .post(format!("{}/internal/v1/credentials/resolve", self.endpoint))
            .header(
                "X-Agentx-Credential-Broker-Token",
                self.token.expose_secret(),
            )
            .json(&serde_json::json!({
                "secretRef": secret_ref,
                "version": version,
                "tenantId": scope.tenant_id,
                "credentialId": scope.credential_id,
                "credentialVersion": scope.credential_version,
                "executionId": scope.execution_id,
                "nodeExecutionId": scope.node_execution_id,
                "attemptId": scope.attempt_id,
                "leaseToken": scope.lease_token,
                "deadline": scope.deadline.format(&Rfc3339)?,
                "handle": scope.handle,
                "handleId": scope.handle_id,
                "consumeHandle": scope.consume_handle,
            }))
            .send()
            .await
            .context("credential broker request failed")?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .context("credential broker response is invalid JSON")?;
        if !status.is_success() {
            bail!("credential broker failed with HTTP {status}");
        }
        let encoded = body
            .get("secretBase64")
            .and_then(serde_json::Value::as_str)
            .context("credential broker response does not contain secretBase64")?;
        let value = STANDARD.decode(encoded)?;
        self.cache
            .lock()
            .map_err(|_| anyhow::anyhow!("credential cache lock is poisoned"))?
            .insert(
                cache_key,
                CachedSecret {
                    value: PlainSecret::new(value.clone()),
                    expires_at: scope.deadline,
                },
            );
        Ok(PlainSecret::new(value))
    }

    async fn write(&self, _secret_ref: &str, _value: &PlainSecret) -> Result<u64> {
        bail!("worker credential broker is read-only")
    }

    async fn destroy(&self, _secret_ref: &str, _version: u64) -> Result<()> {
        bail!("worker credential broker is read-only")
    }
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
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            address: address.trim_end_matches('/').to_owned(),
            mount,
            token: SecretString::from(token),
        })
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
        let bytes = STANDARD
            .decode(encoded)
            .context("Vault secret value is not base64")?;
        Ok(PlainSecret::new(bytes))
    }

    async fn write(&self, secret_ref: &str, value: &PlainSecret) -> Result<u64> {
        let response = self
            .client
            .post(self.data_url(secret_ref)?)
            .header("X-Vault-Token", self.token.expose_secret())
            .json(&serde_json::json!({
                "data": { "value": STANDARD.encode(value.expose()) }
            }))
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
            .json(&serde_json::json!({ "versions": [version] }))
            .send()
            .await
            .context("Vault destroy request failed")?;
        if !response.status().is_success() {
            bail!("Vault destroy failed with HTTP {}", response.status());
        }
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

#[derive(Clone)]
pub enum CredentialSource {
    Local(Arc<CredentialKeyring>),
    External(Arc<dyn SecretProvider>),
}

pub struct CredentialSecretRecord<'a> {
    pub provider: &'a str,
    pub secret_ref: Option<&'a str>,
    pub provider_version: Option<&'a str>,
    pub key_id: Option<&'a str>,
    pub nonce: Option<&'a [u8]>,
    pub ciphertext: Option<&'a [u8]>,
    pub aad: &'a [u8],
}

impl CredentialSource {
    #[must_use]
    pub fn local(keyring: Arc<CredentialKeyring>) -> Self {
        Self::Local(keyring)
    }

    #[must_use]
    pub fn external(provider: Arc<dyn SecretProvider>) -> Self {
        Self::External(provider)
    }

    pub async fn resolve(
        &self,
        secret: CredentialSecretRecord<'_>,
        scope: Option<&SecretReadScope>,
    ) -> Result<PlainSecret> {
        match (secret.provider, self) {
            ("local_encrypted", Self::Local(keyring)) => keyring.decrypt(
                secret
                    .key_id
                    .context("local credential key id is missing")?,
                secret.nonce.context("local credential nonce is missing")?,
                secret
                    .ciphertext
                    .context("local credential ciphertext is missing")?,
                secret.aad,
            ),
            ("local_encrypted", Self::External(_)) => {
                bail!("local encrypted credential is unavailable in external-only mode")
            }
            (external, Self::External(provider_impl)) if external != "local_encrypted" => {
                let version = secret
                    .provider_version
                    .and_then(|value| value.parse::<u64>().ok());
                let secret_ref = secret
                    .secret_ref
                    .context("external credential secret reference is missing")?;
                match scope {
                    Some(scope) => provider_impl.read_scoped(secret_ref, version, scope).await,
                    None => provider_impl.read(secret_ref, version).await,
                }
            }
            (external, Self::Local(_)) if external != "local_encrypted" => {
                bail!("external credential provider {external} is unavailable")
            }
            _ => bail!("unsupported credential provider {}", secret.provider),
        }
    }
}

#[derive(Clone)]
pub struct MySqlCredentialResolver {
    pool: MySqlPool,
    source: CredentialSource,
}

impl MySqlCredentialResolver {
    #[must_use]
    pub fn new(pool: MySqlPool, keyring: CredentialKeyring) -> Self {
        Self {
            pool,
            source: CredentialSource::local(Arc::new(keyring)),
        }
    }

    #[must_use]
    pub fn new_with_source(pool: MySqlPool, source: CredentialSource) -> Self {
        Self { pool, source }
    }

    async fn load(
        &self,
        tenant_id: TenantId,
        credential_id: Uuid,
        version: Option<u64>,
        context: Option<&agentx_application::RuntimeContext>,
    ) -> Result<ResolvedCredential> {
        let row = if let Some(version) = version {
            sqlx::query("SELECT c.credential_type,v.version_number,v.provider,v.secret_ref,v.provider_version,v.key_id,v.nonce,v.ciphertext FROM credentials c JOIN credential_secret_versions v ON v.credential_id=c.id AND v.tenant_id=c.tenant_id WHERE c.tenant_id=? AND c.id=? AND c.status='active' AND v.version_number=?")
                .bind(tenant_id.as_uuid()).bind(credential_id).bind(version).fetch_optional(&self.pool).await?
        } else {
            sqlx::query("SELECT c.credential_type,v.version_number,v.provider,v.secret_ref,v.provider_version,v.key_id,v.nonce,v.ciphertext FROM credentials c JOIN credential_secret_versions v ON v.credential_id=c.id AND v.tenant_id=c.tenant_id AND v.version_number=c.current_secret_version WHERE c.tenant_id=? AND c.id=? AND c.status='active'")
                .bind(tenant_id.as_uuid()).bind(credential_id).fetch_optional(&self.pool).await?
        }.context("Credential version is missing or disabled")?;
        let version_number: u64 = row.try_get("version_number")?;
        let aad = format!("{}/{credential_id}/{version_number}", tenant_id.as_uuid());
        let provider: String = row.try_get("provider")?;
        let secret_ref: Option<String> = row.try_get("secret_ref")?;
        let provider_version: Option<String> = row.try_get("provider_version")?;
        let key_id: Option<String> = row.try_get("key_id")?;
        let nonce: Option<Vec<u8>> = row.try_get("nonce")?;
        let ciphertext: Option<Vec<u8>> = row.try_get("ciphertext")?;
        let scope = match (&self.source, context) {
            (CredentialSource::External(_), Some(context)) => Some({
                let handle = context
                    .credential_handles
                    .get(&credential_id)
                    .context("Runtime Credential Handle is missing")?;
                if version.is_some_and(|version| version != handle.version) {
                    bail!("Runtime Credential Handle version does not match the snapshot");
                }
                SecretReadScope {
                    tenant_id: context.tenant_id.as_uuid(),
                    credential_id,
                    credential_version: handle.version,
                    execution_id: context.execution_id.as_uuid(),
                    node_execution_id: context.node_execution_id.as_uuid(),
                    attempt_id: context.attempt_id.as_uuid(),
                    lease_token: context.lease_token,
                    deadline: handle.expires_at.min(context.deadline),
                    handle: Some(handle.handle.clone()),
                    handle_id: None,
                    consume_handle: true,
                }
            }),
            _ => None,
        };
        let plaintext = self
            .source
            .resolve(
                CredentialSecretRecord {
                    provider: &provider,
                    secret_ref: secret_ref.as_deref(),
                    provider_version: provider_version.as_deref(),
                    key_id: key_id.as_deref(),
                    nonce: nonce.as_deref(),
                    ciphertext: ciphertext.as_deref(),
                    aad: aad.as_bytes(),
                },
                scope.as_ref(),
            )
            .await?;
        Ok(ResolvedCredential {
            credential_type: row.try_get("credential_type")?,
            secret: SecretMaterial::new(plaintext.expose().to_vec()),
        })
    }
}

#[async_trait]
impl CredentialResolver for MySqlCredentialResolver {
    async fn resolve(
        &self,
        tenant_id: TenantId,
        credential_id: Uuid,
    ) -> Result<ResolvedCredential> {
        self.load(tenant_id, credential_id, None, None).await
    }

    async fn resolve_version(
        &self,
        tenant_id: TenantId,
        credential_id: Uuid,
        version: u64,
    ) -> Result<ResolvedCredential> {
        self.load(tenant_id, credential_id, Some(version), None)
            .await
    }

    async fn resolve_for_runtime(
        &self,
        context: &agentx_application::RuntimeContext,
        credential_id: Uuid,
        version: Option<u64>,
    ) -> Result<ResolvedCredential> {
        self.load(context.tenant_id, credential_id, version, Some(context))
            .await
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::{
        CredentialKeyring, CredentialSource, PlainSecret, SecretProvider, VaultSecretProvider,
    };

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

    #[tokio::test]
    async fn external_provider_failure_does_not_fall_back_to_local_ciphertext() {
        let provider = VaultSecretProvider::new(
            reqwest::Client::new(),
            "http://127.0.0.1:1",
            "secret",
            SecretString::from("unreachable-vault-token"),
        );
        let source = CredentialSource::external(std::sync::Arc::new(provider));
        let result = source
            .resolve(
                super::CredentialSecretRecord {
                    provider: "vault_kv_v2",
                    secret_ref: Some("agentx/fail-closed"),
                    provider_version: Some("1"),
                    key_id: Some("local-key-that-must-not-be-used"),
                    nonce: Some(&[0_u8; 12]),
                    ciphertext: Some(b"local-fallback-must-not-be-used"),
                    aad: b"tenant/credential/1",
                },
                None,
            )
            .await;

        let error = result.err().expect("unreachable Vault must fail closed");
        assert!(error.to_string().contains("Vault read request failed"));
    }

    #[tokio::test]
    #[ignore = "requires a real Vault KV v2 endpoint"]
    async fn vault_kv_v2_rotates_reads_versions_and_destroys_old_material() {
        let provider = VaultSecretProvider::from_env().expect("Vault test configuration");
        let secret_ref = format!("agentx/integration/{}", Uuid::now_v7());
        let first = PlainSecret::new(b"agentx-vault-version-one".to_vec());
        let second = PlainSecret::new(b"agentx-vault-version-two".to_vec());

        let first_version = provider.write(&secret_ref, &first).await.expect("write v1");
        let second_version = provider
            .write(&secret_ref, &second)
            .await
            .expect("write v2");
        assert_eq!(first_version, 1);
        assert_eq!(second_version, 2);
        assert_eq!(
            provider
                .read(&secret_ref, Some(first_version))
                .await
                .expect("read v1")
                .expose(),
            first.expose()
        );
        assert_eq!(
            provider
                .read(&secret_ref, Some(second_version))
                .await
                .expect("read v2")
                .expose(),
            second.expose()
        );

        provider
            .destroy(&secret_ref, first_version)
            .await
            .expect("destroy v1");
        assert!(
            provider
                .read(&secret_ref, Some(first_version))
                .await
                .is_err()
        );
        assert_eq!(
            provider
                .read(&secret_ref, Some(second_version))
                .await
                .expect("v2 remains readable")
                .expose(),
            second.expose()
        );
    }
}
