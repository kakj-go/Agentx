use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api_error::{ApiError, ApiResult},
    control_api::{AuthResponse, ControlApiState, create_session},
};

const PERMISSIONS: &[(&str, &str)] = &[
    ("company:view", "View company"),
    ("company:manage", "Manage company"),
    ("department:view", "View departments"),
    ("department:manage", "Manage departments"),
    ("department:delete", "Delete departments"),
    ("user:view", "View users"),
    ("user:create", "Create users"),
    ("user:update", "Update users"),
    ("user:disable", "Disable users"),
    ("role:view", "View roles"),
    ("role:manage", "Manage roles"),
    ("role:delete", "Delete roles"),
    ("role:assign", "Assign roles"),
    ("audit:view", "View audit events"),
    ("workflow:view", "View workflows"),
    ("workflow:create", "Create workflows"),
    ("workflow:edit", "Edit workflows"),
    ("workflow:archive", "Archive workflows"),
    ("workflow:delete", "Delete workflows and environments"),
    ("workflow:publish", "Publish workflows"),
    ("workflow:manage_member", "Manage workflow members"),
    ("workflow:manage_permission", "Manage workflow permissions"),
    ("credential:view", "View credentials"),
    ("credential:manage", "Manage credentials"),
    ("credential:delete", "Delete credentials"),
    ("model:view", "View models"),
    ("model:manage", "Manage models"),
    ("model:delete", "Delete models"),
    ("mcp:view", "View MCP servers and tools"),
    ("mcp:manage", "Manage MCP servers and tool policies"),
    ("mcp:delete", "Delete MCP servers"),
    ("mcp:discover", "Discover MCP tools"),
    ("mcp:debug", "Debug MCP tools"),
    ("canvas_plugin:view", "View canvas plugins"),
    ("canvas_plugin:manage", "Manage canvas plugins"),
    ("skill:view", "View skills"),
    ("skill:manage", "Manage skills"),
    ("skill:delete", "Delete skills"),
    ("knowledge:view", "View knowledge resources"),
    ("knowledge:manage", "Manage knowledge resources"),
    ("knowledge:delete", "Delete knowledge resources"),
    ("memory:view", "View memory resources"),
    ("memory:manage", "Manage memory resources"),
    ("memory:delete", "Delete memory resources"),
    ("sandbox:view", "View Sandbox Profiles"),
    ("sandbox:manage", "Manage Sandbox Profiles"),
    ("sandbox:delete", "Delete Sandbox Profiles"),
    ("resource:grant", "Grant resources"),
    ("application:view", "View applications"),
    ("application:manage", "Manage applications"),
    ("application:delete", "Delete applications"),
    ("application:invoke", "Invoke applications"),
    ("application:manage_key", "Manage application API keys"),
    ("dataset:view", "View datasets"),
    ("dataset:manage", "Manage datasets"),
    ("dataset:delete", "Delete datasets"),
    ("evaluation_profile:view", "View evaluation profiles"),
    ("evaluation_profile:manage", "Manage evaluation profiles"),
    ("evaluation_profile:delete", "Delete evaluation profiles"),
    ("evaluation:view", "View evaluations"),
    ("evaluation:manage", "Manage evaluations"),
    ("approval:view", "View approval tasks"),
    ("approval:act", "Act on approval tasks"),
    ("approval:manage", "Manage approval tasks"),
    ("notification:view", "View own notifications"),
    ("execution:view", "View workflow executions"),
    ("execution:run", "Run workflow versions"),
    ("execution:cancel", "Cancel workflow executions"),
    ("execution:fork", "Fork workflow executions"),
    ("trace:view", "View workflow traces"),
    ("runtime:view", "View workflow runtime status"),
    ("runtime:manage", "Manage workflow runtime operations"),
];

