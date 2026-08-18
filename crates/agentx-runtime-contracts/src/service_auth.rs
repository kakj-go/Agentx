use std::collections::{BTreeSet, HashMap};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ContentHash;

pub const SERVICE_TOKEN_TTL_SECONDS: i64 = 300;
pub const DELEGATION_TOKEN_TTL_SECONDS: i64 = 60;
pub const USER_ACCESS_TOKEN_TTL_SECONDS: i64 = 900;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    Api,
    Publisher,
    Projector,
    Retention,
    DebugOrchestrator,
    EvaluationOrchestrator,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceClaimsV1 {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub role: ControlRole,
    pub scope: BTreeSet<String>,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationClaimsV1 {
    pub iss: String,
    pub aud: String,
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub token_version: u64,
    pub tenant_wide: bool,
    pub scope: BTreeSet<String>,
    pub application_ids: BTreeSet<Uuid>,
    pub workflow_ids: BTreeSet<Uuid>,
    pub execution_ids: BTreeSet<Uuid>,
    pub session_ids: BTreeSet<Uuid>,
    pub request_hash: ContentHash,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserAccessClaimsV1 {
    pub iss: String,
    pub aud: String,
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub token_version: u64,
    pub kind: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

#[derive(Debug, Error)]
pub enum ServiceJwtError {
    #[error("JWT key id is missing")]
    MissingKeyId,
    #[error("JWT key id is not trusted")]
    UnknownKeyId,
    #[error("JWT role is not allowed")]
    RoleNotAllowed,
    #[error("JWT scope is not allowed")]
    ScopeNotAllowed,
    #[error("JWT lifetime exceeds the contract")]
    LifetimeExceeded,
    #[error("JWT is invalid or expired")]
    Invalid,
    #[error("JWT signing failed")]
    Signing,
}

pub fn issue_service_token(
    key_id: &str,
    private_key_pem: &[u8],
    claims: &ServiceClaimsV1,
) -> Result<String, ServiceJwtError> {
    ensure_lifetime(claims.iat, claims.exp, SERVICE_TOKEN_TTL_SECONDS)?;
    issue(key_id, private_key_pem, claims)
}

pub fn issue_delegation_token(
    key_id: &str,
    private_key_pem: &[u8],
    claims: &DelegationClaimsV1,
) -> Result<String, ServiceJwtError> {
    ensure_lifetime(claims.iat, claims.exp, DELEGATION_TOKEN_TTL_SECONDS)?;
    issue(key_id, private_key_pem, claims)
}

pub fn issue_user_access_token(
    key_id: &str,
    private_key_pem: &[u8],
    claims: &UserAccessClaimsV1,
) -> Result<String, ServiceJwtError> {
    ensure_lifetime(claims.iat, claims.exp, USER_ACCESS_TOKEN_TTL_SECONDS)?;
    if claims.kind != "access" {
        return Err(ServiceJwtError::RoleNotAllowed);
    }
    issue(key_id, private_key_pem, claims)
}

pub fn verify_user_access_token(
    token: &str,
    trusted_keys: &HashMap<String, Vec<u8>>,
    issuer: &str,
    audience: &str,
) -> Result<UserAccessClaimsV1, ServiceJwtError> {
    let claims = verify::<UserAccessClaimsV1>(token, trusted_keys, issuer, audience)?;
    ensure_lifetime(claims.iat, claims.exp, USER_ACCESS_TOKEN_TTL_SECONDS)?;
    if claims.kind != "access" {
        return Err(ServiceJwtError::RoleNotAllowed);
    }
    Ok(claims)
}

fn issue<T: Serialize>(
    key_id: &str,
    private_key_pem: &[u8],
    claims: &T,
) -> Result<String, ServiceJwtError> {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_owned());
    let key = EncodingKey::from_rsa_pem(private_key_pem).map_err(|_| ServiceJwtError::Signing)?;
    encode(&header, claims, &key).map_err(|_| ServiceJwtError::Signing)
}

pub fn verify_service_token(
    token: &str,
    trusted_keys: &HashMap<String, Vec<u8>>,
    issuer: &str,
    audience: &str,
    allowed_roles: &BTreeSet<ControlRole>,
    required_scopes: &BTreeSet<String>,
) -> Result<ServiceClaimsV1, ServiceJwtError> {
    let claims = verify::<ServiceClaimsV1>(token, trusted_keys, issuer, audience)?;
    ensure_lifetime(claims.iat, claims.exp, SERVICE_TOKEN_TTL_SECONDS)?;
    if !allowed_roles.contains(&claims.role) {
        return Err(ServiceJwtError::RoleNotAllowed);
    }
    if !required_scopes.is_subset(&claims.scope) {
        return Err(ServiceJwtError::ScopeNotAllowed);
    }
    Ok(claims)
}

pub fn verify_delegation_token(
    token: &str,
    trusted_keys: &HashMap<String, Vec<u8>>,
    issuer: &str,
    audience: &str,
    tenant_id: Uuid,
    required_scopes: &BTreeSet<String>,
) -> Result<DelegationClaimsV1, ServiceJwtError> {
    let claims = verify::<DelegationClaimsV1>(token, trusted_keys, issuer, audience)?;
    ensure_lifetime(claims.iat, claims.exp, DELEGATION_TOKEN_TTL_SECONDS)?;
    if claims.tenant_id != tenant_id {
        return Err(ServiceJwtError::ScopeNotAllowed);
    }
    if !required_scopes.is_subset(&claims.scope) {
        return Err(ServiceJwtError::ScopeNotAllowed);
    }
    Ok(claims)
}

fn verify<T: for<'de> Deserialize<'de>>(
    token: &str,
    trusted_keys: &HashMap<String, Vec<u8>>,
    issuer: &str,
    audience: &str,
) -> Result<T, ServiceJwtError> {
    let header = jsonwebtoken::decode_header(token).map_err(|_| ServiceJwtError::Invalid)?;
    let key_id = header.kid.ok_or(ServiceJwtError::MissingKeyId)?;
    let public_key = trusted_keys
        .get(&key_id)
        .ok_or(ServiceJwtError::UnknownKeyId)?;
    let key = DecodingKey::from_rsa_pem(public_key).map_err(|_| ServiceJwtError::Invalid)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.leeway = 0;
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    decode::<T>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|_| ServiceJwtError::Invalid)
}

fn ensure_lifetime(iat: i64, exp: i64, maximum: i64) -> Result<(), ServiceJwtError> {
    if exp <= iat || exp - iat > maximum {
        return Err(ServiceJwtError::LifetimeExceeded);
    }
    Ok(())
}

#[must_use]
pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use uuid::Uuid;

    use super::{
        ControlRole, DELEGATION_TOKEN_TTL_SECONDS, DelegationClaimsV1, SERVICE_TOKEN_TTL_SECONDS,
        ServiceClaimsV1, issue_delegation_token, issue_service_token, verify_delegation_token,
        verify_service_token,
    };

    const PRIVATE_KEY: &[u8] = include_bytes!("../tests/fixtures/service-private.pem");
    const PUBLIC_KEY: &[u8] = include_bytes!("../tests/fixtures/service-public.pem");

    fn claims() -> ServiceClaimsV1 {
        ServiceClaimsV1 {
            iss: "agentx-control".into(),
            aud: "agentx-runtime-internal".into(),
            sub: "publisher".into(),
            role: ControlRole::Publisher,
            scope: BTreeSet::from(["bundle:prepare".into()]),
            iat: super::now_unix(),
            exp: super::now_unix() + SERVICE_TOKEN_TTL_SECONDS,
            jti: Uuid::now_v7(),
        }
    }

    #[test]
    fn rs256_validates_kid_role_audience_and_scope() {
        let token = issue_service_token("current", PRIVATE_KEY, &claims()).unwrap();
        let keys = HashMap::from([
            ("previous".into(), PUBLIC_KEY.to_vec()),
            ("current".into(), PUBLIC_KEY.to_vec()),
        ]);
        let verified = verify_service_token(
            &token,
            &keys,
            "agentx-control",
            "agentx-runtime-internal",
            &BTreeSet::from([ControlRole::Publisher]),
            &BTreeSet::from(["bundle:prepare".into()]),
        )
        .unwrap();
        assert_eq!(verified.role, ControlRole::Publisher);
        assert!(
            verify_service_token(
                &token,
                &keys,
                "agentx-control",
                "wrong-audience",
                &BTreeSet::from([ControlRole::Publisher]),
                &BTreeSet::new(),
            )
            .is_err()
        );
        assert!(
            verify_service_token(
                &token,
                &keys,
                "wrong-issuer",
                "agentx-runtime-internal",
                &BTreeSet::from([ControlRole::Publisher]),
                &BTreeSet::new(),
            )
            .is_err()
        );
        assert!(
            verify_service_token(
                &token,
                &keys,
                "agentx-control",
                "agentx-runtime-internal",
                &BTreeSet::from([ControlRole::Api]),
                &BTreeSet::new(),
            )
            .is_err()
        );
        assert!(
            verify_service_token(
                &token,
                &keys,
                "agentx-control",
                "agentx-runtime-internal",
                &BTreeSet::from([ControlRole::Publisher]),
                &BTreeSet::from(["deployment:activate".into()]),
            )
            .is_err()
        );
    }

    #[test]
    fn token_lifetime_is_bounded() {
        let mut claims = claims();
        claims.exp += 1;
        assert!(issue_service_token("current", PRIVATE_KEY, &claims).is_err());
    }

    #[test]
    fn removed_and_expired_keys_are_rejected() {
        let token = issue_service_token("removed", PRIVATE_KEY, &claims()).unwrap();
        let keys = HashMap::from([("current".into(), PUBLIC_KEY.to_vec())]);
        assert!(
            verify_service_token(
                &token,
                &keys,
                "agentx-control",
                "agentx-runtime-internal",
                &BTreeSet::from([ControlRole::Publisher]),
                &BTreeSet::new(),
            )
            .is_err()
        );

        let mut expired = claims();
        expired.iat = super::now_unix() - SERVICE_TOKEN_TTL_SECONDS;
        expired.exp = super::now_unix() - 1;
        let token = issue_service_token("current", PRIVATE_KEY, &expired).unwrap();
        assert!(
            verify_service_token(
                &token,
                &keys,
                "agentx-control",
                "agentx-runtime-internal",
                &BTreeSet::from([ControlRole::Publisher]),
                &BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn delegation_is_short_lived_and_tenant_scoped() {
        let now = super::now_unix();
        let tenant_id = Uuid::now_v7();
        let claims = DelegationClaimsV1 {
            iss: "agentx-control".into(),
            aud: "agentx-runtime-query".into(),
            sub: Uuid::now_v7(),
            tenant_id,
            token_version: 1,
            tenant_wide: false,
            scope: BTreeSet::from(["query:execution".into()]),
            application_ids: BTreeSet::new(),
            workflow_ids: BTreeSet::new(),
            execution_ids: BTreeSet::new(),
            session_ids: BTreeSet::new(),
            request_hash: crate::content_hash(&"delegation-test").unwrap(),
            iat: now,
            exp: now + DELEGATION_TOKEN_TTL_SECONDS,
            jti: Uuid::now_v7(),
        };
        let token = issue_delegation_token("current", PRIVATE_KEY, &claims).unwrap();
        let keys = HashMap::from([("current".into(), PUBLIC_KEY.to_vec())]);
        verify_delegation_token(
            &token,
            &keys,
            "agentx-control",
            "agentx-runtime-query",
            tenant_id,
            &BTreeSet::from(["query:execution".into()]),
        )
        .unwrap();
        assert!(
            verify_delegation_token(
                &token,
                &keys,
                "agentx-control",
                "agentx-runtime-query",
                Uuid::now_v7(),
                &BTreeSet::new(),
            )
            .is_err()
        );
    }
}
