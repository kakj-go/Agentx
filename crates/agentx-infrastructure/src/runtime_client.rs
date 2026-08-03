use agentx_application::{
    AcceptedExecution, ExecutionRuntime, ForkExecutionCommand, RequestExecution,
    ResumeExecutionCommand, SideEffectConfirmationCommand,
};
use agentx_domain::{ExecutionId, ExecutionStatus, TenantId};
use agentx_runtime_rpc::v1::{
    CancelExecutionRequest, ConfirmSideEffectRequest, ForkExecutionRequest,
    RequestExecutionRequest, ResumeExecutionRequest,
    runtime_coordinator_client::RuntimeCoordinatorClient,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::MySqlPool;
use tonic::transport::{Channel, Endpoint};

#[derive(Clone)]
pub struct GrpcExecutionRuntime {
    client: RuntimeCoordinatorClient<Channel>,
    pool: MySqlPool,
}

impl GrpcExecutionRuntime {
    pub fn connect(endpoint: &str, pool: MySqlPool) -> Result<Self> {
        let channel = Endpoint::from_shared(endpoint.to_owned())
            .context("runtime coordinator endpoint is invalid")?
            .connect_lazy();
        Ok(Self {
            client: RuntimeCoordinatorClient::new(channel),
            pool,
        })
    }
}

#[async_trait]
impl ExecutionRuntime for GrpcExecutionRuntime {
    async fn request_execution(&self, request: RequestExecution) -> Result<AcceptedExecution> {
        let response = self
            .client
            .clone()
            .request_execution(RequestExecutionRequest {
                tenant_id: request.tenant_id.to_string(),
                workflow_version_id: request.workflow_version_id.to_string(),
                invocation_id: request.invocation_id.map(|value| value.to_string()),
                session_id: request.session_id.map(|value| value.to_string()),
                requested_by: request.requested_by.map(|value| value.to_string()),
                trigger_type: request.trigger_type,
                input_json: serde_json::to_string(&request.input)?,
                idempotency_key: request.idempotency_key,
                caller_execution_id: None,
            })
            .await
            .context("runtime coordinator request failed")?
            .into_inner();
        Ok(AcceptedExecution {
            execution_id: response.execution_id.parse()?,
            status: parse_status(&response.status)?,
        })
    }

    async fn get_execution(
        &self,
        tenant_id: TenantId,
        id: ExecutionId,
    ) -> Result<Option<ExecutionStatus>> {
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM workflow_executions WHERE tenant_id=? AND id=?",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        status.as_deref().map(parse_status).transpose()
    }

    async fn cancel_execution(&self, tenant_id: TenantId, id: ExecutionId) -> Result<()> {
        self.client
            .clone()
            .cancel_execution(CancelExecutionRequest {
                tenant_id: tenant_id.to_string(),
                execution_id: id.to_string(),
                actor_user_id: None,
            })
            .await
            .context("runtime coordinator cancel failed")?;
        Ok(())
    }

    async fn fork_execution(&self, command: ForkExecutionCommand) -> Result<AcceptedExecution> {
        let response = self
            .client
            .clone()
            .fork_execution(ForkExecutionRequest {
                tenant_id: command.tenant_id.to_string(),
                source_execution_id: command.source_execution_id.to_string(),
                checkpoint_id: command.checkpoint_id.to_string(),
                mode: command.mode,
                node_id: command.node_id,
                input_overrides_json: serde_json::to_string(&command.input_overrides)?,
                side_effect_decisions_json: serde_json::to_string(&command.side_effect_decisions)?,
                actor_user_id: command.actor_user_id.to_string(),
                idempotency_key: command.idempotency_key,
            })
            .await
            .context("runtime coordinator fork failed")?
            .into_inner();
        Ok(AcceptedExecution {
            execution_id: response.execution_id.parse()?,
            status: parse_status(&response.status)?,
        })
    }

    async fn resume_execution(&self, command: ResumeExecutionCommand) -> Result<bool> {
        let response = self
            .client
            .clone()
            .resume_execution(ResumeExecutionRequest {
                tenant_id: command.tenant_id.to_string(),
                execution_id: command.execution_id.to_string(),
                node_execution_id: command.node_execution_id.to_string(),
                resume_token: command.resume_token,
                output_port: command.output_port,
                payload_json: serde_json::to_string(&command.payload)?,
                idempotency_key: command.idempotency_key,
                actor_user_id: command.actor_user_id.map(|value| value.to_string()),
            })
            .await
            .context("runtime coordinator resume failed")?
            .into_inner();
        Ok(response.replayed)
    }

    async fn confirm_side_effect(&self, command: SideEffectConfirmationCommand) -> Result<bool> {
        let response = self
            .client
            .clone()
            .confirm_side_effect(ConfirmSideEffectRequest {
                tenant_id: command.tenant_id.to_string(),
                execution_id: command.execution_id.to_string(),
                node_execution_id: command.node_execution_id.to_string(),
                checkpoint_id: command.checkpoint_id.map(|value| value.to_string()),
                decision: command.decision,
                actor_user_id: command.actor_user_id.to_string(),
                idempotency_key: command.idempotency_key,
            })
            .await
            .context("runtime coordinator side-effect confirmation failed")?
            .into_inner();
        Ok(response.replayed)
    }
}

fn parse_status(value: &str) -> Result<ExecutionStatus> {
    Ok(match value {
        "created" => ExecutionStatus::Created,
        "queued" => ExecutionStatus::Queued,
        "running" => ExecutionStatus::Running,
        "waiting" => ExecutionStatus::Waiting,
        "waiting_approval" => ExecutionStatus::WaitingApproval,
        "suspended" => ExecutionStatus::Suspended,
        "succeeded" => ExecutionStatus::Succeeded,
        "failed" => ExecutionStatus::Failed,
        "cancelled" => ExecutionStatus::Cancelled,
        "timed_out" => ExecutionStatus::TimedOut,
        other => anyhow::bail!("runtime returned unknown execution status {other}"),
    })
}