#[derive(Serialize)]
struct BootstrapStatus {
    required: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BootstrapRequest {
    company_name: String,
    admin_username: String,
    admin_display_name: String,
    password: String,
    locale: String,
    timezone: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangePasswordRequest {
    token: String,
    password: String,
}

pub fn routes() -> Router<ControlApiState> {
    Router::new()
        .route("/api/v1/bootstrap/status", get(bootstrap_status))
        .route("/api/v1/bootstrap", post(bootstrap))
        .route("/api/v1/auth/change-password", post(change_password))
}

async fn bootstrap_status(
    State(state): State<ControlApiState>,
) -> ApiResult<Json<BootstrapStatus>> {
    let value =
        sqlx::query_scalar::<_, String>("SELECT state FROM bootstrap_state WHERE singleton_id=1")
            .fetch_optional(&state.pool)
            .await?;
    Ok(Json(BootstrapStatus {
        required: value.as_deref() != Some("completed"),
    }))
}

async fn bootstrap(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(input): Json<BootstrapRequest>,
) -> ApiResult<(StatusCode, CookieJar, Json<AuthResponse>)> {
    validate_name(&input.company_name, "companyName")?;
    validate_name(&input.admin_display_name, "adminDisplayName")?;
    validate_username(&input.admin_username)?;
    validate_password(&input.password)?;
    if !matches!(input.locale.as_str(), "zh-CN" | "en-US") {
        return Err(ApiError::bad_request(
            "INVALID_LOCALE",
            "Locale must be zh-CN or en-US",
        ));
    }
    if input.timezone.trim().is_empty() || input.timezone.len() > 64 {
        return Err(ApiError::bad_request(
            "INVALID_TIMEZONE",
            "Timezone is invalid",
        ));
    }
    let password_hash = Argon2::default()
        .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(ApiError::internal)?
        .to_string();
    let tenant_id = Uuid::now_v7();
    let department_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT IGNORE INTO bootstrap_state(singleton_id,state) VALUES(1,'required')")
        .execute(&mut *tx)
        .await?;
    let current: String =
        sqlx::query_scalar("SELECT state FROM bootstrap_state WHERE singleton_id=1 FOR UPDATE")
            .fetch_one(&mut *tx)
            .await?;
    if current != "required" {
        return Err(ApiError::conflict(
            "BOOTSTRAP_COMPLETED",
            "Company initialization has already completed",
        ));
    }
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
            .bind(Uuid::now_v7()).bind(tenant_id).bind(code).bind(name)
            .execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name,is_root) VALUES(?,?,NULL,?,?,TRUE)")
        .bind(department_id).bind(tenant_id).bind(input.company_name.trim()).bind(normalize(&input.company_name))
        .execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(?,?,?,0)",
    )
    .bind(tenant_id)
    .bind(department_id)
    .bind(department_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status,password_change_required) VALUES(?,?,?,?,?,'active',FALSE)")
        .bind(user_id).bind(tenant_id).bind(input.admin_username.trim()).bind(normalize(&input.admin_username)).bind(input.admin_display_name.trim())
        .execute(&mut *tx).await?;
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
    crate::iam_api::emit_user_admission(&mut tx, tenant_id, user_id, true).await?;
    seed_quotas(&mut tx, tenant_id, user_id).await?;
    sqlx::query("UPDATE bootstrap_state SET state='completed',tenant_id=?,completed_at=UTC_TIMESTAMP(6) WHERE singleton_id=1 AND state='required'")
        .bind(tenant_id).execute(&mut *tx).await?;
    tx.commit().await?;
    let (jar, response) = create_session(&state, jar, tenant_id, user_id, 1).await?;
    Ok((StatusCode::CREATED, jar, response))
}

async fn change_password(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(input): Json<ChangePasswordRequest>,
) -> ApiResult<(CookieJar, Json<AuthResponse>)> {
    validate_password(&input.password)?;
    let claims = super::control_api::decode_token(&state.auth, &input.token, "change_password")?;
    let password_hash = Argon2::default()
        .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(ApiError::internal)?
        .to_string();
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE users SET status='active',password_change_required=FALSE,token_version=token_version+1,version=version+1 WHERE id=? AND tenant_id=? AND token_version=? AND status IN ('invited','active')")
        .bind(claims.sub).bind(claims.tid).bind(claims.ver).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::unauthorized(
            "INVALID_TOKEN",
            "Change password token is no longer valid",
        ));
    }
    sqlx::query("UPDATE user_credentials SET password_hash=?,password_changed_at=UTC_TIMESTAMP(6) WHERE user_id=?")
        .bind(password_hash).bind(claims.sub).execute(&mut *tx).await?;
    sqlx::query("UPDATE refresh_sessions SET revoked_at=COALESCE(revoked_at,UTC_TIMESTAMP(6)) WHERE user_id=?")
        .bind(claims.sub).execute(&mut *tx).await?;
    crate::iam_api::emit_user_admission(&mut tx, claims.tid, claims.sub, true).await?;
    tx.commit().await?;
    create_session(&state, jar, claims.tid, claims.sub, claims.ver + 1).await
}

