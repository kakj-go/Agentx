use agentx_api_types::PageResponse;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, require_department_scope, validate_name},
    error::{AppError, AppResult},
    grants::require_resource_visible,
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProfileListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProfileVersionResponse {
    pub id: Uuid,
    pub version_number: u64,
    pub runner: String,
    pub image_digest: String,
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub pids_limit: u32,
    pub disk_bytes: u64,
    pub timeout_seconds: u32,
    pub output_limit_bytes: u64,
    pub network_policy: Value,
    pub configuration_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProfileResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub current_version_number: u64,
    pub owner_department_id: Uuid,
    pub version: u64,
    pub current: SandboxProfileVersionResponse,
    pub versions: Vec<SandboxProfileVersionResponse>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub type SandboxProfilePage = PageResponse<SandboxProfileResponse>;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProfileVersionInput {
    pub runner: String,
    pub image_digest: String,
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub pids_limit: u32,
    pub disk_bytes: u64,
    pub timeout_seconds: u32,
    pub output_limit_bytes: u64,
    pub network_policy: Value,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxProfileRequest {
    pub name: String,
    pub description: Option<String>,
    pub owner_department_id: Uuid,
    #[serde(flatten)]
    pub configuration: SandboxProfileVersionInput,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSandboxProfileRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: u64,
}

#[utoipa::path(
    operation_id = "list_sandbox_profiles",
    get,
    path = "/api/v1/sandbox-profiles"
)]
pub async fn list_profiles(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<SandboxProfileListQuery>,
) -> AppResult<Json<SandboxProfilePage>> {
    actor.require("sandbox:view")?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = u64::from((page - 1) * page_size);
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let status = query.status.unwrap_or_default();
    let rows = if actor.company_admin {
        sqlx::query("SELECT id,COUNT(*) OVER() total_count FROM sandbox_profiles WHERE tenant_id=? AND (?='' OR status=?) AND (?='%%' OR name LIKE ?) ORDER BY updated_at DESC LIMIT ? OFFSET ?").bind(actor.tenant_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(offset).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT p.id,COUNT(*) OVER() total_count FROM sandbox_profiles p WHERE p.tenant_id=? AND (EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=p.tenant_id AND dc.descendant_id=p.owner_department_id) OR EXISTS(SELECT 1 FROM resource_grants rg JOIN department_closure dc ON dc.tenant_id=rg.tenant_id AND dc.ancestor_id=rg.subject_id WHERE rg.tenant_id=p.tenant_id AND rg.subject_type='department' AND rg.resource_type='sandbox_profile' AND rg.resource_id=p.id AND rg.operation_key IN ('view','manage','use') AND dc.descendant_id=?)) AND (?='' OR p.status=?) AND (?='%%' OR p.name LIKE ?) ORDER BY p.updated_at DESC LIMIT ? OFFSET ?").bind(actor.tenant_id).bind(actor.user_id).bind(actor.department_id).bind(&status).bind(&status).bind(&search).bind(&search).bind(page_size).bind(offset).fetch_all(&state.pool).await?
    };
    let total = rows
        .first()
        .map_or(Ok(0_i64), |row| row.try_get("total_count"))? as u64;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(load(&state, actor.tenant_id, row.try_get("id")?).await?);
    }
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(operation_id="create_sandbox_profile",post,path="/api/v1/sandbox-profiles",request_body=CreateSandboxProfileRequest)]
pub async fn create_profile(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(input): Json<CreateSandboxProfileRequest>,
) -> AppResult<(StatusCode, Json<SandboxProfileResponse>)> {
    actor.require("sandbox:manage")?;
    require_department_scope(&state.pool, &actor, input.owner_department_id).await?;
    let name = validate_name(&input.name, 160)?;
    let description = validate_description(input.description)?;
    validate_configuration(&input.configuration)?;
    let profile_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let hash = configuration_hash(&input.configuration)?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query("INSERT INTO sandbox_profiles(id,tenant_id,name,description,owner_department_id,created_by) VALUES(?,?,?,?,?,?)").bind(profile_id).bind(actor.tenant_id).bind(&name).bind(&description).bind(input.owner_department_id).bind(actor.user_id).execute(&mut *transaction).await?;
    insert_version(
        &mut transaction,
        &actor,
        profile_id,
        version_id,
        1,
        &input.configuration,
        &hash,
    )
    .await?;
    audit(
        &mut transaction,
        &actor,
        "sandbox_profile.created",
        "sandbox_profile",
        profile_id,
        json!({"name":name,"versionId":version_id,"configurationHash":hash}),
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load(&state, actor.tenant_id, profile_id).await?),
    ))
}

