use std::{pin::Pin, sync::Arc, time::Duration};

use agentx_application::{SandboxCommand, SandboxCreateRequest, SandboxEvent, SandboxRuntime};
use agentx_infrastructure::OpenSandboxAdapter;
use agentx_runtime_rpc::sandbox_v1::{
    CommandCompleted, CommandEvent, CreateSandboxRequest, DownloadRequest, ExecuteCommandRequest,
    FileChunk, OperationResult, SandboxLeaseRequest, SandboxMetrics, UploadChunk,
    sandbox_manager_server::SandboxManager,
};
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::store::{
    LeaseStore, profile_timeout, profile_version, reconciliation_labels, runtime_status,
};

#[derive(Clone)]
pub struct SandboxManagerService {
    store: LeaseStore,
    adapter: Arc<OpenSandboxAdapter>,
}

impl SandboxManagerService {
    pub fn new(store: LeaseStore, adapter: OpenSandboxAdapter) -> Self {
        Self {
            store,
            adapter: Arc::new(adapter),
        }
    }
    pub fn adapter(&self) -> Arc<OpenSandboxAdapter> {
        self.adapter.clone()
    }
}

type RpcStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl SandboxManager for SandboxManagerService {
    type ExecuteStream = RpcStream<CommandEvent>;
    type DownloadStream = RpcStream<FileChunk>;

    async fn create(
        &self,
        request: Request<CreateSandboxRequest>,
    ) -> Result<Response<agentx_runtime_rpc::sandbox_v1::SandboxLease>, Status> {
        let request = request.into_inner();
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let mut context = self
            .store
            .validate_scope(scope)
            .await
            .map_err(runtime_status)?;
        if request.idempotency_key.is_empty() {
            return Err(Status::invalid_argument("idempotency_key is required"));
        }
        if let Some(existing) = self
            .store
            .existing(&context, &request.idempotency_key)
            .await
            .map_err(runtime_status)?
        {
            self.store
                .bind_credentials(&context, existing.id, &request.credentials)
                .await
                .map_err(runtime_status)?;
            if let Some(active) = existing.active_rpc(&self.store).map_err(runtime_status)? {
                return Ok(Response::new(active));
            }
            if matches!(existing.status.as_str(), "creating" | "orphaned") {
                let matches = self
                    .adapter
                    .list_by_labels(&reconciliation_labels(
                        existing.attempt_id,
                        &existing.idempotency_key,
                    ))
                    .await
                    .map_err(runtime_status)?;
                if matches.len() == 1 {
                    let endpoint = self
                        .adapter
                        .endpoint_auth_document(&matches[0])
                        .await
                        .map_err(runtime_status)?;
                    let lease = self
                        .store
                        .mark_ready(existing.id, &matches[0], existing.expires_at, &endpoint)
                        .await
                        .map_err(runtime_status)?;
                    return Ok(Response::new(lease));
                }
                return Err(Status::unavailable(
                    "SANDBOX_CREATE_OUTCOME_UNKNOWN: reconciliation did not find exactly one Sandbox",
                ));
            }
            return Err(Status::failed_precondition(format!(
                "SANDBOX_CREATE_PREVIOUSLY_{}",
                existing.status.to_ascii_uppercase()
            )));
        }
        let profile =
            LeaseStore::parse_profile(&request.profile_snapshot_json).map_err(runtime_status)?;
        context.resources = vec![profile.clone()];
        let profile_version = profile_version(&profile).map_err(runtime_status)?;
        let timeout = profile_timeout(&profile)
            .map_err(runtime_status)?
            .clamp(60, 86_400);
        let expires = OffsetDateTime::now_utc() + time::Duration::seconds(timeout as i64);
        let lease_id = self
            .store
            .insert_creating(&context, profile_version, &request.idempotency_key, expires)
            .await
            .map_err(runtime_status)?;
        if let Err(error) = self
            .store
            .bind_credentials(&context, lease_id, &request.credentials)
            .await
        {
            self.store
                .mark_failed(lease_id, &error.to_string(), false)
                .await
                .map_err(runtime_status)?;
            return Err(runtime_status(error));
        }
        let labels: Value = parse_json(&request.labels_json, json!({})).map_err(runtime_status)?;
        let policy: Value =
            parse_json(&request.network_policy_json, Value::Null).map_err(runtime_status)?;
        let created = self
            .adapter
            .create(
                &context,
                SandboxCreateRequest {
                    profile,
                    labels,
                    network_policy: policy,
                    credentials: Vec::new(),
                },
            )
            .await;
        let vendor_lease = match created {
            Ok(value) => value,
            Err(error) if error.outcome_unknown => {
                let matches = self
                    .adapter
                    .list_by_labels(&reconciliation_labels(
                        context.attempt_id.as_uuid(),
                        &request.idempotency_key,
                    ))
                    .await
                    .map_err(runtime_status)?;
                if matches.len() != 1 {
                    self.store
                        .mark_failed(lease_id, &error.to_string(), true)
                        .await
                        .map_err(runtime_status)?;
                    return Err(Status::unavailable(
                        "SANDBOX_CREATE_OUTCOME_UNKNOWN: OpenSandbox create outcome is unknown and reconciliation was inconclusive",
                    ));
                }
                agentx_application::SandboxLease {
                    lease_id,
                    sandbox_id: matches[0].clone(),
                    lease_token: String::new(),
                    expires_at: expires,
                }
            }
            Err(error) => {
                self.store
                    .mark_failed(lease_id, &error.to_string(), false)
                    .await
                    .map_err(runtime_status)?;
                return Err(runtime_status(error));
            }
        };
        let endpoint = self
            .adapter
            .endpoint_auth_document(&vendor_lease.sandbox_id)
            .await
            .map_err(runtime_status)?;
        let lease = self
            .store
            .mark_ready(
                lease_id,
                &vendor_lease.sandbox_id,
                vendor_lease.expires_at,
                &endpoint,
            )
            .await
            .map_err(runtime_status)?;
        Ok(Response::new(lease))
    }

    async fn execute(
        &self,
        request: Request<ExecuteCommandRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let request = request.into_inner();
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let rpc_lease = request
            .lease
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox lease is required"))?;
        let valid = self
            .store
            .validate_lease(scope, rpc_lease)
            .await
            .map_err(runtime_status)?;
        let working_directory = supported_working_directory(&request.working_directory)
            .ok_or_else(|| {
                Status::invalid_argument("Sandbox command working directory is not approved")
            })?
            .to_owned();
        let mut environment: Value =
            parse_json(&request.environment_json, json!({})).map_err(runtime_status)?;
        let lease_id = valid.lease.lease_id;
        let credentials = self
            .store
            .resolve_credentials(&valid.context, lease_id)
            .await
            .map_err(runtime_status)?;
        let patterns = secret_patterns(&credentials);
        let mut secret_paths = Vec::with_capacity(credentials.len());
        let environment = environment.as_object_mut().ok_or_else(|| {
            Status::invalid_argument("Sandbox command environment must be an object")
        })?;
        for credential in &credentials {
            if environment.contains_key(&credential.environment_name) {
                self.store
                    .revoke_credentials(lease_id)
                    .await
                    .map_err(runtime_status)?;
                return Err(Status::invalid_argument(
                    "Sandbox command environment conflicts with a Credential file variable",
                ));
            }
            let path = format!(
                "{working_directory}/.agentx-secrets/{}.json",
                credential.handle_id.simple()
            );
            if let Err(error) = self
                .adapter
                .upload(
                    &valid.context,
                    &valid.lease,
                    &path,
                    credential.secret.expose().to_vec(),
                )
                .await
            {
                let _ = self.store.revoke_credentials(lease_id).await;
                return Err(runtime_status(error));
            }
            environment.insert(
                credential.environment_name.clone(),
                Value::String(path.clone()),
            );
            secret_paths.push(path);
        }
        self.store
            .set_status(lease_id, "running")
            .await
            .map_err(runtime_status)?;
        let stream = self
            .adapter
            .execute(
                &valid.context,
                SandboxCommand {
                    lease: valid.lease.clone(),
                    argv: request.argv,
                    environment: Value::Object(environment.clone()),
                    working_directory: working_directory.clone(),
                },
            )
            .await;
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                let _ = cleanup_secret_files(
                    &self.adapter,
                    &valid.context,
                    &valid.lease,
                    &working_directory,
                    &secret_paths,
                )
                .await;
                let _ = self.store.revoke_credentials(lease_id).await;
                let _ = self.store.set_status(lease_id, "ready").await;
                return Err(runtime_status(error));
            }
        };
        let cancellation = valid.context.cancellation.clone();
        let store = self.store.clone();
        let adapter = self.adapter.clone();
        let context = valid.context.clone();
        let lease = valid.lease.clone();
        let ttl_wait = Duration::from_millis(
            (lease.expires_at - OffsetDateTime::now_utc())
                .whole_milliseconds()
                .max(0) as u64,
        );
        let (sender, receiver) = mpsc::channel(16);
        tokio::spawn(async move {
            let mut stream = stream;
            let ttl = tokio::time::sleep(ttl_wait);
            tokio::pin!(ttl);
            let mut stdout = StreamRedactor::new(patterns.clone());
            let mut stderr = StreamRedactor::new(patterns);
            let mut stdout_sequence = 0_u64;
            let mut stderr_sequence = 0_u64;
            let mut completed = None;
            let mut terminal_error_sent = false;
            loop {
                let event = tokio::select! {
                    _ = sender.closed() => { cancellation.cancel(); break },
                    _ = &mut ttl => {
                        let _ = sender.send(Err(Status::deadline_exceeded("SANDBOX_TTL_EXPIRED: Sandbox lease reached its configured TTL"))).await;
                        terminal_error_sent = true;
                        break;
                    }
                    event = stream.next() => event,
                };
                let Some(event) = event else { break };
                match event {
                    Ok(SandboxEvent::Stdout { sequence, data }) => {
                        stdout_sequence = stdout_sequence.max(sequence);
                        let data = stdout.push(&data, false);
                        if !data.is_empty()&&sender.send(Ok(CommandEvent{sequence,event:Some(agentx_runtime_rpc::sandbox_v1::command_event::Event::Stdout(data))})).await.is_err(){cancellation.cancel();break;}
                    }
                    Ok(SandboxEvent::Stderr { sequence, data }) => {
                        stderr_sequence = stderr_sequence.max(sequence);
                        let data = stderr.push(&data, false);
                        if !data.is_empty()&&sender.send(Ok(CommandEvent{sequence,event:Some(agentx_runtime_rpc::sandbox_v1::command_event::Event::Stderr(data))})).await.is_err(){cancellation.cancel();break;}
                    }
                    Ok(SandboxEvent::Completed { exit_code, partial }) => {
                        completed = Some((exit_code, partial));
                        break;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(runtime_status(error))).await;
                        break;
                    }
                }
            }
            let stdout_tail = stdout.push(&[], true);
            if !stdout_tail.is_empty() {
                let _ = sender
                    .send(Ok(CommandEvent {
                        sequence: stdout_sequence.saturating_add(1),
                        event: Some(
                            agentx_runtime_rpc::sandbox_v1::command_event::Event::Stdout(
                                stdout_tail,
                            ),
                        ),
                    }))
                    .await;
            }
            let stderr_tail = stderr.push(&[], true);
            if !stderr_tail.is_empty() {
                let _ = sender
                    .send(Ok(CommandEvent {
                        sequence: stderr_sequence.saturating_add(1),
                        event: Some(
                            agentx_runtime_rpc::sandbox_v1::command_event::Event::Stderr(
                                stderr_tail,
                            ),
                        ),
                    }))
                    .await;
            }
            let cleanup = cleanup_secret_files(
                &adapter,
                &context,
                &lease,
                &working_directory,
                &secret_paths,
            )
            .await;
            let _ = store.revoke_credentials(lease_id).await;
            let _ = store.set_status(lease_id, "ready").await;
            if let Err(error) = cleanup
                && !terminal_error_sent
            {
                let _ = sender.send(Err(runtime_status(error))).await;
                return;
            }
            if let Some((exit_code, partial)) = completed {
                let _ = sender
                    .send(Ok(CommandEvent {
                        sequence: u64::MAX,
                        event: Some(
                            agentx_runtime_rpc::sandbox_v1::command_event::Event::Completed(
                                CommandCompleted { exit_code, partial },
                            ),
                        ),
                    }))
                    .await;
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    async fn interrupt(
        &self,
        request: Request<SandboxLeaseRequest>,
    ) -> Result<Response<OperationResult>, Status> {
        let request = request.into_inner();
        let valid = self.validate_cleanup_request(&request).await?;
        if !matches!(valid.status.as_str(), "ready" | "running" | "interrupting") {
            return Ok(Response::new(OperationResult {
                accepted: true,
                replayed: true,
            }));
        }
        self.store
            .set_status(valid.lease.lease_id, "interrupting")
            .await
            .map_err(runtime_status)?;
        self.adapter
            .interrupt(&valid.context, &valid.lease)
            .await
            .map_err(runtime_status)?;
        self.store
            .set_status(valid.lease.lease_id, "ready")
            .await
            .map_err(runtime_status)?;
        Ok(Response::new(OperationResult {
            accepted: true,
            replayed: false,
        }))
    }

    async fn upload(
        &self,
        request: Request<Streaming<UploadChunk>>,
    ) -> Result<Response<OperationResult>, Status> {
        let mut stream = request.into_inner();
        let first = stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("Upload stream is empty"))??;
        let scope = first
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let rpc = first
            .lease
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox lease is required"))?;
        let valid = self
            .store
            .validate_lease(scope, rpc)
            .await
            .map_err(runtime_status)?;
        let path = first.path.clone();
        let mut expected = 0_u64;
        let mut content = Vec::new();
        let first_scope = scope.clone();
        let first_lease = rpc.clone();
        let mut chunk = Some(first);
        loop {
            if let Some(value) = chunk.take() {
                if value.scope.as_ref() != Some(&first_scope)
                    || value.lease.as_ref() != Some(&first_lease)
                    || value.path != path
                    || value.sequence != expected
                {
                    return Err(Status::invalid_argument(
                        "Upload chunks changed scope, lease, path, or sequence",
                    ));
                }
                content.extend_from_slice(&value.data);
                expected += 1;
                if value.eof {
                    break;
                }
            }
            chunk = stream.next().await.transpose()?;
            if chunk.is_none() {
                return Err(Status::invalid_argument("Upload stream ended before eof"));
            }
        }
        self.adapter
            .upload(&valid.context, &valid.lease, &path, content)
            .await
            .map_err(runtime_status)?;
        Ok(Response::new(OperationResult {
            accepted: true,
            replayed: false,
        }))
    }

    async fn download(
        &self,
        request: Request<DownloadRequest>,
    ) -> Result<Response<Self::DownloadStream>, Status> {
        let request = request.into_inner();
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let rpc = request
            .lease
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox lease is required"))?;
        let valid = self
            .store
            .validate_lease(scope, rpc)
            .await
            .map_err(runtime_status)?;
        let content = self
            .adapter
            .download(&valid.context, &valid.lease, &request.path)
            .await
            .map_err(runtime_status)?;
        let mut chunks = content
            .chunks(64 * 1024)
            .enumerate()
            .map(|(index, data)| FileChunk {
                sequence: index as u64,
                data: data.to_vec(),
                eof: (index + 1) * 64 * 1024 >= content.len(),
            })
            .collect::<Vec<_>>();
        if chunks.is_empty() {
            chunks.push(FileChunk {
                sequence: 0,
                data: Vec::new(),
                eof: true,
            });
        }
        Ok(Response::new(Box::pin(tokio_stream::iter(
            chunks.into_iter().map(Ok),
        ))))
    }

    async fn metrics(
        &self,
        request: Request<SandboxLeaseRequest>,
    ) -> Result<Response<SandboxMetrics>, Status> {
        let request = request.into_inner();
        let valid = self.validate_request(&request).await?;
        let metrics = self
            .adapter
            .metrics(&valid.context, &valid.lease)
            .await
            .map_err(runtime_status)?;
        Ok(Response::new(SandboxMetrics {
            cpu_nanos: metrics.cpu_nanos,
            memory_bytes: metrics.memory_bytes,
            pids: metrics.pids,
            disk_bytes: metrics.disk_bytes,
            raw_json: metrics.raw.to_string(),
        }))
    }

    async fn terminate(
        &self,
        request: Request<SandboxLeaseRequest>,
    ) -> Result<Response<OperationResult>, Status> {
        let request = request.into_inner();
        let valid = self.validate_cleanup_request(&request).await?;
        if valid.status == "terminated" {
            return Ok(Response::new(OperationResult {
                accepted: true,
                replayed: true,
            }));
        }
        self.store
            .set_status(valid.lease.lease_id, "terminating")
            .await
            .map_err(runtime_status)?;
        match self.adapter.terminate(&valid.context, &valid.lease).await {
            Ok(()) => {
                self.store
                    .terminate(valid.lease.lease_id)
                    .await
                    .map_err(runtime_status)?;
                Ok(Response::new(OperationResult {
                    accepted: true,
                    replayed: false,
                }))
            }
            Err(error) => {
                let cleanup_pending = self
                    .store
                    .mark_failed(valid.lease.lease_id, &error.to_string(), true)
                    .await
                    .map_err(runtime_status)?;
                if cleanup_pending {
                    Err(Status::unavailable("SANDBOX_CLEANUP_PENDING"))
                } else {
                    Ok(Response::new(OperationResult {
                        accepted: true,
                        replayed: true,
                    }))
                }
            }
        }
    }
}

