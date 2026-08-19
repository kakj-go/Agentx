use std::{collections::BTreeMap, sync::Arc};

use agentx_domain::{ContextScope, NodeExecutionId};
use agentx_node_protocol::{ExecutionStyle, Item, NodeCapability, SideEffectLevel};
use agentx_runtime::{
    ActivationStatus, ExecutionMachine, ExpressionContext, ExpressionEngine, RuntimeExecutionStatus,
};
use agentx_runtime_contracts::{
    RuntimeAuthorizationSnapshotV1, RuntimeResourceBindingV1, WorkerAttemptLeaseV1,
    WorkerResultStatusV1, WorkerResultV1, WorkerTaskV1,
};
use anyhow::Context;
use object_store::ObjectStore;
use serde_json::{Value, json};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    engine_names::{
        activation_status, context_operation_name, context_value, is_terminal, machine_status,
        worker_result_status,
    },
    engine_persistence::{
        apply_context_write, authorize_resources, authorize_snapshot, finish_execution,
        initial_context_for_execution, insert_invocation_event, load_output_namespace,
        merge_output_namespace, persist_lineage, policy_timeout_from_snapshot, single_port_output,
        upsert_activation,
    },
    engine_protocol::{
        complete_command, ensure_command_lease, ensure_result_lease, ensure_task_matches,
        lease_conflict, machine_error, node_event_type, resolve_worker_outputs,
        runtime_bad_request, validate_result_hash,
    },
    error::{RuntimeError, RuntimeResult},
    execution::RuntimeCommandClaim,
};

pub(crate) use crate::engine_persistence::persist_checkpoint;
pub use crate::engine_protocol::worker_result_hash;
#[derive(Clone, Debug)]
pub struct ClaimedWorkerAttempt {
    pub lease: WorkerAttemptLeaseV1,
    pub task: WorkerTaskV1,
    pub node_type: String,
    pub node_version: u32,
    pub run_index: u32,
    pub iteration_index: u32,
    pub node_parameters: Value,
    pub inputs: BTreeMap<String, Vec<Item>>,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub context: Value,
}

pub async fn start_execution(pool: &MySqlPool, claim: &RuntimeCommandClaim) -> RuntimeResult<()> {
    start_execution_resolved(pool, claim, None, None).await
}

pub async fn start_execution_with_state(
    state: &crate::RuntimeState,
    claim: &RuntimeCommandClaim,
) -> RuntimeResult<()> {
    let checkpoint =
        crate::fork_runtime::load_external_checkpoint(state, claim.tenant_id, claim.execution_id)
            .await?;
    start_execution_resolved(&state.pool, claim, checkpoint, Some(state)).await
}

