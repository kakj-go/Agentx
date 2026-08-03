use axum::{Json, extract::State, http::StatusCode};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use sha2::{Digest, Sha256};
use sqlx::{MySql, Row, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    models::{
        AuthResponse, BootstrapRequest, BootstrapStatus, ChangePasswordRequest, LoginRequest,
        MeResponse,
    },
    security::{AuthActor, decode_token, hash_password, issue_token, token_hash, verify_password},
    state::AppState,
};

const REFRESH_COOKIE: &str = "agentx_refresh";
const PERMISSIONS: &[(&str, &str)] = &[
    ("company:view", "View company"),
    ("company:manage", "Manage company"),
    ("department:view", "View departments"),
    ("department:manage", "Manage departments"),
    ("user:view", "View users"),
    ("user:create", "Create users"),
    ("user:update", "Update users"),
    ("user:disable", "Disable users"),
    ("role:view", "View roles"),
    ("role:manage", "Manage roles"),
    ("role:assign", "Assign roles"),
    ("audit:view", "View audit events"),
    ("workflow:view", "View workflows"),
    ("workflow:create", "Create workflows"),
    ("workflow:edit", "Edit workflows"),
    ("workflow:archive", "Archive workflows"),
    ("workflow:publish", "Publish workflows"),
    ("workflow:manage_member", "Manage workflow members"),
    ("workflow:manage_permission", "Manage workflow permissions"),
    ("credential:view", "View credentials"),
    ("credential:manage", "Manage credentials"),
    ("model:view", "View models"),
    ("model:manage", "Manage models"),
    ("mcp:view", "View MCP servers and tools"),
    ("mcp:manage", "Manage MCP servers and tool policies"),
    ("mcp:discover", "Discover MCP tools"),
    ("mcp:debug", "Debug MCP tools"),
    ("skill:view", "View skills"),
    ("skill:manage", "Manage skills"),
    ("knowledge:view", "View knowledge resources"),
    ("knowledge:manage", "Manage knowledge resources"),
    ("memory:view", "View memory resources"),
    ("memory:manage", "Manage memory resources"),
    ("resource:grant", "Grant resources"),
    ("application:view", "View applications"),
    ("application:manage", "Manage applications"),
    ("application:invoke", "Invoke applications"),
    ("application:manage_key", "Manage application API keys"),
    ("dataset:view", "View datasets"),
    ("dataset:manage", "Manage datasets"),
    ("evaluation_profile:view", "View evaluation profiles"),
    ("evaluation_profile:manage", "Manage evaluation profiles"),
    ("evaluation:view", "View evaluations"),
    ("evaluation:manage", "Manage evaluations"),
    ("approval:view", "View approval tasks"),
    ("approval:act", "Act on approval tasks"),
    ("approval:manage", "Manage approval tasks"),
    ("notification:view", "View own notifications"),
    ("execution:view", "View workflow executions"),
    ("execution:cancel", "Cancel workflow executions"),
    ("trace:view", "View workflow traces"),
    ("runtime:view", "View workflow runtime status"),
];

#[utoipa::path(get, path = "/api/v1/bootstrap/status")]
pub async fn bootstrap_status(State(state): State<AppState>) -> AppResult<Json<BootstrapStatus>> {
    let value: String =
        sqlx::query_scalar("SELECT state FROM bootstrap_state WHERE singleton_id=1")
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(BootstrapStatus {
        required: value == "required",
    }))
}

