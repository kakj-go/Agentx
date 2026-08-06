mod runtime_command_consumer;

use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use agentx_infrastructure::{
    artifact::MySqlObjectArtifactStore,
    clients,
    config::RuntimeInfrastructureSettings,
    mysql,
    quota::QuotaAdmission,
    runtime_queue::RuntimeQueue,
    runtime_repository::{
        CreateExecution, ResumeExecution, RuntimeExecutionSource, RuntimeRepository, TaskResult,
    },
};
use agentx_runtime_rpc::v1::{
    CommandAccepted, ConfirmSideEffectRequest, ExecutionAccepted, ForkExecutionRequest,
    HeartbeatLeaseRequest, HeartbeatLeaseResponse, ReportNodeResultRequest,
    RequestExecutionRequest, ResumeExecutionRequest, request_execution_request,
    runtime_coordinator_server::{RuntimeCoordinator, RuntimeCoordinatorServer},
};
use anyhow::{Context, Result};
use futures::StreamExt;
use serde_json::Value;
use tonic::{Request, Response, Status, transport::Server};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct CoordinatorService {
    repository: RuntimeRepository,
}

#[tonic::async_trait]
impl RuntimeCoordinator for CoordinatorService {
    async fn request_execution(
        &self,
        request: Request<RequestExecutionRequest>,
    ) -> Result<Response<ExecutionAccepted>, Status> {
        let request = request.into_inner();
        let execution_type = if request.trigger_type == "sub_workflow" {
            "sub_workflow"
        } else {
            "whole"
        };
        let source = match request
            .source
            .ok_or_else(|| Status::invalid_argument("source is required"))?
        {
            request_execution_request::Source::Version(value) => {
                RuntimeExecutionSource::Version(uuid(&value.version_id, "version.version_id")?)
            }
            request_execution_request::Source::DraftRevision(value) => {
                RuntimeExecutionSource::DraftRevision {
                    workflow_id: uuid(&value.workflow_id, "draft_revision.workflow_id")?,
                    revision: value.revision,
                }
            }
        };
        let debug_plan = parse_json(&request.debug_plan_json, "debug_plan_json")?;
        let runtime_settings = serde_json::json!({
            "mode": "whole",
            "sideEffectDecisions": debug_plan
                .get("sideEffectDecisions")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        });
        let created = self
            .repository
            .create_execution(CreateExecution {
                tenant_id: uuid(&request.tenant_id, "tenant_id")?,
                source,
                invocation_id: optional_uuid(request.invocation_id, "invocation_id")?,
                session_id: optional_uuid(request.session_id, "session_id")?,
                requested_by: optional_uuid(request.requested_by, "requested_by")?,
                trigger_type: request.trigger_type,
                input: parse_json(&request.input_json, "input_json")?,
                idempotency_key: request.idempotency_key,
                caller_execution_id: optional_uuid(
                    request.caller_execution_id,
                    "caller_execution_id",
                )?,
                execution_type: execution_type.into(),
                parent_execution_id: None,
                fork_checkpoint_id: None,
                fork_mode: None,
                runtime_settings,
                debug_plan,
                debug_overlay_snapshot: parse_json(
                    &request.debug_overlay_json,
                    "debug_overlay_json",
                )?,
                draft_resource_snapshots: serde_json::from_str(&request.resource_snapshots_json)
                    .map_err(|_| Status::invalid_argument("resource_snapshots_json is invalid"))?,
                initial_machine: None,
            })
            .await
            .map_err(internal_status)?;
        Ok(Response::new(ExecutionAccepted {
            execution_id: created.execution_id.to_string(),
            status: created.status,
            replayed: created.replayed,
        }))
    }

    async fn cancel_execution(
        &self,
        request: Request<agentx_runtime_rpc::v1::CancelExecutionRequest>,
    ) -> Result<Response<CommandAccepted>, Status> {
        let request = request.into_inner();
        let accepted = self
            .repository
            .cancel_execution(
                uuid(&request.tenant_id, "tenant_id")?,
                uuid(&request.execution_id, "execution_id")?,
            )
            .await
            .map_err(internal_status)?;
        Ok(Response::new(CommandAccepted {
            accepted,
            replayed: !accepted,
        }))
    }

