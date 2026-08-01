use std::{env, net::SocketAddr};

use agentx_api_types::HealthResponse;
use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, routing::get};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    service_name: &'static str,
}

pub async fn run_service(service_name: &'static str) -> Result<()> {
    init_tracing();

    let bind_address = env::var("AGENTX_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse::<SocketAddr>()
        .context("AGENTX_BIND_ADDR must be a valid socket address")?;

    let state = AppState { service_name };
    let router = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("failed to bind {bind_address}"))?;

    info!(service = service_name, %bind_address, "service started");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("service terminated unexpectedly")
}

async fn live(State(state): State<AppState>) -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        service: state.service_name,
        status: "live",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn ready(State(state): State<AppState>) -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        service: state.service_name,
        status: "ready",
        version: env!("CARGO_PKG_VERSION"),
    })
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
        () = ctrl_c => {},
        () = terminate => {},
    }
}
