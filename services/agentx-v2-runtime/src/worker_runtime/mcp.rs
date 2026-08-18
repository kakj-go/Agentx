use agentx_runtime_contracts::{RuntimeResourceBindingV1, VaultSecretReferenceV1};
use serde_json::{Value, json};

use super::{
    RuntimeWorker, WorkerExecution, provider_secret_header, runtime_call_fingerprint, stable_id,
};
use crate::engine::ClaimedWorkerAttempt;

impl RuntimeWorker {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn call_mcp_tool(
        &self,
        claim: &ClaimedWorkerAttempt,
        endpoint: &str,
        tool_name: &str,
        input: Value,
        call_index: u32,
        secret: Option<&VaultSecretReferenceV1>,
        binding: &RuntimeResourceBindingV1,
    ) -> WorkerExecution {
        let request = json!({
            "jsonrpc":"2.0",
            "id":claim.task.attempt_id,
            "method":"tools/call",
            "params":{"name":tool_name,"arguments":input},
        });
        let fingerprint = runtime_call_fingerprint("mcp_tool", &request);
        let call_id = stable_id(
            claim.task.attempt_id,
            format!("mcp_tool:{call_index}").as_bytes(),
        );
        let idempotency_key = format!("{}:mcp_tool:{call_index}", claim.task.attempt_id);
        match self
            .reserve_call(
                claim,
                call_id,
                "mcp_tool",
                &idempotency_key,
                &fingerprint,
                &request,
                call_index,
                Some(binding),
            )
            .await
        {
            Ok(Some(value)) => return WorkerExecution::succeeded(value),
            Ok(None) => {}
            Err(result) => return result,
        }
        let credential = if let Some(reference) = secret {
            let Some(vault) = &self.vault else {
                return self
                    .fail_call(
                        call_id,
                        "VAULT_UNAVAILABLE",
                        "Runtime Vault is not configured",
                        false,
                    )
                    .await;
            };
            match vault.read(reference).await {
                Ok(value) => Some(provider_secret_header(&value, "authorization")),
                Err(error) => {
                    return self
                        .fail_call(call_id, "VAULT_UNAVAILABLE", error.to_string(), false)
                        .await;
                }
            }
        } else {
            None
        };
        if let Err(error) =
            sqlx::query("UPDATE runtime_calls SET status='sent' WHERE id=? AND status='reserved'")
                .bind(call_id)
                .execute(&self.pool)
                .await
        {
            return WorkerExecution::failed(
                "RUNTIME_CALL_STATE_UNAVAILABLE",
                error.to_string(),
                false,
            );
        }
        let (_, session) = match self
            .mcp_rpc(
                endpoint,
                credential.as_deref(),
                None,
                Some(json!(1)),
                "initialize",
                json!({
                    "protocolVersion":"2025-03-26",
                    "capabilities":{},
                    "clientInfo":{"name":"agentx-runtime-worker","version":"1"},
                }),
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                return self
                    .fail_call(call_id, "MCP_INITIALIZE_FAILED", error, false)
                    .await;
            }
        };
        if let Err(error) = self
            .mcp_rpc(
                endpoint,
                credential.as_deref(),
                session.as_deref(),
                None,
                "notifications/initialized",
                json!({}),
            )
            .await
        {
            return self
                .fail_call(call_id, "MCP_INITIALIZE_FAILED", error, false)
                .await;
        }
        let (payload, _) = match self
            .mcp_rpc(
                endpoint,
                credential.as_deref(),
                session.as_deref(),
                Some(json!(claim.task.attempt_id)),
                "tools/call",
                json!({"name":tool_name,"arguments":request["params"]["arguments"].clone()}),
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                return self
                    .fail_call(call_id, "PROVIDER_OUTCOME_UNKNOWN", error, true)
                    .await;
            }
        };
        if let Err(error) = sqlx::query(
            "UPDATE runtime_calls SET status='succeeded',response_json=?,ended_at=UTC_TIMESTAMP(6) WHERE id=? AND status='sent'",
        )
        .bind(&payload)
        .bind(call_id)
        .execute(&self.pool)
        .await
        {
            return WorkerExecution::failed("RUNTIME_CALL_COMMIT_FAILED", error.to_string(), true);
        }
        WorkerExecution::succeeded(payload)
    }

    async fn mcp_rpc(
        &self,
        endpoint: &str,
        credential: Option<&[u8]>,
        session: Option<&str>,
        id: Option<Value>,
        method: &str,
        params: Value,
    ) -> Result<(Value, Option<String>), String> {
        let mut envelope = json!({"jsonrpc":"2.0","method":method,"params":params});
        if let Some(id) = id {
            envelope["id"] = id;
        }
        let mut request = self
            .client
            .post(endpoint)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(&envelope);
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(credential) = credential {
            let value = reqwest::header::HeaderValue::from_bytes(credential)
                .map_err(|_| "MCP credential is not a valid authorization header".to_owned())?;
            request = request.header("authorization", value);
        }
        let response = request.send().await.map_err(|error| error.to_string())?;
        let status = response.status();
        let next_session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .or_else(|| session.map(str::to_owned));
        let bytes = response.bytes().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "MCP HTTP {status}: {}",
                String::from_utf8_lossy(&bytes)
            ));
        }
        if envelope.get("id").is_none() {
            return Ok((Value::Null, next_session));
        }
        let envelope = parse_mcp_response(&bytes)?;
        if let Some(error) = envelope.get("error") {
            return Err(format!("MCP RPC error: {error}"));
        }
        Ok((
            envelope.get("result").cloned().unwrap_or(Value::Null),
            next_session,
        ))
    }
}

fn parse_mcp_response(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("MCP response exceeds 1 MiB".into());
    }
    if let Ok(value) = serde_json::from_slice(bytes) {
        return Ok(value);
    }
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    text.lines()
        .rev()
        .filter_map(|line| line.strip_prefix("data:"))
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .ok_or_else(|| "MCP response is neither JSON nor an SSE JSON event".into())
}

#[cfg(test)]
mod tests {
    use super::parse_mcp_response;
    use serde_json::json;

    #[test]
    fn parses_json_and_streamable_http_sse_envelopes() {
        assert_eq!(
            parse_mcp_response(br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).unwrap(),
            json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}})
        );
        assert_eq!(
            parse_mcp_response(
                b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n"
            )
            .unwrap(),
            json!({"jsonrpc":"2.0","id":2,"result":{"ok":true}})
        );
    }
}
