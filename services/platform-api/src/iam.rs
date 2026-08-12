use std::collections::{HashMap, HashSet};

use agentx_api_types::{PageRequest, PageResponse};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use sqlx::{MySql, Row, Transaction};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult, UniqueConstraint, map_unique},
    models::{
        CreateDepartmentRequest, CreateRoleRequest, CreateUserRequest, DepartmentResponse,
        PermissionResponse, RolePage, RoleResponse, UpdateDepartmentRequest, UpdateRoleRequest,
        UpdateUserRequest, UserPage, UserResponse,
    },
    security::{AuthActor, hash_initial_temporary_password},
    state::AppState,
};

pub(crate) const DEPARTMENT_NAME: UniqueConstraint = UniqueConstraint {
    index: "uq_departments_sibling_name",
    code: "DEPARTMENT_NAME_EXISTS",
    field: "name",
    message: "A department with this name already exists under the selected parent",
};
pub(crate) const USERNAME: UniqueConstraint = UniqueConstraint {
    index: "uq_users_tenant_username",
    code: "USERNAME_EXISTS",
    field: "username",
    message: "This username is already in use",
};
pub(crate) const ROLE_CODE: UniqueConstraint = UniqueConstraint {
    index: "uq_roles_tenant_code",
    code: "ROLE_CODE_EXISTS",
    field: "code",
    message: "A role with this code already exists",
};

#[derive(Deserialize)]
pub struct ListUsersQuery {
    pub page: Option<u32>,
    #[serde(rename = "pageSize")]
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "departmentId")]
    pub department_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct ListRolesQuery {
    pub page: Option<u32>,
    #[serde(rename = "pageSize")]
    pub page_size: Option<u32>,
    pub search: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/departments")]
pub async fn list_departments(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<DepartmentResponse>>> {
    actor.require("department:view")?;
    let rows = if actor.company_admin {
        sqlx::query("SELECT id,parent_id,name,is_root,status,version FROM departments WHERE tenant_id=? ORDER BY is_root DESC,name").bind(actor.tenant_id).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT DISTINCT d.id,d.parent_id,d.name,d.is_root,d.status,d.version FROM departments d JOIN department_closure dc ON dc.descendant_id=d.id AND dc.tenant_id=d.tenant_id JOIN user_roles ur ON ur.scope_department_id=dc.ancestor_id AND ur.tenant_id=d.tenant_id JOIN roles r ON r.id=ur.role_id WHERE d.tenant_id=? AND ur.user_id=? AND r.code='department_admin' ORDER BY d.is_root DESC,d.name").bind(actor.tenant_id).bind(actor.user_id).fetch_all(&state.pool).await?
    };
    Ok(Json(
        rows.into_iter()
            .map(department_from_row)
            .collect::<Result<_, _>>()?,
    ))
}

#[utoipa::path(post, path = "/api/v1/departments", request_body = CreateDepartmentRequest)]
pub async fn create_department(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateDepartmentRequest>,
) -> AppResult<(StatusCode, Json<DepartmentResponse>)> {
    actor.require("department:manage")?;
    validate_name(&input.name)?;
    require_department_scope(&state, &actor, input.parent_id).await?;
    ensure_department_name_available(
        &state,
        actor.tenant_id,
        Some(input.parent_id),
        &normalize(&input.name),
        None,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name) VALUES(?,?,?,?,?)",
    )
    .bind(id)
    .bind(actor.tenant_id)
    .bind(input.parent_id)
    .bind(input.name.trim())
    .bind(normalize(&input.name))
    .execute(&mut *tx)
    .await
    .map_err(|error| map_unique(error, &[DEPARTMENT_NAME]))?;
    sqlx::query("INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) SELECT tenant_id,ancestor_id,?,depth+1 FROM department_closure WHERE tenant_id=? AND descendant_id=? UNION ALL SELECT ?,?,?,0")
        .bind(id).bind(actor.tenant_id).bind(input.parent_id).bind(actor.tenant_id).bind(id).bind(id).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "department.create",
        "department",
        id,
        serde_json::json!({"name": input.name}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(DepartmentResponse {
            id,
            parent_id: Some(input.parent_id),
            name: input.name.trim().to_owned(),
            is_root: false,
            status: "active".to_owned(),
            version: 1,
        }),
    ))
}

