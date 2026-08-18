//! Versioned contracts crossing Agentx control/runtime boundaries.
//!
//! This crate deliberately has no SQLx, Redis, ClickHouse, HTTP server, or
//! service dependencies. Durable rows and transport handlers adapt these DTOs
//! at the edge of their owning plane.

mod canonical;
mod engine;
mod envelope;
mod gateway;
mod ir;
mod legacy;
mod package;
mod publish;
mod query;
mod service_auth;

pub use canonical::{
    ContentHash, ContractError, Ed25519Signature, SignatureAlgorithm, canonical_bytes,
    content_hash, deterministic_uuid, sign_canonical, verify_canonical,
};
pub use engine::*;
pub use envelope::*;
pub use gateway::*;
pub use ir::*;
pub use legacy::*;
pub use package::*;
pub use publish::*;
pub use query::*;
pub use service_auth::*;

pub const BUNDLE_SCHEMA_VERSION: u32 = 1;
pub const IR_SCHEMA_VERSION: u32 = 1;
pub const EVENT_SCHEMA_VERSION: u32 = 1;
pub const INTERNAL_API_VERSION: u32 = 1;
pub const WORKER_PROTOCOL_VERSION: u32 = 1;

pub(crate) fn deserialize_v1<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;

    let version = u32::deserialize(deserializer)?;
    if version == 1 {
        Ok(version)
    } else {
        Err(serde::de::Error::custom(format!(
            "unsupported contract version {version}; expected 1"
        )))
    }
}

#[must_use]
pub const fn is_supported_version(version: u32, current: u32) -> bool {
    version == current || (current > 1 && version + 1 == current)
}
