use std::{str::FromStr, sync::Arc};

use agentx_application::{
    CredentialResolver, ModelEvent, ModelRequest, ModelResponse, ModelRuntime, RuntimeContext,
    RuntimeError, RuntimeResult, RuntimeStream,
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client, StatusCode, header};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use url::Url;

use crate::{runtime_resources::MySqlResourceAuthorizer, sse::SseDecoder};

const MAX_MODEL_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_MODEL_EVENT_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct OpenAiCompatibleRuntime {
    client: Client,
    authorizer: MySqlResourceAuthorizer,
    credentials: Arc<dyn CredentialResolver>,
}

#[derive(Clone)]
struct PreparedModelCall {
    url: Url,
    body: Value,
    credential_id: Option<uuid::Uuid>,
    credential_version: Option<u64>,
    input_rate: Decimal,
    output_rate: Decimal,
    input_estimate: u64,
}

impl OpenAiCompatibleRuntime {
    pub fn new(
        authorizer: MySqlResourceAuthorizer,
        credentials: Arc<dyn CredentialResolver>,
    ) -> RuntimeResult<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| RuntimeError::new("MODEL_CLIENT_INVALID", error.to_string()))?,
            authorizer,
            credentials,
        })
    }

    async fn prepare(
        &self,
        context: &RuntimeContext,
        request: &ModelRequest,
        stream: bool,
    ) -> RuntimeResult<PreparedModelCall> {
        self.authorizer
            .authorize_context(context, &request.resource)
            .await?;
        let snapshot = context.resource(&request.resource).ok_or_else(|| {
            RuntimeError::new("RESOURCE_SNAPSHOT_MISSING", "Model snapshot is missing")
        })?;
        let value = &snapshot.snapshot;
        if value.get("providerType").and_then(Value::as_str) != Some("openai_compatible") {
            return Err(RuntimeError::new(
                "MODEL_PROVIDER_UNSUPPORTED",
                "The selected model provider is not supported by this Worker",
            ));
        }
        let endpoint = value
            .get("endpoint")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RuntimeError::new("MODEL_SNAPSHOT_INVALID", "Model endpoint is missing")
            })?;
        let mut base = Url::parse(endpoint).map_err(|_| {
            RuntimeError::new("MODEL_SNAPSHOT_INVALID", "Model endpoint is invalid")
        })?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(RuntimeError::new(
                "MODEL_SNAPSHOT_INVALID",
                "Model endpoint scheme is unsupported",
            ));
        }
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let url = base.join("chat/completions").map_err(|_| {
            RuntimeError::new("MODEL_SNAPSHOT_INVALID", "Model endpoint cannot be joined")
        })?;
        let (input_rate, output_rate) = model_price(value)?;
        let mut parameters = value
            .get("defaultParameters")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(overrides) = request.parameters.as_object() {
            parameters.extend(overrides.clone());
        }
        parameters.insert(
            "model".into(),
            Value::String(
                value
                    .get("modelName")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        RuntimeError::new("MODEL_SNAPSHOT_INVALID", "Model name is missing")
                    })?
                    .to_owned(),
            ),
        );
        parameters.insert("messages".into(), Value::Array(request.messages.clone()));
        parameters.insert("stream".into(), Value::Bool(stream));
        if !request.tools.is_empty() {
            parameters.insert("tools".into(), Value::Array(request.tools.clone()));
        }
        if stream {
            parameters.insert("stream_options".into(), json!({"include_usage": true}));
        }
        let input_estimate = serde_json::to_vec(&request.messages)
            .map_err(|error| RuntimeError::new("MODEL_REQUEST_INVALID", error.to_string()))?
            .len() as u64;
        let credential_id = value
            .get("credentialId")
            .and_then(Value::as_str)
            .map(uuid::Uuid::parse_str)
            .transpose()
            .map_err(|_| RuntimeError::new("MODEL_SNAPSHOT_INVALID", "Credential ID is invalid"))?;
        let credential_version = credential_id.and_then(|id| {
            context.resources.iter().find_map(|candidate| {
                (candidate.reference.resource_type.as_str() == "credential"
                    && candidate.reference.resource_id == id)
                    .then(|| {
                        candidate
                            .snapshot
                            .get("secretVersion")
                            .and_then(Value::as_u64)
                    })
                    .flatten()
            })
        });
        Ok(PreparedModelCall {
            url,
            body: Value::Object(parameters),
            credential_id,
            credential_version,
            input_rate,
            output_rate,
            input_estimate,
        })
    }

    async fn request(
        &self,
        context: &RuntimeContext,
        prepared: &PreparedModelCall,
    ) -> RuntimeResult<reqwest::Response> {
        let mut builder = self
            .client
            .post(prepared.url.clone())
            .header(header::ACCEPT, "application/json, text/event-stream")
            .json(&prepared.body);
        if let Some(id) = prepared.credential_id {
            let credential = self
                .credentials
                .resolve_for_runtime(context, id, prepared.credential_version)
                .await
                .map_err(|error| {
                    RuntimeError::new("MODEL_CREDENTIAL_INVALID", error.to_string())
                })?;
            let value = std::str::from_utf8(credential.secret.expose()).map_err(|_| {
                RuntimeError::new("MODEL_CREDENTIAL_INVALID", "Credential must be UTF-8")
            })?;
            match credential.credential_type.as_str() {
                "api_key" | "bearer" => builder = builder.bearer_auth(value),
                _ => {
                    return Err(RuntimeError::new(
                        "MODEL_CREDENTIAL_UNSUPPORTED",
                        "OpenAI-compatible runtime requires api_key or bearer credentials",
                    ));
                }
            }
        }
        tokio::select! {
            _ = context.cancellation.cancelled() => Err(RuntimeError::new("RUNTIME_CANCELLED", "Model call was cancelled")),
            result = builder.send() => {
                let response=result.map_err(map_transport)?;
                if response.status().is_success() { Ok(response) } else { Err(map_status(response.status())) }
            }
        }
    }
}