#[utoipa::path(patch, path = "/api/v1/departments/{id}", request_body = UpdateDepartmentRequest, params(("id" = Uuid, Path)))]
pub async fn update_department(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateDepartmentRequest>,
) -> AppResult<Json<DepartmentResponse>> {
    actor.require("department:manage")?;
    validate_name(&input.name)?;
    require_department_scope(&state, &actor, id).await?;
    let current =
        sqlx::query("SELECT parent_id,is_root,version FROM departments WHERE id=? AND tenant_id=?")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("Department"))?;
    if current.try_get::<bool, _>("is_root")? && input.parent_id.is_some() {
        return Err(AppError::bad_request(
            "ROOT_DEPARTMENT_IMMUTABLE",
            "Root department cannot be moved",
        ));
    }
    if let Some(parent) = input.parent_id {
        require_department_scope(&state, &actor, parent).await?;
        if parent == id || is_descendant(&state, actor.tenant_id, id, parent).await? {
            return Err(AppError::bad_request(
                "DEPARTMENT_CYCLE",
                "Department cannot move below itself",
            ));
        }
    }
    ensure_department_name_available(
        &state,
        actor.tenant_id,
        input.parent_id,
        &normalize(&input.name),
        Some(id),
    )
    .await?;
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE departments SET name=?,normalized_name=?,parent_id=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?")
        .bind(input.name.trim()).bind(normalize(&input.name)).bind(input.parent_id).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await.map_err(|error| map_unique(error, &[DEPARTMENT_NAME]))?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "VERSION_CONFLICT",
            "Department was modified by another request",
        ));
    }
    rebuild_closure(&mut tx, actor.tenant_id).await?;
    audit(
        &mut tx,
        &actor,
        "department.update",
        "department",
        id,
        serde_json::json!({"name":input.name,"parentId":input.parent_id}),
    )
    .await?;
    tx.commit().await?;
    let row =
        sqlx::query("SELECT id,parent_id,name,is_root,status,version FROM departments WHERE id=?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(department_from_row(row)?))
}

#[utoipa::path(get, path = "/api/v1/users")]
pub async fn list_users(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListUsersQuery>,
) -> AppResult<Json<UserPage>> {
    actor.require("user:view")?;
    let departments = visible_departments(&state, &actor).await?;
    let visible: HashSet<Uuid> = departments.into_iter().collect();
    let rows=sqlx::query("SELECT u.id,u.username,u.display_name,u.status,u.password_change_required,u.version,ud.department_id,d.name department_name FROM users u JOIN user_departments ud ON ud.user_id=u.id JOIN departments d ON d.id=ud.department_id WHERE u.tenant_id=? ORDER BY u.created_at DESC").bind(actor.tenant_id).fetch_all(&state.pool).await?;
    let search = query.search.unwrap_or_default().to_lowercase();
    let mut items = Vec::new();
    for row in rows {
        let department_id: Uuid = row.try_get("department_id")?;
        if !visible.contains(&department_id) {
            continue;
        }
        if query.department_id.is_some_and(|id| id != department_id) {
            continue;
        }
        let status: String = row.try_get("status")?;
        let username: String = row.try_get("username")?;
        let display_name: String = row.try_get("display_name")?;
        if query
            .status
            .as_ref()
            .is_some_and(|v| v != "all" && v != &status)
            || (!search.is_empty()
                && !username.to_lowercase().contains(&search)
                && !display_name.to_lowercase().contains(&search))
        {
            continue;
        }
        let id: Uuid = row.try_get("id")?;
        items.push(UserResponse {
            id,
            username,
            display_name,
            status,
            password_change_required: row.try_get("password_change_required")?,
            department_id,
            department_name: row.try_get("department_name")?,
            roles: user_roles(&state, id).await?,
            version: row.try_get("version")?,
        });
    }
    let page = PageRequest {
        page: query.page,
        page_size: query.page_size,
    }
    .normalized();
    let total = items.len() as u64;
    let start = (page.offset() as usize).min(items.len());
    let end = (start + page.page_size as usize).min(items.len());
    let items = items.drain(start..end).collect();
    Ok(Json(PageResponse {
        items,
        page: page.page,
        page_size: page.page_size,
        total,
    }))
}

