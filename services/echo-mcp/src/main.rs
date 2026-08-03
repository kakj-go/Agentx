use std::{env, net::SocketAddr};

use axum::{Json as AxumJson, Router, routing::get};
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::{
        StreamableHttpServerConfig,
        streamable_http_server::{
            session::local::LocalSessionManager, tower::StreamableHttpService,
        },
    },
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    #[schemars(description = "Text returned by the echo tool")]
    text: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EchoResponse {
    text: String,
}

#[derive(Debug, Clone)]
struct EchoService {
    tool_router: ToolRouter<Self>,
}

impl EchoService {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl EchoService {
    #[tool(description = "Return the provided text unchanged")]
    fn echo(
        &self,
        Parameters(EchoRequest { text }): Parameters<EchoRequest>,
    ) -> Json<EchoResponse> {
        Json(EchoResponse { text })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Agentx local Echo MCP test server".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "echo_mcp=info".into()),
        )
        .json()
        .init();

    let bind_addr: SocketAddr = env::var("AGENTX_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8090".to_owned())
        .parse()?;
    let cancellation = CancellationToken::new();
    let service: StreamableHttpService<EchoService, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(EchoService::new()),
            Default::default(),
            StreamableHttpServerConfig {
                stateful_mode: true,
                sse_keep_alive: None,
                cancellation_token: cancellation.child_token(),
            },
        );
    let app = Router::new()
        .route(
            "/health",
            get(|| async { AxumJson(serde_json::json!({ "status": "ready" })) }),
        )
        .route(
            "/models",
            get(|| async {
                AxumJson(serde_json::json!({
                    "object": "list",
                    "data": [{ "id": "echo-model", "object": "model" }]
                }))
            }),
        )
        .nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "Echo MCP is listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancellation.cancel();
        })
        .await?;
    Ok(())
}