#[utoipa::path(post, path = "/api/v1/bootstrap", request_body = BootstrapRequest)]
pub async fn bootstrap(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<BootstrapRequest>,
) -> AppResult<(StatusCode, CookieJar, Json<AuthResponse>)> {
    validate_name(&input.company_name, "companyName")?;
    validate_name(&input.admin_display_name, "adminDisplayName")?;
    validate_username(&input.admin_username)?;
    let password_hash = hash_password(&input.password)?;
    if input.locale != "zh-CN" && input.locale != "en-US" {
        return Err(AppError::bad_request(
            "INVALID_LOCALE",
            "Locale must be zh-CN or en-US",
        ));
    }
    if input.timezone.trim().is_empty() || input.timezone.len() > 64 {
        return Err(AppError::bad_request(
            "INVALID_TIMEZONE",
            "Timezone is invalid",
        ));
    }

    let mut tx = state.pool.begin().await?;
    let bootstrap: String =
        sqlx::query_scalar("SELECT state FROM bootstrap_state WHERE singleton_id=1 FOR UPDATE")
            .fetch_one(&mut *tx)
            .await?;
    if bootstrap != "required" {
        return Err(AppError::conflict(
            "BOOTSTRAP_COMPLETED",
            "Company initialization has already completed",
        ));
    }
    let tenant_id = Uuid::now_v7();
    let department_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let request_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,?,?)")
        .bind(tenant_id)
        .bind(input.company_name.trim())
        .bind(normalize(&input.company_name))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO tenant_settings(tenant_id,locale,timezone) VALUES(?,?,?)")
        .bind(tenant_id)
        .bind(&input.locale)
        .bind(&input.timezone)
        .execute(&mut *tx)
        .await?;
    for (code, name) in [("development", "Development"), ("production", "Production")] {
        sqlx::query("INSERT INTO workflow_environments(id,tenant_id,code,name,is_builtin) VALUES(?,?,?,?,TRUE)")
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(code)
            .bind(name)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name,is_root) VALUES(?,?,NULL,?,?,TRUE)").bind(department_id).bind(tenant_id).bind(input.company_name.trim()).bind(normalize(&input.company_name)).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(?,?,?,0)",
    )
    .bind(tenant_id)
    .bind(department_id)
    .bind(department_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status,password_change_required) VALUES(?,?,?,?,?,'active',FALSE)").bind(user_id).bind(tenant_id).bind(input.admin_username.trim()).bind(normalize(&input.admin_username)).bind(input.admin_display_name.trim()).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES(?,?)")
        .bind(user_id)
        .bind(password_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO user_departments(tenant_id,user_id,department_id) VALUES(?,?,?)")
        .bind(tenant_id)
        .bind(user_id)
        .bind(department_id)
        .execute(&mut *tx)
        .await?;
    seed_roles(&mut tx, tenant_id, user_id).await?;
    sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,?,'company.bootstrap','tenant',?,?,JSON_OBJECT('companyName',?))")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(user_id).bind(tenant_id.to_string()).bind(request_id).bind(input.company_name.trim()).execute(&mut *tx).await?;
    sqlx::query("UPDATE bootstrap_state SET state='completed',tenant_id=?,completed_at=CURRENT_TIMESTAMP(6) WHERE singleton_id=1 AND state='required'").bind(tenant_id).execute(&mut *tx).await?;
    let (jar, response) = create_session(&state, &mut tx, jar, tenant_id, user_id, 1).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, jar, Json(response)))
}

