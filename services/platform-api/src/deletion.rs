use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{MySql, MySqlConnection, Row, Transaction};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    control_common::audit,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntityType {
    Workflow,
    Environment,
    Application,
    Credential,
    Model,
    McpServer,
    Skill,
    Knowledge,
    Memory,
    SandboxProfile,
    Dataset,
    EvaluationProfile,
    Department,
    Role,
    ApplicationWebhook,
    ApplicationSchedule,
}

impl EntityType {
    fn key(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Environment => "environment",
            Self::Application => "application",
            Self::Credential => "credential",
            Self::Model => "model",
            Self::McpServer => "mcp_server",
            Self::Skill => "skill",
            Self::Knowledge => "knowledge",
            Self::Memory => "memory",
            Self::SandboxProfile => "sandbox_profile",
            Self::Dataset => "dataset",
            Self::EvaluationProfile => "evaluation_profile",
            Self::Department => "department",
            Self::Role => "role",
            Self::ApplicationWebhook => "application_webhook",
            Self::ApplicationSchedule => "application_schedule",
        }
    }

    fn permission(self) -> &'static str {
        match self {
            Self::Workflow | Self::Environment => "workflow:delete",
            Self::Application | Self::ApplicationWebhook | Self::ApplicationSchedule => {
                "application:delete"
            }
            Self::Credential => "credential:delete",
            Self::Model => "model:delete",
            Self::McpServer => "mcp:delete",
            Self::Skill => "skill:delete",
            Self::Knowledge => "knowledge:delete",
            Self::Memory => "memory:delete",
            Self::SandboxProfile => "sandbox:delete",
            Self::Dataset => "dataset:delete",
            Self::EvaluationProfile => "evaluation_profile:delete",
            Self::Department => "department:delete",
            Self::Role => "role:delete",
        }
    }
}

impl FromStr for EntityType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.replace('-', "_").as_str() {
            "workflow" | "workflows" => Ok(Self::Workflow),
            "environment" | "environments" => Ok(Self::Environment),
            "application" | "applications" => Ok(Self::Application),
            "credential" | "credentials" => Ok(Self::Credential),
            "model" | "model_alias" | "model_aliases" => Ok(Self::Model),
            "mcp" | "mcp_server" | "mcp_servers" => Ok(Self::McpServer),
            "skill" | "skills" => Ok(Self::Skill),
            "knowledge" | "knowledge_resource" | "knowledge_resources" | "rag" => {
                Ok(Self::Knowledge)
            }
            "memory" | "memory_namespace" | "memory_namespaces" => Ok(Self::Memory),
            "sandbox" | "sandbox_profile" | "sandbox_profiles" => Ok(Self::SandboxProfile),
            "dataset" | "datasets" => Ok(Self::Dataset),
            "evaluation_profile" | "evaluation_profiles" => Ok(Self::EvaluationProfile),
            "department" | "departments" => Ok(Self::Department),
            "role" | "roles" => Ok(Self::Role),
            "application_webhook" | "application_webhooks" | "webhook" => {
                Ok(Self::ApplicationWebhook)
            }
            "application_schedule" | "application_schedules" | "schedule" => {
                Ok(Self::ApplicationSchedule)
            }
            _ => Err(AppError::bad_request(
                "INVALID_ENTITY_TYPE",
                "Entity type does not support deletion",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletionReference {
    pub source_module: String,
    pub source_type: String,
    pub source_id: Uuid,
    pub source_name: String,
    pub relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    pub immutable: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletionImpactResponse {
    pub deletable: bool,
    pub target_version: u64,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub references: Vec<DeletionReference>,
}

#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct DeletionImpactQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEntityQuery {
    pub expected_version: u64,
}

#[derive(Debug)]
struct Target {
    name: String,
    version: u64,
    immutable: bool,
}

pub(crate) async fn lock_resource_targets(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    mut targets: Vec<(String, Uuid)>,
) -> AppResult<()> {
    targets.sort();
    targets.dedup();
    for (resource_type, id) in targets {
        let table = match resource_type.as_str() {
            "credential" => "credentials",
            "model" => "model_aliases",
            "mcp_server" => "mcp_servers",
            "mcp_tool" => "mcp_tools",
            "skill" => "skills",
            "rag" => "rag_resources",
            "memory" => "memory_namespaces",
            "sandbox_profile" => "sandbox_profiles",
            "workflow" => "workflows",
            _ => {
                return Err(AppError::bad_request(
                    "INVALID_RESOURCE_TYPE",
                    format!("Unsupported resource type {resource_type}"),
                ));
            }
        };
        let sql = format!("SELECT id FROM {table} WHERE tenant_id=? AND id=? FOR UPDATE");
        let exists: Option<Uuid> = sqlx::query_scalar(&sql)
            .bind(tenant_id)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
        if exists.is_none() {
            return Err(AppError::not_found("Referenced resource"));
        }
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/v1/deletion-impact/{entity_type}/{id}",
    params(("entity_type" = String, Path), ("id" = Uuid, Path), DeletionImpactQuery),
    responses((status = 200, body = DeletionImpactResponse))
)]
pub async fn get_deletion_impact(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((entity_type, id)): Path<(String, Uuid)>,
    Query(query): Query<DeletionImpactQuery>,
) -> AppResult<Json<DeletionImpactResponse>> {
    let entity = entity_type.parse::<EntityType>()?;
    actor.require(entity.permission())?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(
        impact_on_connection(&mut connection, &actor, entity, id, query, false).await?,
    ))
}

macro_rules! delete_handler {
    ($name:ident, $entity:expr, $path:literal) => {
        #[utoipa::path(
            delete,
            path = $path,
            params(("id" = Uuid, Path), DeleteEntityQuery),
            responses((status = 204), (status = 409, body = agentx_api_types::ApiErrorResponse))
        )]
        pub async fn $name(
            State(state): State<AppState>,
            actor: AuthActor,
            Path(id): Path<Uuid>,
            Query(query): Query<DeleteEntityQuery>,
        ) -> AppResult<StatusCode> {
            delete_entity(&state, &actor, $entity, id, query.expected_version).await
        }
    };
}

delete_handler!(
    delete_workflow,
    EntityType::Workflow,
    "/api/v1/workflows/{id}"
);
delete_handler!(
    delete_environment,
    EntityType::Environment,
    "/api/v1/environments/{id}"
);
delete_handler!(
    delete_application,
    EntityType::Application,
    "/api/v1/applications/{id}"
);
delete_handler!(
    delete_credential,
    EntityType::Credential,
    "/api/v1/credentials/{id}"
);
delete_handler!(
    delete_model,
    EntityType::Model,
    "/api/v1/models/aliases/{id}"
);
delete_handler!(
    delete_mcp_server,
    EntityType::McpServer,
    "/api/v1/mcp/servers/{id}"
);
delete_handler!(delete_skill, EntityType::Skill, "/api/v1/skills/{id}");
delete_handler!(
    delete_knowledge,
    EntityType::Knowledge,
    "/api/v1/knowledge/resources/{id}"
);
delete_handler!(
    delete_memory,
    EntityType::Memory,
    "/api/v1/memory/namespaces/{id}"
);
delete_handler!(
    delete_sandbox_profile,
    EntityType::SandboxProfile,
    "/api/v1/sandbox-profiles/{id}"
);
delete_handler!(delete_dataset, EntityType::Dataset, "/api/v1/datasets/{id}");
delete_handler!(
    delete_evaluation_profile,
    EntityType::EvaluationProfile,
    "/api/v1/evaluation-profiles/{id}"
);
delete_handler!(
    delete_department,
    EntityType::Department,
    "/api/v1/departments/{id}"
);
delete_handler!(delete_role, EntityType::Role, "/api/v1/roles/{id}");