#[async_trait]
impl ModelRuntime for OpenAiCompatibleRuntime {
    async fn complete(
        &self,
        context: &RuntimeContext,
        request: ModelRequest,
    ) -> RuntimeResult<ModelResponse> {
        let prepared = self.prepare(context, &request, false).await?;
        let response = self.request(context, &prepared).await?;
        let bytes = response.bytes().await.map_err(map_transport)?;
        if bytes.len() > MAX_MODEL_RESPONSE_BYTES {
            return Err(RuntimeError::new(
                "MODEL_RESPONSE_TOO_LARGE",
                "Model response exceeded 16 MiB",
            ));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            RuntimeError::new("MODEL_PROTOCOL_ERROR", "Model response is not valid JSON")
        })?;
        response_from_json(&value, &prepared)
    }

    async fn stream(
        &self,
        context: &RuntimeContext,
        request: ModelRequest,
    ) -> RuntimeResult<RuntimeStream<ModelEvent>> {
        let prepared = self.prepare(context, &request, true).await?;
        let response = self.request(context, &prepared).await?;
        let mut source = response.bytes_stream();
        let cancellation = context.cancellation.clone();
        let (sender, receiver) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut decoder = SseDecoder::new(MAX_MODEL_EVENT_BYTES, MAX_MODEL_RESPONSE_BYTES);
            let mut message = Map::from_iter([("role".into(), Value::String("assistant".into()))]);
            let mut content = String::new();
            let mut tools = Vec::<Value>::new();
            let mut usage = None;
            let mut stop_reason = "stop".to_owned();
            let mut saw_done = false;
            loop {
                let next = tokio::select! {
                    _ = cancellation.cancelled() => {
                        let _=sender.send(Err(RuntimeError::new("RUNTIME_CANCELLED", "Model stream was cancelled").partial(!content.is_empty() || !tools.is_empty()))).await;
                        return;
                    }
                    value = source.next() => value,
                };
                let Some(chunk) = next else { break };
                let chunk = match chunk {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = sender
                            .send(Err(map_transport(error)
                                .partial(!content.is_empty() || !tools.is_empty())))
                            .await;
                        return;
                    }
                };
                let events = match decoder.push(&chunk) {
                    Ok(events) => events,
                    Err(error) => {
                        let _ = sender
                            .send(Err(error.partial(!content.is_empty() || !tools.is_empty())))
                            .await;
                        return;
                    }
                };
                for event in events {
                    let data = event.data;
                    if data == "[DONE]" {
                        saw_done = true;
                        break;
                    }
                    if data.is_empty() {
                        continue;
                    }
                    let value: Value = match serde_json::from_str(&data) {
                        Ok(value) => value,
                        Err(_) => {
                            let _ = sender
                                .send(Err(RuntimeError::new(
                                    "MODEL_PROTOCOL_ERROR",
                                    "Model SSE event is invalid JSON",
                                )
                                .partial(!content.is_empty() || !tools.is_empty())))
                                .await;
                            return;
                        }
                    };
                    if value.get("usage").is_some() {
                        usage = value.get("usage").cloned();
                    }
                    if let Some(choice) = value
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|v| v.first())
                    {
                        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                            stop_reason = reason.to_owned();
                        }
                        if let Some(delta) = choice.get("delta") {
                            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                                content.push_str(text);
                                let _ = sender
                                    .send(Ok(ModelEvent::TextDelta {
                                        text: text.to_owned(),
                                    }))
                                    .await;
                            }
                            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                                for call in calls {
                                    let index =
                                        call.get("index").and_then(Value::as_u64).unwrap_or(0)
                                            as usize;
                                    while tools.len() <= index {
                                        tools.push(json!({}));
                                    }
                                    merge_tool_delta(&mut tools[index], call);
                                    let _ = sender
                                        .send(Ok(ModelEvent::ToolCallDelta {
                                            index: index as u32,
                                            delta: call.clone(),
                                        }))
                                        .await;
                                }
                            }
                        }
                    }
                }
                if saw_done {
                    break;
                }
            }
            if !saw_done {
                let _ = sender
                    .send(Err(incomplete_stream_error(
                        &decoder,
                        !content.is_empty() || !tools.is_empty(),
                    )))
                    .await;
                return;
            }
            message.insert("content".into(), Value::String(content.clone()));
            if !tools.is_empty() {
                message.insert("tool_calls".into(), Value::Array(tools.clone()));
            }
            let (input_tokens, output_tokens, estimated) = usage_tokens(
                usage.as_ref(),
                prepared.input_estimate,
                content.len() as u64,
            );
            let response = ModelResponse {
                message: Value::Object(message),
                tool_calls: tools,
                input_tokens,
                output_tokens,
                cost_micros: cost(
                    input_tokens,
                    output_tokens,
                    prepared.input_rate,
                    prepared.output_rate,
                ),
                usage_estimated: estimated,
                stop_reason,
                partial: false,
            };
            let _ = sender.send(Ok(ModelEvent::Completed { response })).await;
        });
        Ok(Box::pin(ReceiverStream::new(receiver)))
    }
}