async fn seed_roles(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    admin_id: Uuid,
) -> AppResult<()> {
    for (key, name) in PERMISSIONS {
        sqlx::query("INSERT INTO permissions(id,permission_key,name) VALUES(?,?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)").bind(Uuid::now_v7()).bind(key).bind(name).execute(&mut **tx).await?;
    }
    let company = Uuid::now_v7();
    let department = Uuid::now_v7();
    let member = Uuid::now_v7();
    for (id, code, name, scope) in [
        (company, "company_admin", "Company Admin", "company"),
        (
            department,
            "department_admin",
            "Department Admin",
            "department_tree",
        ),
        (member, "member", "Member", "own"),
    ] {
        sqlx::query("INSERT INTO roles(id,tenant_id,code,name,data_scope,is_builtin) VALUES(?,?,?,?,?,TRUE)").bind(id).bind(tenant_id).bind(code).bind(name).bind(scope).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions").bind(tenant_id).bind(company).execute(&mut **tx).await?;
    for key in [
        "company:view",
        "department:view",
        "department:manage",
        "user:view",
        "user:create",
        "user:update",
        "user:disable",
        "role:view",
        "role:assign",
        "workflow:view",
        "credential:view",
        "model:view",
        "mcp:view",
        "skill:view",
        "knowledge:view",
        "memory:view",
        "application:view",
        "application:invoke",
        "dataset:view",
        "evaluation_profile:view",
        "evaluation:view",
        "approval:view",
        "approval:act",
        "notification:view",
        "execution:view",
        "trace:view",
        "runtime:view",
    ] {
        sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions WHERE permission_key=?").bind(tenant_id).bind(department).bind(key).execute(&mut **tx).await?;
    }
    for key in [
        "company:view",
        "department:view",
        "application:view",
        "application:invoke",
        "approval:view",
        "approval:act",
        "notification:view",
    ] {
        sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions WHERE permission_key=?").bind(tenant_id).bind(member).bind(key).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO user_roles(id,tenant_id,user_id,role_id) VALUES(?,?,?,?)")
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(admin_id)
        .bind(company)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginRequest)]
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<LoginRequest>,
) -> AppResult<(CookieJar, Json<AuthResponse>)> {
    let normalized = normalize(&input.username);
    let login_key = login_key(&normalized);
    let mut tx = state.pool.begin().await?;
    let attempt = lock_login_attempt(&mut tx, &login_key).await?;
    if attempt
        .locked_until
        .is_some_and(|until| until > OffsetDateTime::now_utc())
    {
        audit_login(&mut tx, "auth.login.rate_limited", &login_key).await?;
        tx.commit().await?;
        return Err(login_rate_limited());
    }
    let row = sqlx::query("SELECT u.id,u.tenant_id,u.status,u.password_change_required,u.token_version,uc.password_hash FROM users u JOIN user_credentials uc ON uc.user_id=u.id WHERE u.username_normalized=?")
        .bind(&normalized).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        let locked = record_login_failure(&state, &mut tx, &login_key, attempt).await?;
        tx.commit().await?;
        return Err(if locked {
            login_rate_limited()
        } else {
            invalid_credentials()
        });
    };
    let status: String = row.try_get("status")?;
    let password_hash: String = row.try_get("password_hash")?;
    if !matches!(status.as_str(), "active" | "invited")
        || !verify_password(&input.password, &password_hash)
    {
        let locked = record_login_failure(&state, &mut tx, &login_key, attempt).await?;
        tx.commit().await?;
        return Err(if locked {
            login_rate_limited()
        } else {
            invalid_credentials()
        });
    }
    sqlx::query("DELETE FROM auth_login_attempts WHERE login_key=?")
        .bind(&login_key)
        .execute(&mut *tx)
        .await?;
    let user_id: Uuid = row.try_get("id")?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let version: u64 = row.try_get("token_version")?;
    if row.try_get::<bool, _>("password_change_required")? {
        let (token, _) = issue_token(
            &state.auth,
            user_id,
            tenant_id,
            version,
            "change_password",
            state.auth.change_password_ttl_seconds,
            None,
        )?;
        tx.commit().await?;
        return Ok((
            clear_cookie(jar, &state),
            Json(AuthResponse {
                access_token: None,
                expires_in: None,
                password_change_required: true,
                change_password_token: Some(token),
                user: None,
            }),
        ));
    }
    let (jar, response) = create_session(&state, &mut tx, jar, tenant_id, user_id, version).await?;
    tx.commit().await?;
    Ok((jar, Json(response)))
}

#[utoipa::path(post, path = "/api/v1/auth/refresh")]
pub async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
) -> AppResult<(CookieJar, Json<AuthResponse>)> {
    let token = jar
        .get(REFRESH_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or_else(|| AppError::unauthorized("REFRESH_REQUIRED", "Refresh session is missing"))?;
    let claims = decode_token(&state.auth, &token, "refresh")?;
    let family = claims
        .family
        .ok_or_else(|| AppError::unauthorized("INVALID_TOKEN", "Refresh family is missing"))?;
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query("SELECT rotated_at,revoked_at FROM refresh_sessions WHERE jti_hash=? AND token_family_id=? FOR UPDATE").bind(token_hash(claims.jti)).bind(family).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Err(AppError::unauthorized(
            "SESSION_REVOKED",
            "Refresh session is not active",
        ));
    };
    if row
        .try_get::<Option<OffsetDateTime>, _>("rotated_at")?
        .is_some()
        || row
            .try_get::<Option<OffsetDateTime>, _>("revoked_at")?
            .is_some()
    {
        sqlx::query("UPDATE refresh_sessions SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE token_family_id=?").bind(family).execute(&mut *tx).await?;
        tx.commit().await?;
        return Err(AppError::unauthorized(
            "REFRESH_REPLAY_DETECTED",
            "Refresh token reuse revoked this session family",
        ));
    }
    let user =
        sqlx::query("SELECT status,token_version FROM users WHERE id=? AND tenant_id=? FOR UPDATE")
            .bind(claims.sub)
            .bind(claims.tid)
            .fetch_one(&mut *tx)
            .await?;
    let version: u64 = user.try_get("token_version")?;
    if user.try_get::<String, _>("status")? != "active" || version != claims.ver {
        return Err(AppError::unauthorized(
            "SESSION_REVOKED",
            "Session has been revoked",
        ));
    }
    sqlx::query("UPDATE refresh_sessions SET rotated_at=CURRENT_TIMESTAMP(6) WHERE jti_hash=?")
        .bind(token_hash(claims.jti))
        .execute(&mut *tx)
        .await?;
    let (jar, response) = rotate_session(
        &state, &mut tx, jar, claims.tid, claims.sub, version, family,
    )
    .await?;
    tx.commit().await?;
    Ok((jar, Json(response)))
}

