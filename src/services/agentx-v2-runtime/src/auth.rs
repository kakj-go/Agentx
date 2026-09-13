use std::{
    collections::{BTreeSet, HashMap},
    env,
};

use agentx_runtime_contracts::{
    ControlRole, DelegationClaimsV1, ServiceClaimsV1, UserAccessClaimsV1, verify_delegation_token,
    verify_service_token, verify_user_access_token,
};
use anyhow::{Context, Result};
use axum::http::HeaderMap;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::VerifyingKey;
use uuid::Uuid;

use crate::error::{RuntimeError, RuntimeResult};

pub struct RuntimeTrust {
    issuer: String,
    audience: String,
    keys: HashMap<String, Vec<u8>>,
    bundle_keys: HashMap<String, VerifyingKey>,
    work_package_keys: HashMap<String, VerifyingKey>,
    user_issuer: String,
    user_audience: String,
    user_keys: HashMap<String, Vec<u8>>,
}

impl RuntimeTrust {
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_user_keys(true)
    }

    pub fn from_env_without_user_keys() -> Result<Self> {
        Self::from_env_with_user_keys(false)
    }

    fn from_env_with_user_keys(require_user_keys: bool) -> Result<Self> {
        let keys = serde_json::from_str::<HashMap<String, String>>(
            &env::var("AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON")
                .context("AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON is required")?,
        )?
        .into_iter()
        .map(|(kid, pem)| (kid, pem.into_bytes()))
        .collect();
        let bundle_keys = serde_json::from_str::<HashMap<String, String>>(
            &env::var("AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON")
                .context("AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON is required")?,
        )?
        .into_iter()
        .map(|(kid, encoded)| {
            let bytes = STANDARD
                .decode(encoded)
                .context("Bundle public key is not base64")?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Bundle public key must be 32 bytes"))?;
            Ok((kid, VerifyingKey::from_bytes(&bytes)?))
        })
        .collect::<Result<HashMap<_, _>>>()?;
        let work_package_keys = serde_json::from_str::<HashMap<String, String>>(
            &env::var("AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON")
                .context("AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON is required")?,
        )?
        .into_iter()
        .map(|(kid, encoded)| {
            let bytes = STANDARD
                .decode(encoded)
                .context("Work Package public key is not base64")?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Work Package public key must be 32 bytes"))?;
            Ok((kid, VerifyingKey::from_bytes(&bytes)?))
        })
        .collect::<Result<HashMap<_, _>>>()?;
        let user_keys = match env::var("AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON") {
            Ok(value) => serde_json::from_str::<HashMap<String, String>>(&value)?
                .into_iter()
                .map(|(kid, pem)| (kid, pem.into_bytes()))
                .collect(),
            Err(error) if require_user_keys => {
                return Err(error).context("AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON is required");
            }
            Err(_) => HashMap::new(),
        };
        Ok(Self {
            issuer: env::var("AGENTX_RUNTIME_SERVICE_JWT_ISSUER")
                .unwrap_or_else(|_| "agentx-control".into()),
            audience: env::var("AGENTX_RUNTIME_SERVICE_JWT_AUDIENCE")
                .unwrap_or_else(|_| "agentx-runtime-internal".into()),
            keys,
            bundle_keys,
            work_package_keys,
            user_issuer: env::var("AGENTX_RUNTIME_USER_JWT_ISSUER")
                .unwrap_or_else(|_| "agentx-platform".into()),
            user_audience: env::var("AGENTX_RUNTIME_USER_JWT_AUDIENCE")
                .unwrap_or_else(|_| "agentx-runtime-gateway".into()),
            user_keys,
        })
    }

    pub fn new(issuer: &str, audience: &str, keys: HashMap<String, Vec<u8>>) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            keys,
            bundle_keys: HashMap::new(),
            work_package_keys: HashMap::new(),
            user_issuer: issuer.into(),
            user_audience: audience.into(),
            user_keys: HashMap::new(),
        }
    }

    pub fn with_bundle_key(mut self, kid: &str, key: VerifyingKey) -> Self {
        self.bundle_keys.insert(kid.into(), key);
        self
    }

    pub fn with_work_package_key(mut self, kid: &str, key: VerifyingKey) -> Self {
        self.work_package_keys.insert(kid.into(), key);
        self
    }

    pub fn with_user_key(mut self, kid: &str, key: Vec<u8>) -> Self {
        self.user_keys.insert(kid.into(), key);
        self
    }

    pub fn user(&self, token: &str) -> RuntimeResult<UserAccessClaimsV1> {
        verify_user_access_token(
            token,
            &self.user_keys,
            &self.user_issuer,
            &self.user_audience,
        )
        .map_err(|error| {
            tracing::warn!(%error, "Runtime user JWT rejected");
            RuntimeError::Unauthorized
        })
    }

    pub fn bundle_key(&self, kid: &str) -> RuntimeResult<&VerifyingKey> {
        self.bundle_keys.get(kid).ok_or_else(|| {
            RuntimeError::BadRequest(
                agentx_runtime_contracts::RuntimePublishErrorCodeV1::InvalidSignature,
                "Bundle signature key is not trusted".into(),
            )
        })
    }

    pub fn work_package_key(&self, kid: &str) -> RuntimeResult<&VerifyingKey> {
        self.work_package_keys.get(kid).ok_or_else(|| {
            RuntimeError::BadRequest(
                agentx_runtime_contracts::RuntimePublishErrorCodeV1::InvalidSignature,
                "Work Package signature key is not trusted".into(),
            )
        })
    }

    pub fn publisher(&self, headers: &HeaderMap, scope: &str) -> RuntimeResult<ServiceClaimsV1> {
        let token = bearer(headers)?;
        verify_service_token(
            token,
            &self.keys,
            &self.issuer,
            &self.audience,
            &BTreeSet::from([ControlRole::Publisher]),
            &BTreeSet::from([scope.to_owned()]),
        )
        .map_err(|_| RuntimeError::Unauthorized)
    }

    pub fn projector(&self, headers: &HeaderMap, scope: &str) -> RuntimeResult<ServiceClaimsV1> {
        let token = bearer(headers)?;
        verify_service_token(
            token,
            &self.keys,
            &self.issuer,
            &self.audience,
            &BTreeSet::from([ControlRole::Projector]),
            &BTreeSet::from([scope.to_owned()]),
        )
        .map_err(|error| {
            tracing::warn!(%error, %scope, "Projector Service JWT rejected");
            RuntimeError::Unauthorized
        })
    }

    pub fn delegation(
        &self,
        headers: &HeaderMap,
        tenant_id: Uuid,
        scope: &str,
    ) -> RuntimeResult<DelegationClaimsV1> {
        verify_delegation_token(
            bearer(headers)?,
            &self.keys,
            &self.issuer,
            &self.audience,
            tenant_id,
            &BTreeSet::from([scope.to_owned()]),
        )
        .map_err(|error| {
            tracing::warn!(%error, %tenant_id, %scope, "Delegation JWT rejected");
            RuntimeError::Unauthorized
        })
    }
}

fn bearer(headers: &HeaderMap) -> RuntimeResult<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(RuntimeError::Unauthorized)
}