#[utoipa::path(
    delete,
    path = "/api/v1/applications/{application_id}/webhooks/{id}",
    params(("application_id" = Uuid, Path), ("id" = Uuid, Path), DeleteEntityQuery),
    responses((status = 204), (status = 409, body = agentx_api_types::ApiErrorResponse))
)]
pub async fn delete_application_webhook(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((application_id, id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteEntityQuery>,
) -> AppResult<StatusCode> {
    require_child_parent(
        &state,
        actor.tenant_id,
        "application_webhooks",
        id,
        application_id,
    )
    .await?;
    delete_entity(
        &state,
        &actor,
        EntityType::ApplicationWebhook,
        id,
        query.expected_version,
    )
    .await
}

#[utoipa::path(
    delete,
    path = "/api/v1/applications/{application_id}/schedules/{id}",
    params(("application_id" = Uuid, Path), ("id" = Uuid, Path), DeleteEntityQuery),
    responses((status = 204), (status = 409, body = agentx_api_types::ApiErrorResponse))
)]
pub async fn delete_application_schedule(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((application_id, id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteEntityQuery>,
) -> AppResult<StatusCode> {
    require_child_parent(
        &state,
        actor.tenant_id,
        "application_schedules",
        id,
        application_id,
    )
    .await?;
    delete_entity(
        &state,
        &actor,
        EntityType::ApplicationSchedule,
        id,
        query.expected_version,
    )
    .await
}

async fn require_child_parent(
    state: &AppState,
    tenant_id: Uuid,
    table: &str,
    id: Uuid,
    application_id: Uuid,
) -> AppResult<()> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {table} WHERE id=? AND tenant_id=? AND application_id=?)"
    );
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(tenant_id)
        .bind(application_id)
        .fetch_one(&state.pool)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found("Application child"))
    }
}

async fn delete_entity(
    state: &AppState,
    actor: &AuthActor,
    entity: EntityType,
    id: Uuid,
    expected_version: u64,
) -> AppResult<StatusCode> {
    actor.require(entity.permission())?;
    let mut tx = state.pool.begin().await?;
    let impact = impact_on_connection(
        &mut tx,
        actor,
        entity,
        id,
        DeletionImpactQuery {
            page: Some(1),
            page_size: Some(100),
        },
        true,
    )
    .await?;
    if impact.target_version != expected_version {
        return Err(
            AppError::conflict("VERSION_CONFLICT", "Entity changed before deletion")
                .with_details(impact),
        );
    }
    if !impact.deletable {
        let immutable = impact
            .references
            .iter()
            .any(|reference| reference.source_module == "system");
        let code = if immutable {
            "SYSTEM_ENTITY_IMMUTABLE"
        } else {
            "ENTITY_IN_USE"
        };
        return Err(
            AppError::conflict(code, "Entity is still referenced and cannot be deleted")
                .with_details(impact),
        );
    }
    delete_owned_data(&mut tx, actor.tenant_id, entity, id).await?;
    audit(
        &mut tx,
        actor,
        &format!("{}.deleted", entity.key()),
        entity.key(),
        id,
        json!({"expectedVersion": expected_version}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn impact_on_connection(
    connection: &mut MySqlConnection,
    actor: &AuthActor,
    entity: EntityType,
    id: Uuid,
    query: DeletionImpactQuery,
    lock: bool,
) -> AppResult<DeletionImpactResponse> {
    let target = load_target(connection, actor.tenant_id, entity, id, lock).await?;
    if lock && entity == EntityType::McpServer {
        sqlx::query(
            "SELECT id FROM mcp_tools WHERE tenant_id=? AND server_id=? ORDER BY id FOR UPDATE",
        )
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&mut *connection)
        .await?;
    }
    let mut references = if target.immutable {
        vec![DeletionReference {
            source_module: "system".into(),
            source_type: entity.key().into(),
            source_id: id,
            source_name: target.name,
            relation: "immutable".into(),
            parent_id: None,
            node_id: None,
            node_name: None,
            immutable: true,
        }]
    } else {
        collect_references(connection, actor.tenant_id, entity, id).await?
    };
    references.sort_by(|left, right| {
        (
            &left.source_module,
            &left.source_type,
            &left.source_name,
            left.source_id,
        )
            .cmp(&(
                &right.source_module,
                &right.source_type,
                &right.source_name,
                right.source_id,
            ))
    });
    references.dedup_by(|left, right| {
        left.source_type == right.source_type
            && left.source_id == right.source_id
            && left.relation == right.relation
            && left.node_id == right.node_id
    });
    let total = references.len() as u64;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = ((page - 1) * page_size) as usize;
    let references = references
        .into_iter()
        .skip(offset)
        .take(page_size as usize)
        .collect();
    Ok(DeletionImpactResponse {
        deletable: total == 0,
        target_version: target.version,
        total,
        page,
        page_size,
        references,
    })
}

async fn load_target(
    connection: &mut MySqlConnection,
    tenant_id: Uuid,
    entity: EntityType,
    id: Uuid,
    lock: bool,
) -> AppResult<Target> {
    let base = match entity {
        EntityType::Workflow => {
            "SELECT name,version,FALSE immutable FROM workflows WHERE tenant_id=? AND id=?"
        }
        EntityType::Environment => {
            "SELECT name,version,is_builtin immutable FROM workflow_environments WHERE tenant_id=? AND id=?"
        }
        EntityType::Application => {
            "SELECT name,version,FALSE immutable FROM applications WHERE tenant_id=? AND id=?"
        }
        EntityType::Credential => {
            "SELECT name,version,FALSE immutable FROM credentials WHERE tenant_id=? AND id=?"
        }
        EntityType::Model => {
            "SELECT alias name,version,FALSE immutable FROM model_aliases WHERE tenant_id=? AND id=?"
        }
        EntityType::McpServer => {
            "SELECT name,version,FALSE immutable FROM mcp_servers WHERE tenant_id=? AND id=?"
        }
        EntityType::Skill => {
            "SELECT name,version,FALSE immutable FROM skills WHERE tenant_id=? AND id=?"
        }
        EntityType::Knowledge => {
            "SELECT name,version,FALSE immutable FROM rag_resources WHERE tenant_id=? AND id=?"
        }
        EntityType::Memory => {
            "SELECT name,version,FALSE immutable FROM memory_namespaces WHERE tenant_id=? AND id=?"
        }
        EntityType::SandboxProfile => {
            "SELECT name,version,FALSE immutable FROM sandbox_profiles WHERE tenant_id=? AND id=?"
        }
        EntityType::Dataset => {
            "SELECT name,version,FALSE immutable FROM datasets WHERE tenant_id=? AND id=?"
        }
        EntityType::EvaluationProfile => {
            "SELECT name,version,FALSE immutable FROM evaluation_profiles WHERE tenant_id=? AND id=?"
        }
        EntityType::Department => {
            "SELECT name,version,is_root immutable FROM departments WHERE tenant_id=? AND id=?"
        }
        EntityType::Role => {
            "SELECT name,version,is_builtin immutable FROM roles WHERE tenant_id=? AND id=?"
        }
        EntityType::ApplicationWebhook => {
            "SELECT name,version,FALSE immutable FROM application_webhooks WHERE tenant_id=? AND id=?"
        }
        EntityType::ApplicationSchedule => {
            "SELECT name,version,FALSE immutable FROM application_schedules WHERE tenant_id=? AND id=?"
        }
    };
    let sql = if lock {
        format!("{base} FOR UPDATE")
    } else {
        base.to_owned()
    };
    let row = sqlx::query(&sql)
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| AppError::not_found("Entity"))?;
    Ok(Target {
        name: row.try_get("name")?,
        version: row.try_get("version")?,
        immutable: row.try_get("immutable")?,
    })
}

async fn collect_references(
    connection: &mut MySqlConnection,
    tenant_id: Uuid,
    entity: EntityType,
    id: Uuid,
) -> AppResult<Vec<DeletionReference>> {
    let mut references = Vec::new();
    if let Some(resource_type) = resource_type(entity) {
        for sql in resource_reference_queries(resource_type, entity == EntityType::McpServer) {
            push_reference_rows(connection, tenant_id, id, &sql, &mut references).await?;
        }
    }
    for sql in direct_reference_queries(entity) {
        push_reference_rows(connection, tenant_id, id, sql, &mut references).await?;
    }
    Ok(references)
}

fn resource_type(entity: EntityType) -> Option<&'static str> {
    match entity {
        EntityType::Credential => Some("credential"),
        EntityType::Model => Some("model"),
        EntityType::McpServer => Some("mcp_server"),
        EntityType::Skill => Some("skill"),
        EntityType::Knowledge => Some("rag"),
        EntityType::Memory => Some("memory"),
        EntityType::SandboxProfile => Some("sandbox_profile"),
        _ => None,
    }
}

