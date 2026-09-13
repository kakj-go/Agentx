use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_runtime::{ExecutionMachine, PartialExecutionMode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use uuid::Uuid;

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckpointPayloadV1 {
    pub schema_version: u32,
    pub machine: ExecutionMachine,
    pub context: Value,
}

pub(crate) fn checkpoint_payload(
    machine: &ExecutionMachine,
    context: &Value,
) -> CheckpointPayloadV1 {
    CheckpointPayloadV1 {
        schema_version: 1,
        machine: machine.clone(),
        context: context.clone(),
    }
}

pub(crate) async fn machine_from_checkpoint(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    fork_execution_id: Uuid,
    input: &Value,
    external_payload: Option<CheckpointPayloadV1>,
) -> RuntimeResult<Option<(ExecutionMachine, Value)>> {
    let Some(row) = sqlx::query(
        "SELECT f.mode,f.node_id,c.payload_json,c.payload_artifact_id FROM execution_forks f JOIN checkpoints c ON c.tenant_id=f.tenant_id AND c.id=f.source_checkpoint_id WHERE f.tenant_id=? AND f.fork_execution_id=?",
    )
    .bind(tenant_id)
    .bind(fork_execution_id)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };
    let payload: CheckpointPayloadV1 = if row
        .try_get::<Option<Uuid>, _>("payload_artifact_id")?
        .is_some()
    {
        external_payload.ok_or_else(|| {
            invalid(
                "CHECKPOINT_OBJECT_REQUIRED",
                "External Checkpoint payload must be resolved before the startup transaction",
            )
        })?
    } else {
        serde_json::from_value(
            row.try_get::<Option<Value>, _>("payload_json")?
                .ok_or_else(|| {
                    invalid(
                        "CHECKPOINT_PAYLOAD_MISSING",
                        "Fork Checkpoint has no payload",
                    )
                })?,
        )
        .map_err(|error| invalid("CHECKPOINT_PAYLOAD_INVALID", &error.to_string()))?
    };
    if payload.schema_version != 1 {
        return Err(invalid(
            "CHECKPOINT_VERSION_UNSUPPORTED",
            "Only Checkpoint payload v1 is supported",
        ));
    }
    let mode = match row.try_get::<String, _>("mode")?.as_str() {
        "whole" => PartialExecutionMode::Whole,
        "node" => PartialExecutionMode::Node,
        "to_node" => PartialExecutionMode::ToNode,
        "from_node" => PartialExecutionMode::FromNode,
        _ => return Err(invalid("FORK_MODE_INVALID", "Fork mode is not supported")),
    };
    let machine = payload
        .machine
        .fork_from_checkpoint(
            mode,
            row.try_get::<Option<String>, _>("node_id")?.as_deref(),
            vec![Item {
                json: input.clone(),
                ..Item::default()
            }],
            &json!({}),
        )
        .map_err(|error| invalid("FORK_CHECKPOINT_INVALID", &error.to_string()))?;
    Ok(Some((machine, payload.context)))
}

pub(crate) async fn load_external_checkpoint(
    state: &crate::RuntimeState,
    tenant_id: Uuid,
    fork_execution_id: Uuid,
) -> RuntimeResult<Option<CheckpointPayloadV1>> {
    let Some(row) = sqlx::query(
        "SELECT c.payload_artifact_id,c.payload_hash FROM execution_forks f JOIN checkpoints c ON c.tenant_id=f.tenant_id AND c.id=f.source_checkpoint_id WHERE f.tenant_id=? AND f.fork_execution_id=?",
    )
    .bind(tenant_id)
    .bind(fork_execution_id)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Ok(None);
    };
    let Some(artifact_id) = row.try_get::<Option<Uuid>, _>("payload_artifact_id")? else {
        return Ok(None);
    };
    let expected_hash: String = row.try_get("payload_hash")?;
    let bytes =
        crate::artifact::load_artifact(state, tenant_id, artifact_id, &expected_hash).await?;
    let payload = serde_json::from_slice(&bytes)
        .map_err(|error| invalid("CHECKPOINT_PAYLOAD_INVALID", &error.to_string()))?;
    Ok(Some(payload))
}

pub(crate) enum SideEffectDecision {
    Execute,
    Complete(BTreeMap<String, Vec<Item>>),
}

pub(crate) async fn side_effect_decision(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    fork_execution_id: Uuid,
    node_key: &str,
) -> RuntimeResult<SideEffectDecision> {
    let Some(row) = sqlx::query(
        "SELECT source_execution_id,side_effect_resolution FROM execution_forks WHERE tenant_id=? AND fork_execution_id=?",
    )
    .bind(tenant_id)
    .bind(fork_execution_id)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(SideEffectDecision::Execute);
    };
    match row.try_get::<String, _>("side_effect_resolution")?.as_str() {
        "execute" => Ok(SideEffectDecision::Execute),
        "dry_run" => {
            checkpoint_side_effect_output(
                tx,
                tenant_id,
                fork_execution_id,
                node_key,
                "FORK_DRY_RUN_OUTPUT_MISSING",
            )
            .await
        }
        "reuse_output" => {
            checkpoint_side_effect_output(
                tx,
                tenant_id,
                fork_execution_id,
                node_key,
                "FORK_REUSE_OUTPUT_MISSING",
            )
            .await
        }
        _ => Err(invalid(
            "FORK_SIDE_EFFECT_RESOLUTION_INVALID",
            "Fork side-effect resolution is not supported",
        )),
    }
}

async fn checkpoint_side_effect_output(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    fork_execution_id: Uuid,
    node_key: &str,
    missing_code: &str,
) -> RuntimeResult<SideEffectDecision> {
    let output: Option<Value> = sqlx::query_scalar(
        "SELECT n.output_json FROM execution_forks f JOIN checkpoints c ON c.tenant_id=f.tenant_id AND c.id=f.source_checkpoint_id JOIN node_executions n ON n.tenant_id=f.tenant_id AND n.execution_id=f.source_execution_id AND n.node_key=? AND n.status='succeeded' AND n.output_json IS NOT NULL AND n.updated_at<=c.created_at WHERE f.tenant_id=? AND f.fork_execution_id=? ORDER BY n.run_index DESC,n.ended_at DESC LIMIT 1",
    )
    .bind(node_key)
    .bind(tenant_id)
    .bind(fork_execution_id)
    .fetch_optional(&mut **tx)
    .await?;
    let output = output.ok_or_else(|| {
        invalid(
            missing_code,
            "Source Checkpoint has no successful output for the side-effect node",
        )
    })?;
    serde_json::from_value(output)
        .map(SideEffectDecision::Complete)
        .map_err(|error| invalid("FORK_SIDE_EFFECT_OUTPUT_INVALID", &error.to_string()))
}

fn invalid(code: &str, message: &str) -> RuntimeError {
    RuntimeError::BadRequest(
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::BundleReferenceConflict,
        format!("{code}: {message}"),
    )
}
