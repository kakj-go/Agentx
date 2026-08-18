use std::time::{Duration, Instant};

use agentx_runtime_contracts::{
    RuntimeResourceCheckRequestV1, RuntimeResourceCheckResponseV1,
    RuntimeResourceOperationRequestV1, RuntimeResourceOperationResponseV1,
    RuntimeResourceOperationV1, RuntimeResourceProbeV1, VaultSecretReferenceV1,
};
use axum::{Json, extract::State, http::HeaderMap};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::json;
use time::OffsetDateTime;

use crate::{
    RuntimeState,
    egress::{EgressRequestContext, ProviderHttpClient, validate_provider_url},
    error::{RuntimeError, RuntimeResult},
};

pub async fn execute_resource_check(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<RuntimeResourceCheckRequestV1>,
) -> RuntimeResult<Json<RuntimeResourceCheckResponseV1>> {
    state.trust.publisher(&headers, "runtime.resources.check")?;
    validate_request(&request)?;
    let checked_at = OffsetDateTime::now_utc();
    let started = Instant::now();
    let token = match &request.credential {
        Some(reference) => match state.vault.as_ref() {
            Some(vault) => match vault.read(reference).await {
                Ok(value) => Some(String::from_utf8(value).map_err(|_| {
                    RuntimeError::InvalidRequest(
                        "INVALID_CREDENTIAL",
                        "credential value must be UTF-8".into(),
                    )
                })?),
                Err(_) => {
                    return Ok(Json(response(
                        checked_at,
                        started,
                        "unhealthy",
                        Some("RUNTIME_SECRET_UNAVAILABLE"),
                        Some("runtime credential is unavailable"),
                    )));
                }
            },
            None => {
                return Ok(Json(response(
                    checked_at,
                    started,
                    "unhealthy",
                    Some("RUNTIME_SECRET_UNAVAILABLE"),
                    Some("runtime credential provider is unavailable"),
                )));
            }
        },
        None => None,
    };
    let client = ProviderHttpClient::from_env(agentx_runtime_contracts::EgressRole::RuntimeGateway)
        .map_err(RuntimeError::Internal)?;
    let context = EgressRequestContext::request(request.tenant_id, uuid::Uuid::now_v7());
    let timeout = Duration::from_secs(15);
    let mut outgoing = match &request.probe {
        RuntimeResourceProbeV1::ModelChat { model } => client
            .post(
                &format!(
                    "{}/chat/completions",
                    request.endpoint.trim_end_matches('/')
                ),
                context,
                timeout,
            )
            .map_err(|error| RuntimeError::InvalidRequest("INVALID_ENDPOINT", error.to_string()))?
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "health"}],
                "max_tokens": 1
            })),
        RuntimeResourceProbeV1::McpInitialize => client
            .post(&request.endpoint, context, timeout)
            .map_err(|error| RuntimeError::InvalidRequest("INVALID_ENDPOINT", error.to_string()))?
            .header(ACCEPT, "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "agentx-health",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "agentx-runtime", "version": "1"}
                }
            })),
        RuntimeResourceProbeV1::HttpGet { path } => client
            .get(
                &format!("{}{}", request.endpoint.trim_end_matches('/'), path),
                context,
                timeout,
            )
            .map_err(|error| RuntimeError::InvalidRequest("INVALID_ENDPOINT", error.to_string()))?,
    };
    if let Some(token) = token {
        outgoing = outgoing.bearer_auth(token);
    }
    let result = outgoing.send().await;
    let value = match result {
        Ok(provider_response) if provider_response.status().is_success() => {
            response(checked_at, started, "healthy", None, None)
        }
        Ok(provider_response) => response(
            checked_at,
            started,
            "unhealthy",
            Some("PROVIDER_HTTP_ERROR"),
            Some(&format!("provider returned {}", provider_response.status())),
        ),
        Err(error) => {
            let (code, message) = if error.is_timeout() {
                ("PROVIDER_TIMEOUT", "provider request timed out")
            } else {
                ("PROVIDER_UNAVAILABLE", "provider could not be reached")
            };
            response(checked_at, started, "unhealthy", Some(code), Some(message))
        }
    };
    Ok(Json(value))
}