fn model_price(value: &Value) -> RuntimeResult<(Decimal, Decimal)> {
    let price = value
        .get("price")
        .ok_or_else(|| RuntimeError::new("MODEL_PRICE_MISSING", "Model price is missing"))?;
    if price.get("currency").and_then(Value::as_str) != Some("USD") {
        return Err(RuntimeError::new(
            "MODEL_PRICE_CURRENCY_UNSUPPORTED",
            "M5 runtime cost aggregation supports USD price versions only",
        ));
    }
    Ok((
        decimal(price, "inputPerMillion")?,
        decimal(price, "outputPerMillion")?,
    ))
}

fn incomplete_stream_error(decoder: &SseDecoder, has_partial_output: bool) -> RuntimeError {
    decoder
        .finish()
        .err()
        .unwrap_or_else(|| {
            RuntimeError::new(
                "MODEL_STREAM_INTERRUPTED",
                "Model stream ended before the [DONE] event",
            )
        })
        .partial(has_partial_output)
}

fn response_from_json(value: &Value, prepared: &PreparedModelCall) -> RuntimeResult<ModelResponse> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|v| v.first())
        .ok_or_else(|| RuntimeError::new("MODEL_PROTOCOL_ERROR", "Model response has no choice"))?;
    let message = choice.get("message").cloned().ok_or_else(|| {
        RuntimeError::new("MODEL_PROTOCOL_ERROR", "Model response has no message")
    })?;
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let output_estimate = message
        .get("content")
        .and_then(Value::as_str)
        .map_or(0, |v| v.len() as u64);
    let (input_tokens, output_tokens, estimated) =
        usage_tokens(value.get("usage"), prepared.input_estimate, output_estimate);
    Ok(ModelResponse {
        message,
        tool_calls,
        input_tokens,
        output_tokens,
        cost_micros: cost(
            input_tokens,
            output_tokens,
            prepared.input_rate,
            prepared.output_rate,
        ),
        usage_estimated: estimated,
        stop_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop")
            .to_owned(),
        partial: false,
    })
}

fn decimal(value: &Value, field: &str) -> RuntimeResult<Decimal> {
    let value = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        RuntimeError::new(
            "MODEL_PRICE_MISSING",
            format!("Model price {field} is missing"),
        )
    })?;
    Decimal::from_str(value).map_err(|_| {
        RuntimeError::new(
            "MODEL_PRICE_INVALID",
            format!("Model price {field} is invalid"),
        )
    })
}

fn cost(input: u64, output: u64, input_rate: Decimal, output_rate: Decimal) -> u64 {
    (Decimal::from(input) * input_rate + Decimal::from(output) * output_rate)
        .ceil()
        .to_u64()
        .unwrap_or(u64::MAX)
}

