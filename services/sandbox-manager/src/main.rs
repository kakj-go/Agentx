mod config;
mod service;
mod store;

use std::{env, sync::Arc};

use agentx_infrastructure::mysql;
use agentx_runtime_rpc::sandbox_v1::sandbox_manager_server::SandboxManagerServer;
use anyhow::Result;
use axum::Router;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use service::SandboxManagerService;
use store::LeaseStore;
use tonic::{Request, Status, service::Interceptor, transport::Server};
use tracing::{error, warn};
use uuid::Uuid;

use crate::config::{ManagerSettings, OpenSandboxSettings};

#[tokio::main]
async fn main() -> Result<()> {
    let command = env::args().nth(1);
    if command.as_deref() == Some("doctor-opensandbox") {
        let settings = OpenSandboxSettings::from_env()?;
        settings
            .adapter
            .health()
            .await
            .map_err(anyhow::Error::new)?;
        let sandboxes = settings
            .adapter
            .list_by_labels(&std::collections::BTreeMap::new())
            .await
            .map_err(anyhow::Error::new)?;
        println!(
            "{}",
            json!({
                "status":"ready",
                "provider":"opensandbox",
                "sandboxCount":sandboxes.len(),
                "commit":"e95681e791b33b3893033940cbeaa5ab192bf21b",
                "lifecycleSpecSha256":agentx_infrastructure::opensandbox::LIFECYCLE_SPEC_SHA256,
                "execdSpecSha256":agentx_infrastructure::opensandbox::EXECD_SPEC_SHA256,
                "serverVersion":"0.2.2",
                "execdImage":"opensandbox/execd:v1.0.21"
            })
        );
        return Ok(());
    }
    if command.as_deref() == Some("doctor-drain") {
        let pool =
            mysql::connect(&agentx_infrastructure::config::MySqlSettings::from_env()?).await?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sandbox_leases WHERE status IN ('creating','ready','running','interrupting','terminating','orphaned')",
        )
        .fetch_one(&pool)
        .await?;
        anyhow::ensure!(
            active == 0,
            "{active} active Sandbox lease(s) must be drained first"
        );
        println!("{}", json!({"status":"drained","activeLeases":active}));
        return Ok(());
    }
    let settings = ManagerSettings::from_env()?;
    let pool = mysql::connect(&settings.mysql).await?;
    let store = LeaseStore::new_with_credential_source(
        pool.clone(),
        (*settings.lease_signing_key).clone(),
        settings.endpoint_keyring.clone(),
        settings.credential_source.clone(),
        settings.max_active_per_tenant,
    );
    let service = SandboxManagerService::new(store.clone(), settings.adapter.clone());
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.register("opensandbox", true).await;
    let probe_health = health.clone();
    let probe_pool = pool.clone();
    let probe_adapter = settings.adapter.clone();
    tokio::spawn(async move {
        loop {
            probe_health
                .set_status(
                    "mysql",
                    if mysql::ping(&probe_pool).await.is_ok() {
                        "ready"
                    } else {
                        "unavailable"
                    },
                )
                .await;
            probe_health
                .set_status(
                    "opensandbox",
                    if probe_adapter.health().await.is_ok()
                        && probe_adapter
                            .list_by_labels(&std::collections::BTreeMap::new())
                            .await
                            .is_ok()
                    {
                        "ready"
                    } else {
                        "unavailable"
                    },
                )
                .await;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });

    let interceptor = RpcAuthInterceptor {
        expected: settings.rpc_token.clone(),
    };
    let grpc_bind = settings.grpc_bind;
    let grpc_service = service.clone();
    let grpc = tokio::spawn(async move {
        Server::builder()
            .add_service(SandboxManagerServer::with_interceptor(
                grpc_service,
                interceptor,
            ))
            .serve(grpc_bind)
            .await
    });

    let reaper_store = store.clone();
    let reaper_adapter = service.adapter();
    let reaper_interval = settings.reaper_interval;
    tokio::spawn(async move {
        loop {
            if let Err(error) = reaper_store.reap_once(&reaper_adapter).await {
                error!(%error,"sandbox reaper failed");
            }
            tokio::time::sleep(reaper_interval).await;
        }
    });
    let heartbeat_pool = pool.clone();
    let heartbeat_health = health.clone();
    let instance =
        env::var("HOSTNAME").unwrap_or_else(|_| format!("sandbox-manager-{}", Uuid::now_v7()));
    tokio::spawn(async move {
        let detail = json!({"provider":"opensandbox","commit":"e95681e791b33b3893033940cbeaa5ab192bf21b","lifecycleSpecSha256":agentx_infrastructure::opensandbox::LIFECYCLE_SPEC_SHA256,"execdSpecSha256":agentx_infrastructure::opensandbox::EXECD_SPEC_SHA256,"serverVersion":"0.2.2","execdImage":"opensandbox/execd:v1.0.21"});
        loop {
            match sqlx::query_scalar::<_, Uuid>("SELECT id FROM tenants")
                .fetch_all(&heartbeat_pool)
                .await
            {
                Ok(tenants) => {
                    let status = heartbeat_health.overall_status().await;
                    for tenant in tenants {
                        let _=sqlx::query("INSERT INTO runtime_service_heartbeats(tenant_id,service_type,instance_id,status,detail_json,heartbeat_at) VALUES(?,'sandbox',?,?,?,CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE status=VALUES(status),detail_json=VALUES(detail_json),heartbeat_at=CURRENT_TIMESTAMP(6)").bind(tenant).bind(&instance).bind(status).bind(&detail).execute(&heartbeat_pool).await;
                    }
                }
                Err(error) => warn!(%error,"sandbox manager heartbeat failed"),
            };
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
    let result = agentx_service_kit::serve("sandbox-manager", Router::new(), health).await;
    grpc.abort();
    result
}

#[derive(Clone)]
struct RpcAuthInterceptor {
    expected: Arc<SecretString>,
}

impl Interceptor for RpcAuthInterceptor {
    #[allow(clippy::result_large_err)]
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let supplied = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        if supplied.is_some_and(|value| {
            constant_time_eq(value.as_bytes(), self.expected.expose_secret().as_bytes())
        }) {
            Ok(request)
        } else {
            Err(Status::unauthenticated(
                "sandbox manager RPC token is invalid",
            ))
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let length = left.len().max(right.len());
    for index in 0..length {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}
