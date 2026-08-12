use agentx_api_types::PageResponse;
use agentx_domain::{WorkflowDefinition, canonical_content_hash};
use agentx_node_protocol::{NodeManifestVersion, NodePort, OutputCardinality, PortKind};
use agentx_runtime::NodeRegistry;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{MySql, MySqlPool, Row, Transaction};
use std::collections::HashSet;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub category: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionSummary {
    pub node_type: String,
    pub version: u32,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub icon_key: String,
    pub capability: String,
    pub execution_style: String,
    pub manifest_hash: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionDetail {
    pub node_type: String,
    pub version: u32,
    pub manifest_hash: String,
    pub manifest: Value,
}

#[derive(Clone, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeProviderQuery {
    pub search: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeProviderOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeProviderOptionsResponse {
    pub items: Vec<NodeProviderOption>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeProviderBackend {
    WorkflowVersions,
}

pub type NodeDefinitionPage = PageResponse<NodeDefinitionSummary>;

pub async fn reconcile_builtin_catalog(pool: &MySqlPool) -> anyhow::Result<()> {
    let mut transaction = pool.begin().await?;
    sqlx::query("UPDATE node_definitions SET status='disabled' WHERE tenant_id IS NULL AND node_type IN ('sub_workflow','mcp_tool','skill')")
        .execute(&mut *transaction)
        .await?;
    for manifest in NodeRegistry::m5_defaults().manifests() {
        if manifest.node_type == "sub_workflow" {
            continue;
        }
        let manifest_value = serde_json::to_value(manifest)?;
        let manifest_hash = canonical_content_hash(&manifest_value)?;
        let definition_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM node_definitions WHERE tenant_id IS NULL AND node_type=? FOR UPDATE",
        )
        .bind(&manifest.node_type)
        .fetch_optional(&mut *transaction)
        .await?;
        let definition_id = if let Some(id) = definition_id {
            sqlx::query("UPDATE node_definitions SET display_name=?,source_type='platform',status='active' WHERE id=?")
                .bind(&manifest.display_name)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            id
        } else {
            let id = Uuid::now_v7();
            sqlx::query("INSERT INTO node_definitions(id,tenant_id,node_type,display_name,source_type,status) VALUES(?,NULL,?,?,'platform','active')")
                .bind(id)
                .bind(&manifest.node_type)
                .bind(&manifest.display_name)
                .execute(&mut *transaction)
                .await?;
            id
        };
        let execution_style = serde_json::to_value(&manifest.execution_style)?;
        let side_effect_level = serde_json::to_value(&manifest.side_effect_level)?;
        sqlx::query("INSERT INTO node_definition_versions(id,node_definition_id,version_number,protocol_version,manifest_json,manifest_hash,capability,execution_style,side_effect_level,resume_policy) VALUES(?,?,?,?,?,?,?,?,?,NULL) ON DUPLICATE KEY UPDATE protocol_version=VALUES(protocol_version),manifest_json=VALUES(manifest_json),manifest_hash=VALUES(manifest_hash),capability=VALUES(capability),execution_style=VALUES(execution_style),side_effect_level=VALUES(side_effect_level)")
            .bind(Uuid::now_v7())
            .bind(definition_id)
            .bind(manifest.version)
            .bind(&manifest.protocol_version)
            .bind(&manifest_value)
            .bind(&manifest_hash)
            .bind(manifest.capability.as_str())
            .bind(execution_style.as_str().unwrap_or_default())
            .bind(side_effect_level.as_str().unwrap_or_default())
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn registry_for_tenant(pool: &MySqlPool, tenant_id: Uuid) -> AppResult<NodeRegistry> {
    let rows = sqlx::query("SELECT v.manifest_json FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.status='active' AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.node_type,v.version_number,d.tenant_id IS NULL")
        .bind(tenant_id)
        .fetch_all(pool)
        .await?;
    if rows.is_empty() {
        return Ok(NodeRegistry::m5_defaults());
    }
    let mut registry = NodeRegistry::default();
    let mut seen = HashSet::new();
    for row in rows {
        let manifest: NodeManifestVersion =
            serde_json::from_value(row.try_get("manifest_json")?).map_err(AppError::internal)?;
        if seen.insert((manifest.node_type.clone(), manifest.version)) {
            registry.register(manifest).map_err(AppError::internal)?;
        }
    }
    Ok(registry)
}

pub async fn register_composite_manifest(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    workflow_version_id: Uuid,
    workflow_name: &str,
    version_number: u64,
    definition: &WorkflowDefinition,
) -> AppResult<()> {
    let mut manifest = NodeRegistry::m5_defaults()
        .get("sub_workflow", 1)
        .expect("built-in sub-workflow manifest")
        .clone();
    manifest.node_type = format!("workflow.{}", workflow_version_id.simple());
    manifest.display_name = format!("{workflow_name} v{version_number}");
    manifest.description = "Immutable published workflow".into();
    manifest.category = "workflows".into();
    manifest.keywords = vec![workflow_name.into(), "workflow".into(), "composite".into()];
    manifest.providers.clear();
    for localization in manifest.localizations.values_mut() {
        localization.display_name = manifest.display_name.clone();
        localization.description = manifest.description.clone();
        localization.keywords = manifest.keywords.clone();
    }
    let input_schema = definition.start.inputs.clone();
    let workflow_id: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM workflow_versions WHERE tenant_id=? AND id=?")
            .bind(tenant_id)
            .bind(workflow_version_id)
            .fetch_one(&mut **transaction)
            .await?;
    manifest.parameter_schema = serde_json::json!({
        "type":"object",
        "x-agentx-workflowId":workflow_id,
        "x-agentx-workflowVersionId":workflow_version_id,
        "x-agentx-versionNumber":version_number,
        "x-agentx-contextContract":definition.start.contexts,
        "properties":{
            "workflowVersionId":{
                "type":"string",
                "const":workflow_version_id,
                "default":workflow_version_id,
                "readOnly":true
            },
            "inputs":{
                "allOf":[input_schema],
                "default":{},
                "templatable":true,
                "allowedNamespaces":["inputs","outputs","contexts"],
                "expectedType":"object"
            }
        },
        "required":["workflowVersionId","inputs"],
        "additionalProperties":false
    });
    manifest.ui_schema.order = vec!["inputs".into()];
    manifest.ui_schema.fields.insert(
        "workflowVersionId".into(),
        serde_json::json!({"control":"hidden"}),
    );
    manifest.ui_schema.fields.insert(
        "inputs".into(),
        serde_json::json!({"control":"json","label":"Inputs"}),
    );
    let output_properties = definition
        .end
        .outputs
        .iter()
        .map(|(name, output)| (name.clone(), output.schema.clone()))
        .collect::<serde_json::Map<_, _>>();
    let required = definition
        .end
        .outputs
        .iter()
        .filter(|(_, output)| output.required)
        .map(|(name, _)| Value::String(name.clone()))
        .collect::<Vec<_>>();
    manifest.output_schema = serde_json::json!({
        "type":"object",
        "properties":output_properties,
        "required":required,
        "additionalProperties":false
    });
    if !manifest
        .output_ports
        .iter()
        .any(|port| port.name == "error")
    {
        manifest.output_ports.push(NodePort {
            name: "error".into(),
            kind: PortKind::Error,
            required: false,
            variadic: false,
        });
    }
    manifest
        .output_cardinality
        .insert("main".into(), OutputCardinality::ExactlyOne);
    manifest
        .output_cardinality
        .insert("error".into(), OutputCardinality::ZeroOrMany);
    manifest.output_port_schemas.insert(
        "error".into(),
        serde_json::json!({
            "type":"object",
            "properties":{
                "code":{"type":"string"},
                "message":{"type":"string"},
                "details":{},
                "sourceNodeId":{"type":"string"},
                "sourceNodeKey":{"type":"string"},
                "nodeExecutionId":{"type":"string"},
                "runIndex":{"type":"integer"},
                "iterationIndex":{"type":"integer"},
                "retryable":{"type":"boolean"}
            },
            "required":["code","message","details","sourceNodeId","sourceNodeKey","nodeExecutionId","runIndex","iterationIndex","retryable"],
            "additionalProperties":false
        }),
    );
    let manifest_value = serde_json::to_value(&manifest).map_err(AppError::internal)?;
    let manifest_hash = canonical_content_hash(&manifest_value).map_err(AppError::internal)?;
    let definition_id = Uuid::now_v7();
    sqlx::query("INSERT INTO node_definitions(id,tenant_id,node_type,display_name,source_type,status) VALUES(?,?,?,?, 'composite','active')")
        .bind(definition_id)
        .bind(tenant_id)
        .bind(&manifest.node_type)
        .bind(&manifest.display_name)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO node_definition_versions(id,node_definition_id,version_number,protocol_version,manifest_json,manifest_hash,capability,execution_style,side_effect_level,resume_policy) VALUES(?,?,?,?,?,?,?,?,?,NULL)")
        .bind(Uuid::now_v7())
        .bind(definition_id)
        .bind(manifest.version)
        .bind(&manifest.protocol_version)
        .bind(manifest_value)
        .bind(manifest_hash)
        .bind(manifest.capability.as_str())
        .bind("sub_workflow")
        .bind("none")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/node-definitions", params(NodeDefinitionQuery))]
pub async fn list_node_definitions(
    State(state): State<AppState>,
    actor: AuthActor,
    Query(query): Query<NodeDefinitionQuery>,
) -> AppResult<Json<NodeDefinitionPage>> {
    actor.require("workflow:view")?;
    let rows = sqlx::query("SELECT v.manifest_json,v.manifest_hash FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.status='active' AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.node_type,v.version_number,d.tenant_id IS NULL")
        .bind(actor.tenant_id).fetch_all(&state.pool).await?;
    let search = query
        .search
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let category = query.category.as_deref().unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut manifests = rows
        .into_iter()
        .filter_map(|row| {
            let manifest =
                serde_json::from_value::<NodeManifestVersion>(row.try_get("manifest_json").ok()?)
                    .ok()?;
            if !seen.insert((manifest.node_type.clone(), manifest.version))
                || (!category.is_empty() && manifest.category != category)
                || (!search.is_empty()
                    && !manifest.node_type.to_ascii_lowercase().contains(&search)
                    && !manifest.display_name.to_ascii_lowercase().contains(&search)
                    && !manifest
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_ascii_lowercase().contains(&search)))
            {
                return None;
            }
            Some((manifest, row.try_get::<String, _>("manifest_hash").ok()?))
        })
        .collect::<Vec<_>>();
    manifests.sort_by(|left, right| {
        left.0
            .category
            .cmp(&right.0.category)
            .then(left.0.display_name.cmp(&right.0.display_name))
            .then(left.0.version.cmp(&right.0.version))
    });
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 100);
    let total = manifests.len() as u64;
    let start = ((page - 1) * page_size) as usize;
    let items = manifests
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(|(manifest, hash)| summary(manifest, hash))
        .collect();
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(get, path = "/api/v1/node-definitions/{node_type}/versions/{version}", params(("node_type" = String, Path), ("version" = u32, Path)))]
pub async fn get_node_definition(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((node_type, version)): Path<(String, u32)>,
) -> AppResult<Json<NodeDefinitionDetail>> {
    actor.require("workflow:view")?;
    let row = sqlx::query("SELECT v.manifest_json,v.manifest_hash FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.status='active' AND d.node_type=? AND v.version_number=? AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.tenant_id IS NULL LIMIT 1")
        .bind(&node_type).bind(version).bind(actor.tenant_id).fetch_optional(&state.pool).await?
        .ok_or_else(|| AppError::not_found("Node definition"))?;
    let value: Value = row.try_get("manifest_json")?;
    let hash: String = row.try_get("manifest_hash")?;
    Ok(Json(NodeDefinitionDetail {
        node_type,
        version,
        manifest_hash: hash,
        manifest: value,
    }))
}

#[utoipa::path(get, path = "/api/v1/node-definitions/{node_type}/versions/{version}/providers/{provider}", params(("node_type" = String, Path), ("version" = u32, Path), ("provider" = String, Path), NodeProviderQuery))]
pub async fn list_node_provider_options(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((node_type, version, provider)): Path<(String, u32, String)>,
    Query(query): Query<NodeProviderQuery>,
) -> AppResult<Json<NodeProviderOptionsResponse>> {
    actor.require("workflow:view")?;
    let registry = registry_for_tenant(&state.pool, actor.tenant_id).await?;
    let manifest = registry
        .get(&node_type, version)
        .ok_or_else(|| AppError::not_found("Node definition"))?;
    let backend = provider_backend(manifest, &provider)?;
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let items = match backend {
        NodeProviderBackend::WorkflowVersions => {
            workflow_version_options(&state, &actor, &search, limit).await?
        }
    };
    Ok(Json(NodeProviderOptionsResponse { items }))
}

fn provider_backend(
    manifest: &NodeManifestVersion,
    provider: &str,
) -> AppResult<NodeProviderBackend> {
    if !manifest.providers.iter().any(|item| item == provider) {
        return Err(AppError::not_found("Node option provider"));
    }
    match provider {
        "workflow_versions" => Ok(NodeProviderBackend::WorkflowVersions),
        _ => Err(AppError::unprocessable(
            "NODE_PROVIDER_UNSUPPORTED",
            format!("Provider '{provider}' is not available"),
        )),
    }
}

async fn workflow_version_options(
    state: &AppState,
    actor: &AuthActor,
    search: &str,
    limit: u32,
) -> AppResult<Vec<NodeProviderOption>> {
    let rows = if actor.company_admin {
        sqlx::query("SELECT v.id,v.version_number,w.name FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE v.tenant_id=? AND w.status='active' AND (?='%%' OR w.name LIKE ?) ORDER BY w.name,v.version_number DESC LIMIT ?")
            .bind(actor.tenant_id).bind(search).bind(search).bind(limit).fetch_all(&state.pool).await?
    } else {
        sqlx::query("SELECT v.id,v.version_number,w.name FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE v.tenant_id=? AND w.status='active' AND (w.owner_user_id=? OR w.visibility='company' OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.workflow_id=w.id AND wm.user_id=?) OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id))) AND (?='%%' OR w.name LIKE ?) ORDER BY w.name,v.version_number DESC LIMIT ?")
            .bind(actor.tenant_id).bind(actor.user_id).bind(actor.user_id).bind(actor.user_id)
            .bind(search).bind(search).bind(limit).fetch_all(&state.pool).await?
    };
    rows.into_iter()
        .map(|row| {
            let name: String = row.try_get("name")?;
            let version_number: u64 = row.try_get("version_number")?;
            Ok(NodeProviderOption {
                value: row.try_get::<Uuid, _>("id")?.to_string(),
                label: format!("{name} · v{version_number}"),
                description: Some(format!("Published workflow version {version_number}")),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

fn summary(manifest: NodeManifestVersion, manifest_hash: String) -> NodeDefinitionSummary {
    NodeDefinitionSummary {
        node_type: manifest.node_type,
        version: manifest.version,
        display_name: manifest.display_name,
        description: manifest.description,
        category: manifest.category,
        keywords: manifest.keywords,
        icon_key: manifest.icon_key,
        capability: manifest.capability.as_str().into(),
        execution_style: serde_json::to_value(manifest.execution_style)
            .expect("style serializes")
            .as_str()
            .unwrap_or_default()
            .into(),
        manifest_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_must_be_declared_by_the_exact_manifest() {
        let registry = NodeRegistry::m5_defaults();
        let sub_workflow = registry
            .get("sub_workflow", 1)
            .expect("sub-workflow manifest");
        assert_eq!(
            provider_backend(sub_workflow, "workflow_versions").unwrap(),
            NodeProviderBackend::WorkflowVersions
        );

        let agent = registry.get("agent", 1).expect("agent manifest");
        let undeclared = provider_backend(agent, "workflow_versions").unwrap_err();
        assert_eq!(undeclared.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn declared_providers_without_a_platform_backend_are_rejected() {
        let mut manifest = NodeRegistry::m5_defaults()
            .get("sub_workflow", 1)
            .expect("sub-workflow manifest")
            .clone();
        manifest.providers.push("external_browser_call".into());

        let unsupported = provider_backend(&manifest, "external_browser_call").unwrap_err();
        assert_eq!(unsupported.code, "NODE_PROVIDER_UNSUPPORTED");
    }
}
