use axum::{Json, Router, extract::State, routing::get};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

use crate::{
    api_error::{ApiError, ApiResult},
    control_api::{Actor, ControlApiState},
};

const DIMENSIONS: &[&str] = &[
    "execution_concurrency",
    "node_concurrency",
    "sandbox_concurrency",
    "agent_iterations",
    "tokens",
    "cost_micros",
    "artifact_bytes",
    "cpu_millis",
    "memory_bytes",
    "pids",
    "disk_bytes",
    "ttl_seconds",
];

pub fn routes() -> Router<ControlApiState> {
    Router::new()
        .route("/api/v1/dashboard/summary", get(dashboard))
        .route("/api/v1/runtime/status", get(runtime_status))
        .route("/api/v1/runtime/capabilities", get(capabilities))
        .route("/api/v1/runtime/quotas", get(quotas).put(update_quotas))
}

async fn dashboard(State(state): State<ControlApiState>, actor: Actor) -> ApiResult<Json<Value>> {
    let workflows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflows WHERE tenant_id=? AND status='active'")
            .bind(actor.tenant_id)
            .fetch_one(&state.pool)
            .await?;
    let applications: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM applications WHERE tenant_id=? AND status='active'",
    )
    .bind(actor.tenant_id)
    .fetch_one(&state.pool)
    .await?;
    let approvals:i64=sqlx::query_scalar("SELECT COUNT(*) FROM approval_task_projection WHERE tenant_id=? AND status IN ('pending','claimed') AND projection_deleted=FALSE").bind(actor.tenant_id).fetch_one(&state.pool).await?;
    let evaluations:i64=sqlx::query_scalar("SELECT COUNT(*) FROM evaluation_runs WHERE tenant_id=? AND status IN ('queued','running') AND projection_deleted=FALSE").bind(actor.tenant_id).fetch_one(&state.pool).await?;
    Ok(Json(
        json!({"workflowCount":workflows,"applicationCount":applications,"runningExecutions":Value::Null,"executionsToday":Value::Null,"succeededToday":Value::Null,"failedToday":Value::Null,"costMicrosToday":Value::Null,"pendingApprovals":approvals,"runningEvaluations":evaluations,"runtimeState":"runtime_query_required"}),
    ))
}

async fn runtime_status(
    State(state): State<ControlApiState>,
    actor: Actor,
) -> ApiResult<Json<Value>> {
    actor.require("runtime:view")?;
    let response = state
        .http
        .get(format!("{}/health/ready", state.runtime_query_url))
        .send()
        .await;
    let (status, detail) = match response {
        Ok(response) if response.status().is_success() => {
            ("ready", response.text().await.unwrap_or_default())
        }
        Ok(response) => ("degraded", format!("HTTP {}", response.status())),
        Err(error) => ("unavailable", error.to_string()),
    };
    Ok(Json(
        json!({"components":[{"component":"runtime","status":status,"instances":Value::Null,"queueDepth":Value::Null,"lastHeartbeat":Value::Null}],"running":Value::Null,"waiting":Value::Null,"failedToday":Value::Null,"activeSandboxes":Value::Null,"sandboxCompatibility":Value::Null,"detail":detail}),
    ))
}

async fn capabilities(
    State(state): State<ControlApiState>,
    actor: Actor,
) -> ApiResult<Json<Value>> {
    actor.require("runtime:view")?;
    let health = state
        .http
        .get(format!("{}/health/ready", state.runtime_query_url))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    Ok(Json(capability_response(health)?))
}

fn capability_response(runtime_ready: bool) -> ApiResult<Value> {
    let heartbeat_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(ApiError::internal)?;
    Ok(json!(
        agentx_node_protocol::ALL_RUNTIME_CAPABILITIES
            .iter()
            .map(|capability| json!({
                "instanceId":"v2-runtime-contract",
                "capability":capability.to_string(),
                "nodeProtocolVersion":agentx_node_protocol::NODE_PROTOCOL_VERSION,
                "irSchemaVersions":[1],
                "compilerVersionMin":agentx_runtime::COMPILER_VERSION,
                "compilerVersionMax":agentx_runtime::COMPILER_VERSION,
                "manifestHashes":[],
                "status":if runtime_ready { "ready" } else { "unavailable" },
                "heartbeatAt":heartbeat_at,
            }))
            .collect::<Vec<_>>()
    ))
}