    async fn fork_execution(
        &self,
        request: Request<ForkExecutionRequest>,
    ) -> Result<Response<ExecutionAccepted>, Status> {
        let request = request.into_inner();
        let created = self
            .repository
            .fork_execution(agentx_infrastructure::runtime_repository::ForkExecution {
                tenant_id: uuid(&request.tenant_id, "tenant_id")?,
                source_execution_id: uuid(&request.source_execution_id, "source_execution_id")?,
                checkpoint_id: uuid(&request.checkpoint_id, "checkpoint_id")?,
                mode: request.mode,
                node_id: request.node_id,
                input_overrides: parse_json(&request.input_overrides_json, "input_overrides_json")?,
                side_effect_decisions: parse_json(
                    &request.side_effect_decisions_json,
                    "side_effect_decisions_json",
                )?,
                actor_user_id: uuid(&request.actor_user_id, "actor_user_id")?,
                idempotency_key: request.idempotency_key,
            })
            .await
            .map_err(internal_status)?;
        Ok(Response::new(ExecutionAccepted {
            execution_id: created.execution_id.to_string(),
            status: created.status,
            replayed: created.replayed,
        }))
    }

    async fn resume_execution(
        &self,
        request: Request<ResumeExecutionRequest>,
    ) -> Result<Response<CommandAccepted>, Status> {
        let request = request.into_inner();
        let replayed = self
            .repository
            .resume_execution(ResumeExecution {
                tenant_id: uuid(&request.tenant_id, "tenant_id")?,
                execution_id: uuid(&request.execution_id, "execution_id")?,
                node_execution_id: uuid(&request.node_execution_id, "node_execution_id")?,
                resume_token: request.resume_token,
                output_port: request.output_port,
                payload: parse_json(&request.payload_json, "payload_json")?,
                idempotency_key: request.idempotency_key,
            })
            .await
            .map_err(internal_status)?;
        Ok(Response::new(CommandAccepted {
            accepted: true,
            replayed,
        }))
    }

    async fn confirm_side_effect(
        &self,
        request: Request<ConfirmSideEffectRequest>,
    ) -> Result<Response<CommandAccepted>, Status> {
        let request = request.into_inner();
        let replayed = self
            .repository
            .confirm_side_effect(
                uuid(&request.tenant_id, "tenant_id")?,
                uuid(&request.execution_id, "execution_id")?,
                uuid(&request.node_execution_id, "node_execution_id")?,
                optional_uuid(request.checkpoint_id, "checkpoint_id")?,
                &request.decision,
                uuid(&request.actor_user_id, "actor_user_id")?,
                &request.idempotency_key,
            )
            .await
            .map_err(internal_status)?;
        Ok(Response::new(CommandAccepted {
            accepted: true,
            replayed,
        }))
    }

    async fn report_node_result(
        &self,
        request: Request<ReportNodeResultRequest>,
    ) -> Result<Response<CommandAccepted>, Status> {
        let request = request.into_inner();
        let result = match request.status.as_str() {
            "completed" => TaskResult::Completed(
                serde_json::from_str(&request.outputs_json)
                    .map_err(|error| Status::invalid_argument(error.to_string()))?,
            ),
            "failed" => TaskResult::Failed {
                code: request.error_code.unwrap_or_else(|| "NODE_FAILED".into()),
                message: request
                    .error_message
                    .unwrap_or_else(|| "Node failed".into()),
                retryable: request.retryable,
            },
            "suspended" => {
                TaskResult::Suspended(parse_json(&request.suspend_json, "suspend_json")?)
            }
            other => {
                return Err(Status::invalid_argument(format!(
                    "unknown result status {other}"
                )));
            }
        };
        let accepted = self
            .repository
            .report_task(
                uuid(&request.tenant_id, "tenant_id")?,
                uuid(&request.execution_id, "execution_id")?,
                uuid(&request.node_execution_id, "node_execution_id")?,
                uuid(&request.attempt_id, "attempt_id")?,
                uuid(&request.lease_token, "lease_token")?,
                result,
            )
            .await
            .map_err(internal_status)?;
        Ok(Response::new(CommandAccepted {
            accepted,
            replayed: !accepted,
        }))
    }