impl SandboxManagerService {
    async fn validate_request(
        &self,
        request: &SandboxLeaseRequest,
    ) -> Result<crate::store::ValidLease, Status> {
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let lease = request
            .lease
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox lease is required"))?;
        self.store
            .validate_lease(scope, lease)
            .await
            .map_err(runtime_status)
    }

    async fn validate_cleanup_request(
        &self,
        request: &SandboxLeaseRequest,
    ) -> Result<crate::store::ValidLease, Status> {
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox scope is required"))?;
        let lease = request
            .lease
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("Sandbox lease is required"))?;
        self.store
            .validate_cleanup_lease(scope, lease)
            .await
            .map_err(runtime_status)
    }
}
fn parse_json(value: &str, default: Value) -> anyhow::Result<Value> {
    if value.trim().is_empty() {
        Ok(default)
    } else {
        serde_json::from_str(value).map_err(Into::into)
    }
}

fn secret_patterns(credentials: &[crate::store::ResolvedSandboxCredential]) -> Vec<Vec<u8>> {
    let mut patterns = std::collections::BTreeSet::new();
    for credential in credentials {
        let secret = credential.secret.expose();
        if !secret.is_empty() {
            patterns.insert(secret.to_vec());
        }
        if let Ok(value) = serde_json::from_slice::<Value>(secret) {
            collect_secret_values(&value, &mut patterns);
        }
    }
    let mut patterns = patterns.into_iter().collect::<Vec<_>>();
    patterns.sort_by_key(|value| std::cmp::Reverse(value.len()));
    patterns
}