fn resource_reference_queries(resource_type: &str, include_mcp_tools: bool) -> Vec<String> {
    let target = if include_mcp_tools {
        format!(
            "((r.resource_type='{resource_type}' AND r.resource_id=?) OR (r.resource_type='mcp_tool' AND r.resource_id IN (SELECT id FROM mcp_tools WHERE server_id=?)))"
        )
    } else {
        format!("r.resource_type='{resource_type}' AND r.resource_id=?")
    };
    let mut queries = vec![
        format!(
            "SELECT 'workflows' source_module,'workflow_draft' source_type,w.id source_id,w.name source_name,'draft_resource' relation,NULL parent_id,r.node_id,r.node_name,FALSE immutable FROM workflow_draft_resources r JOIN workflows w ON w.id=r.workflow_id WHERE r.tenant_id=? AND {target}"
        ),
        format!(
            "SELECT 'workflows' source_module,'workflow_version' source_type,v.id source_id,CONCAT(w.name,' v',v.version_number) source_name,'version_resource' relation,w.id parent_id,r.node_id,NULL node_name,TRUE immutable FROM workflow_version_resources r JOIN workflow_versions v ON v.id=r.workflow_version_id JOIN workflows w ON w.id=v.workflow_id WHERE r.tenant_id=? AND {target}"
        ),
        format!(
            "SELECT 'skills' source_module,'skill_version' source_type,v.id source_id,CONCAT(s.name,' v',v.version_number) source_name,'skill_dependency' relation,s.id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM skill_dependencies r JOIN skill_versions v ON v.id=r.skill_version_id JOIN skills s ON s.id=v.skill_id WHERE r.tenant_id=? AND {target}"
        ),
        format!(
            "SELECT 'resourceGrants' source_module,'resource_grant' source_type,r.id source_id,COALESCE(d.name,w.name,CONCAT(r.subject_type,' / ',BIN_TO_UUID(r.subject_id))) source_name,'resource_grant' relation,r.subject_id parent_id,NULL node_id,NULL node_name,FALSE immutable FROM resource_grants r LEFT JOIN departments d ON r.subject_type='department' AND d.tenant_id=r.tenant_id AND d.id=r.subject_id LEFT JOIN workflow_service_identities i ON r.subject_type='workflow_service_identity' AND i.tenant_id=r.tenant_id AND i.id=r.subject_id LEFT JOIN workflows w ON w.tenant_id=i.tenant_id AND w.id=i.workflow_id WHERE r.tenant_id=? AND {target}"
        ),
        format!(
            "SELECT 'runtime' source_module,'runtime_call' source_type,r.id source_id,CONCAT(r.call_kind,' / ',r.idempotency_key) source_name,'runtime_history' relation,r.execution_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM runtime_calls r WHERE r.tenant_id=? AND {target}"
        ),
    ];
    if !include_mcp_tools {
        return queries;
    }
    // MCP queries use the server id twice: directly and through all child tools.
    for query in &mut queries {
        *query = query.replacen("WHERE r.tenant_id=?", "WHERE r.tenant_id=?", 1);
    }
    queries
}

