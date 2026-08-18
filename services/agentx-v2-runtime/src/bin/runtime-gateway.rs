use agentx_v2_runtime::RuntimeState;
use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
};
use futures::StreamExt;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("openapi") {
        let path = arguments
            .next()
            .unwrap_or_else(|| "openapi/trigger-gateway.json".into());
        anyhow::ensure!(arguments.next().is_none(), "unexpected argument");
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&agentx_v2_runtime::gateway::public_openapi())?,
        )?;
        return Ok(());
    }
    let state = RuntimeState::gateway_from_env().await?;
    let lifecycle = agentx_service_kit::ServiceLifecycle::default();
    let metrics = agentx_service_kit::MetricsRegistry::default();
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("runtime_mysql", true).await;
    health.register("runtime_object_storage", true).await;
    health.set_status("runtime_mysql", "ready").await;
    health.set_status("runtime_object_storage", "ready").await;
    let probe_state = state.clone();
    let probe_health = health.clone();
    let probe_lifecycle = lifecycle.clone();
    let probe_progress = agentx_service_kit::RoleProgressWatchdog::start(
        "dependency-probe",
        Duration::from_secs(agentx_service_kit::ROLE_WATCHDOG_TIMEOUT_SECONDS),
        health.clone(),
        lifecycle.clone(),
        metrics.clone(),
    )
    .await;
    tokio::spawn(async move {
        while !probe_lifecycle.is_draining() {
            let started = std::time::Instant::now();
            match sqlx::query("SELECT 1").execute(&probe_state.pool).await {
                Ok(_) => probe_health.set_status("runtime_mysql", "ready").await,
                Err(error) => {
                    probe_health
                        .set_status("runtime_mysql", "unavailable")
                        .await;
                    tracing::warn!(%error, "Runtime Gateway MySQL readiness probe failed");
                }
            }
            match probe_state.objects.list(None).next().await.transpose() {
                Ok(_) => {
                    probe_health
                        .set_status("runtime_object_storage", "ready")
                        .await;
                }
                Err(error) => {
                    probe_health
                        .set_status("runtime_object_storage", "unavailable")
                        .await;
                    tracing::warn!(%error, "Runtime Gateway object storage readiness probe failed");
                }
            }
            probe_progress.processed_since(started).await;
            tokio::select! {
                () = probe_lifecycle.cancelled() => break,
                () = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    });
    let router = Router::new()
        .route(
            "/internal/runtime/v1/events:export",
            get(agentx_v2_runtime::event_export::export_events),
        )
        .route(
            "/internal/runtime/v1/governance-snapshots:export",
            post(agentx_v2_runtime::event_export::export_governance_snapshot),
        )
        .route(
            "/internal/runtime/v1/objects:upload",
            post(agentx_v2_runtime::object_upload::upload_object),
        )
        .route(
            "/internal/runtime/v1/bundles:prepare",
            post(agentx_v2_runtime::publish::prepare_bundle),
        )
        .route(
            "/internal/runtime/v1/admission-commands:apply",
            post(agentx_v2_runtime::publish::apply_admission),
        )
        .route(
            "/internal/runtime/v1/deployments:activate",
            post(agentx_v2_runtime::publish::activate_deployment),
        )
        .route(
            "/internal/runtime/v1/deployments:rollback",
            post(agentx_v2_runtime::publish::rollback_deployment),
        )
        .route(
            "/internal/runtime/v1/deployments:disable",
            post(agentx_v2_runtime::publish::disable_deployment),
        )
        .route(
            "/internal/runtime/v1/query/invocations:search",
            post(agentx_v2_runtime::query::search_invocations),
        )
        .route(
            "/internal/runtime/v1/query/invocations/{id}",
            get(agentx_v2_runtime::query::get_invocation),
        )
        .route(
            "/internal/runtime/v1/query/executions:search",
            post(agentx_v2_runtime::query::search_executions),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}",
            get(agentx_v2_runtime::query::get_execution),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/nodes",
            get(agentx_v2_runtime::query::get_execution_nodes),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/nodes/{node_execution_id}",
            get(agentx_v2_runtime::query::get_execution_node),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/events",
            get(agentx_v2_runtime::query::get_execution_events),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/waits",
            get(agentx_v2_runtime::query::get_execution_waits),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/checkpoints",
            get(agentx_v2_runtime::query::get_execution_checkpoints),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/runtime-details",
            get(agentx_v2_runtime::query::get_execution_runtime_details),
        )
        .route(
            "/internal/runtime/v1/query/executions/{id}/artifacts/{artifact_id}",
            get(agentx_v2_runtime::query::get_execution_artifact),
        )
        .route(
            "/internal/runtime/v1/query/sessions:search",
            post(agentx_v2_runtime::query::search_sessions),
        )
        .route(
            "/internal/runtime/v1/query/sessions/{id}",
            get(agentx_v2_runtime::query::get_session),
        )
        .route(
            "/internal/runtime/v1/session-commands:apply",
            post(agentx_v2_runtime::query::apply_session_command),
        )
        .route(
            "/internal/runtime/v1/work-packages:prepare",
            post(agentx_v2_runtime::internal_engine::prepare_work_package),
        )
        .route(
            "/internal/runtime/v1/work-packages/{package_action}",
            post(agentx_v2_runtime::internal_engine::apply_work_package_action),
        )
        .route(
            "/internal/runtime/v1/runtime-commands:apply",
            post(agentx_v2_runtime::internal_engine::apply_runtime_command),
        )
        .route(
            "/internal/runtime/v1/references:check",
            post(agentx_v2_runtime::internal_engine::check_references),
        )
        .route(
            "/internal/runtime/v1/retention-commands:apply",
            post(agentx_v2_runtime::internal_engine::apply_retention_command),
        )
        .route(
            "/internal/runtime/v1/resource-checks:execute",
            post(agentx_v2_runtime::resource_check::execute_resource_check),
        )
        .route(
            "/internal/runtime/v1/resource-operations:execute",
            post(agentx_v2_runtime::resource_check::execute_resource_operation),
        )
        .nest("/gateway/v1", agentx_v2_runtime::gateway::router())
        .with_state(state);
    agentx_service_kit::serve_with_lifecycle("runtime-gateway", router, health, lifecycle, metrics)
        .await
}