fn usage_tokens(
    usage: Option<&Value>,
    input_estimate: u64,
    output_estimate: u64,
) -> (u64, u64, bool) {
    let input = usage
        .and_then(|v| v.get("prompt_tokens"))
        .and_then(Value::as_u64);
    let output = usage
        .and_then(|v| v.get("completion_tokens"))
        .and_then(Value::as_u64);
    (
        input.unwrap_or(input_estimate),
        output.unwrap_or(output_estimate),
        input.is_none() || output.is_none(),
    )
}

fn merge_tool_delta(target: &mut Value, delta: &Value) {
    let object = target
        .as_object_mut()
        .expect("tool accumulator is an object");
    for key in ["id", "type"] {
        if let Some(value) = delta.get(key) {
            object.insert(key.into(), value.clone());
        }
    }
    if let Some(function) = delta.get("function").and_then(Value::as_object) {
        let target_function = object
            .entry("function")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("function accumulator");
        if let Some(name) = function.get("name") {
            target_function.insert("name".into(), name.clone());
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            let previous = target_function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            target_function.insert(
                "arguments".into(),
                Value::String(format!("{previous}{arguments}")),
            );
        }
    }
}

fn map_transport(error: reqwest::Error) -> RuntimeError {
    if error.is_timeout() {
        RuntimeError::new("MODEL_TIMEOUT", "Model request timed out").retryable(true)
    } else {
        RuntimeError::new(
            "MODEL_PROVIDER_UNAVAILABLE",
            "Model provider could not be reached",
        )
        .retryable(true)
        .outcome_unknown(error.is_request())
    }
}

fn map_status(status: StatusCode) -> RuntimeError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => RuntimeError::new(
            "MODEL_AUTHENTICATION_FAILED",
            "Model provider rejected the credential",
        ),
        StatusCode::TOO_MANY_REQUESTS => RuntimeError::new(
            "MODEL_RATE_LIMITED",
            "Model provider rate limit was reached",
        )
        .retryable(true),
        StatusCode::BAD_REQUEST => RuntimeError::new(
            "MODEL_REQUEST_REJECTED",
            "Model provider rejected the request",
        ),
        _ if status.is_server_error() => RuntimeError::new(
            "MODEL_PROVIDER_UNAVAILABLE",
            format!("Model provider returned HTTP {}", status.as_u16()),
        )
        .retryable(true),
        _ => RuntimeError::new(
            "MODEL_HTTP_STATUS",
            format!("Model provider returned HTTP {}", status.as_u16()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_uses_micros_without_float_rounding() {
        assert_eq!(
            cost(
                100,
                25,
                Decimal::from_str("5.0").unwrap(),
                Decimal::from_str("15.0").unwrap()
            ),
            875
        );
    }

    #[test]
    fn merges_fragmented_tool_arguments() {
        let mut value = json!({});
        merge_tool_delta(
            &mut value,
            &json!({"id":"call","function":{"name":"echo","arguments":"{\"a\":"}}),
        );
        merge_tool_delta(&mut value, &json!({"function":{"arguments":"1}"}}));
        assert_eq!(value["function"]["arguments"], "{\"a\":1}");
    }

    #[test]
    fn rejects_missing_or_non_usd_price_versions() {
        let missing = model_price(&json!({})).unwrap_err();
        assert_eq!(missing.code, "MODEL_PRICE_MISSING");

        let currency = model_price(&json!({
            "price": {
                "currency": "EUR",
                "inputPerMillion": "1",
                "outputPerMillion": "2"
            }
        }))
        .unwrap_err();
        assert_eq!(currency.code, "MODEL_PRICE_CURRENCY_UNSUPPORTED");

        let field = model_price(&json!({
            "price": {"currency": "USD", "inputPerMillion": "1"}
        }))
        .unwrap_err();
        assert_eq!(field.code, "MODEL_PRICE_MISSING");
    }

    #[test]
    fn eof_without_done_is_never_reported_as_a_complete_model_response() {
        let mut complete_event = SseDecoder::new(1024, 4096);
        complete_event
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();
        let error = incomplete_stream_error(&complete_event, true);
        assert_eq!(error.code, "MODEL_STREAM_INTERRUPTED");
        assert!(error.partial);

        let mut truncated_event = SseDecoder::new(1024, 4096);
        truncated_event.push(b"data: {\"choices\":[").unwrap();
        let error = incomplete_stream_error(&truncated_event, true);
        assert_eq!(error.code, "SSE_STREAM_INCOMPLETE");
        assert!(error.partial);
    }
}
