use std::{env, net::SocketAddr};

use axum::{
    Json as AxumJson, Router,
    body::Body,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
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
use serde_json::{Value, json};
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
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/v2/runtime/model", post(v2_runtime_model))
        .route("/v2/runtime/mcp", post(v2_runtime_mcp))
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

async fn v2_runtime_model(headers: HeaderMap, AxumJson(request): AxumJson<Value>) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer m5-model-secret")
    {
        return (
            StatusCode::UNAUTHORIZED,
            AxumJson(json!({"error":{"message":"invalid fixture credential"}})),
        )
            .into_response();
    }
    let input = request.get("input").cloned().unwrap_or(Value::Null);
    let agent_call = request
        .pointer("/parameters/maxIterations")
        .and_then(Value::as_u64)
        .is_some();
    let has_tool_result = input.get("tool").is_some();
    if agent_call && !has_tool_result {
        return AxumJson(json!({
            "toolCall":{"text":"agentx-v2-tool"},
            "usage":{"inputTokens":12,"outputTokens":8,"tokens":20,"costMicros":5}
        }))
        .into_response();
    }
    AxumJson(json!({
        "done":true,
        "answer":"agentx-v2-model",
        "input":input,
        "usage":{"inputTokens":12,"outputTokens":8,"tokens":20,"costMicros":5}
    }))
    .into_response()
}

async fn v2_runtime_mcp(headers: HeaderMap, AxumJson(request): AxumJson<Value>) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer m5-model-secret")
    {
        return (
            StatusCode::UNAUTHORIZED,
            AxumJson(json!({"error":{"message":"invalid fixture credential"}})),
        )
            .into_response();
    }
    AxumJson(json!({
        "content":{
            "text":"agentx-v2-mcp",
            "arguments":request.pointer("/params/arguments").cloned().unwrap_or(Value::Null)
        }
    }))
    .into_response()
}

async fn embeddings(headers: HeaderMap, AxumJson(request): AxumJson<Value>) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer m5-model-secret")
    {
        return (
            StatusCode::UNAUTHORIZED,
            AxumJson(json!({"error":{"message":"invalid fixture credential"}})),
        )
            .into_response();
    }
    let inputs = match request.get("input") {
        Some(Value::Array(values)) => values.clone(),
        Some(value) => vec![value.clone()],
        None => Vec::new(),
    };
    let default_dimensions =
        if request.get("model").and_then(Value::as_str) == Some("echo-embedding-1536") {
            1536
        } else {
            8
        };
    let dimensions = request
        .get("dimensions")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (1..=3072).contains(value))
        .unwrap_or(default_dimensions);
    let mut token_count = 0_u64;
    let data = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let text = input
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| input.to_string());
            token_count += text.len().div_ceil(4) as u64;
            let mut embedding = vec![0_f64; dimensions];
            for (offset, byte) in text.bytes().enumerate() {
                embedding[offset % dimensions] += f64::from(byte) / 255.0;
            }
            json!({"object":"embedding","embedding":embedding,"index":index})
        })
        .collect::<Vec<_>>();
    AxumJson(json!({
        "object":"list",
        "data":data,
        "model":request.get("model").cloned().unwrap_or_else(||json!("text-embedding-3-small")),
        "usage":{"prompt_tokens":token_count,"total_tokens":token_count}
    }))
    .into_response()
}