pub async fn execute_resource_operation(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<RuntimeResourceOperationRequestV1>,
) -> RuntimeResult<Json<RuntimeResourceOperationResponseV1>> {
    state
        .trust
        .publisher(&headers, "runtime.resources.execute")?;
    validate_endpoint_and_credential(
        &request.endpoint,
        request.tenant_id,
        request.credential.as_ref(),
    )?;
    if !(1..=60).contains(&request.timeout_seconds) {
        return Err(RuntimeError::InvalidRequest(
            "INVALID_TIMEOUT",
            "resource operation timeout must contain 1 to 60 seconds".into(),
        ));
    }
    let credential = resolve_credential(&state, request.credential.as_ref()).await?;
    let client = ProviderHttpClient::from_env(agentx_runtime_contracts::EgressRole::RuntimeGateway)
        .map_err(RuntimeError::Internal)?;
    let context = EgressRequestContext::request(request.tenant_id, request.operation_id);
    let timeout = Duration::from_secs(u64::from(request.timeout_seconds));
    let started = Instant::now();
    let session = mcp_initialize(
        &client,
        context,
        timeout,
        &request.endpoint,
        credential.as_deref(),
    )
    .await?;
    let result = match request.operation {
        RuntimeResourceOperationV1::McpDiscover => {
            mcp_rpc(
                &client,
                context,
                timeout,
                &request.endpoint,
                credential.as_deref(),
                "tools/list",
                json!({}),
                session.as_deref(),
                2,
            )
            .await?
            .0
        }
        RuntimeResourceOperationV1::McpCall {
            tool_name,
            arguments,
        } => {
            if tool_name.trim().is_empty() || tool_name.len() > 256 || !arguments.is_object() {
                return Err(RuntimeError::InvalidRequest(
                    "INVALID_MCP_CALL",
                    "MCP tool name and arguments are invalid".into(),
                ));
            }
            mcp_rpc(
                &client,
                context,
                timeout,
                &request.endpoint,
                credential.as_deref(),
                "tools/call",
                json!({"name": tool_name, "arguments": arguments}),
                session.as_deref(),
                2,
            )
            .await?
            .0
        }
    };
    Ok(Json(RuntimeResourceOperationResponseV1 {
        schema_version: 1,
        operation_id: request.operation_id,
        result,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    }))
}

fn validate_request(request: &RuntimeResourceCheckRequestV1) -> RuntimeResult<()> {
    validate_endpoint_and_credential(
        &request.endpoint,
        request.tenant_id,
        request.credential.as_ref(),
    )?;
    if let RuntimeResourceProbeV1::HttpGet { path } = &request.probe
        && (!path.starts_with('/') || path.len() > 512)
    {
        return Err(RuntimeError::InvalidRequest(
            "INVALID_HEALTH_PATH",
            "health path must begin with / and contain at most 512 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_endpoint_and_credential(
    endpoint: &str,
    tenant_id: uuid::Uuid,
    credential: Option<&VaultSecretReferenceV1>,
) -> RuntimeResult<()> {
    validate_provider_url(endpoint).map_err(|_| {
        RuntimeError::InvalidRequest("INVALID_ENDPOINT", "provider endpoint is invalid".into())
    })?;
    if let Some(reference) = credential {
        let prefix = format!("tenants/{tenant_id}/credentials/");
        if reference.version == 0
            || reference.key != "value"
            || !reference.path.starts_with(&prefix)
            || reference.path[prefix.len()..].contains('/')
        {
            return Err(RuntimeError::InvalidRequest(
                "INVALID_CREDENTIAL_REFERENCE",
                "credential reference is not tenant-scoped and versioned".into(),
            ));
        }
    }
    Ok(())
}

async fn resolve_credential(
    state: &RuntimeState,
    reference: Option<&VaultSecretReferenceV1>,
) -> RuntimeResult<Option<String>> {
    let Some(reference) = reference else {
        return Ok(None);
    };
    let vault = state
        .vault
        .as_ref()
        .ok_or(RuntimeError::SecretUnavailable)?;
    let value = vault.read(reference).await?;
    String::from_utf8(value).map(Some).map_err(|_| {
        RuntimeError::InvalidRequest(
            "INVALID_CREDENTIAL",
            "credential value must be UTF-8".into(),
        )
    })
}

async fn mcp_initialize(
    client: &ProviderHttpClient,
    context: EgressRequestContext,
    timeout: Duration,
    endpoint: &str,
    credential: Option<&str>,
) -> RuntimeResult<Option<String>> {
    let (_, session) = mcp_rpc(
        client,
        context,
        timeout,
        endpoint,
        credential,
        "initialize",
        json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "agentx-runtime", "version": "1"}
        }),
        None,
        1,
    )
    .await?;
    let mut notification = client
        .post(endpoint, context, timeout)
        .map_err(|_| RuntimeError::Unavailable)?
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }));
    if let Some(session) = session.as_deref() {
        notification = notification.header("mcp-session-id", session);
    }
    if let Some(credential) = credential {
        notification = notification.bearer_auth(credential);
    }
    let response = notification.send().await.map_err(|error| {
        tracing::warn!(%error, "Runtime MCP notification failed");
        RuntimeError::Unavailable
    })?;
    if !response.status().is_success() {
        return Err(RuntimeError::Unavailable);
    }
    Ok(session)
}