#[utoipa::path(post, path = "/api/v1/auth/change-password", request_body = ChangePasswordRequest)]
pub async fn change_password(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<ChangePasswordRequest>,
) -> AppResult<(CookieJar, Json<AuthResponse>)> {
    let claims = decode_token(&state.auth, &input.token, "change_password")?;
    let password_hash = hash_password(&input.password)?;
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE users SET status='active',password_change_required=FALSE,token_version=token_version+1,version=version+1 WHERE id=? AND tenant_id=? AND token_version=? AND status IN ('invited','active')")
        .bind(claims.sub).bind(claims.tid).bind(claims.ver).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::unauthorized(
            "INVALID_TOKEN",
            "Change password token is no longer valid",
        ));
    }
    sqlx::query("UPDATE user_credentials SET password_hash=?,password_changed_at=CURRENT_TIMESTAMP(6) WHERE user_id=?").bind(password_hash).bind(claims.sub).execute(&mut *tx).await?;
    sqlx::query("UPDATE refresh_sessions SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE user_id=?").bind(claims.sub).execute(&mut *tx).await?;
    let version = claims.ver + 1;
    let (jar, response) =
        create_session(&state, &mut tx, jar, claims.tid, claims.sub, version).await?;
    tx.commit().await?;
    Ok((jar, Json(response)))
}

#[utoipa::path(post, path = "/api/v1/auth/logout")]
pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> AppResult<(CookieJar, StatusCode)> {
    if let Some(cookie) = jar.get(REFRESH_COOKIE) {
        if let Ok(claims) = decode_token(&state.auth, cookie.value(), "refresh") {
            sqlx::query("UPDATE refresh_sessions SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE jti_hash=?").bind(token_hash(claims.jti)).execute(&state.pool).await?;
        }
    }
    Ok((clear_cookie(jar, &state), StatusCode::NO_CONTENT))
}

#[utoipa::path(get, path = "/api/v1/auth/me")]
pub async fn me(State(state): State<AppState>, actor: AuthActor) -> AppResult<Json<MeResponse>> {
    Ok(Json(load_me(&state, &actor).await?))
}

async fn create_session(
    state: &AppState,
    tx: &mut Transaction<'_, MySql>,
    jar: CookieJar,
    tenant_id: Uuid,
    user_id: Uuid,
    version: u64,
) -> AppResult<(CookieJar, AuthResponse)> {
    rotate_session(state, tx, jar, tenant_id, user_id, version, Uuid::now_v7()).await
}

async fn rotate_session(
    state: &AppState,
    tx: &mut Transaction<'_, MySql>,
    jar: CookieJar,
    tenant_id: Uuid,
    user_id: Uuid,
    version: u64,
    family: Uuid,
) -> AppResult<(CookieJar, AuthResponse)> {
    let (access, _) = issue_token(
        &state.auth,
        user_id,
        tenant_id,
        version,
        "access",
        state.auth.access_ttl_seconds,
        None,
    )?;
    let (refresh, claims) = issue_token(
        &state.auth,
        user_id,
        tenant_id,
        version,
        "refresh",
        state.auth.refresh_ttl_seconds,
        Some(family),
    )?;
    sqlx::query("INSERT INTO refresh_sessions(id,tenant_id,user_id,token_family_id,jti_hash,token_version,expires_at) VALUES(?,?,?,?,?,?,FROM_UNIXTIME(?))")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(user_id).bind(family).bind(token_hash(claims.jti)).bind(version).bind(claims.exp).execute(&mut **tx).await?;
    let cookie = Cookie::build((REFRESH_COOKIE, refresh))
        .path("/api/v1/auth")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(state.auth.cookie_secure)
        .max_age(Duration::seconds(state.auth.refresh_ttl_seconds))
        .build();
    Ok((
        jar.add(cookie),
        AuthResponse {
            access_token: Some(access),
            expires_in: Some(state.auth.access_ttl_seconds),
            password_change_required: false,
            change_password_token: None,
            user: None,
        },
    ))
}

