use std::time::{Duration, Instant};

use agentx_runtime_contracts::{
    ProcessEnvironmentCredentialV1, ProcessSessionControlRequestV1, ProcessSessionFramesResponseV1,
    ProcessSessionProofV1, ProcessSessionReplayPolicyV1, ProcessSessionStartRequestV1,
    ProcessSessionStartResponseV1, ProcessSessionWriteRequestV1, RuntimeMcpTransportV2,
    RuntimeResourceBindingV1, RuntimeResourceCheckRequestV1, RuntimeResourceCheckResponseV1,
    RuntimeResourceOperationRequestV1, RuntimeResourceOperationResponseV1,
    RuntimeResourceOperationV1, RuntimeResourceProbeV1, VaultSecretReferenceV1,
};
use axum::{Json, extract::State, http::HeaderMap};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    RuntimeState,
    egress::{EgressRequestContext, ProviderHttpClient, validate_provider_url},
    error::{RuntimeError, RuntimeResult},
    worker_runtime::WorkerProvider,
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
    if !(1..=60).contains(&request.timeout_seconds) {
        return Err(RuntimeError::InvalidRequest(
            "INVALID_TIMEOUT",
            "resource operation timeout must contain 1 to 60 seconds".into(),
        ));
    }
    let started = Instant::now();
    let credential = request.credential;
    let result = match request.transport {
        RuntimeMcpTransportV2::StreamableHttp { endpoint } => {
            execute_http_mcp_operation(
                &state,
                request.tenant_id,
                request.operation_id,
                request.timeout_seconds,
                &endpoint,
                credential.as_ref(),
                false,
                request.operation,
            )
            .await?
        }
        RuntimeMcpTransportV2::Sse { endpoint } => {
            execute_http_mcp_operation(
                &state,
                request.tenant_id,
                request.operation_id,
                request.timeout_seconds,
                &endpoint,
                credential.as_ref(),
                true,
                request.operation,
            )
            .await?
        }
        transport @ RuntimeMcpTransportV2::Stdio { .. } => {
            execute_stdio_mcp_operation(
                &state,
                request.tenant_id,
                request.operation_id,
                request.server_version_id,
                request.timeout_seconds,
                transport,
                request.runtime_sandbox_profile,
                request.operation,
            )
            .await?
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

#[allow(clippy::too_many_arguments)]
async fn execute_http_mcp_operation(
    state: &RuntimeState,
    tenant_id: Uuid,
    operation_id: Uuid,
    timeout_seconds: u32,
    endpoint: &str,
    credential_reference: Option<&VaultSecretReferenceV1>,
    legacy_sse: bool,
    operation: RuntimeResourceOperationV1,
) -> RuntimeResult<serde_json::Value> {
    validate_endpoint_and_credential(endpoint, tenant_id, credential_reference)?;
    let credential = resolve_credential(state, credential_reference).await?;
    let client = ProviderHttpClient::from_env(agentx_runtime_contracts::EgressRole::RuntimeGateway)
        .map_err(RuntimeError::Internal)?;
    let context = EgressRequestContext::request(tenant_id, operation_id);
    let timeout = Duration::from_secs(u64::from(timeout_seconds));
    let result = async {
        let session = mcp_initialize(
            &client,
            context,
            timeout,
            endpoint,
            credential.as_deref(),
            legacy_sse,
        )
        .await?;
        match operation {
            RuntimeResourceOperationV1::McpInitialize => Ok(json!({"initialized":true})),
            RuntimeResourceOperationV1::McpDiscover => Ok(mcp_rpc(
                &client,
                context,
                timeout,
                endpoint,
                credential.as_deref(),
                "tools/list",
                json!({}),
                session.as_deref(),
                2,
                legacy_sse,
            )
            .await?
            .0),
            RuntimeResourceOperationV1::McpCall {
                tool_name,
                arguments,
            } => {
                validate_mcp_call(&tool_name, &arguments)?;
                Ok(mcp_rpc(
                    &client,
                    context,
                    timeout,
                    endpoint,
                    credential.as_deref(),
                    "tools/call",
                    json!({"name": tool_name, "arguments": arguments}),
                    session.as_deref(),
                    2,
                    legacy_sse,
                )
                .await?
                .0)
            }
        }
    }
    .await;
    if legacy_sse {
        client
            .close_legacy_sse_session(&legacy_sse_session_key(context, endpoint))
            .await;
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn execute_stdio_mcp_operation(
    state: &RuntimeState,
    tenant_id: Uuid,
    operation_id: Uuid,
    server_version_id: Uuid,
    timeout_seconds: u32,
    transport: RuntimeMcpTransportV2,
    profile: Option<RuntimeResourceBindingV1>,
    operation: RuntimeResourceOperationV1,
) -> RuntimeResult<serde_json::Value> {
    let RuntimeMcpTransportV2::Stdio {
        command,
        args,
        environment_credential_refs,
        runtime_sandbox,
    } = transport
    else {
        return Err(RuntimeError::InvalidRequest(
            "INVALID_MCP_TRANSPORT",
            "stdio operation requires stdio transport".into(),
        ));
    };
    let profile = profile.ok_or_else(|| {
        RuntimeError::InvalidRequest(
            "MCP_STDIO_SANDBOX_REQUIRED",
            "stdio MCP diagnostics require the exact Runtime Sandbox profile".into(),
        )
    })?;
    if profile.resource_id != runtime_sandbox.resource_id
        || profile.resource_version != runtime_sandbox.resource_version_id.to_string()
        || !matches!(
            &profile.configuration,
            agentx_runtime_contracts::RuntimeResourceConfigurationV1::SandboxProfile { .. }
        )
    {
        return Err(RuntimeError::InvalidRequest(
            "MCP_STDIO_SANDBOX_REQUIRED",
            "stdio MCP diagnostic Sandbox profile does not match the frozen Server Version".into(),
        ));
    }
    for reference in &environment_credential_refs {
        validate_endpoint_and_credential(
            "https://diagnostic.invalid",
            tenant_id,
            Some(&reference.credential),
        )?;
    }
    let execution_id = operation_id;
    let node_execution_id =
        agentx_runtime_contracts::deterministic_uuid(operation_id, b"mcp-control-diagnostic-node");
    let attempt_id = agentx_runtime_contracts::deterministic_uuid(
        operation_id,
        b"mcp-control-diagnostic-attempt",
    );
    let worker_id = agentx_runtime_contracts::deterministic_uuid(
        operation_id,
        b"mcp-control-diagnostic-worker",
    );
    let deadline = OffsetDateTime::now_utc() + time::Duration::seconds(i64::from(timeout_seconds));
    sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,worker_instance_id,fencing_token,locked_until,deadline_at,started_at) VALUES(?,?,?,?,1,'running',?,?,1,?,?,UTC_TIMESTAMP(6))")
        .bind(attempt_id)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(node_execution_id)
        .bind(format!("mcp-control-diagnostic:{operation_id}"))
        .bind(worker_id.to_string())
        .bind(deadline)
        .bind(deadline)
        .execute(&state.pool)
        .await?;
    let manager =
        std::env::var("AGENTX_SANDBOX_MANAGER_ENDPOINT").map_err(|_| RuntimeError::Unavailable)?;
    let client = ProviderHttpClient::from_env(agentx_runtime_contracts::EgressRole::RuntimeGateway)
        .map_err(RuntimeError::Internal)?;
    let timeout = Duration::from_secs(u64::from(timeout_seconds));
    let start_request = ProcessSessionStartRequestV1 {
        api_version: 1,
        identity: agentx_runtime_contracts::ProcessSessionIdentityV1 {
            tenant_id,
            agent_run_id: operation_id,
            mcp_server_version_id: server_version_id,
            sandbox_profile_version_id: runtime_sandbox.resource_version_id,
        },
        execution_id,
        node_execution_id,
        attempt_id,
        worker_id,
        fencing_token: 1,
        operation_id: format!("diagnostic-start:{operation_id}"),
        effect_id: format!("diagnostic-start:{operation_id}"),
        idempotency_key: format!("diagnostic-start:{operation_id}"),
        command,
        args,
        environment_credentials: environment_credential_refs
            .into_iter()
            .map(|reference| ProcessEnvironmentCredentialV1 {
                name: reference.name,
                credential: reference.credential,
            })
            .collect(),
        profile,
        deadline,
    };
    let start_url = format!(
        "{}/internal/runtime/v1/sandbox-process-sessions:start",
        manager.trim_end_matches('/')
    );
    let start = manager_post_json::<_, ProcessSessionStartResponseV1>(
        &client,
        &start_url,
        timeout,
        &start_request,
    )
    .await;
    let start = match start {
        Ok(start) => start,
        Err(error) => {
            finish_diagnostic_attempt(state, attempt_id, "failed").await;
            return Err(error);
        }
    };
    let base_proof = ProcessSessionProofV1 {
        api_version: 1,
        tenant_id,
        execution_id,
        node_execution_id,
        attempt_id,
        agent_run_id: operation_id,
        worker_id,
        fencing_token: 1,
        process_session_id: start.lease.process_session_id,
        lease_id: start.lease.lease_id,
        operation_id: String::new(),
        effect_id: String::new(),
        idempotency_key: String::new(),
        deadline,
    };
    let operation_result = async {
        let initialize = json!({
            "jsonrpc":"2.0","id":"diagnostic-initialize","method":"initialize",
            "params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"agentx-control-diagnostic","version":"1.1"}}
        });
        let initialized = process_write(
            &client,
            &manager,
            timeout,
            &base_proof,
            "initialize",
            initialize,
        )
        .await?;
        if initialized
            .get("result")
            .is_none()
        {
            return Err(RuntimeError::Unavailable);
        }
        process_write(
            &client,
            &manager,
            timeout,
            &base_proof,
            "initialized",
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .await?;
        match operation {
            RuntimeResourceOperationV1::McpInitialize => Ok(json!({"initialized":true})),
            RuntimeResourceOperationV1::McpDiscover => {
                let envelope = process_write(
                    &client,
                    &manager,
                    timeout,
                    &base_proof,
                    "tools-list",
                    json!({"jsonrpc":"2.0","id":"diagnostic-operation","method":"tools/list","params":{}}),
                )
                .await?;
                rpc_result(envelope)
            }
            RuntimeResourceOperationV1::McpCall {
                tool_name,
                arguments,
            } => {
                validate_mcp_call(&tool_name, &arguments)?;
                let envelope = process_write(
                    &client,
                    &manager,
                    timeout,
                    &base_proof,
                    "tools-call",
                    json!({"jsonrpc":"2.0","id":"diagnostic-operation","method":"tools/call","params":{"name":tool_name,"arguments":arguments}}),
                )
                .await?;
                rpc_result(envelope)
            }
        }
    }
    .await;
    let terminate_url = format!(
        "{}/internal/runtime/v1/sandbox-process-sessions/{}:terminate",
        manager.trim_end_matches('/'),
        start.lease.process_session_id
    );
    let terminate =
        manager_post_json::<_, agentx_runtime_contracts::ProcessSessionControlResponseV1>(
            &client,
            &terminate_url,
            timeout,
            &ProcessSessionControlRequestV1 {
                proof: proof_for(&base_proof, "terminate"),
            },
        )
        .await;
    finish_diagnostic_attempt(
        state,
        attempt_id,
        if operation_result.is_ok() && terminate.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
    )
    .await;
    let result = operation_result?;
    terminate?;
    Ok(result)
}

async fn manager_post_json<T: serde::Serialize + ?Sized, R: serde::de::DeserializeOwned>(
    client: &ProviderHttpClient,
    endpoint: &str,
    timeout: Duration,
    request: &T,
) -> RuntimeResult<R> {
    let response = client
        .post_sandbox_manager(endpoint, timeout)
        .map_err(RuntimeError::Internal)?
        .json(request)
        .send()
        .await
        .map_err(|_| RuntimeError::Unavailable)?;
    if !response.status().is_success() {
        return Err(RuntimeError::Unavailable);
    }
    response.json().await.map_err(|_| RuntimeError::Unavailable)
}

async fn process_write(
    client: &ProviderHttpClient,
    manager: &str,
    timeout: Duration,
    proof: &ProcessSessionProofV1,
    operation: &str,
    frame: serde_json::Value,
) -> RuntimeResult<serde_json::Value> {
    let expected_id = frame.get("id").cloned();
    let endpoint = format!(
        "{}/internal/runtime/v1/sandbox-process-sessions/{}:write",
        manager.trim_end_matches('/'),
        proof.process_session_id
    );
    let response: ProcessSessionFramesResponseV1 = manager_post_json(
        client,
        &endpoint,
        timeout,
        &ProcessSessionWriteRequestV1 {
            proof: proof_for(proof, operation),
            frame,
            replay_policy: ProcessSessionReplayPolicyV1::Safe,
        },
    )
    .await?;
    match expected_id {
        Some(expected) => response
            .frames
            .into_iter()
            .rev()
            .find(|frame| frame.stream == "stdout" && frame.payload.get("id") == Some(&expected))
            .map(|frame| frame.payload)
            .ok_or(RuntimeError::Unavailable),
        None => Ok(serde_json::Value::Null),
    }
}

fn proof_for(proof: &ProcessSessionProofV1, operation: &str) -> ProcessSessionProofV1 {
    ProcessSessionProofV1 {
        operation_id: format!("diagnostic-{operation}:{}", proof.agent_run_id),
        effect_id: format!("diagnostic-{operation}:{}", proof.agent_run_id),
        idempotency_key: format!("diagnostic-{operation}:{}", proof.agent_run_id),
        ..proof.clone()
    }
}

fn rpc_result(envelope: serde_json::Value) -> RuntimeResult<serde_json::Value> {
    if envelope.get("error").is_some() {
        return Err(RuntimeError::Unavailable);
    }
    Ok(envelope
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

async fn finish_diagnostic_attempt(state: &RuntimeState, attempt_id: Uuid, status: &str) {
    let _ = sqlx::query(
        "UPDATE node_attempts SET status=?,locked_until=NULL,ended_at=UTC_TIMESTAMP(6) WHERE id=?",
    )
    .bind(status)
    .bind(attempt_id)
    .execute(&state.pool)
    .await;
}

fn validate_mcp_call(tool_name: &str, arguments: &serde_json::Value) -> RuntimeResult<()> {
    if tool_name.trim().is_empty() || tool_name.len() > 256 || !arguments.is_object() {
        return Err(RuntimeError::InvalidRequest(
            "INVALID_MCP_CALL",
            "MCP tool name and arguments are invalid".into(),
        ));
    }
    Ok(())
}

async fn mcp_initialize(
    client: &ProviderHttpClient,
    context: EgressRequestContext,
    timeout: Duration,
    endpoint: &str,
    credential: Option<&str>,
    legacy_sse: bool,
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
        legacy_sse,
    )
    .await?;
    if legacy_sse {
        mcp_rpc(
            client,
            context,
            timeout,
            endpoint,
            credential,
            "notifications/initialized",
            json!({}),
            session.as_deref(),
            0,
            true,
        )
        .await?;
        return Ok(session);
    }
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
    legacy_sse: bool,
) -> RuntimeResult<(serde_json::Value, Option<String>)> {
    if legacy_sse {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/json".parse().expect("static header"),
        );
        headers.insert(ACCEPT, "text/event-stream".parse().expect("static header"));
        if let Some(credential) = credential {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {credential}")
                    .parse()
                    .map_err(|_| RuntimeError::SecretUnavailable)?,
            );
        }
        let mut envelope = json!({"jsonrpc":"2.0","method":method,"params":params});
        if id != 0 {
            envelope["id"] = json!(id);
        }
        let response = client
            .legacy_sse_rpc(
                &legacy_sse_session_key(context, endpoint),
                endpoint,
                context,
                timeout,
                headers,
                &envelope,
            )
            .await
            .map_err(|_| RuntimeError::Unavailable)?;
        if !response.status.is_success() {
            return Err(RuntimeError::Unavailable);
        }
        if id == 0 {
            return Ok((serde_json::Value::Null, session.map(ToOwned::to_owned)));
        }
        let envelope = parse_mcp_response(&response.body)?;
        if envelope.get("error").is_some() {
            return Err(RuntimeError::Unavailable);
        }
        return Ok((
            envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            session.map(ToOwned::to_owned),
        ));
    }
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

fn legacy_sse_session_key(context: EgressRequestContext, endpoint: &str) -> String {
    format!(
        "resource-operation:{}:{}",
        context
            .request_id
            .or(context.execution_id)
            .unwrap_or(Uuid::nil()),
        crate::worker_support::raw_hash(&json!(endpoint))
    )
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
                "http://echo-mcp.agentx-deps.svc.cluster.local:8090/mcp",
                Uuid::now_v7(),
                None,
            )
            .is_ok()
        );
    }
}