async fn quotas(State(state): State<ControlApiState>, actor: Actor) -> ApiResult<Json<Value>> {
    actor.require("runtime:view")?;
    let rows=sqlx::query("SELECT p.dimension_key,CAST(p.hard_limit AS CHAR) hard_limit,p.period_seconds,p.version,CAST(COALESCE((SELECT SUM(r.amount) FROM quota_admin_reservations r WHERE r.tenant_id=p.tenant_id AND r.dimension_key=p.dimension_key AND r.status='active' AND r.expires_at>UTC_TIMESTAMP(6)),0) AS CHAR) active_reserved,CAST(COALESCE((SELECT SUM(u.amount) FROM quota_usage_projection u WHERE u.tenant_id=p.tenant_id AND u.dimension_key=p.dimension_key),0) AS CHAR) period_usage FROM quota_policies p WHERE p.tenant_id=? ORDER BY p.dimension_key").bind(actor.tenant_id).fetch_all(&state.pool).await?;
    let values=rows.into_iter().map(|r|json!({"dimension":r.try_get::<String,_>("dimension_key").unwrap(),"hardLimit":r.try_get::<String,_>("hard_limit").unwrap(),"periodSeconds":r.try_get::<Option<u64>,_>("period_seconds").unwrap(),"activeReserved":r.try_get::<String,_>("active_reserved").unwrap(),"periodUsage":r.try_get::<String,_>("period_usage").unwrap(),"version":r.try_get::<u64,_>("version").unwrap()})).collect::<Vec<_>>();
    Ok(Json(json!(values)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateRequest {
    policies: Vec<PolicyInput>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyInput {
    dimension: String,
    hard_limit: String,
    period_seconds: Option<u64>,
}
async fn update_quotas(
    State(state): State<ControlApiState>,
    actor: Actor,
    Json(input): Json<UpdateRequest>,
) -> ApiResult<Json<Value>> {
    actor.require("runtime:manage")?;
    if input.policies.is_empty() {
        return Err(ApiError::bad_request(
            "QUOTA_POLICY_REQUIRED",
            "At least one quota policy is required",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut tx = state.pool.begin().await?;
    for policy in input.policies {
        if !DIMENSIONS.contains(&policy.dimension.as_str())
            || !seen.insert(policy.dimension.clone())
            || policy
                .hard_limit
                .parse::<f64>()
                .ok()
                .is_none_or(|v| !v.is_finite() || v < 0.0)
        {
            return Err(ApiError::bad_request(
                "INVALID_QUOTA_POLICY",
                "Quota dimension or limit is invalid",
            ));
        }
        sqlx::query("INSERT INTO quota_policies(tenant_id,dimension_key,hard_limit,period_seconds,updated_by) VALUES(?,?,?,?,?) ON DUPLICATE KEY UPDATE hard_limit=VALUES(hard_limit),period_seconds=VALUES(period_seconds),version=version+1,updated_by=VALUES(updated_by)").bind(actor.tenant_id).bind(policy.dimension).bind(policy.hard_limit).bind(policy.period_seconds).bind(actor.user_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    quotas(State(state), actor).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezes_twelve_quota_dimensions() {
        assert_eq!(DIMENSIONS.len(), 12);
    }

    #[test]
    fn capability_response_preserves_the_existing_browser_array_contract() {
        let response = capability_response(true).unwrap();
        let items = response.as_array().unwrap();
        assert_eq!(
            items.len(),
            agentx_node_protocol::ALL_RUNTIME_CAPABILITIES.len()
        );
        assert!(items.iter().all(|item| {
            item["status"] == "ready"
                && item["nodeProtocolVersion"] == agentx_node_protocol::NODE_PROTOCOL_VERSION
                && item["heartbeatAt"].is_string()
        }));
    }
}