#[allow(clippy::too_many_arguments)]
async fn mcp_rpc(
    client: &ProviderHttpClient,
    context: EgressRequestContext,
    timeout: Duration,
    endpoint: &str,
    credential: Option<&str>,
    method: &str,
    params: serde_json::Value,
    session: Option<&str>,
    id: u64,
) -> RuntimeResult<(serde_json::Value, Option<String>)> {
    let mut outgoing = client
        .post(endpoint, context, timeout)
        .map_err(|_| RuntimeError::Unavailable)?
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    if let Some(session) = session {
        outgoing = outgoing.header("mcp-session-id", session);
    }
    if let Some(credential) = credential {
        outgoing = outgoing.bearer_auth(credential);
    }
    let response = outgoing.send().await.map_err(|error| {
        tracing::warn!(%error, %method, "Runtime MCP request failed");
        RuntimeError::Unavailable
    })?;
    let next_session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .or_else(|| session.map(ToOwned::to_owned));
    if !response.status().is_success() {
        return Err(RuntimeError::Unavailable);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| RuntimeError::Unavailable)?;
    if bytes.len() > 1024 * 1024 {
        return Err(RuntimeError::InvalidRequest(
            "MCP_RESPONSE_TOO_LARGE",
            "MCP response exceeds 1 MiB".into(),
        ));
    }
    let envelope = parse_mcp_response(&bytes)?;
    if envelope.get("error").is_some() {
        return Err(RuntimeError::Unavailable);
    }
    Ok((
        envelope
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        next_session,
    ))
}

fn parse_mcp_response(bytes: &[u8]) -> RuntimeResult<serde_json::Value> {
    if let Ok(value) = serde_json::from_slice(bytes) {
        return Ok(value);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| RuntimeError::Unavailable)?;
    text.lines()
        .rev()
        .filter_map(|line| line.strip_prefix("data:"))
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .ok_or(RuntimeError::Unavailable)
}

fn response(
    checked_at: OffsetDateTime,
    started: Instant,
    status: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> RuntimeResourceCheckResponseV1 {
    RuntimeResourceCheckResponseV1 {
        schema_version: 1,
        status: status.into(),
        latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        error_code: error_code.map(str::to_owned),
        error_message: error_message.map(str::to_owned),
        checked_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentx_runtime_contracts::VaultSecretReferenceV1;
    use uuid::Uuid;

    #[test]
    fn rejects_cross_tenant_or_unversioned_credential_references() {
        let tenant = Uuid::now_v7();
        let mut request = RuntimeResourceCheckRequestV1 {
            schema_version: 1,
            tenant_id: tenant,
            endpoint: "https://provider.example/v1".into(),
            credential: Some(VaultSecretReferenceV1 {
                mount: "secret".into(),
                path: format!("tenants/{tenant}/credentials/credential-1"),
                key: "value".into(),
                version: 1,
            }),
            probe: RuntimeResourceProbeV1::ModelChat {
                model: "model-1".into(),
            },
        };
        assert!(validate_request(&request).is_ok());
        request.credential.as_mut().unwrap().version = 0;
        assert!(validate_request(&request).is_err());
        request.credential.as_mut().unwrap().version = 1;
        request.credential.as_mut().unwrap().path = "tenants/other/credentials/credential-1".into();
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn rejects_literal_metadata_and_private_provider_addresses() {
        for endpoint in [
            "http://169.254.169.254/latest/meta-data",
            "http://127.0.0.1:8090/mcp",
            "http://10.0.0.8/mcp",
            "http://[::1]/mcp",
        ] {
            assert!(
                validate_endpoint_and_credential(endpoint, Uuid::now_v7(), None).is_err(),
                "{endpoint} must be rejected"
            );
        }
        assert!(
            validate_endpoint_and_credential(
                "http://echo-mcp.agentx-v2-deps.svc.cluster.local:8090/mcp",
                Uuid::now_v7(),
                None,
            )
            .is_ok()
        );
    }
}