fn direct_reference_queries(entity: EntityType) -> &'static [&'static str] {
    match entity {
        EntityType::Workflow => &[
            "SELECT 'applications' source_module,'application' source_type,a.id source_id,a.name source_name,'workflow' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM applications a WHERE a.tenant_id=? AND a.workflow_id=?",
            "SELECT 'executions' source_module,'execution' source_type,e.id source_id,CONCAT('Execution ',BIN_TO_UUID(e.id)) source_name,'workflow' relation,NULL parent_id,NULL node_id,NULL node_name,TRUE immutable FROM workflow_executions e WHERE e.tenant_id=? AND e.workflow_id=?",
            "SELECT 'approvals' source_module,'approval' source_type,a.id source_id,CONCAT('Approval ',BIN_TO_UUID(a.id)) source_name,'workflow' relation,a.execution_id parent_id,a.node_id,NULL node_name,TRUE immutable FROM approval_tasks a WHERE a.tenant_id=? AND a.workflow_id=?",
            "SELECT 'evaluations' source_module,'evaluation_run' source_type,e.id source_id,e.name source_name,'workflow_version' relation,e.workflow_version_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM evaluation_runs e JOIN workflow_versions v ON v.id=e.workflow_version_id WHERE e.tenant_id=? AND v.workflow_id=?",
            "SELECT 'workflows' source_module,'workflow_draft' source_type,w.id source_id,w.name source_name,'subworkflow_draft' relation,NULL parent_id,r.node_id,r.node_name,FALSE immutable FROM workflow_draft_resources r JOIN workflows w ON w.id=r.workflow_id WHERE r.tenant_id=? AND r.resource_type='workflow' AND r.resource_id=?",
            "SELECT 'workflows' source_module,'workflow_version' source_type,v.id source_id,CONCAT(w.name,' v',v.version_number) source_name,'subworkflow_version' relation,w.id parent_id,r.node_id,NULL node_name,TRUE immutable FROM workflow_version_resources r JOIN workflow_versions v ON v.id=r.workflow_version_id JOIN workflows w ON w.id=v.workflow_id WHERE r.tenant_id=? AND r.resource_type='workflow' AND r.resource_id=?",
        ],
        EntityType::Environment => &[
            "SELECT 'workflows' source_module,'workflow_deployment' source_type,d.id source_id,CONCAT(w.name,' #',d.sequence_number) source_name,'environment' relation,w.id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM workflow_deployments d JOIN workflows w ON w.id=d.workflow_id WHERE d.tenant_id=? AND d.environment_id=?",
            "SELECT 'applications' source_module,'application_deployment' source_type,d.id source_id,CONCAT(a.name,' #',d.sequence_number) source_name,'environment' relation,a.id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM application_deployments d JOIN applications a ON a.id=d.application_id WHERE d.tenant_id=? AND d.environment_id=?",
        ],
        EntityType::Application => &[
            "SELECT 'applications' source_module,'session' source_type,s.id source_id,COALESCE(s.title,CONCAT('Session ',BIN_TO_UUID(s.id))) source_name,'application' relation,NULL parent_id,NULL node_id,NULL node_name,TRUE immutable FROM application_sessions s WHERE s.tenant_id=? AND s.application_id=?",
            "SELECT 'applications' source_module,'invocation' source_type,i.id source_id,CONCAT(i.caller_type,' invocation') source_name,'application' relation,i.session_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM application_invocations i WHERE i.tenant_id=? AND i.application_id=?",
            "SELECT 'executions' source_module,'execution' source_type,e.id source_id,CONCAT('Execution ',BIN_TO_UUID(e.id)) source_name,'application_deployment' relation,e.application_deployment_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM workflow_executions e JOIN application_deployments d ON d.id=e.application_deployment_id WHERE e.tenant_id=? AND d.application_id=?",
        ],
        EntityType::Credential => &[
            "SELECT 'models' source_module,'model_deployment' source_type,d.id source_id,CONCAT(d.connection_name,' / ',d.model_name) source_name,'credential' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM model_deployments d WHERE d.tenant_id=? AND d.credential_id=?",
            "SELECT 'mcp' source_module,'mcp_server_version' source_type,v.id source_id,CONCAT(s.name,' v',v.version_number) source_name,'credential' relation,s.id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM mcp_server_versions v JOIN mcp_servers s ON s.id=v.server_id WHERE v.tenant_id=? AND v.credential_id=?",
            "SELECT 'knowledge' source_module,'knowledge_connection' source_type,c.id source_id,c.name source_name,'credential' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM rag_connections c WHERE c.tenant_id=? AND c.credential_id=?",
            "SELECT 'memory' source_module,'memory_connection' source_type,c.id source_id,c.name source_name,'credential' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM memory_connections c WHERE c.tenant_id=? AND c.credential_id=?",
        ],
        EntityType::Dataset => &[
            "SELECT 'evaluations' source_module,'evaluation_run' source_type,e.id source_id,e.name source_name,'dataset_version' relation,e.dataset_version_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM evaluation_runs e JOIN dataset_versions v ON v.id=e.dataset_version_id WHERE e.tenant_id=? AND v.dataset_id=?",
        ],
        EntityType::EvaluationProfile => &[
            "SELECT 'evaluations' source_module,'evaluation_run' source_type,e.id source_id,e.name source_name,'evaluation_profile_version' relation,e.evaluation_profile_version_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM evaluation_runs e JOIN evaluation_profile_versions v ON v.id=e.evaluation_profile_version_id WHERE e.tenant_id=? AND v.profile_id=?",
        ],
        EntityType::Department => &[
            "SELECT 'organization' source_module,'department' source_type,d.id source_id,d.name source_name,'parent_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM departments d WHERE d.tenant_id=? AND d.parent_id=?",
            "SELECT 'organization' source_module,'user' source_type,u.id source_id,u.display_name source_name,'department_membership' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM user_departments ud JOIN users u ON u.id=ud.user_id WHERE ud.tenant_id=? AND ud.department_id=?",
            "SELECT 'roles' source_module,'user_role' source_type,ur.id source_id,CONCAT(u.display_name,' / ',r.name) source_name,'role_scope' relation,r.id parent_id,NULL node_id,NULL node_name,FALSE immutable FROM user_roles ur JOIN users u ON u.id=ur.user_id JOIN roles r ON r.id=ur.role_id WHERE ur.tenant_id=? AND ur.scope_department_id=?",
            "SELECT 'workflows' source_module,'workflow' source_type,w.id source_id,w.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM workflows w WHERE w.tenant_id=? AND w.owner_department_id=?",
            "SELECT 'credentials' source_module,'credential' source_type,c.id source_id,c.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM credentials c WHERE c.tenant_id=? AND c.owner_department_id=?",
            "SELECT 'models' source_module,'model_deployment' source_type,d.id source_id,d.connection_name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM model_deployments d WHERE d.tenant_id=? AND d.owner_department_id=?",
            "SELECT 'mcp' source_module,'mcp_server' source_type,s.id source_id,s.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM mcp_servers s WHERE s.tenant_id=? AND s.owner_department_id=?",
            "SELECT 'skills' source_module,'skill' source_type,s.id source_id,s.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM skills s WHERE s.tenant_id=? AND s.owner_department_id=?",
            "SELECT 'knowledge' source_module,'knowledge_connection' source_type,c.id source_id,c.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM rag_connections c WHERE c.tenant_id=? AND c.owner_department_id=?",
            "SELECT 'knowledge' source_module,'knowledge_resource' source_type,r.id source_id,r.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM rag_resources r WHERE r.tenant_id=? AND r.owner_department_id=?",
            "SELECT 'memory' source_module,'memory_connection' source_type,c.id source_id,c.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM memory_connections c WHERE c.tenant_id=? AND c.owner_department_id=?",
            "SELECT 'memory' source_module,'memory_namespace' source_type,n.id source_id,n.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM memory_namespaces n WHERE n.tenant_id=? AND n.owner_department_id=?",
            "SELECT 'sandbox' source_module,'sandbox_profile' source_type,p.id source_id,p.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM sandbox_profiles p WHERE p.tenant_id=? AND p.owner_department_id=?",
            "SELECT 'applications' source_module,'application' source_type,a.id source_id,a.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM applications a WHERE a.tenant_id=? AND a.owner_department_id=?",
            "SELECT 'datasets' source_module,'dataset' source_type,d.id source_id,d.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM datasets d WHERE d.tenant_id=? AND d.owner_department_id=?",
            "SELECT 'evaluations' source_module,'evaluation_profile' source_type,p.id source_id,p.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,FALSE immutable FROM evaluation_profiles p WHERE p.tenant_id=? AND p.owner_department_id=?",
            "SELECT 'evaluations' source_module,'evaluation_run' source_type,e.id source_id,e.name source_name,'owner_department' relation,NULL parent_id,NULL node_id,NULL node_name,TRUE immutable FROM evaluation_runs e WHERE e.tenant_id=? AND e.owner_department_id=?",
            "SELECT 'resourceGrants' source_module,'resource_grant' source_type,g.id source_id,CONCAT(g.resource_type,' / ',BIN_TO_UUID(g.resource_id)) source_name,'department_subject' relation,g.resource_id parent_id,NULL node_id,NULL node_name,FALSE immutable FROM resource_grants g WHERE g.tenant_id=? AND g.subject_type='department' AND g.subject_id=?",
        ],
        EntityType::Role => &[
            "SELECT 'organization' source_module,'user_role' source_type,ur.id source_id,u.display_name source_name,'role_assignment' relation,u.id parent_id,NULL node_id,NULL node_name,FALSE immutable FROM user_roles ur JOIN users u ON u.id=ur.user_id WHERE ur.tenant_id=? AND ur.role_id=?",
        ],
        EntityType::ApplicationWebhook => &[
            "SELECT 'applications' source_module,'invocation' source_type,i.id source_id,CONCAT('Webhook invocation ',BIN_TO_UUID(i.id)) source_name,'caller' relation,i.application_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM application_invocations i WHERE i.tenant_id=? AND i.caller_type='webhook' AND i.caller_id=?",
        ],
        EntityType::ApplicationSchedule => &[
            "SELECT 'applications' source_module,'invocation' source_type,i.id source_id,CONCAT('Schedule invocation ',BIN_TO_UUID(i.id)) source_name,'caller' relation,i.application_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM application_invocations i WHERE i.tenant_id=? AND i.caller_type='schedule' AND i.caller_id=?",
        ],
        EntityType::SandboxProfile => &[
            "SELECT 'runtime' source_module,'sandbox_lease' source_type,l.id source_id,CONCAT('Sandbox lease ',BIN_TO_UUID(l.id)) source_name,'profile_version' relation,l.execution_id parent_id,NULL node_id,NULL node_name,TRUE immutable FROM sandbox_leases l JOIN sandbox_profile_versions v ON v.id=l.profile_version_id WHERE l.tenant_id=? AND v.profile_id=?",
        ],
        _ => &[],
    }
}

