use std::{sync::Arc, time::Duration};

use agentx_application::{
    CredentialResolver, McpToolRequest, McpToolResponse, McpToolRuntime, RuntimeContext,
    RuntimeError, RuntimeResult,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client, RequestBuilder, header};
use serde_json::{Value, json};
use url::Url;

use crate::{runtime_resources::MySqlResourceAuthorizer, sse::SseDecoder};

const LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct HttpMcpToolRuntime {
    client: Client,
    authorizer: MySqlResourceAuthorizer,
    credentials: Arc<dyn CredentialResolver>,
}

struct McpRpcCall<'a> {
    endpoint: &'a Url,
    session: Option<&'a str>,
    id: u64,
    method: &'a str,
    params: Value,
}

impl HttpMcpToolRuntime {
    pub fn new(
        authorizer: MySqlResourceAuthorizer,
        credentials: Arc<dyn CredentialResolver>,
    ) -> RuntimeResult<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| RuntimeError::new("MCP_CLIENT_INVALID", e.to_string()))?,
            authorizer,
            credentials,
        })
    }

    async fn authorize_all(
        &self,
        context: &RuntimeContext,
        request: &McpToolRequest,
    ) -> RuntimeResult<()> {
        self.authorizer
            .authorize_context(context, &request.resource)
            .await?;
        for dependency in &context.resources {
            if matches!(
                dependency.reference.resource_type.as_str(),
                "mcp_server" | "credential"
            ) {
                self.authorizer
                    .authorize_context(context, &dependency.reference)
                    .await?;
            }
        }
        Ok(())
    }

    async fn authenticated(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        builder: RequestBuilder,
    ) -> RuntimeResult<RequestBuilder> {
        let Some(id) = snapshot.get("credentialId").and_then(Value::as_str) else {
            return Ok(builder);
        };
        let id = uuid::Uuid::parse_str(id).map_err(|_| {
            RuntimeError::new("MCP_SNAPSHOT_INVALID", "MCP Credential ID is invalid")
        })?;
        let version = context
            .resources
            .iter()
            .find(|r| r.reference.resource_id == id)
            .and_then(|r| r.snapshot.get("secretVersion"))
            .and_then(Value::as_u64);
        let credential = self
            .credentials
            .resolve_for_runtime(context, id, version)
            .await
            .map_err(|e| RuntimeError::new("MCP_CREDENTIAL_INVALID", e.to_string()))?;
        let secret = std::str::from_utf8(credential.secret.expose()).map_err(|_| {
            RuntimeError::new("MCP_CREDENTIAL_INVALID", "MCP credential is not UTF-8")
        })?;
        match credential.credential_type.as_str() {
            "api_key" | "bearer" => Ok(builder.bearer_auth(secret)),
            "basic" => {
                let value: Value = serde_json::from_str(secret).map_err(|_| {
                    RuntimeError::new("MCP_CREDENTIAL_INVALID", "Basic credential must be JSON")
                })?;
                Ok(builder.basic_auth(
                    value
                        .get("username")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    value.get("password").and_then(Value::as_str),
                ))
            }
            _ => Err(RuntimeError::new(
                "MCP_CREDENTIAL_UNSUPPORTED",
                "MCP credential type is unsupported",
            )),
        }
    }

    async fn rpc(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        call: McpRpcCall<'_>,
    ) -> RuntimeResult<(Value, Option<String>)> {
        let mut builder = self
            .client
            .post(call.endpoint.clone())
            .header(header::ACCEPT, "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":call.id,"method":call.method,"params":call.params}));
        if let Some(session) = call.session {
            builder = builder.header("mcp-session-id", session);
        }
        builder = self.authenticated(context, snapshot, builder).await?;
        let response = tokio::select! {_=context.cancellation.cancelled()=>return Err(RuntimeError::new("RUNTIME_CANCELLED","MCP call was cancelled")),value=builder.timeout(Duration::from_secs(snapshot.get("timeoutSeconds").and_then(Value::as_u64).unwrap_or(30))).send()=>value.map_err(mcp_transport)?};
        if !response.status().is_success() {
            return Err(RuntimeError::new(
                "MCP_HTTP_STATUS",
                format!("MCP server returned HTTP {}", response.status().as_u16()),
            )
            .retryable(response.status().is_server_error()));
        }
        let next_session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| call.session.map(str::to_owned));
        let bytes = response.bytes().await.map_err(|error| {
            mark_outcome_unknown(mcp_transport(error), call.method == "tools/call")
        })?;
        if bytes.len() > LIMIT {
            return Err(RuntimeError::new(
                "MCP_RESPONSE_TOO_LARGE",
                "MCP response exceeded 1 MiB",
            ));
        }
        let value = parse_rpc(&bytes)?;
        if let Some(error) = value.get("error") {
            return Err(RuntimeError::new(
                "MCP_REMOTE_ERROR",
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("MCP server returned an error"),
            ));
        }
        Ok((
            value.get("result").cloned().unwrap_or(Value::Null),
            next_session,
        ))
    }

    async fn notification(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        endpoint: &Url,
        session: &str,
    ) -> RuntimeResult<()> {
        let builder = self
            .client
            .post(endpoint.clone())
            .header("mcp-session-id", session)
            .header(header::ACCEPT, "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}));
        let response = self
            .authenticated(context, snapshot, builder)
            .await?
            .send()
            .await
            .map_err(mcp_transport)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(RuntimeError::new(
                "MCP_HTTP_STATUS",
                format!(
                    "MCP initialized notification returned HTTP {}",
                    response.status().as_u16()
                ),
            ))
        }
    }

    async fn streamable(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        name: &str,
        args: Value,
    ) -> RuntimeResult<Value> {
        let endpoint = endpoint(snapshot)?;
        let (_, session) = self
            .rpc(
                context,
                snapshot,
                McpRpcCall {
                    endpoint: &endpoint,
                    session: None,
                    id: 1,
                    method: "initialize",
                    params: json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"Agentx","version":"0.1.0"}}),
                },
            )
            .await?;
        if let Some(session) = session.as_deref() {
            self.notification(context, snapshot, &endpoint, session)
                .await?;
        }
        self.rpc(
            context,
            snapshot,
            McpRpcCall {
                endpoint: &endpoint,
                session: session.as_deref(),
                id: 2,
                method: "tools/call",
                params: json!({"name":name,"arguments":args}),
            },
        )
        .await
        .map(|v| v.0)
    }

    async fn legacy(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        name: &str,
        args: Value,
    ) -> RuntimeResult<Value> {
        let configured = endpoint(snapshot)?;
        let builder = self
            .client
            .get(configured.clone())
            .header(header::ACCEPT, "text/event-stream");
        let response = self
            .authenticated(context, snapshot, builder)
            .await?
            .send()
            .await
            .map_err(mcp_transport)?;
        if !response.status().is_success() {
            return Err(RuntimeError::new(
                "MCP_HTTP_STATUS",
                "MCP SSE connection failed",
            ));
        }
        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::new(128 * 1024, LIMIT);
        let post = loop {
            let chunk = next_legacy_chunk(context, &mut stream, false).await?;
            let mut found = None;
            for event in decoder.push(&chunk)? {
                if event.event.as_deref() == Some("endpoint") {
                    found = Some(event.data);
                    break;
                }
            }
            if let Some(value) = found {
                break same_origin_join(&configured, &value)?;
            }
        };
        legacy_send(self,context,snapshot,&post,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"Agentx","version":"0.1.0"}}}), false).await?;
        wait_legacy(context, &mut stream, &mut decoder, 1, false).await?;
        legacy_send(
            self,
            context,
            snapshot,
            &post,
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            false,
        )
        .await?;
        legacy_send(self,context,snapshot,&post,json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}}), true).await?;
        wait_legacy(context, &mut stream, &mut decoder, 2, true).await
    }
}