#[utoipa::path(post, path = "/api/v1/users", request_body = CreateUserRequest)]
pub async fn create_user(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateUserRequest>,
) -> AppResult<(StatusCode, Json<UserResponse>)> {
    actor.require("user:create")?;
    validate_username(&input.username)?;
    validate_name(&input.display_name)?;
    require_department_scope(&state, &actor, input.department_id).await?;
    let password_hash = hash_initial_temporary_password()?;
    let role = validate_assignable_role(&state, &actor, input.role_id).await?;
    let normalized_username = normalize(&input.username);
    ensure_simple_unique(
        &state,
        "users",
        "username_normalized",
        actor.tenant_id,
        &normalized_username,
        USERNAME,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status,password_change_required) VALUES(?,?,?,?,?,'invited',TRUE)").bind(id).bind(actor.tenant_id).bind(input.username.trim()).bind(normalized_username).bind(input.display_name.trim()).execute(&mut *tx).await.map_err(|error| map_unique(error, &[USERNAME]))?;
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES(?,?)")
        .bind(id)
        .bind(password_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO user_departments(tenant_id,user_id,department_id) VALUES(?,?,?)")
        .bind(actor.tenant_id)
        .bind(id)
        .bind(input.department_id)
        .execute(&mut *tx)
        .await?;
    let scope = if role == "department_admin" {
        Some(input.department_id)
    } else {
        None
    };
    sqlx::query("INSERT INTO user_roles(id,tenant_id,user_id,role_id,scope_department_id) VALUES(?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(input.role_id).bind(scope).execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "user.create",
        "user",
        id,
        serde_json::json!({"role":role}),
    )
    .await?;
    tx.commit().await?;
    let department_name = sqlx::query_scalar("SELECT name FROM departments WHERE id=?")
        .bind(input.department_id)
        .fetch_one(&state.pool)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(UserResponse {
            id,
            username: input.username.trim().to_owned(),
            display_name: input.display_name.trim().to_owned(),
            status: "invited".to_owned(),
            password_change_required: true,
            department_id: input.department_id,
            department_name,
            roles: vec![role],
            version: 1,
        }),
    ))
}

