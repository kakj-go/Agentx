use std::collections::HashSet;

use agentx_api_types::PageResponse;
use agentx_node_protocol::NodeManifestVersion;
use agentx_runtime::NodeRegistry;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    api_error::{ApiError, ApiResult},
    control_api::{Actor, ControlApiState},
};

pub fn routes() -> Router<ControlApiState> {
    Router::new()
        .route("/api/v1/node-definitions", get(list_node_definitions))
        .route(
            "/api/v1/node-definitions/{node_type}/versions/{version}",
            get(get_node_definition),
        )
        .route(
            "/api/v1/node-definitions/{node_type}/versions/{version}/providers/{provider}",
            get(list_provider_options),
        )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    search: Option<String>,
    category: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeDefinitionSummary {
    node_type: String,
    version: u32,
    display_name: String,
    description: String,
    category: String,
    keywords: Vec<String>,
    icon_key: String,
    capability: String,
    execution_style: String,
    manifest_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeDefinitionDetail {
    node_type: String,
    version: u32,
    manifest_hash: String,
    manifest: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderQuery {
    search: Option<String>,
    limit: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderOption {
    value: String,
    label: String,
    description: Option<String>,
}

#[derive(Serialize)]
struct ProviderOptionsResponse {
    items: Vec<ProviderOption>,
}

async fn list_node_definitions(
    State(state): State<ControlApiState>,
    actor: Actor,
    Query(query): Query<CatalogQuery>,
) -> ApiResult<Json<PageResponse<NodeDefinitionSummary>>> {
    actor.require("workflow:view")?;
    let mut manifests = manifests_for_tenant(&state, actor.tenant_id).await?;
    let search = query.search.unwrap_or_default().trim().to_ascii_lowercase();
    let category = query.category.unwrap_or_default();
    manifests.retain(|(manifest, _)| {
        (category.is_empty() || manifest.category == category)
            && (search.is_empty()
                || manifest.node_type.to_ascii_lowercase().contains(&search)
                || manifest.display_name.to_ascii_lowercase().contains(&search)
                || manifest
                    .keywords
                    .iter()
                    .any(|keyword| keyword.to_ascii_lowercase().contains(&search)))
    });
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
    let items = manifests
        .into_iter()
        .skip(((page - 1) * page_size) as usize)
        .take(page_size as usize)
        .map(|(manifest, manifest_hash)| summary(manifest, manifest_hash))
        .collect();
    Ok(Json(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

async fn get_node_definition(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path((node_type, version)): Path<(String, u32)>,
) -> ApiResult<Json<NodeDefinitionDetail>> {
    actor.require("workflow:view")?;
    let (manifest, hash) = manifests_for_tenant(&state, actor.tenant_id)
        .await?
        .into_iter()
        .find(|(manifest, _)| manifest.node_type == node_type && manifest.version == version)
        .ok_or_else(|| ApiError::not_found("Node definition"))?;
    Ok(Json(NodeDefinitionDetail {
        node_type,
        version,
        manifest_hash: hash,
        manifest: serde_json::to_value(manifest).map_err(ApiError::internal)?,
    }))
}

async fn list_provider_options(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path((node_type, version, provider)): Path<(String, u32, String)>,
    Query(query): Query<ProviderQuery>,
) -> ApiResult<Json<ProviderOptionsResponse>> {
    actor.require("workflow:view")?;
    let manifest = manifests_for_tenant(&state, actor.tenant_id)
        .await?
        .into_iter()
        .find(|(manifest, _)| manifest.node_type == node_type && manifest.version == version)
        .map(|(manifest, _)| manifest)
        .ok_or_else(|| ApiError::not_found("Node definition"))?;
    if !manifest.providers.iter().any(|value| value == &provider) {
        return Err(ApiError::not_found("Node option provider"));
    }
    if provider != "workflow_versions" {
        return Err(ApiError::unprocessable(
            "NODE_PROVIDER_UNSUPPORTED",
            format!("Provider '{provider}' is not available"),
        ));
    }
    let search = format!("%{}%", query.search.unwrap_or_default().trim());
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = sqlx::query("SELECT v.id,v.version_number,w.name FROM workflow_versions v JOIN workflows w ON w.tenant_id=v.tenant_id AND w.id=v.workflow_id WHERE v.tenant_id=? AND w.status='active' AND (?='%%' OR w.name LIKE ?) AND (w.owner_user_id=? OR w.visibility='company' OR EXISTS(SELECT 1 FROM workflow_members wm WHERE wm.tenant_id=w.tenant_id AND wm.workflow_id=w.id AND wm.user_id=?)) ORDER BY w.name,v.version_number DESC LIMIT ?")
        .bind(actor.tenant_id).bind(&search).bind(&search).bind(actor.user_id).bind(actor.user_id).bind(limit).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let version_number: u64 = row.try_get("version_number")?;
            let name: String = row.try_get("name")?;
            Ok(ProviderOption {
                value: row.try_get::<Uuid, _>("id")?.to_string(),
                label: format!("{name} · v{version_number}"),
                description: Some(format!("Published workflow version {version_number}")),
            })
        })
        .collect::<Result<_, sqlx::Error>>()?;
    Ok(Json(ProviderOptionsResponse { items }))
}

async fn manifests_for_tenant(
    state: &ControlApiState,
    tenant: Uuid,
) -> ApiResult<Vec<(NodeManifestVersion, String)>> {
    let rows = sqlx::query("SELECT v.manifest_json,v.manifest_hash FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.status='active' AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.tenant_id IS NULL,d.node_type,v.version_number")
        .bind(tenant).fetch_all(&state.pool).await?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let manifest: NodeManifestVersion =
            serde_json::from_value(row.try_get("manifest_json")?).map_err(ApiError::internal)?;
        if seen.insert((manifest.node_type.clone(), manifest.version)) {
            result.push((manifest, row.try_get("manifest_hash")?));
        }
    }
    for manifest in NodeRegistry::m5_defaults().manifests() {
        if seen.insert((manifest.node_type.clone(), manifest.version)) {
            let value = serde_json::to_value(manifest).map_err(ApiError::internal)?;
            let hash = agentx_domain::canonical_content_hash(&value).map_err(ApiError::internal)?;
            result.push((manifest.clone(), hash));
        }
    }
    Ok(result)
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
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default(),
        manifest_hash,
    }
}
