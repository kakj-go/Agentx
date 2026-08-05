mod endpoint;
mod models;

use std::{collections::BTreeMap, str::FromStr, sync::Arc, time::Duration};

use agentx_application::{
    RuntimeContext, RuntimeError, RuntimeResult, RuntimeStream, SandboxCommand,
    SandboxCreateRequest, SandboxEvent, SandboxLease, SandboxMetrics, SandboxRuntime,
};
use async_trait::async_trait;
use futures::StreamExt;
use ipnet::IpNet;
use models::{
    CommandStreamEvent, CreateSandboxRequest as VendorCreateRequest, EndpointResponse,
    ExecdMetrics, ImageSpec, ListSandboxesResponse, RunCommandRequest, SandboxResponse,
};
use reqwest::{
    Client, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
    multipart,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use url::Url;
use uuid::Uuid;

use crate::sse::SseDecoder;
use endpoint::EndpointPolicy;

pub const LIFECYCLE_SPEC_SHA256: &str =
    "da84de4d80cdad83c47d771135645fbeb8d7477bc8f908cc4b374397010ed6d2";
pub const EXECD_SPEC_SHA256: &str =
    "0f03effe1dc5f340d13e39d6e8c815b5bdebb880183db05bea7d592696d5f5e0";
const EXECD_PORT: u16 = 44_772;
const LIFECYCLE_API_KEY_HEADER: &str = "open-sandbox-api-key";
const PROVIDER_TTL_GRACE_SECONDS: u64 = 30;

#[derive(Clone)]
pub struct OpenSandboxAdapter {
    client: Client,
    lifecycle: Url,
    api_key: SecretString,
    endpoint_policy: Arc<EndpointPolicy>,
    request_timeout: Duration,
    idle_timeout: Duration,
    max_event_bytes: usize,
    max_stream_bytes: usize,
    max_file_bytes: usize,
    secure_access: bool,
    use_server_proxy: bool,
    active_commands: Arc<Mutex<BTreeMap<String, String>>>,
}

impl OpenSandboxAdapter {
    pub fn new(endpoint: Url, api_key: SecretString) -> RuntimeResult<Self> {
        Self::with_policy(endpoint, api_key, Vec::new(), Vec::new())
    }

    pub fn with_policy(
        mut endpoint: Url,
        api_key: SecretString,
        allowed_hosts: Vec<String>,
        allowed_cidrs: Vec<String>,
    ) -> RuntimeResult<Self> {
        if endpoint.scheme() != "https" && endpoint.scheme() != "http" {
            return Err(protocol_error(
                "OpenSandbox lifecycle endpoint must use HTTP or HTTPS",
            ));
        }
        if endpoint.username() != ""
            || endpoint.password().is_some()
            || endpoint.host_str().is_none()
        {
            return Err(protocol_error(
                "OpenSandbox lifecycle endpoint contains forbidden URL components",
            ));
        }
        let allow_http = endpoint.scheme() == "http";
        let path = endpoint.path().trim_end_matches('/').to_owned();
        let lifecycle_path = if path.ends_with("/v1") {
            format!("{path}/")
        } else {
            "/v1/".to_owned()
        };
        endpoint.set_path(&lifecycle_path);
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let cidrs = allowed_cidrs
            .into_iter()
            .map(|value| {
                IpNet::from_str(&value)
                    .map_err(|_| protocol_error("OpenSandbox endpoint CIDR allowlist is invalid"))
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        let endpoint_policy = EndpointPolicy::new(
            &endpoint,
            allowed_hosts
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            cidrs,
            allow_http,
        )?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(transport_error)?;
        Ok(Self {
            client,
            lifecycle: endpoint,
            api_key,
            endpoint_policy: Arc::new(endpoint_policy),
            request_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(30),
            max_event_bytes: 256 * 1024,
            max_stream_bytes: 16 * 1024 * 1024,
            max_file_bytes: 64 * 1024 * 1024,
            secure_access: true,
            use_server_proxy: true,
            active_commands: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    #[must_use]
    pub fn with_secure_access(mut self, secure_access: bool) -> Self {
        self.secure_access = secure_access;
        self
    }

    #[must_use]
    pub fn with_server_proxy(mut self, use_server_proxy: bool) -> Self {
        self.use_server_proxy = use_server_proxy;
        self
    }

    pub async fn health(&self) -> RuntimeResult<()> {
        let mut health = self.lifecycle.clone();
        health.set_path("/health");
        let response = self
            .client
            .get(health)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(transport_error)?;
        require_status(response, &[StatusCode::OK])
            .await
            .map(|_| ())
    }

    pub async fn list_by_labels(
        &self,
        labels: &BTreeMap<String, String>,
    ) -> RuntimeResult<Vec<String>> {
        let mut url = self
            .lifecycle
            .join("sandboxes")
            .map_err(protocol_url_error)?;
        if !labels.is_empty() {
            let metadata = labels
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&");
            url.query_pairs_mut()
                .append_pair("metadata", &metadata)
                .append_pair("pageSize", "100");
        }
        let response = self
            .lifecycle_request(self.client.get(url))
            .send()
            .await
            .map_err(transport_error)?;
        let response = require_status(response, &[StatusCode::OK]).await?;
        let body: ListSandboxesResponse = response.json().await.map_err(protocol_json_error)?;
        Ok(body.items.into_iter().map(|sandbox| sandbox.id).collect())
    }

    pub async fn endpoint_auth_document(&self, sandbox_id: &str) -> RuntimeResult<Value> {
        let (endpoint, headers) = self.endpoint(sandbox_id).await?;
        let headers = headers
            .iter()
            .map(|(name, value)| {
                value
                    .to_str()
                    .map(|value| (name.as_str().to_owned(), Value::String(value.to_owned())))
                    .map_err(|_| protocol_error("OpenSandbox returned a non-text execd header"))
            })
            .collect::<RuntimeResult<serde_json::Map<_, _>>>()?;
        Ok(json!({"endpoint":endpoint.to_string(),"headers":headers}))
    }

    async fn create_vendor(&self, request: VendorCreateRequest) -> RuntimeResult<SandboxResponse> {
        let url = self
            .lifecycle
            .join("sandboxes")
            .map_err(protocol_url_error)?;
        let response = self
            .lifecycle_request(self.client.post(url))
            .json(&request)
            .send()
            .await
            .map_err(|error| transport_error(error).outcome_unknown(true))?;
        let response = require_status(response, &[StatusCode::ACCEPTED]).await?;
        response.json().await.map_err(protocol_json_error)
    }

    async fn get_vendor(&self, sandbox_id: &str) -> RuntimeResult<SandboxResponse> {
        validate_identifier(sandbox_id)?;
        let url = self
            .lifecycle
            .join(&format!("sandboxes/{sandbox_id}"))
            .map_err(protocol_url_error)?;
        let response = self
            .lifecycle_request(self.client.get(url))
            .send()
            .await
            .map_err(transport_error)?;
        let response = require_status(response, &[StatusCode::OK]).await?;
        response.json().await.map_err(protocol_json_error)
    }

    async fn endpoint(&self, sandbox_id: &str) -> RuntimeResult<(Url, HeaderMap)> {
        validate_identifier(sandbox_id)?;
        let mut url = self
            .lifecycle
            .join(&format!("sandboxes/{sandbox_id}/endpoints/{EXECD_PORT}"))
            .map_err(protocol_url_error)?;
        if self.use_server_proxy {
            url.query_pairs_mut()
                .append_pair("use_server_proxy", "true");
        }
        let response = self
            .lifecycle_request(self.client.get(url))
            .send()
            .await
            .map_err(transport_error)?;
        let response = require_status(response, &[StatusCode::OK]).await?;
        let endpoint: EndpointResponse = response.json().await.map_err(protocol_json_error)?;
        let (url, headers) = self
            .endpoint_policy
            .validate(&endpoint.endpoint, &endpoint.headers)?;
        if self.use_server_proxy {
            let expected_path = format!("/v1/sandboxes/{sandbox_id}/proxy/{EXECD_PORT}/");
            if !self
                .endpoint_policy
                .is_lifecycle_origin(&url, &self.lifecycle)
                || url.path() != expected_path
            {
                return Err(protocol_error(
                    "OpenSandbox returned an invalid server-proxy endpoint",
                ));
            }
        }
        Ok((url, headers))
    }

    async fn wait_execd_ready(
        &self,
        context: &RuntimeContext,
        sandbox_id: &str,
    ) -> RuntimeResult<()> {
        let bounded_deadline = OffsetDateTime::now_utc() + time::Duration::seconds(30);
        let readiness_deadline = context.deadline.min(bounded_deadline);
        loop {
            if context.cancellation.is_cancelled() {
                return Err(cancelled_error());
            }
            if OffsetDateTime::now_utc() >= readiness_deadline {
                return Err(RuntimeError::new(
                    "SANDBOX_CREATE_TIMEOUT",
                    "OpenSandbox execd did not become ready before the deadline",
                )
                .retryable(true));
            }
            let result: RuntimeResult<()> = async {
                let (url, headers) = self.endpoint(sandbox_id).await?;
                let headers = self.execd_headers(&url, headers)?;
                let ping = url.join("ping").map_err(protocol_url_error)?;
                let response = self
                    .client
                    .get(ping)
                    .headers(headers)
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await
                    .map_err(transport_error)?;
                require_status(response, &[StatusCode::OK]).await?;
                Ok(())
            }
            .await;
            match result {
                Ok(()) => return Ok(()),
                Err(error) if error.retryable => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn kill(&self, sandbox_id: &str) -> RuntimeResult<()> {
        validate_identifier(sandbox_id)?;
        let url = self
            .lifecycle
            .join(&format!("sandboxes/{sandbox_id}"))
            .map_err(protocol_url_error)?;
        for attempt in 0..3 {
            let result = match self
                .lifecycle_request(self.client.delete(url.clone()))
                .send()
                .await
            {
                Ok(response) => {
                    require_status(response, &[StatusCode::NO_CONTENT, StatusCode::NOT_FOUND])
                        .await
                        .map(|_| ())
                }
                Err(error) => Err(transport_error(error)),
            };
            match result {
                Err(error) if error.retryable && attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
                }
                result => return result,
            }
        }
        unreachable!("bounded Sandbox termination retries always return")
    }

    fn lifecycle_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("OPEN-SANDBOX-API-KEY", self.api_key.expose_secret())
            .timeout(self.request_timeout)
    }

    fn execd_headers(&self, url: &Url, mut headers: HeaderMap) -> RuntimeResult<HeaderMap> {
        if self
            .endpoint_policy
            .is_lifecycle_origin(url, &self.lifecycle)
        {
            headers.insert(
                HeaderName::from_static(LIFECYCLE_API_KEY_HEADER),
                HeaderValue::from_str(self.api_key.expose_secret()).map_err(|_| {
                    protocol_error("OpenSandbox API key is not a valid HTTP header value")
                })?,
            );
        }
        Ok(headers)
    }
}

#[async_trait]
impl SandboxRuntime for OpenSandboxAdapter {
    async fn create(
        &self,
        context: &RuntimeContext,
        request: SandboxCreateRequest,
    ) -> RuntimeResult<SandboxLease> {
        if context.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let profile = &request.profile.snapshot;
        let image = required_str(profile, "imageDigest")?;
        if !is_digest_image(image) {
            return Err(RuntimeError::new(
                "SANDBOX_PROFILE_INVALID",
                "Sandbox image must be pinned by sha256 digest",
            ));
        }
        let timeout = required_u64(profile, "timeoutSeconds")?.clamp(60, 86_400);
        let provider_timeout = timeout
            .saturating_add(PROVIDER_TTL_GRACE_SECONDS)
            .min(86_400);
        let mut resource_limits = BTreeMap::new();
        resource_limits.insert(
            "cpu".into(),
            format!("{}m", required_u64(profile, "cpuMillis")?),
        );
        resource_limits.insert(
            "memory".into(),
            required_u64(profile, "memoryBytes")?.to_string(),
        );
        resource_limits.insert(
            "pids".into(),
            required_u64(profile, "pidsLimit")?.to_string(),
        );
        resource_limits.insert(
            "ephemeral-storage".into(),
            required_u64(profile, "diskBytes")?.to_string(),
        );
        let profile_policy = profile
            .get("networkPolicy")
            .cloned()
            .unwrap_or_else(|| json!({"defaultAction":"deny","egress":[]}));
        let network_policy = if request.network_policy.is_null() {
            profile_policy
        } else {
            request.network_policy
        };
        if network_policy
            .get("defaultAction")
            .and_then(Value::as_str)
            .unwrap_or("deny")
            != "deny"
        {
            return Err(RuntimeError::new(
                "SANDBOX_NETWORK_POLICY_INVALID",
                "Sandbox network policy must default to deny",
            ));
        }
        let mut metadata = BTreeMap::from([
            ("agentx-tenant".into(), context.tenant_id.to_string()),
            ("agentx-execution".into(), context.execution_id.to_string()),
            ("agentx-attempt".into(), context.attempt_id.to_string()),
            (
                "agentx-request".into(),
                metadata_hash(&context.idempotency_key),
            ),
        ]);
        if let Some(labels) = request.labels.as_object() {
            for (key, value) in labels {
                if key.starts_with("agentx-") {
                    continue;
                }
                let value = value.as_str().ok_or_else(|| {
                    RuntimeError::new("SANDBOX_LABEL_INVALID", "Sandbox labels must be strings")
                })?;
                metadata.insert(key.clone(), value.to_owned());
            }
        }
        let mut response = self
            .create_vendor(VendorCreateRequest {
                image: ImageSpec {
                    uri: image.to_owned(),
                },
                timeout: provider_timeout,
                resource_limits,
                entrypoint: vec!["tail".into(), "-f".into(), "/dev/null".into()],
                metadata,
                network_policy,
                secure_access: self.secure_access,
            })
            .await?;
        if response.status.state == "Pending" {
            loop {
                if context.cancellation.is_cancelled() {
                    return Err(cancelled_error());
                }
                if OffsetDateTime::now_utc() >= context.deadline {
                    return Err(RuntimeError::new(
                        "SANDBOX_CREATE_TIMEOUT",
                        "OpenSandbox did not become ready before the deadline",
                    )
                    .outcome_unknown(true));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                response = self.get_vendor(&response.id).await?;
                if response.status.state != "Pending" {
                    break;
                }
            }
        }
        if response.status.state != "Running" {
            let reason = response.status.reason.unwrap_or_else(|| "unknown".into());
            return Err(RuntimeError::new(
                "SANDBOX_CREATE_FAILED",
                response.status.message.unwrap_or_else(|| {
                    format!(
                        "OpenSandbox entered state {} ({reason})",
                        response.status.state
                    )
                }),
            ));
        }
        for (key, expected) in [
            ("agentx-tenant", context.tenant_id.to_string()),
            ("agentx-execution", context.execution_id.to_string()),
            ("agentx-attempt", context.attempt_id.to_string()),
        ] {
            if response.metadata.get(key) != Some(&expected) {
                let _ = self.kill(&response.id).await;
                return Err(protocol_error(
                    "OpenSandbox did not preserve the required Agentx labels",
                ));
            }
        }
        if let Err(error) = self.wait_execd_ready(context, &response.id).await {
            if let Err(cleanup) = self.kill(&response.id).await {
                return Err(RuntimeError::new(
                    "SANDBOX_CLEANUP_PENDING",
                    format!("{error}; cleanup failed: {cleanup}"),
                )
                .retryable(true)
                .outcome_unknown(true));
            }
            return Err(error);
        }
        let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(timeout as i64);
        Ok(SandboxLease {
            lease_id: Uuid::now_v7(),
            sandbox_id: response.id,
            lease_token: Uuid::now_v7().to_string(),
            expires_at,
        })
    }

    async fn execute(
        &self,
        context: &RuntimeContext,
        command: SandboxCommand,
    ) -> RuntimeResult<RuntimeStream<SandboxEvent>> {
        validate_working_path(&command.working_directory)?;
        if command.argv.is_empty() || command.argv.iter().any(|value| value.contains('\0')) {
            return Err(RuntimeError::new(
                "SANDBOX_COMMAND_INVALID",
                "Sandbox command argv is invalid",
            ));
        }
        let environment = command
            .environment
            .as_object()
            .ok_or_else(|| {
                RuntimeError::new(
                    "SANDBOX_COMMAND_INVALID",
                    "Sandbox environment must be an object",
                )
            })?
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "SANDBOX_COMMAND_INVALID",
                            "Sandbox environment values must be strings",
                        )
                    })
            })
            .collect::<RuntimeResult<BTreeMap<_, _>>>()?;
        let (url, headers) = self.endpoint(&command.lease.sandbox_id).await?;
        let headers = self.execd_headers(&url, headers)?;
        let endpoint = url.join("command").map_err(protocol_url_error)?;
        let timeout_ms = (context.deadline - OffsetDateTime::now_utc())
            .whole_milliseconds()
            .max(1) as u64;
        let response = self
            .client
            .post(endpoint)
            .headers(headers.clone())
            .json(&RunCommandRequest {
                command: shell_join(&command.argv),
                cwd: command.working_directory,
                background: false,
                timeout: timeout_ms,
                environment,
            })
            .send()
            .await
            .map_err(|error| transport_error(error).outcome_unknown(true))?;
        let response = require_status(response, &[StatusCode::OK]).await?;
        let mut bytes = response.bytes_stream();
        let cancellation = context.cancellation.clone();
        let client = self.client.clone();
        let idle_timeout = self.idle_timeout;
        let max_event_bytes = self.max_event_bytes;
        let max_stream_bytes = self.max_stream_bytes;
        let active_commands = self.active_commands.clone();
        let sandbox_id = command.lease.sandbox_id.clone();
        let (sender, receiver) = mpsc::channel(16);
        tokio::spawn(async move {
            let mut decoder = SseDecoder::new(max_event_bytes, max_stream_bytes);
            let mut sequence = 0_u64;
            let mut command_id = None::<String>;
            let mut exit_code = 0_i32;
            let mut partial = false;
            let mut completed = false;
            let total_timeout = tokio::time::sleep(Duration::from_millis(timeout_ms));
            tokio::pin!(total_timeout);
            loop {
                let next = tokio::select! {
                    _ = cancellation.cancelled() => { partial = true; None },
                    _ = &mut total_timeout => {
                        let _=sender.send(Err(RuntimeError::new("SANDBOX_STREAM_TOTAL_TIMEOUT","OpenSandbox command stream exceeded its deadline").partial(true))).await;
                        partial=true;
                        None
                    },
                    value = tokio::time::timeout(idle_timeout, bytes.next()) => match value {
                        Ok(value) => value,
                        Err(_) => { let _=sender.send(Err(RuntimeError::new("SANDBOX_STREAM_IDLE_TIMEOUT","OpenSandbox command stream became idle").partial(true))).await; partial=true; None }
                    }
                };
                let Some(chunk) = next else { break };
                let events = match chunk
                    .map_err(transport_error)
                    .and_then(|chunk| decoder.push(&chunk))
                {
                    Ok(events) => events,
                    Err(error) => {
                        let _ = sender.send(Err(error.partial(true))).await;
                        partial = true;
                        break;
                    }
                };
                for event in events {
                    let parsed: CommandStreamEvent = match serde_json::from_str(&event.data) {
                        Ok(value) => value,
                        Err(_) => {
                            sequence += 1;
                            if sender
                                .send(Ok(SandboxEvent::Stdout {
                                    sequence,
                                    data: event.data.into_bytes(),
                                }))
                                .await
                                .is_err()
                            {
                                partial = true;
                                break;
                            }
                            continue;
                        }
                    };
                    let output = match parsed.kind.as_str() {
                        "init" => {
                            command_id = Some(parsed.text.clone());
                            active_commands
                                .lock()
                                .await
                                .insert(sandbox_id.clone(), parsed.text);
                            None
                        }
                        "stdout" => {
                            sequence += 1;
                            Some(SandboxEvent::Stdout {
                                sequence,
                                data: parsed.text.into_bytes(),
                            })
                        }
                        "stderr" => {
                            sequence += 1;
                            Some(SandboxEvent::Stderr {
                                sequence,
                                data: parsed.text.into_bytes(),
                            })
                        }
                        "error" => {
                            exit_code = parsed
                                .exit_code
                                .or_else(|| {
                                    parsed
                                        .error
                                        .as_ref()
                                        .and_then(|error| error.evalue.parse().ok())
                                })
                                .or_else(|| parsed.evalue.parse().ok())
                                .unwrap_or(1);
                            None
                        }
                        "execution_complete" => {
                            completed = true;
                            Some(SandboxEvent::Completed {
                                exit_code,
                                partial: false,
                            })
                        }
                        "ping" | "status" | "execution_count" | "result" => None,
                        _ => {
                            let _ = sender
                                .send(Err(protocol_error(
                                    "OpenSandbox returned an unsupported command event",
                                )))
                                .await;
                            partial = true;
                            break;
                        }
                    };
                    if let Some(output) = output {
                        if sender.send(Ok(output)).await.is_err() {
                            partial = true;
                            break;
                        }
                    }
                }
                if partial || completed {
                    break;
                }
            }
            if !partial && !completed {
                let error = decoder.finish().err().unwrap_or_else(|| {
                    RuntimeError::new(
                        "SANDBOX_STREAM_INCOMPLETE",
                        "OpenSandbox command stream ended before completion",
                    )
                    .partial(true)
                });
                let _ = sender.send(Err(error)).await;
                partial = true;
            }
            if partial {
                if let Some(id) = command_id {
                    if let Ok(mut interrupt) = url.join("command") {
                        interrupt.query_pairs_mut().append_pair("id", &id);
                        let _ = client.delete(interrupt).headers(headers).send().await;
                    }
                }
                let _ = sender
                    .send(Ok(SandboxEvent::Completed {
                        exit_code,
                        partial: true,
                    }))
                    .await;
            }
            active_commands.lock().await.remove(&sandbox_id);
        });
        Ok(Box::pin(ReceiverStream::new(receiver)))
    }

    async fn interrupt(
        &self,
        _context: &RuntimeContext,
        lease: &SandboxLease,
    ) -> RuntimeResult<()> {
        let (mut url, headers) = self.endpoint(&lease.sandbox_id).await?;
        let headers = self.execd_headers(&url, headers)?;
        let command_id = self
            .active_commands
            .lock()
            .await
            .get(&lease.sandbox_id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(
                    "SANDBOX_COMMAND_NOT_RUNNING",
                    "Sandbox has no active command",
                )
            })?;
        url.set_path(&format!("{}/command", url.path().trim_end_matches('/')));
        url.query_pairs_mut().append_pair("id", &command_id);
        let response = self
            .client
            .delete(url)
            .headers(headers)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(transport_error)?;
        require_status(response, &[StatusCode::OK, StatusCode::NOT_FOUND])
            .await
            .map(|_| ())
    }

    async fn upload(
        &self,
        _context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
        content: Vec<u8>,
    ) -> RuntimeResult<()> {
        validate_working_path(path)?;
        if content.len() > self.max_file_bytes {
            return Err(RuntimeError::new(
                "SANDBOX_FILE_LIMIT",
                "Sandbox upload exceeds the configured byte limit",
            ));
        }
        let (url, headers) = self.endpoint(&lease.sandbox_id).await?;
        let headers = self.execd_headers(&url, headers)?;
        let endpoint = url.join("files/upload").map_err(protocol_url_error)?;
        let file_name = path
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("file")
            .to_owned();
        let form = multipart::Form::new()
            .part(
                "metadata",
                multipart::Part::text(json!({"path":path,"mode":600}).to_string())
                    .file_name("metadata")
                    .mime_str("application/json")
                    .map_err(transport_error)?,
            )
            .part(
                "file",
                multipart::Part::bytes(content)
                    .file_name(file_name)
                    .mime_str("application/octet-stream")
                    .map_err(transport_error)?,
            );
        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .multipart(form)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(|error| transport_error(error).outcome_unknown(true))?;
        require_status(response, &[StatusCode::OK])
            .await
            .map(|_| ())
    }

    async fn download(
        &self,
        _context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
    ) -> RuntimeResult<Vec<u8>> {
        validate_working_path(path)?;
        let (mut url, headers) = self.endpoint(&lease.sandbox_id).await?;
        let headers = self.execd_headers(&url, headers)?;
        url.set_path(&format!(
            "{}/files/download",
            url.path().trim_end_matches('/')
        ));
        url.query_pairs_mut().append_pair("path", path);
        let response = self
            .client
            .get(url)
            .headers(headers)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(transport_error)?;
        let response =
            require_status(response, &[StatusCode::OK, StatusCode::PARTIAL_CONTENT]).await?;
        if response
            .content_length()
            .is_some_and(|length| length > self.max_file_bytes as u64)
        {
            return Err(RuntimeError::new(
                "SANDBOX_FILE_LIMIT",
                "Sandbox download exceeds the configured byte limit",
            ));
        }
        let bytes = response.bytes().await.map_err(transport_error)?;
        if bytes.len() > self.max_file_bytes {
            return Err(RuntimeError::new(
                "SANDBOX_FILE_LIMIT",
                "Sandbox download exceeds the configured byte limit",
            ));
        }
        Ok(bytes.to_vec())
    }

    async fn metrics(
        &self,
        _context: &RuntimeContext,
        lease: &SandboxLease,
    ) -> RuntimeResult<SandboxMetrics> {
        let (url, headers) = self.endpoint(&lease.sandbox_id).await?;
        let headers = self.execd_headers(&url, headers)?;
        let endpoint = url.join("metrics").map_err(protocol_url_error)?;
        let response = self
            .client
            .get(endpoint)
            .headers(headers)
            .timeout(self.request_timeout)
            .send()
            .await
            .map_err(transport_error)?;
        let response = require_status(response, &[StatusCode::OK]).await?;
        let value: ExecdMetrics = response.json().await.map_err(protocol_json_error)?;
        let raw = serde_json::to_value(&value).map_err(protocol_json_error)?;
        Ok(SandboxMetrics {
            cpu_nanos: ((value.cpu_used_pct / 100.0) * value.cpu_count * 1_000_000_000.0).max(0.0)
                as u64,
            memory_bytes: (value.mem_used_mib * 1_048_576.0).max(0.0) as u64,
            pids: 0,
            disk_bytes: 0,
            raw,
        })
    }

    async fn terminate(
        &self,
        _context: &RuntimeContext,
        lease: &SandboxLease,
    ) -> RuntimeResult<()> {
        self.kill(&lease.sandbox_id).await
    }
}

async fn require_status(
    response: reqwest::Response,
    expected: &[StatusCode],
) -> RuntimeResult<reqwest::Response> {
    if expected.contains(&response.status()) {
        return Ok(response);
    }
    let status = response.status();
    let retryable = status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format!("OpenSandbox returned HTTP {status}"));
    Err(RuntimeError::new(
        if status == StatusCode::NOT_FOUND {
            "SANDBOX_NOT_FOUND"
        } else {
            "SANDBOX_PROVIDER_ERROR"
        },
        message,
    )
    .retryable(retryable))
}

fn validate_identifier(value: &str) -> RuntimeResult<()> {
    if value.is_empty()
        || value.len() > 255
        || !value
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    {
        return Err(RuntimeError::new(
            "SANDBOX_ID_INVALID",
            "Sandbox identifier is invalid",
        ));
    }
    Ok(())
}

fn validate_working_path(path: &str) -> RuntimeResult<()> {
    const WORKSPACE_ROOTS: [&str; 2] = ["/workspace", "/home/playwright"];
    let within_workspace = WORKSPACE_ROOTS.iter().any(|root| {
        path == *root
            || path
                .strip_prefix(root)
                .is_some_and(|suffix| suffix.starts_with('/'))
    });
    if !within_workspace
        || path.contains('\\')
        || path.split('/').any(|segment| segment == "..")
        || path.contains('\0')
    {
        return Err(RuntimeError::new(
            "SANDBOX_PATH_FORBIDDEN",
            "Sandbox path must stay within an approved workspace",
        ));
    }
    Ok(())
}

fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|value| format!("'{}'", value.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}
fn is_digest_image(value: &str) -> bool {
    value.rsplit_once("@sha256:").is_some_and(|(_, digest)| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
    })
}
fn metadata_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..63].to_owned()
}
fn required_str<'a>(value: &'a Value, key: &str) -> RuntimeResult<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        RuntimeError::new(
            "SANDBOX_PROFILE_INVALID",
            format!("Sandbox profile is missing {key}"),
        )
    })
}
fn required_u64(value: &Value, key: &str) -> RuntimeResult<u64> {
    value.get(key).and_then(Value::as_u64).ok_or_else(|| {
        RuntimeError::new(
            "SANDBOX_PROFILE_INVALID",
            format!("Sandbox profile is missing {key}"),
        )
    })
}
fn protocol_error(message: &str) -> RuntimeError {
    RuntimeError::new("SANDBOX_PROTOCOL_UNSUPPORTED", message)
}
fn protocol_url_error(_error: url::ParseError) -> RuntimeError {
    protocol_error("OpenSandbox returned an invalid URL")
}
fn protocol_json_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(
        "SANDBOX_PROTOCOL_UNSUPPORTED",
        format!("OpenSandbox response does not match the fixed protocol: {error}"),
    )
}
fn transport_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new("SANDBOX_TRANSPORT_ERROR", error.to_string()).retryable(true)
}
fn cancelled_error() -> RuntimeError {
    RuntimeError::new("EXECUTION_CANCELLED", "Sandbox operation was cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        convert::Infallible,
        sync::{
            Arc as StdArc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use agentx_domain::{
        AttemptId, ExecutionId, NodeExecutionId, TenantId, TraceId, WorkflowId,
        WorkflowServiceIdentityId, WorkflowVersionId,
    };
    use axum::{
        Json, Router,
        body::{Body, Bytes},
        extract::{OriginalUri, Path, State},
        http::HeaderMap as AxumHeaderMap,
        response::Response,
        routing::{delete, get, post},
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tokio::{net::TcpListener, task::JoinHandle};
    use tokio_util::sync::CancellationToken;

    #[derive(Clone, Copy)]
    enum StreamMode {
        Complete,
        Burst,
        Idle,
        Heartbeat,
        Incomplete,
        Unsupported,
        Redirect,
    }

    #[derive(Clone)]
    struct FakeServerState {
        origin: String,
        endpoint_origin: StdArc<StdMutex<Option<String>>>,
        stream_mode: StreamMode,
        redirect_to: Option<String>,
        command_headers: StdArc<StdMutex<Vec<AxumHeaderMap>>>,
        interrupt_count: StdArc<AtomicUsize>,
        kill_count: StdArc<AtomicUsize>,
        readiness_failures: StdArc<AtomicUsize>,
        list_uri: StdArc<StdMutex<Option<String>>>,
    }

    async fn spawn_fake_server(
        stream_mode: StreamMode,
        redirect_to: Option<String>,
    ) -> (Url, FakeServerState, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = FakeServerState {
            origin: format!("http://{address}"),
            endpoint_origin: StdArc::new(StdMutex::new(None)),
            stream_mode,
            redirect_to,
            command_headers: StdArc::new(StdMutex::new(Vec::new())),
            interrupt_count: StdArc::new(AtomicUsize::new(0)),
            kill_count: StdArc::new(AtomicUsize::new(0)),
            readiness_failures: StdArc::new(AtomicUsize::new(0)),
            list_uri: StdArc::new(StdMutex::new(None)),
        };
        let router = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/v1/sandboxes", get(fake_list))
            .route("/v1/sandboxes/{sandbox_id}", delete(fake_kill))
            .route(
                "/v1/sandboxes/{sandbox_id}/endpoints/44772",
                get(fake_endpoint),
            )
            .route(
                "/v1/sandboxes/{sandbox_id}/proxy/44772/command",
                post(fake_command).delete(fake_interrupt),
            )
            .route(
                "/v1/sandboxes/{sandbox_id}/proxy/44772/ping",
                get(fake_ping),
            )
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (Url::parse(&state.origin).unwrap(), state, task)
    }

    async fn fake_list(
        State(state): State<FakeServerState>,
        OriginalUri(uri): OriginalUri,
    ) -> Json<Value> {
        *state.list_uri.lock().unwrap() = Some(uri.to_string());
        Json(json!({
            "items": [{
                "id": "sbx-list",
                "status": {"state": "Running"},
                "metadata": {},
                "expiresAt": null
            }]
        }))
    }

    async fn fake_endpoint(
        State(state): State<FakeServerState>,
        Path(sandbox_id): Path<String>,
    ) -> Json<Value> {
        let endpoint_origin = state
            .endpoint_origin
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| state.origin.clone());
        Json(json!({
            "endpoint": format!(
                "{}/v1/sandboxes/{sandbox_id}/proxy/44772/",
                endpoint_origin
            ),
            "headers": {"x-execd-access-token": "execd-test-token"}
        }))
    }

    async fn fake_command(
        State(state): State<FakeServerState>,
        headers: AxumHeaderMap,
    ) -> Response {
        state.command_headers.lock().unwrap().push(headers);
        if matches!(state.stream_mode, StreamMode::Redirect) {
            return Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header("location", state.redirect_to.as_deref().unwrap())
                .body(Body::empty())
                .unwrap();
        }
        let mode = state.stream_mode;
        let stream = async_stream::stream! {
            match mode {
                StreamMode::Complete => {
                    yield Ok::<Bytes, Infallible>(Bytes::from_static(
                        b"{\"type\":\"execution_complete\"}\n\n",
                    ));
                }
                StreamMode::Burst => {
                    let mut output = String::new();
                    for index in 0..40 {
                        output.push_str(&format!(
                            "{{\"type\":\"stdout\",\"text\":\"{index}\"}}\n\n"
                        ));
                    }
                    output.push_str("{\"type\":\"execution_complete\"}\n\n");
                    yield Ok(Bytes::from(output));
                }
                StreamMode::Idle => {
                    yield Ok(Bytes::from_static(b"{\"type\":\"init\",\"text\":\"cmd-idle\"}\n\n"));
                    std::future::pending::<()>().await;
                }
                StreamMode::Heartbeat => {
                    yield Ok(Bytes::from_static(b"{\"type\":\"init\",\"text\":\"cmd-heartbeat\"}\n\n"));
                    loop {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        yield Ok(Bytes::from_static(b"{\"type\":\"ping\"}\n\n"));
                    }
                }
                StreamMode::Incomplete => {
                    yield Ok(Bytes::from_static(b"{\"type\":\"stdout\",\"text\":\"partial\"}\n\n"));
                }
                StreamMode::Unsupported => {
                    yield Ok(Bytes::from_static(b"{\"type\":\"init\",\"text\":\"cmd-unsupported\"}\n\n{\"type\":\"future_event\"}\n\n"));
                }
                StreamMode::Redirect => unreachable!(),
            }
        };
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap()
    }

    async fn fake_interrupt(State(state): State<FakeServerState>) -> StatusCode {
        state.interrupt_count.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK
    }

    async fn fake_ping(State(state): State<FakeServerState>) -> StatusCode {
        if state.readiness_failures.load(Ordering::SeqCst) > 0 {
            state.readiness_failures.fetch_sub(1, Ordering::SeqCst);
            StatusCode::BAD_GATEWAY
        } else {
            StatusCode::OK
        }
    }

    async fn fake_kill(State(state): State<FakeServerState>) -> StatusCode {
        let attempt = state.kill_count.fetch_add(1, Ordering::SeqCst);
        if attempt < 2 {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NO_CONTENT
        }
    }

    fn test_context(deadline_after: Duration) -> RuntimeContext {
        RuntimeContext {
            tenant_id: TenantId::new(),
            workflow_service_identity_id: WorkflowServiceIdentityId::new(),
            workflow_id: WorkflowId::new(),
            workflow_version_id: Some(WorkflowVersionId::new()),
            execution_id: ExecutionId::new(),
            node_execution_id: NodeExecutionId::new(),
            attempt_id: AttemptId::new(),
            lease_token: Uuid::now_v7(),
            trace_id: TraceId::new(),
            span_id: Uuid::now_v7(),
            deadline: OffsetDateTime::now_utc() + time::Duration::try_from(deadline_after).unwrap(),
            cancellation: CancellationToken::new(),
            idempotency_key: "opensandbox-contract-test".into(),
            resources: Vec::new(),
        }
    }

    fn test_command() -> SandboxCommand {
        SandboxCommand {
            lease: SandboxLease {
                lease_id: Uuid::now_v7(),
                sandbox_id: "sbx-test".into(),
                lease_token: "lease-token".into(),
                expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(1),
            },
            argv: vec!["printf".into(), "ok".into()],
            environment: json!({}),
            working_directory: "/workspace".into(),
        }
    }

    fn adapter(endpoint: Url) -> OpenSandboxAdapter {
        OpenSandboxAdapter::new(endpoint, SecretString::from("lifecycle-secret")).unwrap()
    }

    #[test]
    fn vendored_specs_match_fixed_hashes() {
        let lifecycle =
            include_bytes!("../../../../vendor/opensandbox/specs/sandbox-lifecycle.yml");
        let execd = include_bytes!("../../../../vendor/opensandbox/specs/execd-api.yaml");
        assert_eq!(
            format!("{:x}", Sha256::digest(lifecycle)),
            LIFECYCLE_SPEC_SHA256
        );
        assert_eq!(format!("{:x}", Sha256::digest(execd)), EXECD_SPEC_SHA256);
    }

    #[test]
    fn argv_is_posix_escaped_and_paths_are_confined() {
        assert_eq!(
            shell_join(&["printf".into(), "a'b".into()]),
            "'printf' 'a'\\''b'"
        );
        assert!(validate_working_path("/workspace/output.txt").is_ok());
        assert!(validate_working_path("/home/playwright/browser.py").is_ok());
        assert!(validate_working_path("/workspace/../etc/passwd").is_err());
        assert!(validate_working_path("/home/playwright/../../etc/passwd").is_err());
        assert!(validate_working_path("/workspace-escape/output.txt").is_err());
        assert!(validate_working_path("/home/playwright-escape/output.txt").is_err());
        assert!(validate_working_path("\\workspace\\out").is_err());
        assert!(validate_working_path("C:\\workspace\\out").is_err());
    }

    #[test]
    fn secure_access_is_enabled_unless_the_deployment_opts_out() {
        let adapter = OpenSandboxAdapter::new(
            Url::parse("https://sandbox.example.test").unwrap(),
            SecretString::from("test-key"),
        )
        .unwrap();
        assert!(adapter.secure_access);
        assert!(!adapter.with_secure_access(false).secure_access);
    }

    #[test]
    fn reconciliation_hash_fits_opensandbox_metadata_labels() {
        let value = metadata_hash("execution:node:attempt:create");
        assert_eq!(value.len(), 63);
        assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn provider_timeout_keeps_cleanup_grace_without_exceeding_the_protocol_limit() {
        assert_eq!(
            60_u64
                .saturating_add(PROVIDER_TTL_GRACE_SECONDS)
                .min(86_400),
            90
        );
        assert_eq!(
            86_400_u64
                .saturating_add(PROVIDER_TTL_GRACE_SECONDS)
                .min(86_400),
            86_400
        );
    }

    #[tokio::test]
    async fn list_encodes_all_reconciliation_labels_as_one_metadata_filter() {
        let (endpoint, state, task) = spawn_fake_server(StreamMode::Complete, None).await;
        let result = adapter(endpoint)
            .list_by_labels(&BTreeMap::from([
                ("agentx-attempt".into(), "attempt value".into()),
                ("agentx-request".into(), "request&value".into()),
            ]))
            .await
            .unwrap();
        assert_eq!(result, vec!["sbx-list"]);
        let uri = state.list_uri.lock().unwrap().clone().unwrap();
        let query = Url::parse(&format!("http://localhost{uri}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            query.get("metadata").map(String::as_str),
            Some("agentx-attempt=attempt value&agentx-request=request&value")
        );
        assert_eq!(query.get("pageSize").map(String::as_str), Some("100"));
        task.abort();
    }

    #[tokio::test]
    async fn execd_readiness_retries_transient_proxy_failures() {
        let (endpoint, state, task) = spawn_fake_server(StreamMode::Complete, None).await;
        state.readiness_failures.store(2, Ordering::SeqCst);
        adapter(endpoint)
            .wait_execd_ready(&test_context(Duration::from_secs(2)), "sbx-test")
            .await
            .unwrap();
        assert_eq!(state.readiness_failures.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn bounded_stream_preserves_order_for_a_slow_consumer() {
        let (endpoint, _state, task) = spawn_fake_server(StreamMode::Burst, None).await;
        let context = test_context(Duration::from_secs(2));
        let mut stream = adapter(endpoint)
            .execute(&context, test_command())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        for expected in 0_u64..40 {
            match stream.next().await.unwrap().unwrap() {
                SandboxEvent::Stdout { sequence, data } => {
                    assert_eq!(sequence, expected + 1);
                    assert_eq!(data, expected.to_string().into_bytes());
                }
                _ => panic!("expected ordered stdout event"),
            }
        }
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            SandboxEvent::Completed {
                exit_code: 0,
                partial: false
            }
        ));
        task.abort();
    }

    #[tokio::test]
    async fn stream_idle_total_and_incomplete_failures_are_partial() {
        for (mode, expected_code, deadline, idle) in [
            (
                StreamMode::Idle,
                "SANDBOX_STREAM_IDLE_TIMEOUT",
                Duration::from_secs(2),
                Duration::from_millis(20),
            ),
            (
                StreamMode::Heartbeat,
                "SANDBOX_STREAM_TOTAL_TIMEOUT",
                Duration::from_millis(35),
                Duration::from_secs(1),
            ),
        ] {
            let (endpoint, _state, task) = spawn_fake_server(mode, None).await;
            let mut current = adapter(endpoint);
            current.idle_timeout = idle;
            let context = test_context(deadline);
            let mut stream = current.execute(&context, test_command()).await.unwrap();
            let error = stream.next().await.unwrap().unwrap_err();
            assert_eq!(error.code, expected_code);
            assert!(error.partial);
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                SandboxEvent::Completed { partial: true, .. }
            ));
            task.abort();
        }

        let (endpoint, _state, task) = spawn_fake_server(StreamMode::Incomplete, None).await;
        let context = test_context(Duration::from_secs(2));
        let mut stream = adapter(endpoint)
            .execute(&context, test_command())
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            SandboxEvent::Stdout { .. }
        ));
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.code, "SANDBOX_STREAM_INCOMPLETE");
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            SandboxEvent::Completed { partial: true, .. }
        ));
        task.abort();
    }

    #[tokio::test]
    async fn cancellation_and_unknown_events_interrupt_the_active_command() {
        let (endpoint, state, task) = spawn_fake_server(StreamMode::Idle, None).await;
        let current = adapter(endpoint);
        let context = test_context(Duration::from_secs(2));
        let cancellation = context.cancellation.clone();
        let mut stream = current.execute(&context, test_command()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if current
                    .active_commands
                    .lock()
                    .await
                    .contains_key("sbx-test")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cancellation.cancel();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            SandboxEvent::Completed { partial: true, .. }
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.interrupt_count.load(Ordering::SeqCst) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();

        let (endpoint, state, task) = spawn_fake_server(StreamMode::Unsupported, None).await;
        let context = test_context(Duration::from_secs(2));
        let mut stream = adapter(endpoint)
            .execute(&context, test_command())
            .await
            .unwrap();
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.code, "SANDBOX_PROTOCOL_UNSUPPORTED");
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            SandboxEvent::Completed { partial: true, .. }
        ));
        assert_eq!(state.interrupt_count.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn redirects_are_not_followed_and_lifecycle_key_does_not_cross_origin() {
        let attacker_hits = StdArc::new(AtomicUsize::new(0));
        let attacker_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let attacker_address = attacker_listener.local_addr().unwrap();
        let attacker_state = attacker_hits.clone();
        let attacker = tokio::spawn(async move {
            let router = Router::new().fallback(move || {
                let hits = attacker_state.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            });
            axum::serve(attacker_listener, router).await.unwrap();
        });
        let redirect = format!("http://{attacker_address}/steal");
        let (execd_endpoint, execd_state, execd_server) =
            spawn_fake_server(StreamMode::Redirect, Some(redirect)).await;
        let (endpoint, lifecycle_state, lifecycle_server) =
            spawn_fake_server(StreamMode::Complete, None).await;
        *lifecycle_state.endpoint_origin.lock().unwrap() =
            Some(execd_endpoint.as_str().trim_end_matches('/').to_owned());
        let current = OpenSandboxAdapter::with_policy(
            endpoint,
            SecretString::from("lifecycle-secret"),
            vec!["127.0.0.1".into()],
            vec![],
        )
        .unwrap()
        .with_server_proxy(false);
        let context = test_context(Duration::from_secs(2));
        let error = match current.execute(&context, test_command()).await {
            Ok(_) => panic!("redirected command must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code, "SANDBOX_PROVIDER_ERROR");
        assert_eq!(attacker_hits.load(Ordering::SeqCst), 0);
        let headers = execd_state.command_headers.lock().unwrap();
        assert!(headers[0].contains_key("x-execd-access-token"));
        assert!(!headers[0].contains_key(LIFECYCLE_API_KEY_HEADER));
        lifecycle_server.abort();
        execd_server.abort();
        attacker.abort();
    }

    #[tokio::test]
    async fn terminate_retries_only_retryable_provider_failures() {
        let (endpoint, state, task) = spawn_fake_server(StreamMode::Complete, None).await;
        adapter(endpoint).kill("sbx-test").await.unwrap();
        assert_eq!(state.kill_count.load(Ordering::SeqCst), 3);
        task.abort();
    }
}