fn collect_secret_values(value: &Value, patterns: &mut std::collections::BTreeSet<Vec<u8>>) {
    match value {
        Value::String(value) if !value.is_empty() => {
            patterns.insert(value.as_bytes().to_vec());
        }
        Value::Array(values) => {
            for value in values {
                collect_secret_values(value, patterns);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_secret_values(value, patterns);
            }
        }
        _ => {}
    }
}

struct StreamRedactor {
    patterns: Vec<Vec<u8>>,
    pending: Vec<u8>,
    keep: usize,
}

impl StreamRedactor {
    fn new(patterns: Vec<Vec<u8>>) -> Self {
        let keep = patterns
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        Self {
            patterns,
            pending: Vec::new(),
            keep,
        }
    }

    fn push(&mut self, data: &[u8], final_chunk: bool) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let safe_start_limit = if final_chunk {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(self.keep)
        };
        let mut output = Vec::new();
        let mut index = 0;
        while index < safe_start_limit {
            if let Some(pattern) = self.patterns.iter().find(|pattern| {
                !pattern.is_empty() && self.pending[index..].starts_with(pattern.as_slice())
            }) {
                output.extend_from_slice(b"[REDACTED]");
                index += pattern.len();
            } else {
                output.push(self.pending[index]);
                index += 1;
            }
        }
        self.pending.drain(..index);
        output
    }
}

