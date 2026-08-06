use std::time::Duration;

use agentx_application::{
    CancelExecutionCommandPayload, ResumeExecutionCommandPayload, RuntimeCommandType,
    StartExecutionCommandPayload,
};
use agentx_infrastructure::{
    runtime_commands::{ClaimedRuntimeCommand, RuntimeCommandRepository},
    runtime_repository::{
        CreateExecution, ResumeExecution, RuntimeExecutionSource, RuntimeRepository,
    },
};
use anyhow::{Context, Result};
use serde_json::json;
use tracing::{error, warn};

pub async fn run(repository: RuntimeRepository) {
    let commands = RuntimeCommandRepository::new(repository.pool().clone());
    loop {
        if let Err(error) = commands.reconcile_expired().await {
            warn!(%error, "runtime command lease reconciliation failed");
        }
        match commands.claim(50, 30).await {
            Ok(batch) if batch.is_empty() => tokio::time::sleep(Duration::from_millis(250)).await,
            Ok(batch) => {
                for command in batch {
                    if let Err(error) = process(&commands, &repository, &command).await {
                        error!(
                            %error,
                            command_id = %command.command.id,
                            command_type = command.command.command_type.as_str(),
                            "runtime command processing failed"
                        );
                        let delay = 2_u64.saturating_pow(command.attempt_count.min(8));
                        let message = error.to_string();
                        if is_non_retryable_command_error(&message) {
                            let _ = commands
                                .fail_terminal(&command, "RUNTIME_COMMAND_FAILED", &message)
                                .await;
                        } else {
                            let _ = commands
                                .fail(&command, "RUNTIME_COMMAND_FAILED", &message, delay)
                                .await;
                        }
                    }
                }
            }
            Err(error) => {
                error!(%error, "runtime command claim failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn is_non_retryable_command_error(message: &str) -> bool {
    [
        "RESOURCE_GRANT_MISSING",
        "RESOURCE_UNAVAILABLE",
        "RESOURCE_SNAPSHOT_MISSING",
        "WORKFLOW_VERSION_NOT_FOUND",
        "INVALID_WORKFLOW",
    ]
    .iter()
    .any(|code| message.contains(code))
}

async fn process(
    commands: &RuntimeCommandRepository,
    repository: &RuntimeRepository,
    claimed: &ClaimedRuntimeCommand,
) -> Result<()> {
    let (execution_id, result) = match claimed.command.command_type {
        RuntimeCommandType::StartExecution => {
            let payload: StartExecutionCommandPayload =
                serde_json::from_value(claimed.command.payload.clone())
                    .context("invalid start_execution command payload")?;
            let created = repository
                .create_execution(CreateExecution {
                    tenant_id: claimed.command.tenant_id.as_uuid(),
                    source: RuntimeExecutionSource::Version(payload.workflow_version_id),
                    invocation_id: payload.invocation_id,
                    session_id: payload.session_id,
                    requested_by: payload.requested_by,
                    trigger_type: payload.trigger_type,
                    input: payload.input,
                    idempotency_key: Some(format!("runtime-command:{}", claimed.command.id)),
                    caller_execution_id: None,
                    execution_type: "whole".into(),
                    parent_execution_id: None,
                    fork_checkpoint_id: None,
                    fork_mode: None,
                    runtime_settings: payload.runtime_settings,
                    debug_plan: json!({}),
                    debug_overlay_snapshot: json!({}),
                    draft_resource_snapshots: Vec::new(),
                    initial_machine: None,
                })
                .await?;
            (
                Some(created.execution_id),
                json!({
                    "executionId": created.execution_id,
                    "executionStatus": created.status,
                    "replayed": created.replayed,
                }),
            )
        }
        RuntimeCommandType::CancelExecution => {
            let payload: CancelExecutionCommandPayload =
                serde_json::from_value(claimed.command.payload.clone())
                    .context("invalid cancel_execution command payload")?;
            let accepted = repository
                .cancel_execution(claimed.command.tenant_id.as_uuid(), payload.execution_id)
                .await?;
            (
                Some(payload.execution_id),
                json!({"executionId":payload.execution_id,"accepted":accepted}),
            )
        }
        RuntimeCommandType::ResumeExecution => {
            let payload: ResumeExecutionCommandPayload =
                serde_json::from_value(claimed.command.payload.clone())
                    .context("invalid resume_execution command payload")?;
            let replayed = repository
                .resume_execution(ResumeExecution {
                    tenant_id: claimed.command.tenant_id.as_uuid(),
                    execution_id: payload.execution_id,
                    node_execution_id: payload.node_execution_id,
                    resume_token: payload.resume_token,
                    output_port: payload.output_port,
                    payload: payload.payload,
                    idempotency_key: claimed.command.idempotency_key.clone(),
                })
                .await?;
            (
                Some(payload.execution_id),
                json!({"executionId":payload.execution_id,"replayed":replayed}),
            )
        }
    };
    commands
        .complete_with_event(claimed, execution_id, result)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_non_retryable_command_error;

    #[test]
    fn permanent_resource_errors_do_not_enter_backoff_retry() {
        assert!(is_non_retryable_command_error(
            "RESOURCE_GRANT_MISSING: model grant was revoked"
        ));
        assert!(is_non_retryable_command_error(
            "RESOURCE_UNAVAILABLE: credential disabled"
        ));
        assert!(!is_non_retryable_command_error("database connection reset"));
    }
}
