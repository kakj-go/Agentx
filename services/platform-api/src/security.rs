use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    config::AuthSettings,
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Claims {
    pub sub: Uuid,
    pub tid: Uuid,
    pub ver: u64,
    pub kind: String,
    pub jti: Uuid,
    pub family: Option<Uuid>,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Clone, Debug)]
pub struct AuthActor {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub department_id: Uuid,
    pub permissions: Vec<String>,
    pub roles: Vec<String>,
    pub company_admin: bool,
}

pub const INITIAL_TEMPORARY_PASSWORD: &str = "123456";

pub fn hash_password(password: &str) -> AppResult<String> {
    validate_password(password)?;
    hash_unchecked(password)
}

pub fn hash_initial_temporary_password() -> AppResult<String> {
    hash_unchecked(INITIAL_TEMPORARY_PASSWORD)
}

fn hash_unchecked(password: &str) -> AppResult<String> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(AppError::internal)
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

pub fn validate_password(password: &str) -> AppResult<()> {
    if !(12..=128).contains(&password.len()) {
        return Err(AppError::bad_request(
            "INVALID_PASSWORD",
            "Password must contain 12 to 128 characters",
        ));
    }
    Ok(())
}

pub fn issue_token(
    settings: &AuthSettings,
    user_id: Uuid,
    tenant_id: Uuid,
    version: u64,
    kind: &str,
    ttl: i64,
    family: Option<Uuid>,
) -> AppResult<(String, Claims)> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let claims = Claims {
        sub: user_id,
        tid: tenant_id,
        ver: version,
        kind: kind.to_owned(),
        jti: Uuid::now_v7(),
        family,
        iss: settings.issuer.clone(),
        aud: settings.audience.clone(),
        iat: now,
        exp: now + ttl,
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(settings.signing_secret.expose_secret().as_bytes()),
    )
    .map_err(AppError::internal)?;
    Ok((token, claims))
}

pub fn decode_token(
    settings: &AuthSettings,
    token: &str,
    expected_kind: &str,
) -> AppResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[&settings.issuer]);
    validation.set_audience(&[&settings.audience]);
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(settings.signing_secret.expose_secret().as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::unauthorized("INVALID_TOKEN", "Token is invalid or expired"))?
    .claims;
    if claims.kind != expected_kind {
        return Err(AppError::unauthorized(
            "INVALID_TOKEN",
            "Token type is invalid",
        ));
    }
    Ok(claims)
}

pub fn token_hash(value: Uuid) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl FromRequestParts<AppState> for AuthActor {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| {
                AppError::unauthorized("AUTHENTICATION_REQUIRED", "Authentication is required")
            })?;
        let claims = decode_token(&state.auth, value, "access")?;
        load_actor(state, &claims).await
    }
}

pub async fn load_actor(state: &AppState, claims: &Claims) -> AppResult<AuthActor> {
    let user = sqlx::query("SELECT u.username, u.display_name, u.token_version, u.status, ud.department_id FROM users u JOIN user_departments ud ON ud.user_id=u.id WHERE u.id=? AND u.tenant_id=?")
        .bind(claims.sub).bind(claims.tid).fetch_optional(&state.pool).await?
        .ok_or_else(|| AppError::unauthorized("INVALID_TOKEN", "User no longer exists"))?;
    let status: String = user.try_get("status")?;
    let version: u64 = user.try_get("token_version")?;
    if status != "active" || version != claims.ver {
        return Err(AppError::unauthorized(
            "SESSION_REVOKED",
            "Session has been revoked",
        ));
    }
    let rows = sqlx::query("SELECT r.code, r.data_scope, ur.scope_department_id, p.permission_key FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.tenant_id=ur.tenant_id LEFT JOIN role_permissions rp ON rp.role_id=r.id LEFT JOIN permissions p ON p.id=rp.permission_id WHERE ur.user_id=? AND ur.tenant_id=? AND r.status='active'")
        .bind(claims.sub).bind(claims.tid).fetch_all(&state.pool).await?;
    let mut roles = Vec::new();
    let mut permissions = Vec::new();
    for row in rows {
        let code: String = row.try_get("code")?;
        if !roles.contains(&code) {
            roles.push(code.clone());
        }
        if let Ok(Some(key)) = row.try_get::<Option<String>, _>("permission_key") {
            if !permissions.contains(&key) {
                permissions.push(key);
            }
        }
    }
    Ok(AuthActor {
        tenant_id: claims.tid,
        user_id: claims.sub,
        username: user.try_get("username")?,
        display_name: user.try_get("display_name")?,
        department_id: user.try_get("department_id")?,
        company_admin: roles.iter().any(|role| role == "company_admin"),
        roles,
        permissions,
    })
}

impl AuthActor {
    pub fn require(&self, permission: &str) -> AppResult<()> {
        if self.permissions.iter().any(|value| value == permission) {
            Ok(())
        } else {
            Err(AppError::forbidden(format!(
                "Missing permission {permission}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::{
        INITIAL_TEMPORARY_PASSWORD, decode_token, hash_initial_temporary_password, hash_password,
        issue_token, verify_password,
    };
    use crate::config::AuthSettings;

    fn settings() -> AuthSettings {
        AuthSettings {
            signing_secret: SecretString::from(
                "a-development-secret-with-more-than-32-characters".to_owned(),
            ),
            issuer: "issuer".to_owned(),
            audience: "audience".to_owned(),
            access_ttl_seconds: 900,
            refresh_ttl_seconds: 604_800,
            change_password_ttl_seconds: 600,
            cookie_secure: false,
            login_max_failures: 5,
            login_failure_window_seconds: 900,
            login_lock_seconds: 900,
        }
    }

    #[test]
    fn password_hash_is_salted_and_verifiable() {
        let first = hash_password("correct horse battery staple").expect("hash");
        let second = hash_password("correct horse battery staple").expect("hash");
        assert_ne!(first, second);
        assert!(verify_password("correct horse battery staple", &first));
        assert!(!verify_password("incorrect password", &first));
    }

    #[test]
    fn initial_temporary_password_uses_the_dedicated_short_password_path() {
        assert!(hash_password(INITIAL_TEMPORARY_PASSWORD).is_err());
        let hash = hash_initial_temporary_password().expect("temporary password hash");
        assert!(verify_password(INITIAL_TEMPORARY_PASSWORD, &hash));
    }

    #[test]
    fn token_validates_kind_issuer_and_audience() {
        let settings = settings();
        let user = Uuid::now_v7();
        let tenant = Uuid::now_v7();
        let (token, claims) =
            issue_token(&settings, user, tenant, 3, "access", 60, None).expect("token");
        let decoded = decode_token(&settings, &token, "access").expect("decode");
        assert_eq!(claims.jti, decoded.jti);
        assert_eq!(decoded.sub, user);
        assert_eq!(decoded.ver, 3);
        assert!(decode_token(&settings, &token, "refresh").is_err());
    }
}
