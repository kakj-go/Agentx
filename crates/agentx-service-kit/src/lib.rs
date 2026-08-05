use std::{collections::BTreeMap, env, net::SocketAddr, sync::Arc};

use agentx_api_types::{DependencyHealth, HealthResponse};
use anyhow::{Context, Result};
use axum::{
    Extension, Json, Router,
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub struct RequestId(pub Uuid);

#[derive(Clone, Default)]
pub struct HealthRegistry {
    dependencies: Arc<RwLock<BTreeMap<String, DependencyHealth>>>,
}

impl HealthRegistry {
    pub async fn register(&self, name: impl Into<String>, required: bool) {
        let name = name.into();
        self.dependencies.write().await.insert(
            name.clone(),
            DependencyHealth {
                name,
                status: "unknown".to_owned(),
                required,
            },
        );
    }

    pub async fn set_status(&self, name: &str, status: impl Into<String>) {
        if let Some(dependency) = self.dependencies.write().await.get_mut(name) {
            dependency.status = status.into();
        }
    }

    async fn snapshot(&self) -> Vec<DependencyHealth> {
        self.dependencies.read().await.values().cloned().collect()
    }

    pub async fn overall_status(&self) -> &'static str {
        let dependencies = self.snapshot().await;
        if dependencies
            .iter()
            .any(|dependency| dependency.required && dependency.status != "ready")
        {
            "unavailable"
        } else if dependencies
            .iter()
            .any(|dependency| !dependency.required && dependency.status != "ready")
        {
            "degraded"
        } else {
            "ready"
        }
    }
}

#[derive(Clone)]
struct HealthState {
    service_name: &'static str,
    registry: HealthRegistry,
}

pub async fn run_service(service_name: &'static str) -> Result<()> {
    serve(service_name, Router::new(), HealthRegistry::default()).await
}

pub async fn serve(
    service_name: &'static str,
    router: Router,
    registry: HealthRegistry,
) -> Result<()> {
    init_tracing();

    let bind_address = env::var("AGENTX_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse::<SocketAddr>()
        .context("AGENTX_BIND_ADDR must be a valid socket address")?;

    let health_state = HealthState {
        service_name,
        registry,
    };
    let router = router
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .layer(Extension(health_state))
        .layer(middleware::from_fn(request_id))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("failed to bind {bind_address}"))?;

    info!(service = service_name, %bind_address, "service started");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("service terminated unexpectedly")
}

async fn request_id(mut request: Request, next: Next) -> Response {
    let request_id = Uuid::now_v7();
    request.extensions_mut().insert(RequestId(request_id));
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .entry(HeaderName::from_static("x-request-id"))
        .or_insert_with(|| {
            HeaderValue::from_str(&request_id.to_string())
                .expect("UUID is always a valid header value")
        });
    response
}

async fn live(Extension(state): Extension<HealthState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        service: state.service_name.to_owned(),
        status: "live".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        dependencies: Vec::new(),
    })
}

async fn ready(Extension(state): Extension<HealthState>) -> (StatusCode, Json<HealthResponse>) {
    let dependencies = state.registry.snapshot().await;
    let ready = dependencies
        .iter()
        .all(|dependency| !dependency.required || dependency.status == "ready");
    let degraded = dependencies
        .iter()
        .any(|dependency| !dependency.required && dependency.status != "ready");
    let status = if ready {
        if degraded { "degraded" } else { "ready" }
    } else {
        "not_ready"
    };

    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(HealthResponse {
            service: state.service_name.to_owned(),
            status: status.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            dependencies,
        }),
    )
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .json()
        .try_init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{HealthRegistry, HealthState, ready};
    use axum::{Extension, http::StatusCode};

    #[tokio::test]
    async fn readiness_recovers_and_optional_failures_only_degrade() {
        let registry = HealthRegistry::default();
        registry.register("mysql", true).await;
        registry.register("redis", false).await;
        let state = HealthState {
            service_name: "test-service",
            registry: registry.clone(),
        };

        registry.set_status("mysql", "unavailable").await;
        registry.set_status("redis", "degraded").await;
        let (status, body) = ready(Extension(state.clone())).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.status, "not_ready");

        registry.set_status("mysql", "ready").await;
        let (status, body) = ready(Extension(state.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "degraded");

        registry.set_status("redis", "ready").await;
        let (status, body) = ready(Extension(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ready");
    }
}
