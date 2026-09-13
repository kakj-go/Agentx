use std::{env, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MYSQL_NOW_EXPRESSION: &str = "UTC_TIMESTAMP(6)";
pub const MYSQL_CLAIM_SUFFIX: &str = "FOR UPDATE SKIP LOCKED";
pub const DEFAULT_LEASE_SECONDS: u64 = 30;
pub const DEFAULT_HEARTBEAT_SECONDS: u64 = 10;
pub const DEFAULT_BATCH_SIZE: u32 = 100;

pub const CLAIM_INDEX_COLUMNS: &str = "status, available_at, locked_until";
pub const LEASE_MATCH_PREDICATE: &str =
    "locked_by = ? AND fencing_token = ? AND locked_until > UTC_TIMESTAMP(6)";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LeasePolicy {
    pub lease_duration: Duration,
    pub heartbeat_interval: Duration,
    pub batch_size: u32,
}

impl Default for LeasePolicy {
    fn default() -> Self {
        Self {
            lease_duration: Duration::from_secs(DEFAULT_LEASE_SECONDS),
            heartbeat_interval: Duration::from_secs(DEFAULT_HEARTBEAT_SECONDS),
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }
}

impl LeasePolicy {
    pub fn validate(&self) -> Result<(), LeaseError> {
        if self.lease_duration.is_zero()
            || self.heartbeat_interval.is_zero()
            || self.heartbeat_interval >= self.lease_duration
            || self.batch_size == 0
            || self.batch_size > DEFAULT_BATCH_SIZE
        {
            return Err(LeaseError::InvalidPolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct LeaseOwner(pub Uuid);

impl LeaseOwner {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the stable identity of this process. Kubernetes injects the
    /// Pod UID through `AGENTX_INSTANCE_ID`; local processes intentionally
    /// fall back to an ephemeral UUID.
    pub fn for_process() -> Result<Self, LeaseError> {
        match env::var("AGENTX_INSTANCE_ID") {
            Ok(value) if !value.trim().is_empty() => Uuid::parse_str(value.trim())
                .map(Self)
                .map_err(|_| LeaseError::InvalidOwner),
            _ => Ok(Self::new()),
        }
    }
}

impl Default for LeaseOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct FencingToken(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LeaseGrant {
    pub owner: LeaseOwner,
    pub token: FencingToken,
    pub locked_until: OffsetDateTime,
}

impl LeaseGrant {
    pub fn verify(
        &self,
        owner: LeaseOwner,
        token: FencingToken,
        database_now: OffsetDateTime,
    ) -> Result<(), LeaseError> {
        if self.owner != owner || self.token != token || self.locked_until <= database_now {
            return Err(LeaseError::LeaseLost);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LeaseError {
    #[error("lease policy is invalid")]
    InvalidPolicy,
    #[error("AGENTX_INSTANCE_ID must be a UUID when configured")]
    InvalidOwner,
    #[error("lease was lost, expired, or fenced")]
    LeaseLost,
}

pub fn require_single_lease_write(rows_affected: u64) -> Result<(), LeaseError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(LeaseError::LeaseLost)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use time::OffsetDateTime;

    use super::{
        FencingToken, LeaseError, LeaseGrant, LeaseOwner, LeasePolicy, require_single_lease_write,
    };

    #[test]
    fn defaults_are_bounded() {
        LeasePolicy::default().validate().unwrap();
        assert!(
            LeasePolicy {
                lease_duration: Duration::from_secs(10),
                heartbeat_interval: Duration::from_secs(10),
                batch_size: 1,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn old_owner_token_and_expiry_are_rejected() {
        let owner = LeaseOwner::new();
        let now = OffsetDateTime::now_utc();
        let grant = LeaseGrant {
            owner,
            token: FencingToken(7),
            locked_until: now + time::Duration::seconds(30),
        };
        grant.verify(owner, FencingToken(7), now).unwrap();
        assert_eq!(
            grant.verify(LeaseOwner::new(), FencingToken(7), now),
            Err(LeaseError::LeaseLost)
        );
        assert_eq!(
            grant.verify(owner, FencingToken(6), now),
            Err(LeaseError::LeaseLost)
        );
        assert_eq!(
            grant.verify(owner, FencingToken(7), now + time::Duration::seconds(31)),
            Err(LeaseError::LeaseLost)
        );
    }

    #[test]
    fn conditional_write_must_match_exactly_one_live_lease() {
        require_single_lease_write(1).unwrap();
        assert_eq!(require_single_lease_write(0), Err(LeaseError::LeaseLost));
        assert_eq!(require_single_lease_write(2), Err(LeaseError::LeaseLost));
    }

    #[test]
    fn process_owner_falls_back_outside_kubernetes() {
        // Environment mutation is deliberately avoided here because tests may
        // execute concurrently. The fallback is the normal unit-test path.
        if std::env::var_os("AGENTX_INSTANCE_ID").is_none() {
            assert_ne!(LeaseOwner::for_process().unwrap().0, uuid::Uuid::nil());
        }
    }
}