#[utoipa::path(patch, path = "/api/v1/users/{id}", request_body = UpdateUserRequest, params(("id" = Uuid, Path)))]
pub async fn update_user(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateUserRequest>,
) -> AppResult<Json<UserResponse>> {
    actor.require("user:update")?;
    validate_name(&input.display_name)?;
    let target = load_target_user(&state, &actor, id).await?;
    require_department_scope(&state, &actor, input.department_id).await?;
    if !actor.company_admin && target.roles.iter().any(|r| r == "company_admin") {
        return Err(AppError::forbidden("Company Admin cannot be modified"));
    }
    let role = validate_assignable_role(&state, &actor, input.role_id).await?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE users SET display_name=?,version=version+1,token_version=token_version+1 WHERE id=? AND tenant_id=? AND version=?").bind(input.display_name.trim()).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "VERSION_CONFLICT",
            "User was modified by another request",
        ));
    }
    sqlx::query("UPDATE user_departments SET department_id=? WHERE user_id=? AND tenant_id=?")
        .bind(input.department_id)
        .bind(id)
        .bind(actor.tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM user_roles WHERE user_id=? AND tenant_id=?")
        .bind(id)
        .bind(actor.tenant_id)
        .execute(&mut *tx)
        .await?;
    let scope = if role == "department_admin" {
        Some(input.department_id)
    } else {
        None
    };
    sqlx::query("INSERT INTO user_roles(id,tenant_id,user_id,role_id,scope_department_id) VALUES(?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(id).bind(input.role_id).bind(scope).execute(&mut *tx).await?;
    revoke_user_sessions(&mut tx, id).await?;
    audit(
        &mut tx,
        &actor,
        "user.update",
        "user",
        id,
        serde_json::json!({"role":role}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(UserResponse {
        id,
        username: target.username,
        display_name: input.display_name.trim().to_owned(),
        status: target.status,
        password_change_required: target.password_change_required,
        department_id: input.department_id,
        department_name: sqlx::query_scalar("SELECT name FROM departments WHERE id=?")
            .bind(input.department_id)
            .fetch_one(&state.pool)
            .await?,
        roles: vec![role],
        version: input.version + 1,
    }))
}

#[utoipa::path(post, path = "/api/v1/users/{id}/disable", params(("id" = Uuid, Path)))]
pub async fn disable_user(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("user:disable")?;
    if id == actor.user_id {
        return Err(AppError::bad_request(
            "SELF_DISABLE_DENIED",
            "You cannot disable your own account",
        ));
    }
    let target = load_target_user(&state, &actor, id).await?;
    if !actor.company_admin && target.roles.iter().any(|r| r == "company_admin") {
        return Err(AppError::forbidden("Company Admin cannot be disabled"));
    }
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET status='disabled',token_version=token_version+1,version=version+1 WHERE id=? AND tenant_id=?").bind(id).bind(actor.tenant_id).execute(&mut *tx).await?;
    revoke_user_sessions(&mut tx, id).await?;
    audit(
        &mut tx,
        &actor,
        "user.disable",
        "user",
        id,
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/roles")]
pub async fn list_roles(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<ListRolesQuery>,
) -> AppResult<Json<RolePage>> {
    actor.require("role:view")?;
    let rows=sqlx::query("SELECT r.id,r.code,r.name,r.description,r.data_scope,r.is_builtin,r.status,r.version,COUNT(DISTINCT ur.user_id) member_count FROM roles r LEFT JOIN user_roles ur ON ur.role_id=r.id WHERE r.tenant_id=? GROUP BY r.id ORDER BY r.is_builtin DESC,r.name").bind(actor.tenant_id).fetch_all(&state.pool).await?;
    let search = query.search.unwrap_or_default().to_lowercase();
    let mut items = Vec::new();
    for row in rows {
        let name: String = row.try_get("name")?;
        let code: String = row.try_get("code")?;
        if !search.is_empty()
            && !name.to_lowercase().contains(&search)
            && !code.to_lowercase().contains(&search)
        {
            continue;
        }
        let id: Uuid = row.try_get("id")?;
        items.push(RoleResponse {
            id,
            code,
            name,
            description: row.try_get("description")?,
            data_scope: row.try_get("data_scope")?,
            is_builtin: row.try_get("is_builtin")?,
            status: row.try_get("status")?,
            permissions: role_permissions(&state, id).await?,
            member_count: row.try_get::<i64, _>("member_count")? as u64,
            version: row.try_get("version")?,
        });
    }
    let page = PageRequest {
        page: query.page,
        page_size: query.page_size,
    }
    .normalized();
    let total = items.len() as u64;
    let start = (page.offset() as usize).min(items.len());
    let end = (start + page.page_size as usize).min(items.len());
    let items = items.drain(start..end).collect();
    Ok(Json(PageResponse {
        items,
        page: page.page,
        page_size: page.page_size,
        total,
    }))
}

#[utoipa::path(get, path = "/api/v1/permissions")]
pub async fn list_permissions(
    State(state): State<AppState>,
    actor: AuthActor,
) -> AppResult<Json<Vec<PermissionResponse>>> {
    actor.require("role:view")?;
    let rows = sqlx::query(
        "SELECT permission_key,name,description FROM permissions ORDER BY permission_key",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut permissions = Vec::new();
    for row in rows {
        let key: String = row.try_get("permission_key")?;
        if actor.permissions.contains(&key) {
            permissions.push(PermissionResponse {
                key,
                name: row.try_get("name")?,
                description: row.try_get("description")?,
            });
        }
    }
    Ok(Json(permissions))
}

#[utoipa::path(post, path = "/api/v1/roles", request_body = CreateRoleRequest)]
pub async fn create_role(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateRoleRequest>,
) -> AppResult<(StatusCode, Json<RoleResponse>)> {
    actor.require("role:manage")?;
    if !actor.company_admin {
        return Err(AppError::forbidden("Only Company Admin can create roles"));
    }
    validate_role(
        &input.code,
        &input.name,
        &input.data_scope,
        &input.permissions,
        &actor,
    )?;
    let normalized_code = normalize_code(&input.code);
    ensure_simple_unique(
        &state,
        "roles",
        "code",
        actor.tenant_id,
        &normalized_code,
        ROLE_CODE,
    )
    .await?;
    let id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO roles(id,tenant_id,code,name,description,data_scope,is_builtin) VALUES(?,?,?,?,?,?,FALSE)").bind(id).bind(actor.tenant_id).bind(normalized_code.clone()).bind(input.name.trim()).bind(&input.description).bind(&input.data_scope).execute(&mut *tx).await.map_err(|error| map_unique(error, &[ROLE_CODE]))?;
    replace_permissions(&mut tx, actor.tenant_id, id, &input.permissions).await?;
    audit(
        &mut tx,
        &actor,
        "role.create",
        "role",
        id,
        serde_json::json!({"code":input.code}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(RoleResponse {
            id,
            code: normalized_code,
            name: input.name.trim().to_owned(),
            description: input.description,
            data_scope: input.data_scope,
            is_builtin: false,
            status: "active".to_owned(),
            permissions: input.permissions,
            member_count: 0,
            version: 1,
        }),
    ))
}

#[utoipa::path(patch, path = "/api/v1/roles/{id}", request_body = UpdateRoleRequest, params(("id" = Uuid, Path)))]
pub async fn update_role(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateRoleRequest>,
) -> AppResult<Json<RoleResponse>> {
    actor.require("role:manage")?;
    if !actor.company_admin {
        return Err(AppError::forbidden("Only Company Admin can edit roles"));
    }
    let current =
        sqlx::query("SELECT code,is_builtin,status FROM roles WHERE id=? AND tenant_id=?")
            .bind(id)
            .bind(actor.tenant_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("Role"))?;
    if current.try_get::<bool, _>("is_builtin")? {
        return Err(AppError::bad_request(
            "BUILTIN_ROLE_IMMUTABLE",
            "Built-in roles cannot be edited",
        ));
    }
    validate_role(
        "custom",
        &input.name,
        &input.data_scope,
        &input.permissions,
        &actor,
    )?;
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE roles SET name=?,description=?,data_scope=?,version=version+1 WHERE id=? AND tenant_id=? AND version=?").bind(input.name.trim()).bind(&input.description).bind(&input.data_scope).bind(id).bind(actor.tenant_id).bind(input.version).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "VERSION_CONFLICT",
            "Role was modified by another request",
        ));
    }
    replace_permissions(&mut tx, actor.tenant_id, id, &input.permissions).await?;
    audit(
        &mut tx,
        &actor,
        "role.update",
        "role",
        id,
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT user_id) FROM user_roles WHERE role_id=?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(RoleResponse {
        id,
        code: current.try_get("code")?,
        name: input.name.trim().to_owned(),
        description: input.description,
        data_scope: input.data_scope,
        is_builtin: false,
        status: current.try_get("status")?,
        permissions: input.permissions,
        member_count: count as u64,
        version: input.version + 1,
    }))
}

async fn require_department_scope(
    state: &AppState,
    actor: &AuthActor,
    department_id: Uuid,
) -> AppResult<()> {
    if actor.company_admin {
        return Ok(());
    }
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND ur.user_id=? AND r.code='department_admin' AND dc.descendant_id=?)").bind(actor.tenant_id).bind(actor.user_id).bind(department_id).fetch_one(&state.pool).await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "Department is outside your management scope",
        ))
    }
}