    async fn heartbeat_lease(
        &self,
        request: Request<HeartbeatLeaseRequest>,
    ) -> Result<Response<HeartbeatLeaseResponse>, Status> {
        let request = request.into_inner();
        let (valid, cancellation_requested) = self
            .repository
            .heartbeat(
                uuid(&request.tenant_id, "tenant_id")?,
                uuid(&request.attempt_id, "attempt_id")?,
                uuid(&request.lease_token, "lease_token")?,
                &request.worker_instance_id,
                30,
            )
            .await
            .map_err(internal_status)?;
        Ok(Response::new(HeartbeatLeaseResponse {
            valid,
            cancellation_requested,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings = RuntimeInfrastructureSettings::from_env()?;
    let health_settings = settings.clone();
    let pool = mysql::connect(&settings.mysql).await?;
    let quota_admission = QuotaAdmission::new(settings.redis.clone());
    let object_store = clients::object_store(&settings.object_storage)?;
    let checkpoint_store: Arc<dyn agentx_application::ArtifactStore> = Arc::new(
        MySqlObjectArtifactStore::new(pool.clone(), object_store)
            .with_quota_admission(quota_admission.clone()),
    );
    let repository = RuntimeRepository::new(pool.clone())
        .with_quota_admission(quota_admission.clone())
        .with_checkpoint_artifacts(
            checkpoint_store,
            env::var("AGENTX_CHECKPOINT_ARTIFACT_THRESHOLD_BYTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(64 * 1024),
        );
    let queue = RuntimeQueue::new(settings.redis.clone());
    queue.ensure_groups().await?;
    let service = CoordinatorService {
        repository: repository.clone(),
    };
    let grpc_address = env::var("AGENTX_RUNTIME_GRPC_BIND")
        .unwrap_or_else(|_| "0.0.0.0:9090".into())
        .parse::<SocketAddr>()
        .context("AGENTX_RUNTIME_GRPC_BIND is invalid")?;
    tokio::spawn(async move {
        info!(%grpc_address, "runtime coordinator gRPC started");
        if let Err(error) = Server::builder()
            .add_service(RuntimeCoordinatorServer::new(service))
            .serve(grpc_address)
            .await
        {
            error!(%error, "runtime coordinator gRPC stopped");
        }
    });
    tokio::spawn(outbox_loop(repository.clone(), queue));
    tokio::spawn(runtime_command_consumer::run(repository.clone()));
    tokio::spawn(reaper_loop(repository.clone(), quota_admission));
    tokio::spawn(checkpoint_artifact_loop(repository.clone()));
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.register("redis", true).await;
    health.register("object_storage", true).await;
    tokio::spawn(heartbeat_loop(
        repository.clone(),
        "coordinator",
        health.clone(),
    ));
    tokio::spawn(coordinator_dependency_health_loop(
        health.clone(),
        health_settings,
        pool,
    ));
    agentx_service_kit::serve("workflow-coordinator", axum::Router::new(), health).await
}

async fn checkpoint_artifact_loop(repository: RuntimeRepository) {
    loop {
        if let Err(error) = repository.externalize_checkpoints(100).await {
            error!(%error, "checkpoint artifact externalization failed");
        }
        if let Err(error) = repository.externalize_execution_results(100).await {
            error!(%error, "execution result artifact externalization failed");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn outbox_loop(repository: RuntimeRepository, queue: RuntimeQueue) {
    loop {
        match repository.claim_outbox(100, 30).await {
            Ok(deliveries) if deliveries.is_empty() => {
                tokio::time::sleep(Duration::from_millis(250)).await
            }
            Ok(deliveries) => {
                for delivery in deliveries {
                    match queue.publish(&delivery).await {
                        Ok(_) => {
                            if let Err(error) = repository.mark_outbox_published(&delivery).await {
                                error!(%error, outbox_id=%delivery.id, "failed to mark runtime message published");
                            }
                        }
                        Err(error) => {
                            warn!(%error, outbox_id=%delivery.id, "runtime dispatch failed");
                            let _ = repository
                                .mark_outbox_failed(&delivery, &error.to_string())
                                .await;
                        }
                    }
                }
            }
            Err(error) => {
                error!(%error, "runtime outbox loop failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn reaper_loop(repository: RuntimeRepository, quota_admission: QuotaAdmission) {
    let mut calibration_tick = 0_u8;
    loop {
        if let Err(error) = repository.reap_expired_leases().await {
            error!(%error, "runtime lease reaper failed");
        }
        if let Err(error) = repository.resume_due_waits().await {
            error!(%error, "runtime wait scanner failed");
        }
        if let Err(error) = agentx_infrastructure::quota::reap_expired(repository.pool()).await {
            error!(%error, "quota reservation reaper failed");
        }
        calibration_tick = calibration_tick.wrapping_add(1);
        if calibration_tick >= 15 {
            calibration_tick = 0;
            if let Err(error) = quota_admission.rebuild_from_mysql(repository.pool()).await {
                warn!(%error, "quota Redis admission calibration failed");
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn coordinator_dependency_health_loop(
    health: agentx_service_kit::HealthRegistry,
    settings: RuntimeInfrastructureSettings,
    pool: sqlx::MySqlPool,
) {
    loop {
        health
            .set_status(
                "mysql",
                if mysql::ping(&pool).await.is_ok() {
                    "ready"
                } else {
                    "unavailable"
                },
            )
            .await;
        health
            .set_status(
                "redis",
                if clients::connect_redis(&settings.redis).await.is_ok() {
                    "ready"
                } else {
                    "unavailable"
                },
            )
            .await;
        let object_ready = match clients::object_store(&settings.object_storage) {
            Ok(store) => matches!(store.list(None).next().await, Some(Ok(_)) | None),
            Err(_) => false,
        };
        health
            .set_status(
                "object_storage",
                if object_ready { "ready" } else { "unavailable" },
            )
            .await;
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn heartbeat_loop(
    repository: RuntimeRepository,
    service_type: &'static str,
    health: agentx_service_kit::HealthRegistry,
) {
    let instance = format!("{}-{}", service_type, Uuid::now_v7());
    loop {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM tenants")
            .fetch_all(repository.pool())
            .await
        {
            Ok(tenants) => {
                let status = health.overall_status().await;
                for tenant in tenants {
                    let _=sqlx::query("INSERT INTO runtime_service_heartbeats(tenant_id,service_type,instance_id,status,detail_json,heartbeat_at) VALUES(?,?,?,?,JSON_OBJECT(),CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE status=VALUES(status),heartbeat_at=CURRENT_TIMESTAMP(6)")
                    .bind(tenant).bind(service_type).bind(&instance).bind(status).execute(repository.pool()).await;
                }
            }
            Err(error) => warn!(%error, "runtime heartbeat tenant query failed"),
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[allow(clippy::result_large_err)]
fn uuid(value: &str, field: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(value).map_err(|_| Status::invalid_argument(format!("{field} must be a UUID")))
}
#[allow(clippy::result_large_err)]
fn optional_uuid(value: Option<String>, field: &str) -> Result<Option<Uuid>, Status> {
    value.map(|value| uuid(&value, field)).transpose()
}
#[allow(clippy::result_large_err)]
fn parse_json(value: &str, field: &str) -> Result<Value, Status> {
    serde_json::from_str(value)
        .map_err(|error| Status::invalid_argument(format!("{field}: {error}")))
}
fn internal_status(error: anyhow::Error) -> Status {
    if error.to_string().contains("not found") {
        Status::not_found(error.to_string())
    } else if error.to_string().contains("IDEMPOTENCY") || error.to_string().contains("INVALID") {
        Status::failed_precondition(error.to_string())
    } else {
        error!(%error, "runtime command failed");
        Status::internal("runtime command failed")
    }
}