#[utoipa::path(operation_id="get_sandbox_profile",get,path="/api/v1/sandbox-profiles/{id}",params(("id"=Uuid,Path)))]
pub async fn get_profile(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SandboxProfileResponse>> {
    actor.require("sandbox:view")?;
    require_resource_visible(&state, &actor, "sandbox_profile", id).await?;
    Ok(Json(load(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(operation_id="update_sandbox_profile",patch,path="/api/v1/sandbox-profiles/{id}",request_body=UpdateSandboxProfileRequest,params(("id"=Uuid,Path)))]
pub async fn update_profile(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateSandboxProfileRequest>,
) -> AppResult<Json<SandboxProfileResponse>> {
    actor.require("sandbox:manage")?;
    require_resource_visible(&state, &actor, "sandbox_profile", id).await?;
    let name = validate_name(&input.name, 160)?;
    let description = validate_description(input.description)?;
    if !matches!(input.status.as_str(), "active" | "disabled") {
        return Err(AppError::bad_request(
            "INVALID_STATUS",
            "Sandbox Profile status is invalid",
        ));
    }
    let changed=sqlx::query("UPDATE sandbox_profiles SET name=?,description=?,status=?,version=version+1 WHERE tenant_id=? AND id=? AND version=?").bind(name).bind(description).bind(input.status).bind(actor.tenant_id).bind(id).bind(input.version).execute(&state.pool).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "SANDBOX_PROFILE_VERSION_CONFLICT",
            "Sandbox Profile changed",
        ));
    }
    Ok(Json(load(&state, actor.tenant_id, id).await?))
}

#[utoipa::path(operation_id="create_sandbox_profile_version",post,path="/api/v1/sandbox-profiles/{id}/versions",request_body=SandboxProfileVersionInput,params(("id"=Uuid,Path)))]
pub async fn create_version(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<SandboxProfileVersionInput>,
) -> AppResult<(StatusCode, Json<SandboxProfileVersionResponse>)> {
    actor.require("sandbox:manage")?;
    require_resource_visible(&state, &actor, "sandbox_profile", id).await?;
    validate_configuration(&input)?;
    let hash = configuration_hash(&input)?;
    let version_id = Uuid::now_v7();
    let mut transaction = state.pool.begin().await?;
    let current:Option<u64>=sqlx::query_scalar("SELECT current_version_number FROM sandbox_profiles WHERE tenant_id=? AND id=? AND status='active' FOR UPDATE").bind(actor.tenant_id).bind(id).fetch_optional(&mut *transaction).await?;
    let version_number = current.ok_or_else(|| AppError::not_found("Sandbox Profile"))? + 1;
    let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sandbox_profile_versions WHERE tenant_id=? AND profile_id=? AND configuration_hash=?)").bind(actor.tenant_id).bind(id).bind(&hash).fetch_one(&mut *transaction).await?;
    if duplicate {
        return Err(AppError::conflict(
            "SANDBOX_PROFILE_CONFIGURATION_EXISTS",
            "This Sandbox Profile configuration already exists",
        ));
    }
    insert_version(
        &mut transaction,
        &actor,
        id,
        version_id,
        version_number,
        &input,
        &hash,
    )
    .await?;
    sqlx::query("UPDATE sandbox_profiles SET current_version_number=?,version=version+1 WHERE tenant_id=? AND id=?").bind(version_number).bind(actor.tenant_id).bind(id).execute(&mut *transaction).await?;
    audit(
        &mut transaction,
        &actor,
        "sandbox_profile.version_created",
        "sandbox_profile",
        id,
        json!({"versionId":version_id,"versionNumber":version_number,"configurationHash":hash}),
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_version(&state, actor.tenant_id, version_id).await?),
    ))
}

async fn insert_version(
    transaction: &mut sqlx::Transaction<'_, sqlx::MySql>,
    actor: &AuthActor,
    profile_id: Uuid,
    version_id: Uuid,
    version_number: u64,
    input: &SandboxProfileVersionInput,
    hash: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO sandbox_profile_versions(id,tenant_id,profile_id,version_number,runner,image_digest,cpu_millis,memory_bytes,pids_limit,disk_bytes,timeout_seconds,output_limit_bytes,network_policy_json,configuration_hash,created_by) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(version_id).bind(actor.tenant_id).bind(profile_id).bind(version_number).bind(&input.runner).bind(&input.image_digest).bind(input.cpu_millis).bind(input.memory_bytes).bind(input.pids_limit).bind(input.disk_bytes).bind(input.timeout_seconds).bind(input.output_limit_bytes).bind(&input.network_policy).bind(hash).bind(actor.user_id).execute(&mut **transaction).await?;
    Ok(())
}

