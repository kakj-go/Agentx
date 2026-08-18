use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("contract serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("contract canonicalization failed: {0}")]
    Canonicalization(String),
    #[error("invalid content hash: {0}")]
    InvalidContentHash(String),
    #[error("content hash does not match the immutable payload")]
    ContentHashMismatch,
    #[error("invalid Ed25519 signature")]
    InvalidSignature,
    #[error("invalid Runtime policy: {0}")]
    InvalidRuntimePolicy(&'static str),
    #[error("Work Package expiry must be later than creation time")]
    InvalidWorkPackageTtl,
    #[error("contract contains a non-canonical or cross-tenant immutable reference")]
    InvalidImmutableReference,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        let hex = value.strip_prefix("sha256:").ok_or_else(|| {
            ContractError::InvalidContentHash("hash must start with sha256:".into())
        })?;
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ContractError::InvalidContentHash(
                "digest must be 64 lowercase hexadecimal characters".into(),
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ContractError> {
    serde_jcs::to_vec(value).map_err(|error| ContractError::Canonicalization(error.to_string()))
}

pub fn content_hash<T: Serialize>(value: &T) -> Result<ContentHash, ContractError> {
    let digest = Sha256::digest(canonical_bytes(value)?);
    ContentHash::parse(format!("sha256:{digest:x}"))
}

#[must_use]
pub fn deterministic_uuid(namespace: Uuid, label: &[u8]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(label);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ed25519Signature {
    pub key_id: String,
    pub algorithm: SignatureAlgorithm,
    pub signature_base64: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithm {
    Ed25519,
}

pub fn sign_canonical<T: Serialize>(
    key_id: impl Into<String>,
    key: &SigningKey,
    value: &T,
) -> Result<Ed25519Signature, ContractError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let signature = key.sign(&canonical_bytes(value)?);
    Ok(Ed25519Signature {
        key_id: key_id.into(),
        algorithm: SignatureAlgorithm::Ed25519,
        signature_base64: STANDARD.encode(signature.to_bytes()),
    })
}

pub fn verify_canonical<T: Serialize>(
    key: &VerifyingKey,
    value: &T,
    signature: &Ed25519Signature,
) -> Result<(), ContractError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let bytes = STANDARD
        .decode(&signature.signature_base64)
        .map_err(|_| ContractError::InvalidSignature)?;
    let signature = Signature::from_slice(&bytes).map_err(|_| ContractError::InvalidSignature)?;
    key.verify(&canonical_bytes(value)?, &signature)
        .map_err(|_| ContractError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use serde_json::json;

    use super::{ContentHash, content_hash, deterministic_uuid, sign_canonical, verify_canonical};

    #[test]
    fn jcs_hash_ignores_object_order_and_preserves_array_order() {
        assert_eq!(
            content_hash(&json!({"b": 2, "a": 1})).unwrap(),
            content_hash(&json!({"a": 1, "b": 2})).unwrap()
        );
        assert_ne!(
            content_hash(&json!([1, 2])).unwrap(),
            content_hash(&json!([2, 1])).unwrap()
        );
        assert_eq!(
            ContentHash::parse(content_hash(&json!({"value": 1.0})).unwrap().to_string()).unwrap(),
            content_hash(&json!({"value": 1})).unwrap()
        );
    }

    #[test]
    fn ed25519_rejects_tampering() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let original = json!({"tenantId": "tenant-a", "sequence": 7});
        let signature = sign_canonical("publisher-a", &signing_key, &original).unwrap();
        verify_canonical(&signing_key.verifying_key(), &original, &signature).unwrap();
        assert!(
            verify_canonical(
                &signing_key.verifying_key(),
                &json!({"tenantId": "tenant-a", "sequence": 8}),
                &signature
            )
            .is_err()
        );
    }

    #[test]
    fn content_hash_requires_the_frozen_wire_format() {
        assert!(ContentHash::parse("sha256:ABC").is_err());
        assert!(ContentHash::parse(format!("sha256:{}", "a".repeat(64))).is_ok());
    }

    #[test]
    fn deterministic_ids_are_stable_across_planes() {
        let namespace = uuid::Uuid::from_u128(42);
        assert_eq!(
            deterministic_uuid(namespace, b"fork-execution"),
            deterministic_uuid(namespace, b"fork-execution")
        );
        assert_ne!(
            deterministic_uuid(namespace, b"fork-execution"),
            deterministic_uuid(namespace, b"fork-record")
        );
    }
}