async fn load_me(state: &AppState, actor: &AuthActor) -> AppResult<MeResponse> {
    let row = sqlx::query("SELECT t.name company_name,ts.locale,ts.timezone,d.name department_name FROM tenants t JOIN tenant_settings ts ON ts.tenant_id=t.id JOIN departments d ON d.id=? WHERE t.id=?")
        .bind(actor.department_id).bind(actor.tenant_id).fetch_one(&state.pool).await?;
    Ok(MeResponse {
        id: actor.user_id,
        username: actor.username.clone(),
        display_name: actor.display_name.clone(),
        company_id: actor.tenant_id,
        company_name: row.try_get("company_name")?,
        department_id: actor.department_id,
        department_name: row.try_get("department_name")?,
        roles: actor.roles.clone(),
        permissions: actor.permissions.clone(),
        locale: row.try_get("locale")?,
        timezone: row.try_get("timezone")?,
    })
}

fn clear_cookie(jar: CookieJar, state: &AppState) -> CookieJar {
    jar.remove(
        Cookie::build(REFRESH_COOKIE)
            .path("/api/v1/auth")
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(state.auth.cookie_secure)
            .build(),
    )
}
fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
fn invalid_credentials() -> AppError {
    AppError::unauthorized("INVALID_CREDENTIALS", "Username or password is incorrect")
}

fn login_rate_limited() -> AppError {
    AppError::too_many_requests(
        "TOO_MANY_LOGIN_ATTEMPTS",
        "Too many login attempts. Try again later",
    )
}

fn login_key(username: &str) -> String {
    format!("{:x}", Sha256::digest(username.as_bytes()))
}

struct LoginAttempt {
    failure_count: u32,
    window_started_at: OffsetDateTime,
    locked_until: Option<OffsetDateTime>,
}

async fn lock_login_attempt(
    tx: &mut Transaction<'_, MySql>,
    login_key: &str,
) -> AppResult<LoginAttempt> {
    sqlx::query("INSERT IGNORE INTO auth_login_attempts(login_key) VALUES(?)")
        .bind(login_key)
        .execute(&mut **tx)
        .await?;
    let row = sqlx::query("SELECT failure_count,window_started_at,locked_until FROM auth_login_attempts WHERE login_key=? FOR UPDATE")
        .bind(login_key)
        .fetch_one(&mut **tx)
        .await?;
    Ok(LoginAttempt {
        failure_count: row.try_get("failure_count")?,
        window_started_at: row.try_get("window_started_at")?,
        locked_until: row.try_get("locked_until")?,
    })
}

async fn record_login_failure(
    state: &AppState,
    tx: &mut Transaction<'_, MySql>,
    login_key: &str,
    attempt: LoginAttempt,
) -> AppResult<bool> {
    let now = OffsetDateTime::now_utc();
    let reset = now - attempt.window_started_at
        >= Duration::seconds(state.auth.login_failure_window_seconds);
    let failures = if reset {
        1
    } else {
        attempt.failure_count.saturating_add(1)
    };
    let locked_until = (failures >= state.auth.login_max_failures)
        .then(|| now + Duration::seconds(state.auth.login_lock_seconds));
    sqlx::query("UPDATE auth_login_attempts SET failure_count=?,window_started_at=?,locked_until=? WHERE login_key=?")
        .bind(failures)
        .bind(if reset { now } else { attempt.window_started_at })
        .bind(locked_until)
        .bind(login_key)
        .execute(&mut **tx)
        .await?;
    audit_login(tx, "auth.login.failed", login_key).await?;
    Ok(locked_until.is_some())
}

async fn audit_login(
    tx: &mut Transaction<'_, MySql>,
    action: &str,
    login_key: &str,
) -> AppResult<()> {
    let tenant_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT tenant_id FROM bootstrap_state WHERE singleton_id=1 AND state='completed'",
    )
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    if let Some(tenant_id) = tenant_id {
        sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,NULL,?,'login',?,?,JSON_OBJECT())")
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(action)
            .bind(login_key)
            .bind(Uuid::now_v7())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
fn validate_name(value: &str, field: &str) -> AppResult<()> {
    if value.trim().is_empty() || value.trim().len() > 100 {
        Err(AppError::bad_request(
            "INVALID_FIELD",
            format!("{field} is required and must not exceed 100 characters"),
        ))
    } else {
        Ok(())
    }
}
fn validate_username(value: &str) -> AppResult<()> {
    let value = value.trim();
    if value.len() < 3
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        Err(AppError::bad_request(
            "INVALID_USERNAME",
            "Username must contain 3 to 64 letters, digits, '.', '_' or '-'",
        ))
    } else {
        Ok(())
    }
}