async fn chat_completions(headers: HeaderMap, AxumJson(request): AxumJson<Value>) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer m5-model-secret")
    {
        return (
            StatusCode::UNAUTHORIZED,
            AxumJson(json!({"error":{"message":"invalid fixture credential"}})),
        )
            .into_response();
    }
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let agent_purpose = request
        .pointer("/metadata/agentPurpose")
        .and_then(Value::as_str)
        .unwrap_or("agent_turn");
    let p3_overflow_requested = messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| content.contains("P3_CONTEXT_OVERFLOW"))
    });
    let p3_overflow_compacted = messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| content.contains("P3_OVERFLOW_COMPACTED"))
    });
    if agent_purpose == "agent_turn" && p3_overflow_requested && !p3_overflow_compacted {
        return (
            StatusCode::BAD_REQUEST,
            AxumJson(json!({"error":{"code":"context_length_exceeded","message":"maximum context length exceeded by P3 fixture"}})),
        )
            .into_response();
    }
    let tool_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .count();
    let requested_loop = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .any(|content| content.contains("m5-loop"));
    let kakj_identity = messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("你叫 kakj"))
    });
    let large_trace_response = messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| content.contains("TRACE_LARGE_RESPONSE"))
    });
    let p3_skill_context = messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("immutable V2-04 Skill result"))
    });
    let tool = request
        .pointer("/tools/0/function/name")
        .and_then(Value::as_str);
    let latest_user_content = messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let requested_memory_write = latest_user_content.contains("P3_MEMORY_WRITE");
    let requested_memory_recall = latest_user_content.contains("P3_MEMORY_RECALL");
    let selected_tool = if requested_memory_write {
        request
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| {
                tools.iter().find_map(|candidate| {
                    (candidate.pointer("/function/name").and_then(Value::as_str)
                        == Some("memory_write"))
                    .then(|| "memory_write")
                })
            })
            .or(tool)
    } else if requested_memory_recall {
        request
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| {
                tools.iter().find_map(|candidate| {
                    (candidate.pointer("/function/name").and_then(Value::as_str)
                        == Some("memory_recall"))
                    .then(|| "memory_recall")
                })
            })
            .or(tool)
    } else {
        tool
    };
    let tool_call = selected_tool
        .filter(|_| requested_loop || tool_messages == 0)
        .map(|name| {
            let arguments = match name {
                "memory_write" => json!({
                    "text":"p3-subject-memory",
                    "metadata":{"fixture":true}
                }),
                "memory_recall" => json!({"query":"p3-subject-memory","topK":5}),
                _ => json!({"text":"m5-tool-result"}),
            };
            json!({
                "id": format!("m5-call-{tool_messages}"),
                "type": "function",
                "function": {"name": name, "arguments": arguments.to_string()}
            })
        });
    let finish_reason = if tool_call.is_some() {
        "tool_calls"
    } else {
        "stop"
    };
    let content =
        tool_call
            .is_none()
            .then_some(if agent_purpose == "compaction" && p3_overflow_requested {
                "P3_OVERFLOW_COMPACTED"
            } else if agent_purpose == "compaction" {
                "P3_THRESHOLD_COMPACTED"
            } else if kakj_identity {
                "你好，我叫 kakj。"
            } else if p3_skill_context {
                "P3-04 Agent attachment completed; skill_context=true"
            } else {
                "M5 Agent completed after the MCP tool result"
            });
    let usage = json!({"prompt_tokens": 24 + tool_messages, "completion_tokens": if tool_call.is_some() { 12 } else { 9 }, "total_tokens": 45 + tool_messages});
    if request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let delta = if let Some(call) = tool_call {
            json!({"tool_calls":[{"index":0,"id":call["id"],"type":"function","function":{"name":call["function"]["name"],"arguments":call["function"]["arguments"]}}]})
        } else {
            json!({"content":content})
        };
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"id":"m5-stream","choices":[{"index":0,"delta":delta,"finish_reason":finish_reason}]}),
            json!({"id":"m5-stream","choices":[],"usage":usage})
        );
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(body))
            .expect("fixture response");
    }
    let mut message = json!({"role":"assistant","content":content});
    if let Some(call) = tool_call {
        message["tool_calls"] = json!([call]);
    }
    let mut response = json!({
        "id":"m5-completion",
        "object":"chat.completion",
        "model":request.get("model").cloned().unwrap_or_else(||json!("echo-model")),
        "choices":[{"index":0,"message":message,"finish_reason":finish_reason}],
        "usage":usage
    });
    if large_trace_response {
        response["fixture_trace_payload"] = Value::String("trace-artifact-marker|".repeat(1_024));
    }
    AxumJson(response).into_response()
}

#[cfg(test)]
mod tests {
    use super::{chat_completions, embeddings};
    use axum::{
        Json,
        http::{HeaderMap, HeaderValue, header},
        response::IntoResponse,
    };
    use serde_json::json;

    #[tokio::test]
    async fn model_fixture_requires_auth_and_completes_after_tool_output() {
        let unauthorized = chat_completions(HeaderMap::new(), Json(json!({}))).await;
        assert_eq!(unauthorized.status(), 401);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer m5-model-secret"),
        );
        let response = chat_completions(headers, Json(json!({"messages":[{"role":"tool","content":"ok"}],"tools":[{"function":{"name":"echo"}}]}))).await.into_response();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn model_fixture_applies_the_system_identity_prompt() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer m5-model-secret"),
        );
        let response = chat_completions(
            headers,
            Json(json!({"messages":[
                {"role":"system","content":"你叫 kakj"},
                {"role":"user","content":"你是谁？"}
            ]})),
        )
        .await
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value.pointer("/choices/0/message/content"),
            Some(&json!("你好，我叫 kakj。"))
        );
    }

    #[tokio::test]
    async fn model_fixture_reports_p3_skill_external_context() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer m5-model-secret"),
        );
        let response = chat_completions(
            headers,
            Json(json!({"messages":[
                {"role":"system","content":"Return the immutable V2-04 Skill result."},
                {"role":"tool","content":"mcp complete"}
            ],"tools":[{"function":{"name":"echo"}}]})),
        )
        .await
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value.pointer("/choices/0/message/content"),
            Some(&json!(
                "P3-04 Agent attachment completed; skill_context=true"
            ))
        );
    }

    #[tokio::test]
    async fn model_fixture_can_emit_a_large_trace_only_provider_response() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer m5-model-secret"),
        );
        let response = chat_completions(
            headers,
            Json(json!({"messages":[{
                "role":"system",
                "content":"你叫 kakj\nTRACE_LARGE_RESPONSE"
            }]})),
        )
        .await
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.len() > 16 * 1024);
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            value["fixture_trace_payload"]
                .as_str()
                .is_some_and(|payload| payload.contains("trace-artifact-marker"))
        );
        assert_eq!(
            value.pointer("/choices/0/message/content"),
            Some(&json!("你好，我叫 kakj。"))
        );
    }

    #[tokio::test]
    async fn embedding_fixture_is_authenticated_and_deterministic() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer m5-model-secret"),
        );
        let request = json!({"model":"text-embedding-3-small","input":["alpha","beta"]});
        let first = embeddings(headers.clone(), Json(request.clone())).await;
        let second = embeddings(headers, Json(request)).await;
        assert_eq!(first.status(), 200);
        assert_eq!(second.status(), 200);
        assert_eq!(
            axum::body::to_bytes(first.into_body(), usize::MAX)
                .await
                .unwrap(),
            axum::body::to_bytes(second.into_body(), usize::MAX)
                .await
                .unwrap()
        );
    }
}