async fn seed_roles(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant: Uuid,
    admin: Uuid,
) -> ApiResult<()> {
    for (key, name) in PERMISSIONS {
        sqlx::query("INSERT INTO permissions(id,permission_key,name) VALUES(?,?,?) ON DUPLICATE KEY UPDATE name=VALUES(name)")
            .bind(Uuid::now_v7()).bind(key).bind(name).execute(&mut **tx).await?;
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
        sqlx::query("INSERT INTO roles(id,tenant_id,code,name,data_scope,is_builtin) VALUES(?,?,?,?,?,TRUE)")
            .bind(id).bind(tenant).bind(code).bind(name).bind(scope).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions")
        .bind(tenant).bind(company).execute(&mut **tx).await?;
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
        "canvas_plugin:view",
        "skill:view",
        "knowledge:view",
        "memory:view",
        "sandbox:view",
        "application:view",
        "application:invoke",
        "dataset:view",
        "evaluation_profile:view",
        "evaluation:view",
        "approval:view",
        "approval:act",
        "resource:grant",
        "notification:view",
        "execution:view",
        "trace:view",
        "runtime:view",
    ] {
        sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions WHERE permission_key=?")
            .bind(tenant).bind(department).bind(key).execute(&mut **tx).await?;
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
        sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions WHERE permission_key=?")
            .bind(tenant).bind(member).bind(key).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO user_roles(id,tenant_id,user_id,role_id) VALUES(?,?,?,?)")
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(admin)
        .bind(company)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn seed_quotas(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant: Uuid,
    user: Uuid,
) -> ApiResult<()> {
    for (dimension, limit, period) in [
        ("execution_concurrency", "1000", None),
        ("node_concurrency", "5000", None),
        ("sandbox_concurrency", "500", None),
        ("agent_iterations", "120000", Some(86_400_u64)),
        ("tokens", "1000000000", Some(86_400_u64)),
        ("cost_micros", "1000000000000", Some(86_400_u64)),
        ("artifact_bytes", "1000000000000", None),
        ("cpu_millis", "1000000", None),
        ("memory_bytes", "1099511627776", None),
        ("pids", "100000", None),
        ("disk_bytes", "1000000000000", None),
        ("ttl_seconds", "604800", None),
    ] {
        sqlx::query("INSERT INTO quota_policies(tenant_id,dimension_key,hard_limit,period_seconds,updated_by) VALUES(?,?,?,?,?)")
            .bind(tenant).bind(dimension).bind(limit).bind(period).bind(user).execute(&mut **tx).await?;
    }
    Ok(())
}

fn validate_name(value: &str, field: &'static str) -> ApiResult<()> {
    if value.trim().is_empty() || value.trim().len() > 160 {
        return Err(ApiError::bad_request(
            "INVALID_NAME",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn validate_username(value: &str) -> ApiResult<()> {
    let value = value.trim();
    if !(3..=64).contains(&value.len())
        || !value
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'.' | b'_' | b'-'))
    {
        return Err(ApiError::bad_request(
            "INVALID_USERNAME",
            "Username format is invalid",
        ));
    }
    Ok(())
}

fn validate_password(value: &str) -> ApiResult<()> {
    if !(12..=128).contains(&value.len()) {
        return Err(ApiError::bad_request(
            "INVALID_PASSWORD",
            "Password must contain 12 to 128 characters",
        ));
    }
    Ok(())
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::PERMISSIONS;

    #[test]
    fn permission_catalog_covers_backend_and_web_authorization_checks() {
        let catalog = PERMISSIONS
            .iter()
            .map(|(key, _)| (*key).to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog.len(), PERMISSIONS.len(), "duplicate permission key");

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("platform-control must be inside the workspace");
        let mut required = BTreeSet::new();
        collect_permissions(
            &workspace.join("src/services/platform-control/src"),
            "require(\"",
            "\")",
            &mut required,
        );
        collect_permissions(
            &workspace.join("src/web/src"),
            "hasPermission('",
            "')",
            &mut required,
        );

        let missing = required.difference(&catalog).cloned().collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "authorization checks missing from bootstrap permission catalog: {missing:?}"
        );
    }

    fn collect_permissions(
        directory: &Path,
        prefix: &str,
        suffix: &str,
        permissions: &mut BTreeSet<String>,
    ) {
        for entry in fs::read_dir(directory).expect("read source directory") {
            let path = entry.expect("read source entry").path();
            if path.is_dir() {
                collect_permissions(&path, prefix, suffix, permissions);
                continue;
            }
            if !matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rs" | "ts" | "tsx")
            ) {
                continue;
            }
            let source = fs::read_to_string(&path).expect("read source file");
            for tail in source.split(prefix).skip(1) {
                let Some((permission, _)) = tail.split_once(suffix) else {
                    continue;
                };
                permissions.insert(permission.to_owned());
            }
        }
    }
}