async fn push_reference_rows(
    connection: &mut MySqlConnection,
    tenant_id: Uuid,
    id: Uuid,
    sql: &str,
    output: &mut Vec<DeletionReference>,
) -> AppResult<()> {
    let placeholder_count = sql.as_bytes().iter().filter(|byte| **byte == b'?').count();
    let mut query = sqlx::query(sql).bind(tenant_id).bind(id);
    if placeholder_count == 3 {
        query = query.bind(id);
    }
    for row in query.fetch_all(&mut *connection).await? {
        output.push(DeletionReference {
            source_module: row.try_get("source_module")?,
            source_type: row.try_get("source_type")?,
            source_id: row.try_get("source_id")?,
            source_name: row.try_get("source_name")?,
            relation: row.try_get("relation")?,
            parent_id: row.try_get("parent_id")?,
            node_id: row.try_get("node_id")?,
            node_name: row.try_get("node_name")?,
            immutable: row.try_get("immutable")?,
        });
    }
    Ok(())
}

async fn delete_owned_data(
    connection: &mut MySqlConnection,
    tenant_id: Uuid,
    entity: EntityType,
    id: Uuid,
) -> AppResult<()> {
    match entity {
        EntityType::Workflow => delete_workflow_owned(connection, tenant_id, id).await?,
        EntityType::Application => delete_application_owned(connection, tenant_id, id).await?,
        EntityType::Model => delete_model_owned(connection, tenant_id, id).await?,
        EntityType::McpServer => delete_mcp_owned(connection, tenant_id, id).await?,
        EntityType::Skill => delete_skill_owned(connection, tenant_id, id).await?,
        EntityType::Knowledge => delete_external_resource(connection, tenant_id, id, true).await?,
        EntityType::Memory => delete_external_resource(connection, tenant_id, id, false).await?,
        EntityType::Dataset => delete_dataset_owned(connection, tenant_id, id).await?,
        EntityType::EvaluationProfile => delete_profile_owned(connection, tenant_id, id).await?,
        EntityType::SandboxProfile => {
            execute(
                connection,
                "DELETE FROM sandbox_profile_versions WHERE tenant_id=? AND profile_id=?",
                tenant_id,
                id,
            )
            .await?;
            execute(connection, "DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='sandbox_profile' AND resource_id=?", tenant_id, id).await?;
            execute(
                connection,
                "DELETE FROM sandbox_profiles WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::Credential => {
            execute(
                connection,
                "DELETE FROM credential_secret_versions WHERE tenant_id=? AND credential_id=?",
                tenant_id,
                id,
            )
            .await?;
            execute(connection, "DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='credential' AND resource_id=?", tenant_id, id).await?;
            execute(
                connection,
                "DELETE FROM credentials WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::Environment => {
            execute(
                connection,
                "DELETE FROM workflow_environments WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::Department => {
            execute(connection, "DELETE FROM department_closure WHERE tenant_id=? AND (ancestor_id=? OR descendant_id=?)", tenant_id, id).await?;
            execute(
                connection,
                "DELETE FROM departments WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::Role => {
            execute(
                connection,
                "DELETE FROM role_permissions WHERE tenant_id=? AND role_id=?",
                tenant_id,
                id,
            )
            .await?;
            execute(
                connection,
                "DELETE FROM roles WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::ApplicationWebhook => {
            execute(
                connection,
                "DELETE FROM application_webhooks WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
        EntityType::ApplicationSchedule => {
            execute(
                connection,
                "DELETE FROM application_schedules WHERE tenant_id=? AND id=?",
                tenant_id,
                id,
            )
            .await?;
        }
    }
    Ok(())
}

async fn execute(
    connection: &mut MySqlConnection,
    sql: &str,
    tenant_id: Uuid,
    id: Uuid,
) -> AppResult<()> {
    let count = sql.as_bytes().iter().filter(|byte| **byte == b'?').count();
    let mut query = sqlx::query(sql).bind(tenant_id).bind(id);
    if count == 3 {
        query = query.bind(id);
    }
    query.execute(&mut *connection).await?;
    Ok(())
}

async fn delete_workflow_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    let identity: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=?",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *c)
    .await?;
    if let Some(identity) = identity {
        execute(
            c,
            "DELETE FROM resource_grants WHERE tenant_id=? AND subject_id=?",
            tenant,
            identity,
        )
        .await?;
    }
    for sql in [
        "DELETE FROM workflow_debug_overlays WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_deployment_heads WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM deployment_history WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_deployments WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_version_resources WHERE tenant_id=? AND workflow_version_id IN (SELECT id FROM workflow_versions WHERE workflow_id=?)",
        "DELETE FROM workflow_versions WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_draft_resources WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_draft_revisions WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_drafts WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_members WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=?",
        "DELETE FROM workflows WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

async fn delete_application_owned(
    c: &mut MySqlConnection,
    tenant: Uuid,
    id: Uuid,
) -> AppResult<()> {
    for sql in [
        "DELETE FROM trigger_bindings WHERE tenant_id=? AND application_id=?",
        "DELETE FROM application_deployment_heads WHERE tenant_id=? AND application_id=?",
        "DELETE FROM application_api_keys WHERE tenant_id=? AND application_id=?",
        "DELETE FROM application_webhooks WHERE tenant_id=? AND application_id=?",
        "DELETE FROM application_schedules WHERE tenant_id=? AND application_id=?",
        "DELETE FROM application_deployments WHERE tenant_id=? AND application_id=?",
        "DELETE FROM applications WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

async fn delete_model_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    let rows = sqlx::query("WITH RECURSIVE chain AS (SELECT d.id,d.supersedes_deployment_id,0 depth FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.id=? UNION ALL SELECT d.id,d.supersedes_deployment_id,c.depth+1 FROM model_deployments d JOIN chain c ON d.id=c.supersedes_deployment_id) SELECT id FROM chain ORDER BY depth")
        .bind(tenant).bind(id).fetch_all(&mut *c).await?;
    let deployments = rows
        .into_iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
    execute(c, "DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='model' AND resource_id=?", tenant, id).await?;
    execute(
        c,
        "DELETE FROM model_alias_deployment_history WHERE tenant_id=? AND alias_id=?",
        tenant,
        id,
    )
    .await?;
    execute(
        c,
        "DELETE FROM model_aliases WHERE tenant_id=? AND id=?",
        tenant,
        id,
    )
    .await?;
    for deployment in deployments {
        let externally_referenced: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM model_aliases WHERE tenant_id=? AND deployment_id=? UNION ALL SELECT 1 FROM model_alias_deployment_history WHERE tenant_id=? AND (previous_deployment_id=? OR deployment_id=?) UNION ALL SELECT 1 FROM model_deployments WHERE tenant_id=? AND supersedes_deployment_id=?)",
        )
        .bind(tenant)
        .bind(deployment)
        .bind(tenant)
        .bind(deployment)
        .bind(deployment)
        .bind(tenant)
        .bind(deployment)
        .fetch_one(&mut *c)
        .await?;
        if !externally_referenced {
            execute(
                c,
                "DELETE FROM model_price_versions WHERE tenant_id=? AND deployment_id=?",
                tenant,
                deployment,
            )
            .await?;
            execute(
                c,
                "DELETE FROM model_deployments WHERE tenant_id=? AND id=?",
                tenant,
                deployment,
            )
            .await?;
        }
    }
    Ok(())
}

async fn delete_mcp_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    for sql in [
        "DELETE FROM mcp_tool_policies WHERE tenant_id=? AND tool_id IN (SELECT id FROM mcp_tools WHERE server_id=?)",
        "DELETE FROM mcp_tool_versions WHERE tenant_id=? AND tool_id IN (SELECT id FROM mcp_tools WHERE server_id=?)",
        "DELETE FROM mcp_tools WHERE tenant_id=? AND server_id=?",
        "DELETE FROM mcp_discovery_runs WHERE tenant_id=? AND server_id=?",
        "DELETE FROM mcp_server_versions WHERE tenant_id=? AND server_id=?",
        "DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='mcp_server' AND resource_id=?",
        "DELETE FROM mcp_servers WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

async fn delete_skill_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    for sql in [
        "DELETE FROM skill_dependencies WHERE tenant_id=? AND skill_version_id IN (SELECT id FROM skill_versions WHERE skill_id=?)",
        "DELETE FROM skill_file_references WHERE tenant_id=? AND skill_version_id IN (SELECT id FROM skill_versions WHERE skill_id=?)",
        "DELETE FROM skill_version_files WHERE tenant_id=? AND skill_version_id IN (SELECT id FROM skill_versions WHERE skill_id=?)",
        "DELETE FROM skill_versions WHERE tenant_id=? AND skill_id=?",
        "DELETE FROM skill_file_revisions WHERE tenant_id=? AND entry_id IN (SELECT id FROM skill_workspace_entries WHERE skill_id=?)",
        "UPDATE skill_workspace_entries SET parent_id=NULL WHERE tenant_id=? AND skill_id=?",
        "DELETE FROM skill_workspace_entries WHERE tenant_id=? AND skill_id=?",
        "DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='skill' AND resource_id=?",
        "DELETE FROM skills WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

async fn delete_external_resource(
    c: &mut MySqlConnection,
    tenant: Uuid,
    id: Uuid,
    rag: bool,
) -> AppResult<()> {
    let (resource_table, connection_table, resource_type) = if rag {
        ("rag_resources", "rag_connections", "rag")
    } else {
        ("memory_namespaces", "memory_connections", "memory")
    };
    let connection_id: Uuid = sqlx::query_scalar(&format!(
        "SELECT connection_id FROM {resource_table} WHERE tenant_id=? AND id=?"
    ))
    .bind(tenant)
    .bind(id)
    .fetch_one(&mut *c)
    .await?;
    execute(c, &format!("DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='{resource_type}' AND resource_id=?"), tenant, id).await?;
    execute(
        c,
        &format!("DELETE FROM {resource_table} WHERE tenant_id=? AND id=?"),
        tenant,
        id,
    )
    .await?;
    let sibling_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM {resource_table} WHERE tenant_id=? AND connection_id=?"
    ))
    .bind(tenant)
    .bind(connection_id)
    .fetch_one(&mut *c)
    .await?;
    if sibling_count == 0 {
        execute(c, &format!("DELETE FROM resource_health_checks WHERE tenant_id=? AND resource_type='{resource_type}_connection' AND resource_id=?"), tenant, connection_id).await?;
        execute(
            c,
            &format!("DELETE FROM {connection_table} WHERE tenant_id=? AND id=?"),
            tenant,
            connection_id,
        )
        .await?;
    }
    Ok(())
}

async fn delete_dataset_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    for sql in [
        "DELETE FROM dataset_version_cases WHERE tenant_id=? AND dataset_version_id IN (SELECT id FROM dataset_versions WHERE dataset_id=?)",
        "DELETE FROM dataset_versions WHERE tenant_id=? AND dataset_id=?",
        "DELETE FROM dataset_cases WHERE tenant_id=? AND dataset_id=?",
        "DELETE FROM datasets WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

async fn delete_profile_owned(c: &mut MySqlConnection, tenant: Uuid, id: Uuid) -> AppResult<()> {
    for sql in [
        "DELETE FROM evaluation_profile_rules WHERE tenant_id=? AND profile_version_id IN (SELECT id FROM evaluation_profile_versions WHERE profile_id=?)",
        "DELETE FROM evaluation_profile_versions WHERE tenant_id=? AND profile_id=?",
        "DELETE FROM evaluation_profiles WHERE tenant_id=? AND id=?",
    ] {
        execute(c, sql, tenant, id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use serde_json::Value;
    use std::{path::Path, str::FromStr};
    use uuid::Uuid;

    use super::{
        DeletionImpactQuery, EntityType, delete_entity, delete_owned_data, impact_on_connection,
        lock_resource_targets,
    };
    use crate::{config::AuthSettings, security::AuthActor, state::AppState};

    #[test]
    fn entity_type_accepts_api_and_business_names() {
        assert_eq!(
            EntityType::from_str("model-aliases").unwrap(),
            EntityType::Model
        );
        assert_eq!(
            EntityType::from_str("knowledge_resource").unwrap(),
            EntityType::Knowledge
        );
        assert!(EntityType::from_str("execution").is_err());
    }

    #[test]
    fn entity_type_matrix_uses_independent_delete_permissions() {
        let cases = [
            ("workflow", EntityType::Workflow, "workflow:delete"),
            ("environment", EntityType::Environment, "workflow:delete"),
            ("application", EntityType::Application, "application:delete"),
            ("credential", EntityType::Credential, "credential:delete"),
            ("model", EntityType::Model, "model:delete"),
            ("mcp_server", EntityType::McpServer, "mcp:delete"),
            ("skill", EntityType::Skill, "skill:delete"),
            ("knowledge", EntityType::Knowledge, "knowledge:delete"),
            ("memory", EntityType::Memory, "memory:delete"),
            (
                "sandbox_profile",
                EntityType::SandboxProfile,
                "sandbox:delete",
            ),
            ("dataset", EntityType::Dataset, "dataset:delete"),
            (
                "evaluation_profile",
                EntityType::EvaluationProfile,
                "evaluation_profile:delete",
            ),
            ("department", EntityType::Department, "department:delete"),
            ("role", EntityType::Role, "role:delete"),
            (
                "application_webhook",
                EntityType::ApplicationWebhook,
                "application:delete",
            ),
            (
                "application_schedule",
                EntityType::ApplicationSchedule,
                "application:delete",
            ),
        ];
        for (name, expected, permission) in cases {
            let entity = EntityType::from_str(name).unwrap();
            assert_eq!(entity, expected);
            assert_eq!(entity.key(), name);
            assert_eq!(entity.permission(), permission);
        }
        for immutable_history in [
            "user",
            "api_key",
            "workflow_version",
            "workflow_deployment",
            "application_deployment",
            "dataset_version",
            "skill_version",
            "execution",
            "approval",
            "notification",
            "evaluation_run",
            "session",
            "message",
        ] {
            assert!(EntityType::from_str(immutable_history).is_err());
        }
    }

    #[test]
    fn delete_permission_is_not_implied_by_manage_permission() {
        let mut actor = actor(Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        actor.permissions = vec!["credential:manage".into()];
        assert_eq!(
            actor.require("credential:delete").unwrap_err().code,
            "FORBIDDEN"
        );
    }

    #[tokio::test]
    async fn mysql_preflight_blocks_real_references_and_delete_is_versioned_and_audited() {
        let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
        let (_container, pool) = crate::migration_tests::start_mysql().await;
        let migrations = sqlx::migrate::Migrator::new(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/mysql"
        )))
        .await
        .expect("load migrations");
        migrations.run(&pool).await.expect("run migrations");

        let tenant = Uuid::now_v7();
        let other_tenant = Uuid::now_v7();
        let department = Uuid::now_v7();
        let other_department = Uuid::now_v7();
        let user = Uuid::now_v7();
        let other_user = Uuid::now_v7();
        seed_identity(&pool, tenant, department, user, "primary").await;
        seed_identity(&pool, other_tenant, other_department, other_user, "other").await;
        let actor = actor(tenant, department, user);
        let state = AppState::new(pool.clone(), auth_settings());

        let credential =
            create_credential(&pool, tenant, department, user, "Draft credential").await;
        let workflow = Uuid::now_v7();
        let draft = Uuid::now_v7();
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Draft consumer',?,?)")
            .bind(workflow).bind(tenant).bind(user).bind(department).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,definition_json,content_hash,updated_by) VALUES(?,?,?,'4.0',JSON_OBJECT(),'draft',?)")
            .bind(draft).bind(tenant).bind(workflow).bind(user).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_draft_resources(id,tenant_id,workflow_id,draft_id,node_id,node_name,resource_type,resource_id,operation_key) VALUES(?,?,?,?,'model','Support model','credential',?,'use')")
            .bind(Uuid::now_v7()).bind(tenant).bind(workflow).bind(draft).bind(credential).execute(&pool).await.unwrap();

        let other_workflow = Uuid::now_v7();
        let other_draft = Uuid::now_v7();
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Other tenant',?,?)")
            .bind(other_workflow).bind(other_tenant).bind(other_user).bind(other_department).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,definition_json,content_hash,updated_by) VALUES(?,?,?,'4.0',JSON_OBJECT(),'other-draft',?)")
            .bind(other_draft).bind(other_tenant).bind(other_workflow).bind(other_user).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_draft_resources(id,tenant_id,workflow_id,draft_id,node_id,node_name,resource_type,resource_id,operation_key) VALUES(?,?,?,?,'foreign','Foreign','credential',?,'use')")
            .bind(Uuid::now_v7()).bind(other_tenant).bind(other_workflow).bind(other_draft).bind(credential).execute(&pool).await.unwrap();

        let mut connection = pool.acquire().await.unwrap();
        let impact = impact_on_connection(
            &mut connection,
            &actor,
            EntityType::Credential,
            credential,
            page(),
            false,
        )
        .await
        .unwrap();
        assert_eq!(impact.total, 1, "other-tenant references must be ignored");
        assert_eq!(impact.references[0].source_name, "Draft consumer");
        assert_eq!(
            impact.references[0].node_name.as_deref(),
            Some("Support model")
        );
        drop(connection);

        let conflict = delete_entity(&state, &actor, EntityType::Credential, credential, 1)
            .await
            .unwrap_err();
        assert_eq!(conflict.code, "ENTITY_IN_USE");
        assert_eq!(
            conflict
                .details
                .as_ref()
                .and_then(|value| value.get("total"))
                .and_then(Value::as_u64),
            Some(1)
        );
        sqlx::query("DELETE FROM workflow_draft_resources WHERE tenant_id=? AND resource_id=?")
            .bind(tenant)
            .bind(credential)
            .execute(&pool)
            .await
            .unwrap();
        let version_conflict = delete_entity(&state, &actor, EntityType::Credential, credential, 0)
            .await
            .unwrap_err();
        assert_eq!(version_conflict.code, "VERSION_CONFLICT");
        delete_entity(&state, &actor, EntityType::Credential, credential, 1)
            .await
            .unwrap();
        assert_eq!(count(&pool, "credentials", credential).await, 0);
        let audited: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE tenant_id=? AND action='credential.deleted' AND target_id=?")
            .bind(tenant).bind(credential.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(audited, 1);

        let raced_credential =
            create_credential(&pool, tenant, department, user, "Preflight race credential").await;
        let mut connection = pool.acquire().await.unwrap();
        let preflight = impact_on_connection(
            &mut connection,
            &actor,
            EntityType::Credential,
            raced_credential,
            page(),
            false,
        )
        .await
        .unwrap();
        assert!(preflight.deletable);
        drop(connection);
        sqlx::query("INSERT INTO workflow_draft_resources(id,tenant_id,workflow_id,draft_id,node_id,node_name,resource_type,resource_id,operation_key) VALUES(?,?,?,?,'late-reference','Late reference','credential',?,'use')")
            .bind(Uuid::now_v7()).bind(tenant).bind(workflow).bind(draft).bind(raced_credential)
            .execute(&pool).await.unwrap();
        let raced = delete_entity(
            &state,
            &actor,
            EntityType::Credential,
            raced_credential,
            preflight.target_version,
        )
        .await
        .unwrap_err();
        assert_eq!(raced.code, "ENTITY_IN_USE");
        assert_eq!(
            raced
                .details
                .as_ref()
                .and_then(|value| value.get("references"))
                .and_then(Value::as_array)
                .and_then(|references| references.first())
                .and_then(|reference| reference.get("nodeName"))
                .and_then(Value::as_str),
            Some("Late reference")
        );

        verify_historical_and_direct_reference_queries(
            &pool, &actor, tenant, department, user, workflow,
        )
        .await;
        verify_mcp_tool_reference_creation_cannot_race_server_deletion(
            &pool, &actor, tenant, department, user, workflow, draft,
        )
        .await;
    }

    async fn verify_mcp_tool_reference_creation_cannot_race_server_deletion(
        pool: &sqlx::MySqlPool,
        actor: &AuthActor,
        tenant: Uuid,
        department: Uuid,
        user: Uuid,
        workflow: Uuid,
        draft: Uuid,
    ) {
        let server = Uuid::now_v7();
        let tool = Uuid::now_v7();
        sqlx::query("INSERT INTO mcp_servers(id,tenant_id,name,owner_department_id,current_version_number,created_by) VALUES(?,?,'Concurrent MCP',?,1,?)")
            .bind(server).bind(tenant).bind(department).bind(user).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO mcp_tools(id,tenant_id,server_id,name,current_version_number) VALUES(?,?,?,'concurrent_tool',1)")
            .bind(tool).bind(tenant).bind(server).execute(pool).await.unwrap();

        let mut delete_tx = pool.begin().await.unwrap();
        let impact = impact_on_connection(
            &mut delete_tx,
            actor,
            EntityType::McpServer,
            server,
            page(),
            true,
        )
        .await
        .unwrap();
        assert!(impact.deletable);

        let create_pool = pool.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let creator = tokio::spawn(async move {
            let mut tx = create_pool.begin().await.unwrap();
            let _ = started_tx.send(());
            lock_resource_targets(&mut tx, tenant, vec![("mcp_tool".into(), tool)]).await?;
            sqlx::query("INSERT INTO workflow_draft_resources(id,tenant_id,workflow_id,draft_id,node_id,node_name,resource_type,resource_id,operation_key) VALUES(?,?,?,?,'concurrent','Concurrent Tool','mcp_tool',?,'use')")
                .bind(Uuid::now_v7()).bind(tenant).bind(workflow).bind(draft).bind(tool)
                .execute(&mut *tx).await?;
            tx.commit().await?;
            crate::error::AppResult::Ok(())
        });
        started_rx.await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !creator.is_finished(),
            "reference creation must wait for the delete lock"
        );

        delete_owned_data(&mut delete_tx, tenant, EntityType::McpServer, server)
            .await
            .unwrap();
        delete_tx.commit().await.unwrap();
        let error = creator.await.unwrap().unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
        assert_eq!(count(pool, "mcp_tools", tool).await, 0);
        let dangling: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_draft_resources WHERE tenant_id=? AND resource_type='mcp_tool' AND resource_id=?")
            .bind(tenant).bind(tool).fetch_one(pool).await.unwrap();
        assert_eq!(dangling, 0);
    }

    async fn verify_historical_and_direct_reference_queries(
        pool: &sqlx::MySqlPool,
        actor: &AuthActor,
        tenant: Uuid,
        department: Uuid,
        user: Uuid,
        workflow: Uuid,
    ) {
        let credential =
            create_credential(pool, tenant, department, user, "Historical credential").await;
        let server = Uuid::now_v7();
        sqlx::query("INSERT INTO mcp_servers(id,tenant_id,name,owner_department_id,current_version_number,created_by) VALUES(?,?,'MCP history',?,2,?)")
            .bind(server).bind(tenant).bind(department).bind(user).execute(pool).await.unwrap();
        for (number, credential_id) in [(1_u64, Some(credential)), (2, None)] {
            sqlx::query("INSERT INTO mcp_server_versions(id,tenant_id,server_id,version_number,transport,endpoint,credential_id,configuration_json,configuration_hash,created_by) VALUES(?,?,?,?,'streamable_http','https://mcp.example.test',?,JSON_OBJECT(),?,?)")
                .bind(Uuid::now_v7()).bind(tenant).bind(server).bind(number).bind(credential_id)
                .bind(format!("hash-{number}")).bind(user).execute(pool).await.unwrap();
        }
        let mut connection = pool.acquire().await.unwrap();
        let impact = impact_on_connection(
            &mut connection,
            actor,
            EntityType::Credential,
            credential,
            page(),
            false,
        )
        .await
        .unwrap();
        assert!(
            impact
                .references
                .iter()
                .any(|reference| reference.source_type == "mcp_server_version"
                    && reference.source_name == "MCP history v1")
        );
        drop(connection);

        let child_department = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO departments(id,tenant_id,name,normalized_name) VALUES(?,?,'Data','data')",
        )
        .bind(child_department)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO rag_connections(id,tenant_id,name,endpoint,owner_department_id,configuration_json) VALUES(?,?,'Knowledge connection','https://rag.example.test',?,JSON_OBJECT())")
            .bind(Uuid::now_v7()).bind(tenant).bind(child_department).execute(pool).await.unwrap();
        let mut connection = pool.acquire().await.unwrap();
        let impact = impact_on_connection(
            &mut connection,
            actor,
            EntityType::Department,
            child_department,
            page(),
            false,
        )
        .await
        .unwrap();
        assert!(
            impact
                .references
                .iter()
                .any(|reference| reference.source_type == "knowledge_connection")
        );
        drop(connection);

        let version = Uuid::now_v7();
        sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'4.0',JSON_OBJECT(),'deletion-version',?)")
            .bind(version).bind(tenant).bind(workflow).bind(user).execute(pool).await.unwrap();
        let environment = Uuid::now_v7();
        sqlx::query("INSERT INTO workflow_environments(id,tenant_id,code,name) VALUES(?,?,'review','Review')")
            .bind(environment).bind(tenant).execute(pool).await.unwrap();
        let application = Uuid::now_v7();
        sqlx::query("INSERT INTO applications(id,tenant_id,workflow_id,name,slug,owner_user_id,owner_department_id) VALUES(?,? ,?,'Review app','review-app',?,?)")
            .bind(application).bind(tenant).bind(workflow).bind(user).bind(department).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO application_deployments(id,tenant_id,application_id,workflow_version_id,environment_id,sequence_number,created_by) VALUES(?,?,?,?,?,1,?)")
            .bind(Uuid::now_v7()).bind(tenant).bind(application).bind(version).bind(environment).bind(user).execute(pool).await.unwrap();
        let mut connection = pool.acquire().await.unwrap();
        let impact = impact_on_connection(
            &mut connection,
            actor,
            EntityType::Environment,
            environment,
            page(),
            false,
        )
        .await
        .unwrap();
        assert!(
            impact
                .references
                .iter()
                .any(|reference| reference.source_type == "application_deployment")
        );
        let root = impact_on_connection(
            &mut connection,
            actor,
            EntityType::Department,
            department,
            page(),
            false,
        )
        .await
        .unwrap();
        assert_eq!(root.references[0].source_module, "system");
    }

    async fn seed_identity(
        pool: &sqlx::MySqlPool,
        tenant: Uuid,
        department: Uuid,
        user: Uuid,
        suffix: &str,
    ) {
        sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,?,?)")
            .bind(tenant)
            .bind(format!("Tenant {suffix}"))
            .bind(format!("tenant-{suffix}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
            .bind(department).bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,?,?,?)")
            .bind(user).bind(tenant).bind(format!("user-{suffix}")).bind(format!("user-{suffix}")).bind(format!("User {suffix}")).execute(pool).await.unwrap();
    }

    async fn create_credential(
        pool: &sqlx::MySqlPool,
        tenant: Uuid,
        department: Uuid,
        user: Uuid,
        name: &str,
    ) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,storage_mode,masked_hint,owner_department_id,created_by) VALUES(?, ?, ?,'bearer','local_encrypted','****',?,?)")
            .bind(id).bind(tenant).bind(name).bind(department).bind(user).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,provider,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,1,'local_encrypted','aes-256-gcm','test',X'00',X'00',?)")
            .bind(Uuid::now_v7()).bind(tenant).bind(id).bind(user).execute(pool).await.unwrap();
        id
    }

    async fn count(pool: &sqlx::MySqlPool, table: &str, id: Uuid) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id=?"))
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn page() -> DeletionImpactQuery {
        DeletionImpactQuery {
            page: Some(1),
            page_size: Some(100),
        }
    }

    fn actor(tenant_id: Uuid, department_id: Uuid, user_id: Uuid) -> AuthActor {
        AuthActor {
            tenant_id,
            user_id,
            username: "admin".into(),
            display_name: "Admin".into(),
            department_id,
            permissions: vec![
                "credential:delete".into(),
                "department:delete".into(),
                "workflow:delete".into(),
            ],
            roles: vec!["company_admin".into()],
            company_admin: true,
        }
    }

    fn auth_settings() -> AuthSettings {
        AuthSettings {
            signing_secret: SecretString::from(
                "safe-deletion-test-signing-secret-32-chars".to_owned(),
            ),
            issuer: "agentx-test".into(),
            audience: "agentx-test".into(),
            access_ttl_seconds: 900,
            refresh_ttl_seconds: 604_800,
            change_password_ttl_seconds: 600,
            cookie_secure: false,
            login_max_failures: 5,
            login_failure_window_seconds: 900,
            login_lock_seconds: 900,
        }
    }
}