async fn start_execution_resolved(
    pool: &MySqlPool,
    claim: &RuntimeCommandClaim,
    external_checkpoint: Option<crate::fork_runtime::CheckpointPayloadV1>,
    runtime_state: Option<&crate::RuntimeState>,
) -> RuntimeResult<()> {
    let runtime_settings: Option<Value> = sqlx::query_scalar(
        "SELECT runtime_settings_json FROM execution_snapshots WHERE tenant_id=? AND execution_id=?",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .fetch_optional(pool)
    .await?;
    let debug_plan = runtime_settings
        .as_ref()
        .map(crate::debug_overlay::plan)
        .transpose()?
        .flatten();
    let debug_input = crate::debug_overlay::resolve_input_source(
        runtime_state,
        pool,
        claim.tenant_id,
        debug_plan.as_ref(),
    )
    .await?;
    let mut tx = pool.begin().await?;
    let command = sqlx::query(
        "SELECT status,locked_by,fencing_token,payload_json,COALESCE(locked_until>UTC_TIMESTAMP(6),FALSE) lease_active FROM runtime_commands WHERE id=? FOR UPDATE",
    )
    .bind(claim.command_id)
    .fetch_one(&mut *tx)
    .await?;
    if command.try_get::<String, _>("status")? == "completed" {
        tx.commit().await?;
        return Ok(());
    }
    ensure_command_lease(&command, claim)?;
    let execution = sqlx::query(
        "SELECT status,state_version,invocation_id,bundle_id,work_package_id,input_json FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .fetch_one(&mut *tx)
    .await?;
    let status: String = execution.try_get("status")?;
    if status != "queued" {
        complete_command(
            &mut tx,
            claim,
            json!({"executionId":claim.execution_id,"replayed":true,"status":status}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    let bundle_id: Uuid = execution.try_get("bundle_id")?;
    let work_package_id: Option<Uuid> = execution.try_get("work_package_id")?;
    let snapshot = sqlx::query(
        "SELECT compiled_ir_json,authorization_snapshot_json,policy_snapshot_json,worker_compatibility_json,resource_snapshot_json FROM execution_snapshots WHERE tenant_id=? AND execution_id=? FOR UPDATE",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .fetch_one(&mut *tx)
    .await?;
    let compiled = serde_json::from_value(snapshot.try_get("compiled_ir_json")?)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let authorization: RuntimeAuthorizationSnapshotV1 =
        serde_json::from_value(snapshot.try_get("authorization_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    authorize_snapshot(&mut tx, &authorization).await?;
    let resources: Vec<RuntimeResourceBindingV1> =
        serde_json::from_value(snapshot.try_get("resource_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    authorize_resources(&mut tx, claim.tenant_id, &resources).await?;
    crate::quota::reserve(
        &mut tx,
        claim.tenant_id,
        "execution_concurrency",
        "execution",
        &claim.execution_id.to_string(),
        &format!("execution:{}:concurrency", claim.execution_id),
        1,
        policy_timeout_from_snapshot(&snapshot)?,
    )
    .await?;
    let policy: agentx_runtime_contracts::RuntimePolicyV1 =
        serde_json::from_value(snapshot.try_get("policy_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    policy
        .validate()
        .map_err(|message| runtime_bad_request("INVALID_RUNTIME_POLICY", message))?;
    let compatibility: agentx_runtime_contracts::WorkerCompatibilityV1 =
        serde_json::from_value(snapshot.try_get("worker_compatibility_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let input = execution
        .try_get::<Option<Value>, _>("input_json")?
        .unwrap_or(Value::Null);
    let (mut machine, context, context_version) = if let Some(checkpoint) =
        crate::fork_runtime::machine_from_checkpoint(
            &mut tx,
            claim.tenant_id,
            claim.execution_id,
            &input,
            external_checkpoint,
        )
        .await?
    {
        (checkpoint.0, checkpoint.1, 0)
    } else {
        let workflow_input = vec![Item {
            json: input.clone(),
            ..Item::default()
        }];
        let machine = if let Some(plan) = debug_plan.as_ref()
            && plan.mode != agentx_runtime_contracts::PartialExecutionModeV1::Whole
        {
            let target = plan.target_node_id.as_deref().ok_or_else(|| {
                runtime_bad_request("DEBUG_TARGET_REQUIRED", "Partial Debug target is missing")
            })?;
            let mode = match plan.mode {
                agentx_runtime_contracts::PartialExecutionModeV1::Node => {
                    agentx_runtime::PartialExecutionMode::Node
                }
                agentx_runtime_contracts::PartialExecutionModeV1::ToNode => {
                    agentx_runtime::PartialExecutionMode::ToNode
                }
                agentx_runtime_contracts::PartialExecutionModeV1::FromNode => {
                    agentx_runtime::PartialExecutionMode::FromNode
                }
                agentx_runtime_contracts::PartialExecutionModeV1::Whole => unreachable!(),
            };
            ExecutionMachine::new_partial(
                compiled,
                mode,
                target,
                debug_input.clone().unwrap_or(workflow_input),
            )
            .map_err(machine_error)?
        } else {
            ExecutionMachine::new(compiled, workflow_input).map_err(machine_error)?
        };
        let (context, context_version) = initial_context_for_execution(
            &mut tx,
            claim.tenant_id,
            claim.execution_id,
            machine.workflow(),
        )
        .await?;
        (machine, context, context_version)
    };
    let state_version = execution.try_get::<u64, _>("state_version")? + 1;
    schedule_ready(
        &mut tx,
        ReadySchedule {
            tenant_id: claim.tenant_id,
            execution_id: claim.execution_id,
            bundle_id,
            work_package_id,
            policy: &policy,
            compatibility: &compatibility,
            resources: &resources,
            context: &context,
            machine: &mut machine,
            state_version,
        },
    )
    .await?;
    persist_machine(
        &mut tx,
        PersistMachineRequest {
            tenant_id: claim.tenant_id,
            execution_id: claim.execution_id,
            bundle_id,
            work_package_id,
            state_version,
            context_version,
            context: &context,
            machine: &machine,
            checkpoint_type: "execution_start",
        },
    )
    .await?;
    let invocation_id = execution.try_get::<Option<Uuid>, _>("invocation_id")?;
    if is_terminal(machine.status()) {
        finish_execution(
            &mut tx,
            claim.tenant_id,
            claim.execution_id,
            work_package_id,
            invocation_id,
            state_version,
            &machine,
            &context,
        )
        .await?;
    } else {
        let updated = sqlx::query(
            "UPDATE workflow_executions SET status=?,state_version=? WHERE tenant_id=? AND id=? AND status='queued' AND state_version=?",
        )
        .bind(machine_status(machine.status()))
        .bind(state_version)
        .bind(claim.tenant_id)
        .bind(claim.execution_id)
        .bind(state_version - 1)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(lease_conflict("Execution state version changed"));
        }
        sqlx::query(
            "UPDATE application_invocations SET status='running',state_version=state_version+1 WHERE tenant_id=? AND execution_id=? AND status='queued'",
        )
        .bind(claim.tenant_id)
        .bind(claim.execution_id)
        .execute(&mut *tx)
        .await?;
        if let Some(invocation_id) = invocation_id {
            insert_invocation_event(
                &mut tx,
                claim.tenant_id,
                invocation_id,
                "invocation.running",
                json!({"executionId":claim.execution_id,"status":"running"}),
            )
            .await?;
        }
    }
    complete_command(
        &mut tx,
        claim,
        json!({"executionId":claim.execution_id,"stateVersion":state_version}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn resume_execution(pool: &MySqlPool, claim: &RuntimeCommandClaim) -> RuntimeResult<()> {
    let mut tx = pool.begin().await?;
    let command = sqlx::query(
        "SELECT status,locked_by,fencing_token,COALESCE(locked_until>UTC_TIMESTAMP(6),FALSE) lease_active FROM runtime_commands WHERE id=? FOR UPDATE",
    )
    .bind(claim.command_id)
    .fetch_one(&mut *tx)
    .await?;
    if command.try_get::<String, _>("status")? == "completed" {
        tx.commit().await?;
        return Ok(());
    }
    ensure_command_lease(&command, claim)?;
    let node_execution_id = claim
        .payload
        .get("nodeExecutionId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .context("Resume command requires nodeExecutionId")?;
    let output_port = claim
        .payload
        .get("outputPort")
        .and_then(Value::as_str)
        .unwrap_or("main");
    let wait_status = claim
        .payload
        .get("waitStatus")
        .and_then(Value::as_str)
        .filter(|status| matches!(*status, "resumed" | "timed_out"))
        .unwrap_or("resumed");
    let payload = claim.payload.get("payload").cloned().unwrap_or(Value::Null);
    let composite_status = claim.payload.get("childStatus").and_then(Value::as_str);
    let execution = sqlx::query(
        "SELECT e.status,e.bundle_id,e.work_package_id,e.state_version,e.invocation_id,s.policy_snapshot_json,s.worker_compatibility_json,s.resource_snapshot_json,r.context_json,r.context_version,r.machine_state_json,r.state_version runtime_state_version FROM workflow_executions e JOIN execution_snapshots s ON s.execution_id=e.id JOIN execution_runtime_state r ON r.execution_id=e.id WHERE e.tenant_id=? AND e.id=? FOR UPDATE",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .fetch_one(&mut *tx)
    .await?;
    let execution_status: String = execution.try_get("status")?;
    if matches!(
        execution_status.as_str(),
        "succeeded" | "failed" | "cancelled" | "timed_out"
    ) {
        complete_command(
            &mut tx,
            claim,
            json!({
                "executionId": claim.execution_id,
                "status": execution_status,
                "applied": false
            }),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    let execution_version: u64 = execution.try_get("state_version")?;
    if execution_version != execution.try_get::<u64, _>("runtime_state_version")? {
        return Err(lease_conflict(
            "Resume observed inconsistent Execution state",
        ));
    }
    let mut machine: ExecutionMachine =
        serde_json::from_value(execution.try_get("machine_state_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    if composite_status.is_some_and(|status| status != "succeeded") {
        machine
            .fail(
                NodeExecutionId::from_uuid(node_execution_id),
                claim
                    .payload
                    .pointer("/error/code")
                    .and_then(Value::as_str)
                    .unwrap_or("COMPOSITE_CHILD_FAILED"),
                claim
                    .payload
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Composite child Execution failed"),
                false,
            )
            .map_err(machine_error)?;
    } else {
        machine
            .resume(
                NodeExecutionId::from_uuid(node_execution_id),
                output_port,
                vec![Item {
                    json: payload.clone(),
                    ..Item::default()
                }],
            )
            .map_err(machine_error)?;
    }
    let next_version = execution_version + 1;
    let policy = serde_json::from_value(execution.try_get("policy_snapshot_json")?)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let compatibility = serde_json::from_value(execution.try_get("worker_compatibility_json")?)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let resources: Vec<RuntimeResourceBindingV1> =
        serde_json::from_value(execution.try_get("resource_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let bundle_id: Uuid = execution.try_get("bundle_id")?;
    let work_package_id: Option<Uuid> = execution.try_get("work_package_id")?;
    let mut context: Value = execution.try_get("context_json")?;
    let mut context_version: u64 = execution.try_get("context_version")?;
    if composite_status == Some("succeeded")
        && let Some(overlay) = claim.payload.get("contextOverlay")
    {
        let before = context.clone();
        crate::composite_execution::merge_context_overlay(&mut context, overlay);
        if context != before {
            context_version = context_version.saturating_add(1);
        }
    }
    schedule_ready(
        &mut tx,
        ReadySchedule {
            tenant_id: claim.tenant_id,
            execution_id: claim.execution_id,
            bundle_id,
            work_package_id,
            policy: &policy,
            compatibility: &compatibility,
            resources: &resources,
            context: &context,
            machine: &mut machine,
            state_version: next_version,
        },
    )
    .await?;
    persist_machine(
        &mut tx,
        PersistMachineRequest {
            tenant_id: claim.tenant_id,
            execution_id: claim.execution_id,
            bundle_id,
            work_package_id,
            state_version: next_version,
            context_version,
            context: &context,
            machine: &machine,
            checkpoint_type: "node_completed",
        },
    )
    .await?;
    let resume_output = single_port_output(output_port, payload.clone());
    let resumed_status = if composite_status.is_some_and(|status| status != "succeeded") {
        "failed"
    } else {
        "succeeded"
    };
    sqlx::query(
        "UPDATE node_attempts SET status=?,output_json=?,error_code=?,error_message=?,ended_at=UTC_TIMESTAMP(6),locked_until=NULL WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND status='suspended'",
    )
    .bind(resumed_status)
    .bind(&resume_output)
    .bind(claim.payload.pointer("/error/code").and_then(Value::as_str))
    .bind(claim.payload.pointer("/error/message").and_then(Value::as_str))
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .bind(node_execution_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE node_executions SET status=?,output_json=?,error_code=?,error_message=?,ended_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND execution_id=? AND id=?",
    )
    .bind(resumed_status)
    .bind(&resume_output)
    .bind(claim.payload.pointer("/error/code").and_then(Value::as_str))
    .bind(claim.payload.pointer("/error/message").and_then(Value::as_str))
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .bind(node_execution_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE wait_subscriptions SET status=?,resumed_at=UTC_TIMESTAMP(6),locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND status='waiting'",
    )
    .bind(wait_status)
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .bind(node_execution_id)
    .execute(&mut *tx)
    .await?;
    let approval = sqlx::query(
        "SELECT id FROM approval_tasks WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND status IN ('approved','rejected') AND resume_status='pending' FOR UPDATE",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .bind(node_execution_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(approval) = approval {
        let task_id: Uuid = approval.try_get("id")?;
        sqlx::query(
            "UPDATE approval_tasks SET resume_status='succeeded',version=version+1 WHERE tenant_id=? AND id=? AND resume_status='pending'",
        )
        .bind(claim.tenant_id)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        crate::event_export::enqueue_approval_event_from_task(&mut tx, claim.tenant_id, task_id)
            .await?;
    }
    if let Err(error) = crate::engine_trace::finish_resumed_spans(
        &mut tx,
        claim.tenant_id,
        claim.execution_id,
        node_execution_id,
        resumed_status,
        wait_status,
        &resume_output,
        claim.payload.pointer("/error/code").and_then(Value::as_str),
    )
    .await
    {
        tracing::warn!(%error, execution_id = %claim.execution_id, "Resumed Span finalization failed");
    }
    sqlx::query(
        "UPDATE bundle_references r JOIN wait_subscriptions w ON w.id=r.owner_id SET r.released_at=UTC_TIMESTAMP(6) WHERE r.tenant_id=? AND r.reference_kind='pending_wait' AND w.execution_id=? AND w.node_execution_id=? AND r.released_at IS NULL",
    )
    .bind(claim.tenant_id)
    .bind(claim.execution_id)
    .bind(node_execution_id)
    .execute(&mut *tx)
    .await?;
    if is_terminal(machine.status()) {
        finish_execution(
            &mut tx,
            claim.tenant_id,
            claim.execution_id,
            work_package_id,
            execution.try_get("invocation_id")?,
            next_version,
            &machine,
            &context,
        )
        .await?;
    } else {
        sqlx::query(
            "UPDATE workflow_executions SET status=?,state_version=? WHERE tenant_id=? AND id=? AND state_version=?",
        )
        .bind(machine_status(machine.status()))
        .bind(next_version)
        .bind(claim.tenant_id)
        .bind(claim.execution_id)
        .bind(execution_version)
        .execute(&mut *tx)
        .await?;
    }
    complete_command(
        &mut tx,
        claim,
        json!({"executionId":claim.execution_id,"stateVersion":next_version}),
    )
    .await?;
    if let Some(child_execution_id) = claim
        .payload
        .get("childExecutionId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    {
        sqlx::query(
            "UPDATE execution_children SET merge_status=?,merged_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND parent_execution_id=? AND child_execution_id=? AND merge_status='pending'",
        )
        .bind(if composite_status == Some("succeeded") {
            "merged"
        } else {
            "discarded"
        })
        .bind(claim.tenant_id)
        .bind(claim.execution_id)
        .bind(child_execution_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn confirm_side_effect(
    pool: &MySqlPool,
    claim: &RuntimeCommandClaim,
) -> RuntimeResult<()> {
    let command: agentx_runtime_contracts::ExecutionCommandV1 =
        serde_json::from_value(claim.payload.clone())
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let agentx_runtime_contracts::ExecutionCommandV1::SideEffectConfirmation {
        execution_id,
        node_execution_id,
        resolution,
        expected_state_version,
    } = command
    else {
        return Err(runtime_bad_request(
            "RUNTIME_COMMAND_INVALID",
            "confirm_side_effect requires a Side Effect Confirmation payload",
        ));
    };
    if execution_id != claim.execution_id {
        return Err(lease_conflict("Side Effect command aggregate changed"));
    }
    let state_version: u64 = sqlx::query_scalar(
        "SELECT state_version FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(claim.tenant_id)
    .bind(execution_id)
    .fetch_one(pool)
    .await?;
    if state_version != expected_state_version {
        return Err(lease_conflict(
            "Side Effect confirmation observed stale Execution state",
        ));
    }
    let mut resume = claim.clone();
    resume.payload = json!({
        "nodeExecutionId": node_execution_id,
        "outputPort": "main",
        "payload": {"sideEffectResolution": resolution},
    });
    resume_execution(pool, &resume).await
}

pub async fn fork_execution(pool: &MySqlPool, claim: &RuntimeCommandClaim) -> RuntimeResult<()> {
    let command: agentx_runtime_contracts::ExecutionCommandV1 =
        serde_json::from_value(claim.payload.clone())
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let agentx_runtime_contracts::ExecutionCommandV1::Fork {
        source_execution_id,
        checkpoint_id,
        mode,
        node_id,
        side_effect_resolution,
    } = command
    else {
        return Err(runtime_bad_request(
            "RUNTIME_COMMAND_INVALID",
            "fork_execution requires a Fork payload",
        ));
    };
    if source_execution_id != claim.execution_id {
        return Err(lease_conflict("Fork command aggregate changed"));
    }
    let fork_id = crate::engine_names::deterministic_uuid(claim.command_id, b"fork-record");
    let fork_execution_id =
        crate::engine_names::deterministic_uuid(claim.command_id, b"fork-execution");
    let mut tx = pool.begin().await?;
    let command_row = sqlx::query(
        "SELECT status,locked_by,fencing_token,COALESCE(locked_until>UTC_TIMESTAMP(6),FALSE) lease_active FROM runtime_commands WHERE id=? FOR UPDATE",
    )
    .bind(claim.command_id)
    .fetch_one(&mut *tx)
    .await?;
    if command_row.try_get::<String, _>("status")? == "completed" {
        tx.commit().await?;
        return Ok(());
    }
    ensure_command_lease(&command_row, claim)?;
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT fork_execution_id FROM execution_forks WHERE tenant_id=? AND idempotency_key=?",
    )
    .bind(claim.tenant_id)
    .bind(format!("runtime-command:{}", claim.command_id))
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(existing) = existing {
        complete_command(
            &mut tx,
            claim,
            json!({"forkExecutionId":existing,"replayed":true}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    let source = sqlx::query(
        "SELECT bundle_id,work_package_id,input_json FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE",
    )
    .bind(claim.tenant_id)
    .bind(source_execution_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RuntimeError::NotFound)?;
    let bundle_id: Uuid = source.try_get("bundle_id")?;
    let checkpoint_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE tenant_id=? AND id=? AND execution_id=? AND bundle_id=?)",
    )
    .bind(claim.tenant_id)
    .bind(checkpoint_id)
    .bind(source_execution_id)
    .bind(bundle_id)
    .fetch_one(&mut *tx)
    .await?;
    if !checkpoint_exists {
        return Err(runtime_bad_request(
            "CHECKPOINT_REFERENCE_INVALID",
            "Fork checkpoint does not belong to the source Execution and Bundle",
        ));
    }
    sqlx::query(
        "INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,application_id,bundle_id,work_package_id,parent_execution_id,admission_epoch,state_version,trace_id,trigger_type,status,started_at,input_json) SELECT ?,tenant_id,workflow_id,workflow_version_id,application_id,bundle_id,work_package_id,id,admission_epoch,1,?,'fork','queued',UTC_TIMESTAMP(6),input_json FROM workflow_executions WHERE tenant_id=? AND id=?",
    )
    .bind(fork_execution_id)
    .bind(Uuid::now_v7())
    .bind(claim.tenant_id)
    .bind(source_execution_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO execution_snapshots(execution_id,tenant_id,workflow_version_id,bundle_id,work_package_id,admission_epoch,state_version,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,resource_snapshot_json,authorization_snapshot_json,policy_snapshot_json,worker_compatibility_json,object_manifest_json,runtime_settings_json,state_hash) SELECT ?,tenant_id,workflow_version_id,bundle_id,work_package_id,admission_epoch,1,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,resource_snapshot_json,authorization_snapshot_json,policy_snapshot_json,worker_compatibility_json,object_manifest_json,runtime_settings_json,state_hash FROM execution_snapshots WHERE tenant_id=? AND execution_id=?",
    )
    .bind(fork_execution_id)
    .bind(claim.tenant_id)
    .bind(source_execution_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO execution_forks(id,tenant_id,source_execution_id,source_checkpoint_id,fork_execution_id,bundle_id,mode,node_id,side_effect_resolution,idempotency_key) VALUES(?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(fork_id)
    .bind(claim.tenant_id)
    .bind(source_execution_id)
    .bind(checkpoint_id)
    .bind(fork_execution_id)
    .bind(bundle_id)
        .bind(crate::engine_names::partial_mode(mode))
    .bind(node_id)
        .bind(crate::engine_names::side_effect_resolution_name(
            side_effect_resolution,
        ))
    .bind(format!("runtime-command:{}", claim.command_id))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO bundle_references(id,tenant_id,bundle_id,reference_kind,owner_id) VALUES(?,?,?,'checkpoint_fork_source',?)",
    )
    .bind(Uuid::now_v7())
    .bind(claim.tenant_id)
    .bind(bundle_id)
    .bind(fork_id)
    .execute(&mut *tx)
    .await?;
    let start_command_id = crate::engine_names::deterministic_uuid(claim.command_id, b"fork-start");
    sqlx::query(
        "INSERT INTO runtime_commands(id,tenant_id,command_type,aggregate_type,aggregate_id,idempotency_key,payload_json,status) VALUES(?,?,'start_execution','execution',?,?,?,'pending')",
    )
    .bind(start_command_id)
    .bind(claim.tenant_id)
    .bind(fork_execution_id.to_string())
    .bind(format!("fork:start:{fork_id}"))
    .bind(json!({"sourceExecutionId":source_execution_id,"checkpointId":checkpoint_id,"forkExecutionId":fork_execution_id}))
    .execute(&mut *tx)
    .await?;
    complete_command(
        &mut tx,
        claim,
        json!({"forkId":fork_id,"forkExecutionId":fork_execution_id,"commandId":start_command_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn register_worker(
    pool: &MySqlPool,
    worker_id: Uuid,
    capability: &str,
    compiler_version: &str,
) -> RuntimeResult<()> {
    sqlx::query(
        "INSERT INTO worker_capabilities(instance_id,capability,node_protocol_version,ir_schema_versions_json,compiler_version_min,compiler_version_max,manifest_hashes_json,status,heartbeat_at) VALUES(?,?,?,JSON_ARRAY(1),?,?,JSON_ARRAY(?),'ready',UTC_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE node_protocol_version=VALUES(node_protocol_version),ir_schema_versions_json=VALUES(ir_schema_versions_json),compiler_version_min=VALUES(compiler_version_min),compiler_version_max=VALUES(compiler_version_max),manifest_hashes_json=VALUES(manifest_hashes_json),status='ready',heartbeat_at=UTC_TIMESTAMP(6)",
    )
    .bind(worker_id.to_string())
    .bind(capability)
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .bind(compiler_version)
    .bind(compiler_version)
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn heartbeat_worker(
    pool: &MySqlPool,
    worker_id: Uuid,
    capability: &str,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE worker_capabilities SET heartbeat_at=UTC_TIMESTAMP(6) WHERE instance_id=? AND capability=? AND status='ready'",
    )
    .bind(worker_id.to_string())
    .bind(capability)
    .execute(pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(lease_conflict("Worker registration was lost"));
    }
    Ok(())
}

pub async fn mark_worker_draining(pool: &MySqlPool, worker_id: Uuid) -> RuntimeResult<()> {
    sqlx::query(
        "UPDATE worker_capabilities SET status='draining',heartbeat_at=UTC_TIMESTAMP(6) WHERE instance_id=? AND status='ready'",
    )
    .bind(worker_id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn claim_worker_attempt(
    pool: &MySqlPool,
    worker_id: Uuid,
    worker_capability: &str,
    task: &WorkerTaskV1,
) -> RuntimeResult<Option<ClaimedWorkerAttempt>> {
    let mut tx = pool.begin().await?;
    let compatible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM worker_capabilities WHERE instance_id=? AND capability=? AND status='ready' AND heartbeat_at>DATE_SUB(UTC_TIMESTAMP(6),INTERVAL 30 SECOND))",
    )
    .bind(worker_id.to_string())
    .bind(worker_capability)
    .fetch_one(&mut *tx)
    .await?;
    if !compatible {
        return Err(runtime_bad_request(
            "WORKER_NOT_COMPATIBLE",
            "Worker is not registered or its capability heartbeat expired",
        ));
    }
    let row = sqlx::query(
        "SELECT a.id,a.tenant_id,a.execution_id,a.node_execution_id,a.capability,a.worker_protocol_version,a.input_json,a.fencing_token,a.deadline_at,n.node_id,n.node_type,n.node_version,n.run_index,n.iteration_index,e.input_json execution_input_json,s.compiled_ir_json,s.resource_snapshot_json,r.context_json FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id JOIN execution_snapshots s ON s.execution_id=a.execution_id JOIN execution_runtime_state r ON r.execution_id=a.execution_id WHERE a.id=? AND a.status='queued' AND (a.locked_until IS NULL OR a.locked_until<=UTC_TIMESTAMP(6)) FOR UPDATE",
    )
    .bind(task.attempt_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    ensure_task_matches(&row, worker_capability, task)?;
    let resources: Vec<RuntimeResourceBindingV1> =
        serde_json::from_value(row.try_get("resource_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    authorize_resources(&mut tx, task.tenant_id, &resources).await?;
    let changed = sqlx::query(
        "UPDATE node_attempts SET status='running',lease_token=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),heartbeat_at=UTC_TIMESTAMP(6),fencing_token=fencing_token+1,worker_instance_id=?,started_at=COALESCE(started_at,UTC_TIMESTAMP(6)) WHERE id=? AND status='queued' AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6))",
    )
    .bind(worker_id)
    .bind(worker_id.to_string())
    .bind(task.attempt_id)
    .execute(&mut *tx)
    .await?;
    if changed.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(None);
    }
    let lease_row = sqlx::query("SELECT fencing_token,locked_until FROM node_attempts WHERE id=?")
        .bind(task.attempt_id)
        .fetch_one(&mut *tx)
        .await?;
    let fencing_token: u64 = lease_row.try_get("fencing_token")?;
    let locked_until: OffsetDateTime = lease_row.try_get("locked_until")?;
    sqlx::query(
        "INSERT INTO worker_leases(node_attempt_id,tenant_id,node_execution_id,lease_token,fencing_token,worker_instance_id,worker_id,capability,acquired_at,heartbeat_at,expires_at,operation_deadline_at,released_at) VALUES(?,?,?,?,?,?,?, ?,UTC_TIMESTAMP(6),UTC_TIMESTAMP(6),?,COALESCE(?,?),NULL) ON DUPLICATE KEY UPDATE lease_token=VALUES(lease_token),fencing_token=VALUES(fencing_token),worker_instance_id=VALUES(worker_instance_id),worker_id=VALUES(worker_id),capability=VALUES(capability),acquired_at=VALUES(acquired_at),heartbeat_at=VALUES(heartbeat_at),expires_at=VALUES(expires_at),operation_deadline_at=VALUES(operation_deadline_at),released_at=NULL,result_hash=NULL",
    )
    .bind(task.attempt_id)
    .bind(task.tenant_id)
    .bind(task.node_execution_id)
    .bind(worker_id)
    .bind(fencing_token)
    .bind(worker_id.to_string())
    .bind(worker_id)
    .bind(worker_capability)
    .bind(locked_until)
    .bind(row.try_get::<Option<OffsetDateTime>, _>("deadline_at")?)
    .bind(locked_until)
    .execute(&mut *tx)
    .await?;
    let compiled: agentx_runtime_contracts::CompiledWorkflowV1 =
        serde_json::from_value(row.try_get("compiled_ir_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let node_type: String = row.try_get("node_type")?;
    let node_id: String = row.try_get("node_id")?;
    let node_version: u32 = row.try_get("node_version")?;
    let raw_parameters = compiled
        .nodes
        .iter()
        .find(|node| {
            node.id == node_id && node.node_type == node_type && node.type_version == node_version
        })
        .map(|node| node.parameters.clone())
        .unwrap_or_else(|| json!({}));
    let inputs: BTreeMap<String, Vec<Item>> = serde_json::from_value(row.try_get("input_json")?)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let current = inputs
        .values()
        .flat_map(|items| items.iter())
        .next()
        .map(|item| item.json.clone())
        .unwrap_or(Value::Null);
    let node_parameters = ExpressionEngine
        .resolve_parameters(
            &raw_parameters,
            &ExpressionContext {
                json: current.clone(),
                input: current,
                inputs: row
                    .try_get::<Option<Value>, _>("execution_input_json")?
                    .unwrap_or(Value::Null),
                outputs: load_output_namespace(&mut tx, task.tenant_id, task.execution_id).await?,
                contexts: row.try_get("context_json")?,
                execution: json!({"id":task.execution_id}),
                ..ExpressionContext::default()
            },
        )
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    tx.commit().await?;
    Ok(Some(ClaimedWorkerAttempt {
        lease: WorkerAttemptLeaseV1 {
            protocol_version: 1,
            attempt_id: task.attempt_id,
            worker_id,
            fencing_token,
            locked_until,
        },
        task: task.clone(),
        node_type,
        node_version,
        run_index: row.try_get("run_index")?,
        iteration_index: row.try_get("iteration_index")?,
        node_parameters,
        inputs,
        resources,
        context: row.try_get("context_json")?,
    }))
}

pub async fn heartbeat_attempt(
    pool: &MySqlPool,
    lease: &WorkerAttemptLeaseV1,
) -> RuntimeResult<WorkerAttemptLeaseV1> {
    let changed = sqlx::query(
        "UPDATE node_attempts SET locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 30 SECOND),heartbeat_at=UTC_TIMESTAMP(6) WHERE id=? AND lease_token=? AND fencing_token=? AND status='running' AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(lease.attempt_id)
    .bind(lease.worker_id)
    .bind(lease.fencing_token)
    .execute(pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(lease_conflict("Attempt Lease was lost"));
    }
    let locked_until: OffsetDateTime =
        sqlx::query_scalar("SELECT locked_until FROM node_attempts WHERE id=?")
            .bind(lease.attempt_id)
            .fetch_one(pool)
            .await?;
    sqlx::query(
        "UPDATE worker_leases SET heartbeat_at=UTC_TIMESTAMP(6),expires_at=? WHERE node_attempt_id=? AND worker_id=? AND fencing_token=? AND released_at IS NULL",
    )
    .bind(locked_until)
    .bind(lease.attempt_id)
    .bind(lease.worker_id)
    .bind(lease.fencing_token)
    .execute(pool)
    .await?;
    Ok(WorkerAttemptLeaseV1 {
        locked_until,
        ..lease.clone()
    })
}

pub async fn submit_worker_result(
    pool: &MySqlPool,
    result: &WorkerResultV1,
) -> RuntimeResult<bool> {
    validate_result_hash(result)?;
    if result.output_object.is_some() {
        return Err(runtime_bad_request(
            "WORKER_RESULT_OBJECT_RESOLVER_REQUIRED",
            "external Worker Result must be submitted through the Runtime object resolver",
        ));
    }
    let encoded = agentx_runtime_contracts::canonical_bytes(&result.outputs)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    if encoded.len() > agentx_runtime_contracts::INLINE_RESULT_LIMIT_BYTES as usize {
        return Err(runtime_bad_request(
            "WORKER_RESULT_EXTERNALIZATION_REQUIRED",
            "Worker Result larger than 64 KiB must use a Runtime object",
        ));
    }
    submit_worker_result_resolved(pool, result, &result.outputs).await
}

pub async fn submit_worker_result_with_objects(
    pool: &MySqlPool,
    objects: Arc<dyn ObjectStore>,
    result: &WorkerResultV1,
) -> RuntimeResult<bool> {
    validate_result_hash(result)?;
    let outputs = resolve_worker_outputs(pool, objects, result).await?;
    submit_worker_result_resolved(pool, result, &outputs).await
}

async fn submit_worker_result_resolved(
    pool: &MySqlPool,
    result: &WorkerResultV1,
    resolved_outputs: &BTreeMap<String, Vec<Item>>,
) -> RuntimeResult<bool> {
    let mut tx = pool.begin().await?;
    if let Some(row) =
        sqlx::query("SELECT result_hash,status FROM worker_result_receipts WHERE attempt_id=?")
            .bind(result.attempt_id)
            .fetch_optional(&mut *tx)
            .await?
    {
        if row.try_get::<String, _>("result_hash")? != result.result_hash.as_str() {
            return Err(lease_conflict(
                "Attempt result was replayed with a different hash",
            ));
        }
        tx.commit().await?;
        return Ok(true);
    }
    let attempt = sqlx::query(
        "SELECT a.tenant_id,a.execution_id,a.node_execution_id,a.attempt_number,a.status,a.lease_token,a.fencing_token,COALESCE(a.locked_until>UTC_TIMESTAMP(6),FALSE) lease_active,n.node_key,COALESCE(NULLIF(n.node_name,''),n.node_key) node_name,n.run_index,e.bundle_id,e.work_package_id,e.state_version,e.invocation_id,e.input_json,s.policy_snapshot_json,s.worker_compatibility_json,s.resource_snapshot_json FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id JOIN execution_snapshots s ON s.execution_id=a.execution_id WHERE a.id=? FOR UPDATE",
    )
    .bind(result.attempt_id)
    .fetch_one(&mut *tx)
    .await?;
    ensure_result_lease(&attempt, result)?;
    let tenant_id: Uuid = attempt.try_get("tenant_id")?;
    let execution_id: Uuid = attempt.try_get("execution_id")?;
    let node_execution_id: Uuid = attempt.try_get("node_execution_id")?;
    let bundle_id: Uuid = attempt.try_get("bundle_id")?;
    let work_package_id: Option<Uuid> = attempt.try_get("work_package_id")?;
    let state = sqlx::query(
        "SELECT state_version,context_version,context_json,machine_state_json FROM execution_runtime_state WHERE tenant_id=? AND execution_id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_one(&mut *tx)
    .await?;
    let current_version: u64 = state.try_get("state_version")?;
    if attempt.try_get::<u64, _>("state_version")? != current_version {
        return Err(lease_conflict("Execution state version changed"));
    }
    let mut machine: ExecutionMachine =
        serde_json::from_value(state.try_get("machine_state_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let node_execution_id = NodeExecutionId::from_uuid(node_execution_id);
    let mut context: Value = state.try_get("context_json")?;
    let mut context_version: u64 = state.try_get("context_version")?;
    let mut effective_status = result.status;
    let mut effective_error_code = result.error_code.clone();
    let mut effective_error_message = result.error_message.clone();
    let mut effective_outputs = resolved_outputs.clone();
    if result.status == WorkerResultStatusV1::Succeeded {
        let activation = machine
            .activation(node_execution_id)
            .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
        let node = &machine.workflow().nodes[activation.node_index];
        if node
            .output_projection
            .as_object()
            .is_some_and(|projection| !projection.is_empty())
        {
            let upstream = load_output_namespace(&mut tx, tenant_id, execution_id).await?;
            crate::output_projection::apply(
                &mut effective_outputs,
                &node.output_projection,
                &ExpressionContext {
                    inputs: attempt
                        .try_get::<Option<Value>, _>("input_json")?
                        .unwrap_or(Value::Null),
                    outputs: upstream,
                    contexts: context.clone(),
                    execution: json!({
                        "executionId":execution_id,
                        "nodeExecutionId":node_execution_id.as_uuid(),
                    }),
                    ..ExpressionContext::default()
                },
            )?;
        }
    }
    match result.status {
        WorkerResultStatusV1::Succeeded => {
            match apply_context_writes(
                &mut tx,
                ContextWriteRequest {
                    tenant_id,
                    execution_id,
                    node_execution_id,
                    attempt_id: result.attempt_id,
                    invocation_id: attempt.try_get("invocation_id")?,
                    input: attempt
                        .try_get::<Option<Value>, _>("input_json")?
                        .unwrap_or(Value::Null),
                    machine: &machine,
                    current_context: &context,
                    context_version,
                    resolved_outputs: &effective_outputs,
                },
            )
            .await?
            {
                ContextWriteOutcome::Applied {
                    context: updated,
                    version,
                } => {
                    context = updated;
                    context_version = version;
                    machine
                        .complete(node_execution_id, effective_outputs.clone())
                        .map_err(machine_error)?;
                }
                ContextWriteOutcome::SessionConflict => {
                    effective_status = WorkerResultStatusV1::Failed;
                    effective_error_code = Some("SESSION_CONTEXT_VERSION_CONFLICT".into());
                    effective_error_message =
                        Some("Session Context changed while the node was running".into());
                    machine
                        .fail(
                            node_execution_id,
                            "SESSION_CONTEXT_VERSION_CONFLICT",
                            "Session Context changed while the node was running",
                            true,
                        )
                        .map_err(machine_error)?;
                }
            }
        }
        WorkerResultStatusV1::Suspended => {
            machine.suspend(node_execution_id).map_err(machine_error)?;
        }
        WorkerResultStatusV1::Failed | WorkerResultStatusV1::OutcomeUnknown => machine
            .fail(
                node_execution_id,
                result
                    .error_code
                    .as_deref()
                    .unwrap_or("WORKER_EXECUTION_FAILED"),
                result
                    .error_message
                    .as_deref()
                    .unwrap_or("Worker execution failed"),
                result.status == WorkerResultStatusV1::Failed,
            )
            .map_err(machine_error)?,
        WorkerResultStatusV1::Cancelled => machine.cancel(),
    }
    let will_retry = effective_status == WorkerResultStatusV1::Failed
        && machine
            .activation(node_execution_id)
            .is_some_and(|activation| activation.status == ActivationStatus::Ready);
    let db_status = worker_result_status(effective_status);
    sqlx::query(
        "UPDATE node_attempts SET status=?,output_json=?,result_hash=?,result_object_id=?,outcome_unknown=?,error_code=?,error_message=?,ended_at=UTC_TIMESTAMP(6),locked_until=NULL,heartbeat_at=NULL WHERE id=?",
    )
    .bind(db_status)
    .bind(
        result
            .output_object
            .is_none()
            .then(|| serde_json::to_value(&effective_outputs))
            .transpose()
            .map_err(|error| RuntimeError::Internal(error.into()))?,
    )
    .bind(result.result_hash.as_str())
    .bind(result.output_object.as_ref().map(|object| object.object_id))
    .bind(result.status == WorkerResultStatusV1::OutcomeUnknown)
    .bind(&effective_error_code)
    .bind(&effective_error_message)
    .bind(result.attempt_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE worker_leases SET released_at=UTC_TIMESTAMP(6),result_hash=? WHERE node_attempt_id=? AND worker_id=? AND fencing_token=?",
    )
    .bind(result.result_hash.as_str())
    .bind(result.attempt_id)
    .bind(result.worker_id)
    .bind(result.fencing_token)
    .execute(&mut *tx)
    .await?;
    crate::quota::settle_attempt_usage(&mut tx, tenant_id, result.attempt_id, &effective_outputs)
        .await?;
    crate::quota::release_attempt(&mut tx, tenant_id, result.attempt_id, "attempt_terminal")
        .await?;
    sqlx::query(
        "UPDATE node_executions SET status=?,output_json=?,error_code=?,error_message=?,ended_at=IF(?='ready',NULL,UTC_TIMESTAMP(6)) WHERE id=?",
    )
    .bind(activation_status(
        machine
            .activation(node_execution_id)
            .map(|activation| activation.status)
            .unwrap_or(ActivationStatus::Failed),
    ))
    .bind(
        result
            .output_object
            .is_none()
            .then(|| serde_json::to_value(&effective_outputs))
            .transpose()
            .map_err(|error| RuntimeError::Internal(error.into()))?,
    )
    .bind(&effective_error_code)
    .bind(&effective_error_message)
    .bind(activation_status(
        machine
            .activation(node_execution_id)
            .map(|activation| activation.status)
            .unwrap_or(ActivationStatus::Failed),
    ))
    .bind(node_execution_id.as_uuid())
    .execute(&mut *tx)
    .await?;
    let output_preview = result
        .output_object
        .is_none()
        .then(|| serde_json::to_value(&effective_outputs).ok())
        .flatten()
        .and_then(|value| crate::trace_delivery::bounded_preview(&value));
    let attempt_event_kind = if effective_status == WorkerResultStatusV1::Suspended {
        agentx_runtime_contracts::TraceEventKindV1::Updated
    } else {
        agentx_runtime_contracts::TraceEventKindV1::Finished
    };
    let mut attempt_trace = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        result.attempt_id,
        Some((
            node_execution_id.as_uuid(),
            agentx_runtime_contracts::TraceSpanKindV1::Node,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Attempt,
        format!("Attempt {}", attempt.try_get::<u32, _>("attempt_number")?),
        attempt_event_kind,
        node_event_type(effective_status),
        db_status,
    );
    attempt_trace.node_execution_id = Some(node_execution_id.as_uuid());
    attempt_trace.attempt_id = Some(result.attempt_id);
    attempt_trace.error_code = effective_error_code.clone();
    attempt_trace.error_message = effective_error_message.clone();
    attempt_trace.content_ref = result.output_object.as_ref().map(|object| object.object_id);
    attempt_trace.content_role = Some("output".into());
    attempt_trace.content_preview = output_preview.clone();
    crate::trace_delivery::enqueue_best_effort(&mut tx, attempt_trace).await;
    let mut node_trace = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        node_execution_id.as_uuid(),
        Some((
            execution_id,
            agentx_runtime_contracts::TraceSpanKindV1::Execution,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Node,
        attempt.try_get::<String, _>("node_name")?,
        if will_retry {
            agentx_runtime_contracts::TraceEventKindV1::Updated
        } else {
            attempt_event_kind
        },
        if will_retry {
            "node.retry_scheduled"
        } else {
            node_event_type(effective_status)
        },
        if will_retry { "retrying" } else { db_status },
    );
    node_trace.node_execution_id = Some(node_execution_id.as_uuid());
    node_trace.attempt_id = Some(result.attempt_id);
    node_trace.error_code = effective_error_code;
    node_trace.error_message = effective_error_message;
    node_trace.content_ref = result.output_object.as_ref().map(|object| object.object_id);
    node_trace.content_role = Some("output".into());
    node_trace.content_preview = output_preview;
    crate::trace_delivery::enqueue_best_effort(&mut tx, node_trace).await;
    let next_version = current_version + 1;
    let policy: agentx_runtime_contracts::RuntimePolicyV1 =
        serde_json::from_value(attempt.try_get("policy_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let compatibility: agentx_runtime_contracts::WorkerCompatibilityV1 =
        serde_json::from_value(attempt.try_get("worker_compatibility_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    let resources: Vec<RuntimeResourceBindingV1> =
        serde_json::from_value(attempt.try_get("resource_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    schedule_ready(
        &mut tx,
        ReadySchedule {
            tenant_id,
            execution_id,
            bundle_id,
            work_package_id,
            policy: &policy,
            compatibility: &compatibility,
            resources: &resources,
            context: &context,
            machine: &mut machine,
            state_version: next_version,
        },
    )
    .await?;
    persist_machine(
        &mut tx,
        PersistMachineRequest {
            tenant_id,
            execution_id,
            bundle_id,
            work_package_id,
            state_version: next_version,
            context_version,
            context: &context,
            machine: &machine,
            checkpoint_type: if machine.status() == RuntimeExecutionStatus::Waiting {
                "node_suspended"
            } else {
                "node_completed"
            },
        },
    )
    .await?;
    sqlx::query(
        "INSERT INTO worker_result_receipts(attempt_id,tenant_id,worker_id,fencing_token,result_hash,status,response_json) VALUES(?,?,?,?,?,'accepted',?)",
    )
    .bind(result.attempt_id)
    .bind(tenant_id)
    .bind(result.worker_id)
    .bind(result.fencing_token)
    .bind(result.result_hash.as_str())
    .bind(json!({"executionId":execution_id,"stateVersion":next_version}))
    .execute(&mut *tx)
    .await?;
    let current_machine_status = machine.status();
    if is_terminal(current_machine_status) {
        finish_execution(
            &mut tx,
            tenant_id,
            execution_id,
            work_package_id,
            attempt.try_get("invocation_id")?,
            next_version,
            &machine,
            &context,
        )
        .await?;
    } else {
        let changed = sqlx::query(
            "UPDATE workflow_executions SET status=?,state_version=? WHERE tenant_id=? AND id=? AND state_version=?",
        )
        .bind(machine_status(current_machine_status))
        .bind(next_version)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(current_version)
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(lease_conflict("Execution state CAS failed"));
        }
    }
    tx.commit().await?;
    Ok(false)
}

enum ContextWriteOutcome {
    Applied { context: Value, version: u64 },
    SessionConflict,
}

struct ContextWriteRequest<'a> {
    tenant_id: Uuid,
    execution_id: Uuid,
    node_execution_id: NodeExecutionId,
    attempt_id: Uuid,
    invocation_id: Option<Uuid>,
    input: Value,
    machine: &'a ExecutionMachine,
    current_context: &'a Value,
    context_version: u64,
    resolved_outputs: &'a BTreeMap<String, Vec<Item>>,
}

async fn apply_context_writes(
    tx: &mut Transaction<'_, MySql>,
    request: ContextWriteRequest<'_>,
) -> RuntimeResult<ContextWriteOutcome> {
    let ContextWriteRequest {
        tenant_id,
        execution_id,
        node_execution_id,
        attempt_id,
        invocation_id,
        input,
        machine,
        current_context,
        context_version,
        resolved_outputs,
    } = request;
    let activation = machine
        .activation(node_execution_id)
        .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
    let node = &machine.workflow().nodes[activation.node_index];
    if node.context_writes.is_empty() {
        return Ok(ContextWriteOutcome::Applied {
            context: current_context.clone(),
            version: context_version,
        });
    }
    let mut outputs = load_output_namespace(tx, tenant_id, execution_id).await?;
    if let Some(namespace) = outputs.as_object_mut() {
        let current = serde_json::to_value(resolved_outputs)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
        merge_output_namespace(namespace, &node.key, activation.run_index, &current);
    }
    let current_item = resolved_outputs
        .values()
        .flat_map(|items| items.iter())
        .next()
        .map(|item| item.json.clone())
        .unwrap_or(Value::Null);
    let expression_context = ExpressionContext {
        json: current_item,
        inputs: input,
        outputs,
        contexts: current_context.clone(),
        execution: json!({"id":execution_id}),
        ..ExpressionContext::default()
    };
    let mut updated = current_context.clone();
    let mut patches = Vec::with_capacity(node.context_writes.len());
    let mut session_writes = 0_u64;
    for write in &node.context_writes {
        let value = ExpressionEngine
            .resolve_parameters(&write.value, &expression_context)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
        let before = context_value(&updated, &write.path).cloned();
        apply_context_write(&mut updated, &write.path, write.operation, value.clone())?;
        let root = write.path.split('.').next().unwrap_or_default();
        let session_scoped = machine
            .workflow()
            .contexts
            .get(root)
            .is_some_and(|definition| definition.scope == ContextScope::Session);
        if session_scoped {
            session_writes = session_writes.saturating_add(1);
        }
        patches.push((write, value, before, session_scoped));
    }
    let next_context_version = context_version.saturating_add(session_writes);
    if session_writes > 0
        && let Some(invocation_id) = invocation_id
    {
        let session = sqlx::query("SELECT s.id,s.application_deployment_id FROM application_invocations i JOIN application_sessions s ON s.tenant_id=i.tenant_id AND s.id=i.session_id WHERE i.tenant_id=? AND i.id=?")
            .bind(tenant_id).bind(invocation_id).fetch_optional(&mut **tx).await?;
        if let Some(session) = session {
            let session_context = Value::Object(
                machine
                    .workflow()
                    .contexts
                    .iter()
                    .filter(|(_, definition)| definition.scope == ContextScope::Session)
                    .filter_map(|(name, _)| {
                        updated
                            .get(name)
                            .cloned()
                            .map(|value| (name.clone(), value))
                    })
                    .collect(),
            );
            let changed = sqlx::query("UPDATE application_session_contexts SET context_json=?,context_version=? WHERE tenant_id=? AND application_deployment_id=? AND session_id=? AND context_version=?")
                .bind(session_context).bind(next_context_version).bind(tenant_id)
                .bind(session.try_get::<Uuid,_>("application_deployment_id")?)
                .bind(session.try_get::<Uuid,_>("id")?).bind(context_version)
                .execute(&mut **tx).await?;
            if changed.rows_affected() != 1 {
                return Ok(ContextWriteOutcome::SessionConflict);
            }
        }
    }
    let mut patch_version = context_version;
    for (index, (write, value, before, session_scoped)) in patches.into_iter().enumerate() {
        let version_before = patch_version;
        if session_scoped {
            patch_version = patch_version.saturating_add(1);
        }
        sqlx::query("INSERT INTO workflow_context_patches(id,tenant_id,execution_id,node_execution_id,attempt_id,patch_index,operation_key,context_path,value_json,context_version_before,context_version_after) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
            .bind(attempt_id).bind(index as u32).bind(context_operation_name(write.operation))
            .bind(&write.path).bind(json!({"before":before,"value":value}))
            .bind(version_before).bind(patch_version).execute(&mut **tx).await?;
    }
    Ok(ContextWriteOutcome::Applied {
        context: updated,
        version: next_context_version,
    })
}

struct ReadySchedule<'a> {
    tenant_id: Uuid,
    execution_id: Uuid,
    bundle_id: Uuid,
    work_package_id: Option<Uuid>,
    policy: &'a agentx_runtime_contracts::RuntimePolicyV1,
    compatibility: &'a agentx_runtime_contracts::WorkerCompatibilityV1,
    resources: &'a [RuntimeResourceBindingV1],
    context: &'a Value,
    machine: &'a mut ExecutionMachine,
    state_version: u64,
}

async fn schedule_ready(
    tx: &mut Transaction<'_, MySql>,
    ready: ReadySchedule<'_>,
) -> RuntimeResult<()> {
    let runtime_settings: Value = sqlx::query_scalar(
        "SELECT runtime_settings_json FROM execution_snapshots WHERE tenant_id=? AND execution_id=?",
    )
    .bind(ready.tenant_id)
    .bind(ready.execution_id)
    .fetch_one(&mut **tx)
    .await?;
    while let Some(node_execution_id) = ready.machine.next_ready() {
        let activation = ready
            .machine
            .activation(node_execution_id)
            .cloned()
            .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
        let node = ready.machine.workflow().nodes[activation.node_index].clone();
        let overlay = crate::debug_overlay::for_node(&runtime_settings, &node.id);
        if let Some(overlay) = overlay
            .as_ref()
            .filter(|overlay| overlay.kind == "temporary_input")
        {
            ready
                .machine
                .replace_ready_inputs(
                    node_execution_id,
                    crate::debug_overlay::items(&overlay.payload),
                )
                .map_err(machine_error)?;
        }
        let attempt_id = ready
            .machine
            .start_attempt(node_execution_id)
            .map_err(machine_error)?;
        if matches!(
            node.execution_style,
            ExecutionStyle::Suspend | ExecutionStyle::SubWorkflow
        ) {
            ready
                .machine
                .suspend(node_execution_id)
                .map_err(machine_error)?;
        }
        let activation = ready
            .machine
            .activation(node_execution_id)
            .cloned()
            .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
        let attempt_number = activation
            .attempts
            .last()
            .map(|attempt| attempt.attempt_number)
            .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("attempt disappeared")))?;
        let retry_delay_ms = if attempt_number > 1 {
            node.settings.wait_between_tries_ms
        } else {
            0
        };
        let reservation_seconds = ready
            .policy
            .operation_deadline_seconds
            .saturating_add(u32::try_from(retry_delay_ms.div_ceil(1_000)).unwrap_or(u32::MAX));
        let available_at = OffsetDateTime::now_utc()
            + Duration::milliseconds(i64::try_from(retry_delay_ms).unwrap_or(i64::MAX));
        let deadline_at =
            available_at + Duration::seconds(i64::from(ready.policy.operation_deadline_seconds));
        upsert_activation(tx, ready.tenant_id, ready.execution_id, &activation, &node).await?;
        let input_json = serde_json::to_value(&activation.inputs)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
        let manifest_version = agentx_node_protocol::NODE_PROTOCOL_VERSION;
        let overlay_completes = overlay
            .as_ref()
            .is_some_and(|overlay| crate::debug_overlay::completes_node(&overlay.kind));
        sqlx::query(
            "INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,worker_protocol_version,ir_schema_version,compiler_version,manifest_version,status,idempotency_key,deadline_at,input_json) VALUES(?,?,?,?,?,?,1,1,?,? ,?,?,?,?)",
        )
        .bind(attempt_id.as_uuid())
        .bind(ready.tenant_id)
        .bind(ready.execution_id)
        .bind(node_execution_id.as_uuid())
        .bind(attempt_number)
        .bind(node.capability.as_str())
        .bind(&ready.machine.workflow().compiler_version)
        .bind(manifest_version)
        .bind(if overlay_completes { "succeeded" } else if matches!(node.execution_style, ExecutionStyle::Suspend | ExecutionStyle::SubWorkflow) { "suspended" } else { "queued" })
        .bind(format!("{}:{node_execution_id}:{attempt_number}", ready.execution_id))
        .bind(deadline_at)
        .bind(&input_json)
        .execute(&mut **tx)
        .await?;
        crate::engine_trace::start_node_attempt_spans(
            tx,
            ready.tenant_id,
            ready.execution_id,
            node_execution_id.as_uuid(),
            attempt_id.as_uuid(),
            &node.name,
            &input_json,
            attempt_number,
            overlay_completes,
        )
        .await;
        if let Some(overlay) = overlay.as_ref().filter(|_| overlay_completes) {
            let outputs = crate::debug_overlay::items(&overlay.payload);
            ready
                .machine
                .complete(node_execution_id, outputs.clone())
                .map_err(machine_error)?;
            sqlx::query(
                "UPDATE node_attempts SET output_json=?,ended_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status='succeeded'",
            )
            .bind(serde_json::to_value(&outputs).map_err(|error| RuntimeError::Internal(error.into()))?)
            .bind(ready.tenant_id)
            .bind(attempt_id.as_uuid())
            .execute(&mut **tx)
            .await?;
            let completed = ready
                .machine
                .activation(node_execution_id)
                .cloned()
                .ok_or_else(|| RuntimeError::Internal(anyhow::anyhow!("activation disappeared")))?;
            upsert_activation(tx, ready.tenant_id, ready.execution_id, &completed, &node).await?;
            let mut trace = crate::trace_delivery::TraceDraft::execution(
                ready.tenant_id,
                ready.execution_id,
                "node.debug_overlay_applied",
                "succeeded",
            );
            trace.node_execution_id = Some(node_execution_id.as_uuid());
            trace.attempt_id = Some(attempt_id.as_uuid());
            trace.attributes = json!({"nodeId":node.id,"overlayKind":overlay.kind});
            crate::trace_delivery::enqueue_best_effort(tx, trace).await;
            continue;
        }
        if node.execution_style == ExecutionStyle::Suspend {
            crate::suspension::create(
                tx,
                ready.tenant_id,
                ready.execution_id,
                ready.bundle_id,
                ready.work_package_id,
                node_execution_id,
                ready.state_version,
                &node,
                ready.machine,
                ready.context,
            )
            .await?;
            continue;
        }
        if node.side_effect_level != SideEffectLevel::None {
            match crate::fork_runtime::side_effect_decision(
                tx,
                ready.tenant_id,
                ready.execution_id,
                &node.key,
            )
            .await?
            {
                crate::fork_runtime::SideEffectDecision::Execute => {}
                crate::fork_runtime::SideEffectDecision::Complete(outputs) => {
                    ready
                        .machine
                        .complete(node_execution_id, outputs.clone())
                        .map_err(machine_error)?;
                    sqlx::query(
                        "UPDATE node_attempts SET status='succeeded',output_json=?,ended_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status='queued'",
                    )
                    .bind(serde_json::to_value(&outputs).map_err(|error| RuntimeError::Internal(error.into()))?)
                    .bind(ready.tenant_id)
                    .bind(attempt_id.as_uuid())
                    .execute(&mut **tx)
                    .await?;
                    let completed = ready
                        .machine
                        .activation(node_execution_id)
                        .cloned()
                        .ok_or_else(|| {
                            RuntimeError::Internal(anyhow::anyhow!(
                                "completed fork activation disappeared"
                            ))
                        })?;
                    upsert_activation(tx, ready.tenant_id, ready.execution_id, &completed, &node)
                        .await?;
                    continue;
                }
            }
        }
        if node.execution_style == ExecutionStyle::SubWorkflow {
            crate::composite_execution::create_child(
                tx,
                ready.tenant_id,
                ready.execution_id,
                ready.bundle_id,
                ready.work_package_id,
                node_execution_id,
                &activation,
                &node,
            )
            .await?;
            continue;
        }
        let compatibility_hash = agentx_runtime_contracts::content_hash(ready.compatibility)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
        let task = WorkerTaskV1 {
            protocol_version: 1,
            task_id: Uuid::now_v7(),
            tenant_id: ready.tenant_id,
            execution_id: ready.execution_id,
            node_execution_id: node_execution_id.as_uuid(),
            attempt_id: attempt_id.as_uuid(),
            capability: node.capability.clone(),
            bundle_id: ready.bundle_id,
            work_package_id: ready.work_package_id,
            state_version: ready.state_version,
            compatibility_hash,
            deadline_at,
        };
        crate::quota::reserve(
            tx,
            ready.tenant_id,
            "node_concurrency",
            "attempt",
            &attempt_id.to_string(),
            &format!("attempt:{attempt_id}:node_concurrency"),
            1,
            reservation_seconds,
        )
        .await?;
        if node.capability == NodeCapability::Sandbox {
            crate::quota::reserve(
                tx,
                ready.tenant_id,
                "sandbox_concurrency",
                "attempt",
                &attempt_id.to_string(),
                &format!("attempt:{attempt_id}:sandbox_concurrency"),
                1,
                reservation_seconds,
            )
            .await?;
        }
        crate::quota::reserve_attempt_budget(
            tx,
            ready.tenant_id,
            reservation_seconds,
            &node,
            attempt_id.as_uuid(),
            ready.resources,
        )
        .await?;
        let task_json =
            serde_json::to_value(&task).map_err(|error| RuntimeError::Internal(error.into()))?;
        let task_hash = agentx_runtime_contracts::content_hash(&task)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
        sqlx::query(
            "INSERT INTO execution_outbox(id,tenant_id,execution_id,node_execution_id,attempt_id,message_type,capability,payload_json,task_hash,status,available_at) VALUES(?,?,?,?,?,'dispatch_node',?,?,?,'pending',?)",
        )
        .bind(Uuid::now_v7())
        .bind(ready.tenant_id)
        .bind(ready.execution_id)
        .bind(node_execution_id.as_uuid())
        .bind(attempt_id.as_uuid())
        .bind(node.capability.as_str())
        .bind(task_json)
        .bind(task_hash.as_str())
        .bind(available_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
struct PersistMachineRequest<'a> {
    tenant_id: Uuid,
    execution_id: Uuid,
    bundle_id: Uuid,
    work_package_id: Option<Uuid>,
    state_version: u64,
    context_version: u64,
    context: &'a Value,
    machine: &'a ExecutionMachine,
    checkpoint_type: &'a str,
}

async fn persist_machine(
    tx: &mut Transaction<'_, MySql>,
    request: PersistMachineRequest<'_>,
) -> RuntimeResult<()> {
    let PersistMachineRequest {
        tenant_id,
        execution_id,
        bundle_id,
        work_package_id,
        state_version,
        context_version,
        context,
        machine,
        checkpoint_type,
    } = request;
    for activation in machine.activations() {
        let node = &machine.workflow().nodes[activation.node_index];
        upsert_activation(tx, tenant_id, execution_id, activation, node).await?;
    }
    for delivery in machine.deliveries() {
        let connection = &machine.workflow().connections[delivery.connection_index];
        let (kind, items) = match &delivery.kind {
            agentx_runtime::DeliveryKind::Data(items) => ("data", Some(items)),
            agentx_runtime::DeliveryKind::ClosedWithoutData => ("closed_without_data", None),
        };
        let inserted = sqlx::query(
            "INSERT IGNORE INTO execution_edge_deliveries(id,tenant_id,execution_id,sequence_number,connection_id,source_node_execution_id,source_port,target_node_id,target_port,target_generation,delivery_kind,item_count,payload_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(delivery.id)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(delivery.sequence)
        .bind(&connection.id)
        .bind(delivery.source_node_execution_id.as_uuid())
        .bind(&connection.source_port)
        .bind(&machine.workflow().nodes[connection.target_node].id)
        .bind(&connection.target_port)
        .bind(delivery.target_generation)
        .bind(kind)
        .bind(items.map_or(0, Vec::len) as u32)
        .bind(items.map(serde_json::to_value).transpose().map_err(|error| RuntimeError::Internal(error.into()))?)
        .execute(&mut **tx)
        .await?
        .rows_affected()
            == 1;
        if inserted && let Some(items) = items {
            persist_lineage(tx, tenant_id, execution_id, delivery.id, items).await?;
        }
    }
    for delivery in machine.end_deliveries() {
        sqlx::query(
            "INSERT IGNORE INTO execution_end_deliveries(tenant_id,execution_id,sequence_number,source_node_execution_id,source_node_id,source_port,target_port,payload_json) VALUES(?,?,?,?,?,?,?,?)",
        )
        .bind(tenant_id)
        .bind(execution_id)
        .bind(delivery.sequence)
        .bind(delivery.source_node_execution_id.as_uuid())
        .bind(&machine.workflow().nodes[delivery.source_node].id)
        .bind(&delivery.source_port)
        .bind(&delivery.target_port)
        .bind(serde_json::to_value(&delivery.items).map_err(|error| RuntimeError::Internal(error.into()))?)
        .execute(&mut **tx)
        .await?;
    }
    let machine_json =
        serde_json::to_value(machine).map_err(|error| RuntimeError::Internal(error.into()))?;
    let machine_hash = agentx_runtime_contracts::content_hash(&machine_json)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let frontier = machine
        .activations()
        .filter(|activation| {
            matches!(
                activation.status,
                ActivationStatus::Ready | ActivationStatus::Running | ActivationStatus::Waiting
            )
        })
        .map(|activation| activation.id.as_uuid())
        .collect::<Vec<_>>();
    let activation_count = machine.activations().count() as u64;
    let delivery_sequence = machine
        .deliveries()
        .iter()
        .map(|delivery| delivery.sequence)
        .chain(
            machine
                .end_deliveries()
                .iter()
                .map(|delivery| delivery.sequence),
        )
        .max()
        .unwrap_or(0);
    let frontier_json =
        serde_json::to_value(frontier).map_err(|error| RuntimeError::Internal(error.into()))?;
    let current_version: Option<u64> = sqlx::query_scalar(
        "SELECT state_version FROM execution_runtime_state WHERE tenant_id=? AND execution_id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(current_version) = current_version {
        if current_version.checked_add(1) != Some(state_version) {
            return Err(lease_conflict("Execution Runtime state CAS failed"));
        }
        let changed = sqlx::query(
            "UPDATE execution_runtime_state SET state_version=?,context_version=?,delivery_sequence=?,activation_count=?,activation_budget=?,current_frontier_json=?,context_json=?,machine_state_json=?,machine_state_hash=? WHERE tenant_id=? AND execution_id=? AND state_version=?",
        )
        .bind(state_version)
        .bind(context_version)
        .bind(delivery_sequence)
        .bind(activation_count)
        .bind(machine.workflow().activation_budget)
        .bind(&frontier_json)
        .bind(context)
        .bind(&machine_json)
        .bind(machine_hash.as_str())
        .bind(tenant_id)
        .bind(execution_id)
        .bind(current_version)
        .execute(&mut **tx)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(lease_conflict("Execution Runtime state CAS failed"));
        }
    } else {
        sqlx::query(
            "INSERT INTO execution_runtime_state(execution_id,tenant_id,state_version,context_version,delivery_sequence,activation_count,activation_budget,current_frontier_json,context_json,machine_state_json,machine_state_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(execution_id)
        .bind(tenant_id)
        .bind(state_version)
        .bind(context_version)
        .bind(delivery_sequence)
        .bind(activation_count)
        .bind(machine.workflow().activation_budget)
        .bind(&frontier_json)
        .bind(context)
        .bind(&machine_json)
        .bind(machine_hash.as_str())
        .execute(&mut **tx)
        .await?;
    }
    persist_checkpoint(
        tx,
        tenant_id,
        execution_id,
        bundle_id,
        work_package_id,
        state_version,
        None,
        machine,
        context,
        checkpoint_type,
    )
    .await?;
    Ok(())
}