#[async_trait]
impl McpToolRuntime for HttpMcpToolRuntime {
    async fn call(
        &self,
        context: &RuntimeContext,
        request: McpToolRequest,
    ) -> RuntimeResult<McpToolResponse> {
        self.authorize_all(context, &request).await?;
        let snapshot = &context
            .resource(&request.resource)
            .ok_or_else(|| {
                RuntimeError::new("RESOURCE_SNAPSHOT_MISSING", "MCP Tool snapshot is missing")
            })?
            .snapshot;
        if let Some(schema) = snapshot.get("inputSchema") {
            jsonschema::validator_for(schema)
                .map_err(|e| RuntimeError::new("MCP_SCHEMA_INVALID", e.to_string()))?
                .validate(&request.arguments)
                .map_err(|e| RuntimeError::new("MCP_ARGUMENT_INVALID", e.to_string()))?;
        }
        let name = snapshot
            .get("toolName")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::new("MCP_SNAPSHOT_INVALID", "MCP Tool name is missing"))?;
        let value = if snapshot.get("transport").and_then(Value::as_str) == Some("sse") {
            self.legacy(context, snapshot, name, request.arguments)
                .await?
        } else {
            self.streamable(context, snapshot, name, request.arguments)
                .await?
        };
        if let Some(schema) = snapshot.get("outputSchema").filter(|v| !v.is_null()) {
            validate_structured_output(schema, &value)?;
        }
        Ok(McpToolResponse {
            content: value
                .get("content")
                .cloned()
                .unwrap_or_else(|| value.clone()),
            structured_content: value.get("structuredContent").cloned(),
            is_error: value
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

fn validate_structured_output(schema: &Value, result: &Value) -> RuntimeResult<()> {
    let structured_content = result.get("structuredContent").ok_or_else(|| {
        RuntimeError::new(
            "MCP_RESULT_INVALID",
            "MCP result is missing structuredContent required by outputSchema",
        )
    })?;
    jsonschema::validator_for(schema)
        .map_err(|e| RuntimeError::new("MCP_SCHEMA_INVALID", e.to_string()))?
        .validate(structured_content)
        .map_err(|e| RuntimeError::new("MCP_RESULT_INVALID", e.to_string()))
}

async fn legacy_send(
    runtime: &HttpMcpToolRuntime,
    context: &RuntimeContext,
    snapshot: &Value,
    endpoint: &Url,
    payload: Value,
    outcome_unknown: bool,
) -> RuntimeResult<()> {
    let builder = runtime.client.post(endpoint.clone()).json(&payload);
    let response = runtime
        .authenticated(context, snapshot, builder)
        .await?
        .send()
        .await
        .map_err(|error| mark_outcome_unknown(mcp_transport(error), outcome_unknown))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(RuntimeError::new(
            "MCP_HTTP_STATUS",
            "MCP SSE message endpoint rejected the request",
        ))
    }
}

async fn wait_legacy(
    context: &RuntimeContext,
    stream: &mut (impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
    decoder: &mut SseDecoder,
    id: u64,
    outcome_unknown: bool,
) -> RuntimeResult<Value> {
    loop {
        let chunk = next_legacy_chunk(context, stream, outcome_unknown).await?;
        for event in decoder.push(&chunk)? {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                if value.get("id").and_then(Value::as_u64) == Some(id) {
                    if let Some(error) = value.get("error") {
                        return Err(RuntimeError::new("MCP_REMOTE_ERROR", error.to_string()));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }
}

async fn next_legacy_chunk(
    context: &RuntimeContext,
    stream: &mut (impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
    outcome_unknown: bool,
) -> RuntimeResult<bytes::Bytes> {
    let remaining = Duration::try_from(context.deadline - time::OffsetDateTime::now_utc())
        .unwrap_or(Duration::ZERO)
        .min(Duration::from_secs(60));
    let next = tokio::select! {
        _ = context.cancellation.cancelled() => return Err(RuntimeError::new("RUNTIME_CANCELLED", "MCP SSE call was cancelled")),
        value = tokio::time::timeout(remaining, stream.next()) => value.map_err(|_| RuntimeError::new("MCP_TIMEOUT", "MCP SSE response timed out").retryable(true))?,
    };
    next.ok_or_else(|| {
        mark_outcome_unknown(
            RuntimeError::new("MCP_SSE_CLOSED", "MCP SSE closed before response"),
            outcome_unknown,
        )
    })?
    .map_err(|error| mark_outcome_unknown(mcp_transport(error), outcome_unknown))
}

fn mark_outcome_unknown(mut error: RuntimeError, value: bool) -> RuntimeError {
    error.outcome_unknown |= value;
    error
}

fn endpoint(snapshot: &Value) -> RuntimeResult<Url> {
    let url = Url::parse(
        snapshot
            .get("endpoint")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::new("MCP_SNAPSHOT_INVALID", "MCP endpoint is missing"))?,
    )
    .map_err(|_| RuntimeError::new("MCP_SNAPSHOT_INVALID", "MCP endpoint is invalid"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(RuntimeError::new(
            "MCP_SNAPSHOT_INVALID",
            "MCP endpoint scheme is unsupported",
        ));
    }
    Ok(url)
}
fn same_origin_join(base: &Url, value: &str) -> RuntimeResult<Url> {
    let target = base.join(value.trim()).map_err(|_| {
        RuntimeError::new("MCP_SSE_ENDPOINT_INVALID", "MCP SSE endpoint is invalid")
    })?;
    if base.scheme() != target.scheme()
        || base.host_str() != target.host_str()
        || base.port_or_known_default() != target.port_or_known_default()
    {
        return Err(RuntimeError::new(
            "MCP_SSE_ENDPOINT_ORIGIN",
            "MCP SSE endpoint changed origin",
        ));
    }
    Ok(target)
}
fn parse_rpc(bytes: &[u8]) -> RuntimeResult<Value> {
    if let Ok(value) = serde_json::from_slice(bytes) {
        return Ok(value);
    }
    let mut decoder = SseDecoder::new(LIMIT, LIMIT);
    decoder
        .push(bytes)?
        .into_iter()
        .rev()
        .find_map(|e| serde_json::from_str(&e.data).ok())
        .ok_or_else(|| {
            RuntimeError::new(
                "MCP_PROTOCOL_ERROR",
                "MCP response is neither JSON nor valid SSE",
            )
        })
}
fn mcp_transport(error: reqwest::Error) -> RuntimeError {
    if error.is_timeout() {
        RuntimeError::new("MCP_TIMEOUT", "MCP request timed out").retryable(true)
    } else {
        RuntimeError::new("MCP_CONNECTION_FAILED", "MCP server could not be reached")
            .retryable(true)
            .outcome_unknown(error.is_request())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamable_and_legacy_payloads_parse_to_the_same_rpc_result() {
        let expected = json!({"content":[{"type":"text","text":"ok"}]});
        let json_response = serde_json::to_vec(&json!({
            "jsonrpc":"2.0",
            "id":2,
            "result":expected
        }))
        .unwrap();
        let sse_response = format!(
            "event: message\ndata: {}\n\n",
            String::from_utf8(json_response.clone()).unwrap()
        );
        assert_eq!(parse_rpc(&json_response).unwrap()["result"], expected);
        assert_eq!(
            parse_rpc(sse_response.as_bytes()).unwrap()["result"],
            expected
        );
    }

    #[test]
    fn legacy_message_endpoint_must_preserve_origin() {
        let base = Url::parse("https://mcp.example.test/sse").unwrap();
        assert_eq!(
            same_origin_join(&base, "/messages?session=one")
                .unwrap()
                .as_str(),
            "https://mcp.example.test/messages?session=one"
        );
        assert!(same_origin_join(&base, "https://attacker.test/messages").is_err());
        assert!(same_origin_join(&base, "//attacker.test/messages").is_err());
    }

    #[test]
    fn side_effecting_tool_transport_failures_are_outcome_unknown() {
        let error = mark_outcome_unknown(
            RuntimeError::new("MCP_SSE_CLOSED", "closed").retryable(true),
            true,
        );
        assert!(error.outcome_unknown);
        assert!(error.retryable);
    }

    #[test]
    fn output_schema_validates_structured_content_in_call_result() {
        let schema = json!({
            "type": "object",
            "required": ["text"],
            "properties": {"text": {"type": "string"}}
        });
        let result = json!({
            "content": [{"type": "text", "text": "Agentx E2E"}],
            "structuredContent": {"text": "Agentx E2E"}
        });

        validate_structured_output(&schema, &result).unwrap();
    }

    #[test]
    fn output_schema_requires_structured_content() {
        let schema = json!({
            "type": "object",
            "required": ["text"],
            "properties": {"text": {"type": "string"}}
        });
        let result = json!({"content": [{"type": "text", "text": "Agentx E2E"}]});

        let error = validate_structured_output(&schema, &result).unwrap_err();
        assert_eq!(error.code, "MCP_RESULT_INVALID");
        assert!(error.message.contains("structuredContent"));
    }
}