async fn cleanup_secret_files(
    adapter: &OpenSandboxAdapter,
    context: &agentx_application::RuntimeContext,
    lease: &agentx_application::SandboxLease,
    working_directory: &str,
    paths: &[String],
) -> agentx_application::RuntimeResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut argv = vec!["rm".into(), "-f".into(), "--".into()];
    argv.extend(paths.iter().cloned());
    let mut stream = adapter
        .execute(
            context,
            SandboxCommand {
                lease: lease.clone(),
                argv,
                environment: json!({}),
                working_directory: working_directory.into(),
            },
        )
        .await?;
    let mut exit_code = None;
    while let Some(event) = stream.next().await {
        if let SandboxEvent::Completed {
            exit_code: value, ..
        } = event?
        {
            exit_code = Some(value);
        }
    }
    if exit_code == Some(0) {
        Ok(())
    } else {
        Err(agentx_application::RuntimeError::new(
            "SANDBOX_CREDENTIAL_CLEANUP_FAILED",
            "Sandbox Credential files could not be removed",
        )
        .retryable(true))
    }
}

fn supported_working_directory(value: &str) -> Option<&'static str> {
    match value {
        "/workspace" => Some("/workspace"),
        "/home/playwright" => Some("/home/playwright"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration as StdDuration,
    };

    use agentx_application::RuntimeResourceSnapshot;
    use agentx_domain::{ResourceOperation, ResourceReference, ResourceType};
    use agentx_infrastructure::{config::MySqlSettings, credential::CredentialKeyring, mysql};
    use secrecy::SecretString;
    use sqlx::Row;
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task::JoinHandle,
    };
    use tonic::Request;
    use url::Url;
    use uuid::Uuid;

    use super::*;

    #[derive(Clone, Default)]
    struct UnknownCreateServerState {
        create_count: Arc<AtomicUsize>,
        list_count: Arc<AtomicUsize>,
        endpoint_count: Arc<AtomicUsize>,
    }

    async fn spawn_unknown_create_server() -> (Url, UnknownCreateServerState, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = format!("http://{address}");
        let state = UnknownCreateServerState::default();
        let task_state = state.clone();
        let task_origin = origin.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let state = task_state.clone();
                let origin = task_origin.clone();
                tokio::spawn(async move {
                    let request = read_http_request(&mut stream).await;
                    let first = request.lines().next().unwrap_or_default();
                    if first.starts_with("POST /v1/sandboxes ") {
                        state.create_count.fetch_add(1, Ordering::SeqCst);
                        let _ = stream.shutdown().await;
                        return;
                    }
                    if first.starts_with("GET /v1/sandboxes?") {
                        state.list_count.fetch_add(1, Ordering::SeqCst);
                        write_json_response(
                            &mut stream,
                            &json!({"items":[{"id":"sbx-reconciled","status":{"state":"Running"},"metadata":{},"expiresAt":null}]}),
                        )
                        .await;
                        return;
                    }
                    if first.starts_with(
                        "GET /v1/sandboxes/sbx-reconciled/endpoints/44772?use_server_proxy=true ",
                    ) {
                        state.endpoint_count.fetch_add(1, Ordering::SeqCst);
                        write_json_response(
                            &mut stream,
                            &json!({
                                "endpoint":format!("{origin}/v1/sandboxes/sbx-reconciled/proxy/44772/"),
                                "headers":{"x-execd-access-token":"reconciled-token"}
                            }),
                        )
                        .await;
                        return;
                    }
                    let _ = stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                        .await;
                });
            }
        });
        (Url::parse(&origin).unwrap(), state, task)
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).await.unwrap_or(0);
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = buffer.windows(4).position(|value| value == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buffer[..header_end + 4]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if buffer.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    async fn write_json_response(stream: &mut TcpStream, value: &Value) {
        let body = serde_json::to_vec(value).unwrap();
        let headers = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        stream.shutdown().await.unwrap();
    }

    #[test]
    fn redacts_secrets_split_across_chunks() {
        let mut redactor = StreamRedactor::new(vec![b"runtime-secret".to_vec()]);
        let mut output = redactor.push(b"before runtime-", false);
        output.extend(redactor.push(b"secret after", false));
        output.extend(redactor.push(&[], true));
        assert_eq!(output, b"before [REDACTED] after");
    }

    #[test]
    fn only_profile_workspace_roots_are_accepted() {
        assert_eq!(
            supported_working_directory("/workspace"),
            Some("/workspace")
        );
        assert_eq!(
            supported_working_directory("/home/playwright"),
            Some("/home/playwright")
        );
        assert_eq!(supported_working_directory("/workspace/subdir"), None);
        assert_eq!(supported_working_directory("/home/playwright-escape"), None);
    }

    #[tokio::test]
    async fn create_response_loss_reconciles_exactly_one_labeled_sandbox() {
        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .expect("start MySQL");
        let settings = MySqlSettings {
            host: "127.0.0.1".into(),
            port: container.get_host_port_ipv4(3306.tcp()).await.unwrap(),
            database: "agentx".into(),
            username: "agentx".into(),
            password: SecretString::from("agentx-test-password"),
            max_connections: 8,
            tls_mode: agentx_infrastructure::config::MySqlTlsMode::Disabled,
            tls_ca_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
        };
        let pool = loop {
            match mysql::connect(&settings).await {
                Ok(pool) => break pool,
                Err(_) => tokio::time::sleep(StdDuration::from_millis(250)).await,
            }
        };
        mysql::run_migrations(&pool).await.expect("run migrations");
        let fixture = seed_manager_scope(&pool).await;
        let (endpoint, server_state, server) = spawn_unknown_create_server().await;
        let adapter = OpenSandboxAdapter::new(endpoint, SecretString::from("test-api-key"))
            .unwrap()
            .with_secure_access(false);
        let keyring = test_keyring();
        let store = LeaseStore::new(
            pool.clone(),
            SecretString::from("lease-signing-key"),
            keyring.clone(),
            keyring,
            4,
        );
        let service = SandboxManagerService::new(store, adapter);
        let request = CreateSandboxRequest {
            scope: Some(fixture.scope.clone()),
            idempotency_key: "unknown-create".into(),
            profile_snapshot_json: serde_json::to_string(&fixture.profile).unwrap(),
            labels_json: "{}".into(),
            network_policy_json: "null".into(),
            credentials: Vec::new(),
        };
        let lease = SandboxManager::create(&service, Request::new(request.clone()))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(lease.sandbox_id, "sbx-reconciled");
        assert!(!lease.lease_token.is_empty());
        assert_eq!(server_state.create_count.load(Ordering::SeqCst), 1);
        assert_eq!(server_state.list_count.load(Ordering::SeqCst), 1);
        assert_eq!(server_state.endpoint_count.load(Ordering::SeqCst), 1);

        let replay = SandboxManager::create(&service, Request::new(request))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(replay.lease_id, lease.lease_id);
        assert_eq!(replay.sandbox_id, lease.sandbox_id);
        assert_eq!(server_state.create_count.load(Ordering::SeqCst), 1);
        assert_eq!(server_state.list_count.load(Ordering::SeqCst), 1);
        let row = sqlx::query("SELECT status,sandbox_id,last_error FROM sandbox_leases WHERE id=?")
            .bind(Uuid::parse_str(&lease.lease_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.try_get::<String, _>("status").unwrap(), "ready");
        assert_eq!(
            row.try_get::<String, _>("sandbox_id").unwrap(),
            "sbx-reconciled"
        );
        assert!(
            row.try_get::<Option<String>, _>("last_error")
                .unwrap()
                .is_none()
        );
        server.abort();
    }

    struct ManagerFixture {
        scope: agentx_runtime_rpc::sandbox_v1::SandboxScope,
        profile: RuntimeResourceSnapshot,
    }

    async fn seed_manager_scope(pool: &sqlx::MySqlPool) -> ManagerFixture {
        let tenant = Uuid::now_v7();
        let user = Uuid::now_v7();
        let department = Uuid::now_v7();
        let workflow = Uuid::now_v7();
        let identity = Uuid::now_v7();
        let version = Uuid::now_v7();
        let execution = Uuid::now_v7();
        let node = Uuid::now_v7();
        let attempt = Uuid::now_v7();
        let worker_lease = Uuid::now_v7();
        let trace = Uuid::now_v7();
        let profile_id = Uuid::now_v7();
        let profile_version = Uuid::now_v7();
        sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Sandbox Manager','sandbox manager')").bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)").bind(department).bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,'sandbox-manager','sandbox-manager','Sandbox Manager')").bind(user).bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Sandbox Manager',?,?)").bind(workflow).bind(tenant).bind(user).bind(department).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)",
        )
        .bind(identity)
        .bind(tenant)
        .bind(workflow)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'2.0',JSON_OBJECT(),'sandbox-manager',?)").bind(version).bind(tenant).bind(workflow).bind(user).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES(?,?,?,?,?,'manual','running',CURRENT_TIMESTAMP(6))").bind(execution).bind(tenant).bind(workflow).bind(version).bind(trace).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO execution_snapshots(execution_id,tenant_id,workflow_version_id,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,resource_snapshot_json,runtime_settings_json,state_hash) VALUES(?,?,?,JSON_OBJECT(),JSON_OBJECT(),'compiled','test',?,JSON_OBJECT(),'state')")
            .bind(execution).bind(tenant).bind(version).bind(json!({"workflowServiceIdentityId":identity})).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES(?,?,?,'code','Code','code',1,0,0,0,'running','sandbox')").bind(node).bind(tenant).bind(execution).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,lease_token,deadline_at) VALUES(?,?,?,?,1,'running','sandbox-manager-attempt',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))").bind(attempt).bind(tenant).bind(execution).bind(node).bind(worker_lease).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO worker_leases(node_attempt_id,tenant_id,node_execution_id,lease_token,worker_instance_id,capability,acquired_at,heartbeat_at,expires_at) VALUES(?,?,?,?,'worker-test','sandbox',CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6),DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 MINUTE))").bind(attempt).bind(tenant).bind(node).bind(worker_lease).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO sandbox_profiles(id,tenant_id,name,owner_department_id,created_by) VALUES(?,?,'Test Profile',?,?)").bind(profile_id).bind(tenant).bind(department).bind(user).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO sandbox_profile_versions(id,tenant_id,profile_id,version_number,runner,image_digest,cpu_millis,memory_bytes,pids_limit,disk_bytes,timeout_seconds,output_limit_bytes,network_policy_json,configuration_hash,created_by) VALUES(?,?,?,1,'python',?,500,536870912,128,1073741824,120,1048576,JSON_OBJECT('defaultAction','deny','egress',JSON_ARRAY()),?,?)")
            .bind(profile_version).bind(tenant).bind(profile_id)
            .bind(format!("example/test@sha256:{}", "a".repeat(64)))
            .bind("b".repeat(64)).bind(user).execute(pool).await.unwrap();
        ManagerFixture {
            scope: agentx_runtime_rpc::sandbox_v1::SandboxScope {
                tenant_id: tenant.to_string(),
                workflow_id: workflow.to_string(),
                workflow_version_id: Some(version.to_string()),
                execution_id: execution.to_string(),
                node_execution_id: node.to_string(),
                attempt_id: attempt.to_string(),
                worker_lease_token: worker_lease.to_string(),
                trace_id: trace.to_string(),
                request_id: "unknown-create".into(),
                deadline_unix_ms: (OffsetDateTime::now_utc() + time::Duration::minutes(2))
                    .unix_timestamp()
                    * 1000,
            },
            profile: RuntimeResourceSnapshot {
                node_id: "code".into(),
                reference: ResourceReference {
                    binding_id: None,
                    binding_role: None,
                    resource_type: ResourceType::SandboxProfile,
                    resource_id: profile_id,
                    resource_version_id: Some(profile_version),
                    operation: ResourceOperation::Use,
                },
                snapshot_hash: "test".into(),
                snapshot: json!({
                    "profileVersionId":profile_version,
                    "runner":"python",
                    "imageDigest":format!("example/test@sha256:{}", "a".repeat(64)),
                    "cpuMillis":500,
                    "memoryBytes":536870912_u64,
                    "pidsLimit":128,
                    "diskBytes":1073741824_u64,
                    "timeoutSeconds":120,
                    "outputLimitBytes":1048576,
                    "networkPolicy":{"defaultAction":"deny","egress":[]}
                }),
            },
        }
    }

    fn test_keyring() -> CredentialKeyring {
        CredentialKeyring::from_json(
            "test-v1".into(),
            &SecretString::from(
                r#"{"keys":{"test-v1":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#,
            ),
        )
        .unwrap()
    }
}
