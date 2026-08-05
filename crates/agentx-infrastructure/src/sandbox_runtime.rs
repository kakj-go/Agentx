use agentx_application::{
    RuntimeContext, RuntimeError, RuntimeResult, RuntimeStream, SandboxCommand,
    SandboxCreateRequest, SandboxEvent, SandboxLease, SandboxMetrics, SandboxRuntime,
};
use agentx_runtime_rpc::sandbox_v1::{
    CreateSandboxRequest, DownloadRequest, ExecuteCommandRequest, SandboxLeaseRequest,
    SandboxScope, UploadChunk, sandbox_manager_client::SandboxManagerClient,
};
use async_trait::async_trait;
use futures::StreamExt;
use tonic::{
    Request,
    metadata::MetadataValue,
    transport::{Channel, Endpoint},
};

#[derive(Clone)]
pub struct SandboxManagerRuntime {
    client: SandboxManagerClient<Channel>,
    authorization: MetadataValue<tonic::metadata::Ascii>,
}

impl SandboxManagerRuntime {
    pub fn connect_lazy(endpoint: &str, token: &str) -> RuntimeResult<Self> {
        let channel = Endpoint::from_shared(endpoint.to_owned())
            .map_err(|error| {
                RuntimeError::new("SANDBOX_MANAGER_CONFIG_INVALID", error.to_string())
            })?
            .connect_lazy();
        let authorization = format!("Bearer {token}").parse().map_err(|_| {
            RuntimeError::new(
                "SANDBOX_MANAGER_CONFIG_INVALID",
                "Sandbox Manager RPC token is not valid metadata",
            )
        })?;
        Ok(Self {
            client: SandboxManagerClient::new(channel),
            authorization,
        })
    }

    fn request<T>(&self, value: T) -> Request<T> {
        let mut request = Request::new(value);
        request
            .metadata_mut()
            .insert("authorization", self.authorization.clone());
        request
    }
}

#[async_trait]
impl SandboxRuntime for SandboxManagerRuntime {
    async fn create(
        &self,
        context: &RuntimeContext,
        request: SandboxCreateRequest,
    ) -> RuntimeResult<SandboxLease> {
        let response = self
            .client
            .clone()
            .create(
                self.request(CreateSandboxRequest {
                    scope: Some(scope(context)),
                    idempotency_key: context.idempotency_key.clone(),
                    profile_snapshot_json: serde_json::to_string(&request.profile)
                        .map_err(protocol_error)?,
                    labels_json: request.labels.to_string(),
                    network_policy_json: request.network_policy.to_string(),
                    credentials: request
                        .credentials
                        .into_iter()
                        .map(
                            |credential| agentx_runtime_rpc::sandbox_v1::SandboxCredentialHandle {
                                resource_id: credential.resource_id.to_string(),
                                handle: credential.handle,
                                environment_name: credential.environment_name,
                            },
                        )
                        .collect(),
                }),
            )
            .await
            .map_err(status_error)?
            .into_inner();
        lease(response)
    }

    async fn execute(
        &self,
        context: &RuntimeContext,
        command: SandboxCommand,
    ) -> RuntimeResult<RuntimeStream<SandboxEvent>> {
        let mut stream = self
            .client
            .clone()
            .execute(self.request(ExecuteCommandRequest {
                scope: Some(scope(context)),
                lease: Some(rpc_lease(&command.lease)),
                argv: command.argv,
                environment_json: command.environment.to_string(),
                working_directory: command.working_directory,
            }))
            .await
            .map_err(status_error)?
            .into_inner();
        let output = async_stream::stream! {
            while let Some(event)=stream.next().await {
                let event=event.map_err(status_error)?;
                use agentx_runtime_rpc::sandbox_v1::command_event::Event;
                match event.event.ok_or_else(||RuntimeError::new("SANDBOX_PROTOCOL_UNSUPPORTED","Sandbox Manager returned an empty command event"))?{
                    Event::Stdout(data)=>yield Ok(SandboxEvent::Stdout{sequence:event.sequence,data}),
                    Event::Stderr(data)=>yield Ok(SandboxEvent::Stderr{sequence:event.sequence,data}),
                    Event::Completed(value)=>yield Ok(SandboxEvent::Completed{exit_code:value.exit_code,partial:value.partial}),
                }
            }
        };
        Ok(Box::pin(output))
    }

    async fn interrupt(&self, context: &RuntimeContext, lease: &SandboxLease) -> RuntimeResult<()> {
        self.client
            .clone()
            .interrupt(self.request(lease_request(context, lease)))
            .await
            .map_err(status_error)?;
        Ok(())
    }

    async fn upload(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
        content: Vec<u8>,
    ) -> RuntimeResult<()> {
        let chunks = content
            .chunks(64 * 1024)
            .enumerate()
            .map(|(index, data)| UploadChunk {
                scope: Some(scope(context)),
                lease: Some(rpc_lease(lease)),
                path: path.to_owned(),
                sequence: index as u64,
                data: data.to_vec(),
                eof: (index + 1) * 64 * 1024 >= content.len(),
            })
            .collect::<Vec<_>>();
        let chunks = if chunks.is_empty() {
            vec![UploadChunk {
                scope: Some(scope(context)),
                lease: Some(rpc_lease(lease)),
                path: path.to_owned(),
                sequence: 0,
                data: Vec::new(),
                eof: true,
            }]
        } else {
            chunks
        };
        self.client
            .clone()
            .upload(self.request(tokio_stream::iter(chunks)))
            .await
            .map_err(status_error)?;
        Ok(())
    }