async fn ensure_department_name_available(
    state: &AppState,
    tenant: Uuid,
    parent: Option<Uuid>,
    normalized_name: &str,
    exclude: Option<Uuid>,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM departments WHERE tenant_id=? AND parent_id <=> ? AND normalized_name=? AND (? IS NULL OR id<>?))",
    )
    .bind(tenant).bind(parent).bind(normalized_name).bind(exclude).bind(exclude)
    .fetch_one(&state.pool).await?;
    if exists {
        Err(AppError::unique(DEPARTMENT_NAME))
    } else {
        Ok(())
    }
}

async fn ensure_simple_unique(
    state: &AppState,
    table: &'static str,
    column: &'static str,
    tenant: Uuid,
    value: &str,
    constraint: UniqueConstraint,
) -> AppResult<()> {
    let sql = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE tenant_id=? AND {column}=?)");
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(value)
        .fetch_one(&state.pool)
        .await?;
    if exists {
        Err(AppError::unique(constraint))
    } else {
        Ok(())
    }
}
async fn visible_departments(state: &AppState, actor: &AuthActor) -> AppResult<Vec<Uuid>> {
    if actor.company_admin {
        return Ok(
            sqlx::query_scalar("SELECT id FROM departments WHERE tenant_id=?")
                .bind(actor.tenant_id)
                .fetch_all(&state.pool)
                .await?,
        );
    }
    Ok(sqlx::query_scalar("SELECT DISTINCT dc.descendant_id FROM user_roles ur JOIN roles r ON r.id=ur.role_id JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND ur.user_id=? AND r.code='department_admin'").bind(actor.tenant_id).bind(actor.user_id).fetch_all(&state.pool).await?)
}
async fn is_descendant(
    state: &AppState,
    tenant: Uuid,
    ancestor: Uuid,
    descendant: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM department_closure WHERE tenant_id=? AND ancestor_id=? AND descendant_id=?)").bind(tenant).bind(ancestor).bind(descendant).fetch_one(&state.pool).await?)
}
async fn rebuild_closure(tx: &mut Transaction<'_, MySql>, tenant: Uuid) -> AppResult<()> {
    let rows = sqlx::query("SELECT id,parent_id FROM departments WHERE tenant_id=?")
        .bind(tenant)
        .fetch_all(&mut **tx)
        .await?;
    let mut parents = HashMap::new();
    for row in rows {
        parents.insert(
            row.try_get::<Uuid, _>("id")?,
            row.try_get::<Option<Uuid>, _>("parent_id")?,
        );
    }
    sqlx::query("DELETE FROM department_closure WHERE tenant_id=?")
        .bind(tenant)
        .execute(&mut **tx)
        .await?;
    for id in parents.keys() {
        let mut current = Some(*id);
        let mut depth = 0u32;
        let mut visited = HashSet::new();
        while let Some(ancestor) = current {
            if !visited.insert(ancestor) {
                return Err(AppError::bad_request(
                    "DEPARTMENT_CYCLE",
                    "Department hierarchy contains a cycle",
                ));
            }
            sqlx::query("INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(?,?,?,?)").bind(tenant).bind(ancestor).bind(*id).bind(depth).execute(&mut **tx).await?;
            current = parents.get(&ancestor).copied().flatten();
            depth += 1;
        }
    }
    Ok(())
}
async fn validate_assignable_role(
    state: &AppState,
    actor: &AuthActor,
    role_id: Uuid,
) -> AppResult<String> {
    actor.require("role:assign")?;
    let row = sqlx::query("SELECT code FROM roles WHERE id=? AND tenant_id=? AND status='active'")
        .bind(role_id)
        .bind(actor.tenant_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Role"))?;
    let code: String = row.try_get("code")?;
    if !actor.company_admin && code == "company_admin" {
        return Err(AppError::forbidden(
            "Department Admin cannot grant Company Admin",
        ));
    }
    let required = role_permissions(state, role_id).await?;
    if required.iter().any(|p| !actor.permissions.contains(p)) {
        return Err(AppError::forbidden(
            "Role contains permissions you do not hold",
        ));
    }
    Ok(code)
}
async fn load_target_user(
    state: &AppState,
    actor: &AuthActor,
    id: Uuid,
) -> AppResult<UserResponse> {
    let row=sqlx::query("SELECT u.username,u.display_name,u.status,u.password_change_required,u.version,ud.department_id,d.name department_name FROM users u JOIN user_departments ud ON ud.user_id=u.id JOIN departments d ON d.id=ud.department_id WHERE u.id=? AND u.tenant_id=?").bind(id).bind(actor.tenant_id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("User"))?;
    let department_id = row.try_get("department_id")?;
    require_department_scope(state, actor, department_id).await?;
    Ok(UserResponse {
        id,
        username: row.try_get("username")?,
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        password_change_required: row.try_get("password_change_required")?,
        department_id,
        department_name: row.try_get("department_name")?,
        roles: user_roles(state, id).await?,
        version: row.try_get("version")?,
    })
}
async fn user_roles(state: &AppState, user: Uuid) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT r.code FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE ur.user_id=? ORDER BY r.name").bind(user).fetch_all(&state.pool).await?)
}
async fn role_permissions(state: &AppState, role: Uuid) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT p.permission_key FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.role_id=? ORDER BY p.permission_key").bind(role).fetch_all(&state.pool).await?)
}
async fn replace_permissions(
    tx: &mut Transaction<'_, MySql>,
    tenant: Uuid,
    role: Uuid,
    permissions: &[String],
) -> AppResult<()> {
    sqlx::query("DELETE FROM role_permissions WHERE role_id=? AND tenant_id=?")
        .bind(role)
        .bind(tenant)
        .execute(&mut **tx)
        .await?;
    for key in permissions {
        let result=sqlx::query("INSERT INTO role_permissions(tenant_id,role_id,permission_id) SELECT ?,?,id FROM permissions WHERE permission_key=?").bind(tenant).bind(role).bind(key).execute(&mut **tx).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::bad_request(
                "INVALID_PERMISSION",
                format!("Unknown permission {key}"),
            ));
        }
    }
    Ok(())
}
async fn revoke_user_sessions(tx: &mut Transaction<'_, MySql>, user: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE refresh_sessions SET revoked_at=COALESCE(revoked_at,CURRENT_TIMESTAMP(6)) WHERE user_id=?").bind(user).execute(&mut **tx).await?;
    Ok(())
}
async fn audit(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    action: &str,
    target_type: &str,
    target: Uuid,
    detail: serde_json::Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(actor.user_id).bind(action).bind(target_type).bind(target.to_string()).bind(Uuid::now_v7()).bind(detail).execute(&mut **tx).await?;
    Ok(())
}
fn department_from_row(row: sqlx::mysql::MySqlRow) -> Result<DepartmentResponse, sqlx::Error> {
    Ok(DepartmentResponse {
        id: row.try_get("id")?,
        parent_id: row.try_get("parent_id")?,
        name: row.try_get("name")?,
        is_root: row.try_get("is_root")?,
        status: row.try_get("status")?,
        version: row.try_get("version")?,
    })
}
fn validate_name(value: &str) -> AppResult<()> {
    if value.trim().is_empty() || value.trim().len() > 100 {
        Err(AppError::bad_request(
            "INVALID_NAME",
            "Name is required and must not exceed 100 characters",
        ))
    } else {
        Ok(())
    }
}
fn validate_username(value: &str) -> AppResult<()> {
    let v = value.trim();
    if v.len() < 3
        || v.len() > 64
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        Err(AppError::bad_request(
            "INVALID_USERNAME",
            "Username format is invalid",
        ))
    } else {
        Ok(())
    }
}
fn validate_role(
    code: &str,
    name: &str,
    scope: &str,
    permissions: &[String],
    actor: &AuthActor,
) -> AppResult<()> {
    validate_name(name)?;
    if !matches!(scope, "company" | "department_tree" | "own") {
        return Err(AppError::bad_request(
            "INVALID_DATA_SCOPE",
            "Data scope is invalid",
        ));
    }
    if normalize_code(code).is_empty() {
        return Err(AppError::bad_request(
            "INVALID_ROLE_CODE",
            "Role code is invalid",
        ));
    }
    if permissions.iter().any(|p| !actor.permissions.contains(p)) {
        return Err(AppError::forbidden(
            "Role contains permissions you do not hold",
        ));
    }
    Ok(())
}
fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
fn normalize_code(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "_")
}