async fn load(state: &AppState, tenant: Uuid, id: Uuid) -> AppResult<SandboxProfileResponse> {
    let row=sqlx::query("SELECT id,name,description,status,current_version_number,owner_department_id,version,updated_at FROM sandbox_profiles WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_optional(&state.pool).await?.ok_or_else(||AppError::not_found("Sandbox Profile"))?;
    let version_rows=sqlx::query("SELECT id,version_number,runner,image_digest,cpu_millis,memory_bytes,pids_limit,disk_bytes,timeout_seconds,output_limit_bytes,network_policy_json,configuration_hash,created_at FROM sandbox_profile_versions WHERE tenant_id=? AND profile_id=? ORDER BY version_number DESC").bind(tenant).bind(id).fetch_all(&state.pool).await?;
    let versions = version_rows
        .into_iter()
        .map(version_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let current_number: u64 = row.try_get("current_version_number")?;
    let current = versions
        .iter()
        .find(|version| version.version_number == current_number)
        .cloned()
        .ok_or_else(|| AppError::internal("Sandbox Profile current version is missing"))?;
    Ok(SandboxProfileResponse {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        status: row.try_get("status")?,
        current_version_number: current_number,
        owner_department_id: row.try_get("owner_department_id")?,
        version: row.try_get("version")?,
        current,
        versions,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_version(
    state: &AppState,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<SandboxProfileVersionResponse> {
    let row=sqlx::query("SELECT id,version_number,runner,image_digest,cpu_millis,memory_bytes,pids_limit,disk_bytes,timeout_seconds,output_limit_bytes,network_policy_json,configuration_hash,created_at FROM sandbox_profile_versions WHERE tenant_id=? AND id=?").bind(tenant).bind(id).fetch_one(&state.pool).await?;
    Ok(version_from_row(row)?)
}

fn version_from_row(
    row: sqlx::mysql::MySqlRow,
) -> Result<SandboxProfileVersionResponse, sqlx::Error> {
    Ok(SandboxProfileVersionResponse {
        id: row.try_get("id")?,
        version_number: row.try_get("version_number")?,
        runner: row.try_get("runner")?,
        image_digest: row.try_get("image_digest")?,
        cpu_millis: row.try_get("cpu_millis")?,
        memory_bytes: row.try_get("memory_bytes")?,
        pids_limit: row.try_get("pids_limit")?,
        disk_bytes: row.try_get("disk_bytes")?,
        timeout_seconds: row.try_get("timeout_seconds")?,
        output_limit_bytes: row.try_get("output_limit_bytes")?,
        network_policy: row.try_get("network_policy_json")?,
        configuration_hash: row.try_get("configuration_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

fn validate_configuration(input: &SandboxProfileVersionInput) -> AppResult<()> {
    if !matches!(
        input.runner.as_str(),
        "python" | "javascript" | "shell" | "browser"
    ) {
        return Err(AppError::bad_request(
            "SANDBOX_RUNNER_INVALID",
            "Sandbox runner is invalid",
        ));
    }
    let digest = input
        .image_digest
        .rsplit_once("@sha256:")
        .is_some_and(|(image, digest)| {
            !image.is_empty()
                && digest.len() == 64
                && digest
                    .bytes()
                    .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
        });
    if !digest {
        return Err(AppError::bad_request(
            "SANDBOX_IMAGE_NOT_PINNED",
            "Sandbox image must be pinned by a lowercase sha256 digest",
        ));
    }
    if input.cpu_millis == 0
        || input.memory_bytes == 0
        || input.pids_limit == 0
        || input.disk_bytes == 0
        || input.output_limit_bytes == 0
        || !(60..=86_400).contains(&input.timeout_seconds)
    {
        return Err(AppError::bad_request(
            "SANDBOX_LIMIT_INVALID",
            "Sandbox resource limits and TTL must be positive and bounded",
        ));
    }
    if input
        .network_policy
        .get("defaultAction")
        .and_then(Value::as_str)
        .unwrap_or("deny")
        != "deny"
    {
        return Err(AppError::bad_request(
            "SANDBOX_NETWORK_POLICY_INVALID",
            "Sandbox network policy must default to deny",
        ));
    }
    Ok(())
}
fn configuration_hash(input: &SandboxProfileVersionInput) -> AppResult<String> {
    let value = serde_json::to_value(input).map_err(AppError::internal)?;
    let bytes = serde_json::to_vec(&canonicalize(&value)).map_err(AppError::internal)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonicalize(&values[key])))
                    .collect::<Map<_, _>>(),
            )
        }
        value => value.clone(),
    }
}
fn validate_description(value: Option<String>) -> AppResult<Option<String>> {
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if value
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1000)
    {
        Err(AppError::bad_request(
            "INVALID_DESCRIPTION",
            "Description is too long",
        ))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{SandboxProfileVersionInput, configuration_hash, validate_configuration};

    fn input(network_policy: serde_json::Value) -> SandboxProfileVersionInput {
        SandboxProfileVersionInput {
            runner: "python".into(),
            image_digest: format!("registry.example/runner@sha256:{}", "a".repeat(64)),
            cpu_millis: 1000,
            memory_bytes: 536_870_912,
            pids_limit: 128,
            disk_bytes: 1_073_741_824,
            timeout_seconds: 300,
            output_limit_bytes: 1_048_576,
            network_policy,
        }
    }

    #[test]
    fn configuration_hash_ignores_object_key_order() {
        let first =
            input(json!({"defaultAction":"deny","allow":[{"host":"example.com","port":443}]}));
        let second =
            input(json!({"allow":[{"port":443,"host":"example.com"}],"defaultAction":"deny"}));
        assert_eq!(
            configuration_hash(&first).unwrap(),
            configuration_hash(&second).unwrap()
        );
    }

    #[test]
    fn network_policy_must_default_to_deny() {
        assert!(validate_configuration(&input(json!({"defaultAction":"deny"}))).is_ok());
        assert!(validate_configuration(&input(json!({"defaultAction":"allow"}))).is_err());
    }
}