    async fn download(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
    ) -> RuntimeResult<Vec<u8>> {
        let mut stream = self
            .client
            .clone()
            .download(self.request(DownloadRequest {
                scope: Some(scope(context)),
                lease: Some(rpc_lease(lease)),
                path: path.to_owned(),
            }))
            .await
            .map_err(status_error)?
            .into_inner();
        let mut result = Vec::new();
        let mut expected = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(status_error)?;
            if chunk.sequence != expected {
                return Err(RuntimeError::new(
                    "SANDBOX_PROTOCOL_UNSUPPORTED",
                    "Sandbox Manager returned out-of-order file chunks",
                ));
            }
            expected += 1;
            result.extend_from_slice(&chunk.data);
            if chunk.eof {
                return Ok(result);
            }
        }
        Err(RuntimeError::new(
            "SANDBOX_PROTOCOL_UNSUPPORTED",
            "Sandbox Manager download ended before eof",
        )
        .partial(!result.is_empty()))
    }

    async fn metrics(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
    ) -> RuntimeResult<SandboxMetrics> {
        let value = self
            .client
            .clone()
            .metrics(self.request(lease_request(context, lease)))
            .await
            .map_err(status_error)?
            .into_inner();
        Ok(SandboxMetrics {
            cpu_nanos: value.cpu_nanos,
            memory_bytes: value.memory_bytes,
            pids: value.pids,
            disk_bytes: value.disk_bytes,
            raw: serde_json::from_str(&value.raw_json).map_err(protocol_error)?,
        })
    }
    async fn terminate(&self, context: &RuntimeContext, lease: &SandboxLease) -> RuntimeResult<()> {
        self.client
            .clone()
            .terminate(self.request(lease_request(context, lease)))
            .await
            .map_err(status_error)?;
        Ok(())
    }
}

fn scope(context: &RuntimeContext) -> SandboxScope {
    SandboxScope {
        tenant_id: context.tenant_id.to_string(),
        workflow_id: context.workflow_id.to_string(),
        workflow_version_id: context.workflow_version_id.map(|value| value.to_string()),
        execution_id: context.execution_id.to_string(),
        node_execution_id: context.node_execution_id.to_string(),
        attempt_id: context.attempt_id.to_string(),
        worker_lease_token: context.lease_token.to_string(),
        trace_id: context.trace_id.to_string(),
        request_id: context.idempotency_key.clone(),
        deadline_unix_ms: (context.deadline.unix_timestamp_nanos() / 1_000_000) as i64,
    }
}
fn rpc_lease(value: &SandboxLease) -> agentx_runtime_rpc::sandbox_v1::SandboxLease {
    agentx_runtime_rpc::sandbox_v1::SandboxLease {
        lease_id: value.lease_id.to_string(),
        sandbox_id: value.sandbox_id.clone(),
        lease_token: value.lease_token.clone(),
        expires_at_unix_ms: (value.expires_at.unix_timestamp_nanos() / 1_000_000) as i64,
    }
}
fn lease(value: agentx_runtime_rpc::sandbox_v1::SandboxLease) -> RuntimeResult<SandboxLease> {
    Ok(SandboxLease {
        lease_id: uuid::Uuid::parse_str(&value.lease_id).map_err(protocol_error)?,
        sandbox_id: value.sandbox_id,
        lease_token: value.lease_token,
        expires_at: time::OffsetDateTime::from_unix_timestamp_nanos(
            value.expires_at_unix_ms as i128 * 1_000_000,
        )
        .map_err(protocol_error)?,
    })
}
fn lease_request(context: &RuntimeContext, lease: &SandboxLease) -> SandboxLeaseRequest {
    SandboxLeaseRequest {
        scope: Some(scope(context)),
        lease: Some(rpc_lease(lease)),
    }
}
fn status_error(error: tonic::Status) -> RuntimeError {
    let stable = stable_sandbox_code(error.message());
    let code = stable.as_deref().unwrap_or_else(|| {
        if error.code() == tonic::Code::Unavailable {
            "SANDBOX_MANAGER_UNAVAILABLE"
        } else {
            "SANDBOX_MANAGER_ERROR"
        }
    });
    RuntimeError::new(code, error.message())
        .retryable(matches!(
            error.code(),
            tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
        ))
        .outcome_unknown(
            error.code() == tonic::Code::Unknown
                || stable.as_deref() == Some("SANDBOX_CREATE_OUTCOME_UNKNOWN"),
        )
}

fn stable_sandbox_code(message: &str) -> Option<String> {
    let start = message.find("SANDBOX_")?;
    let code = message[start..]
        .chars()
        .take_while(|value| value.is_ascii_uppercase() || value.is_ascii_digit() || *value == '_')
        .collect::<String>();
    (code.len() > "SANDBOX_".len()).then_some(code)
}
fn protocol_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new("SANDBOX_PROTOCOL_UNSUPPORTED", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::stable_sandbox_code;

    #[test]
    fn preserves_stable_manager_error_codes() {
        assert_eq!(
            stable_sandbox_code("rpc failed: SANDBOX_CREDENTIAL_HANDLE_REPLAYED: consumed"),
            Some("SANDBOX_CREDENTIAL_HANDLE_REPLAYED".into())
        );
        assert_eq!(stable_sandbox_code("transport unavailable"), None);
    }
}
