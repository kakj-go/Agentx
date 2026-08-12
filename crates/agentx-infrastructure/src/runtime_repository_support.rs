use std::collections::BTreeMap;

use agentx_application::RuntimeResourceSnapshot;
use agentx_domain::{ResourceReference, WorkflowDefinition};
use agentx_node_protocol::{Item, NodeCapability, SideEffectLevel};
use agentx_runtime::{ActivationStatus, AttemptStatus, RuntimeExecutionStatus};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::runtime_repository::TaskResult;

pub(crate) fn invocation_items(input: &Value) -> Vec<Item> {
    match input {
        Value::Array(values) => values
            .iter()
            .cloned()
            .map(|json| Item {
                json,
                ..Item::default()
            })
            .collect(),
        value => vec![Item {
            json: value.clone(),
            ..Item::default()
        }],
    }
}

pub(crate) fn is_subworkflow_type(node_type: &str) -> bool {
    node_type == "sub_workflow" || node_type.starts_with("workflow.")
}

pub(crate) fn resumed_output_map(
    output_port: &str,
    payload: &Value,
) -> BTreeMap<String, Vec<Item>> {
    BTreeMap::from([(output_port.to_owned(), invocation_items(payload))])
}

pub(crate) fn hash_json(value: &Value) -> Result<String> {
    Ok(format!(
        "sha256:v1:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execution_snapshot_hash(
    definition: &Value,
    compiled_ir_hash: &str,
    source: &Value,
    manifest_snapshot: &Value,
    resources: &Value,
    runtime_settings: &Value,
    debug_plan: &Value,
    debug_overlay: &Value,
) -> Result<String> {
    hash_json(&json!({
        "definition": definition,
        "compiledIrHash": compiled_ir_hash,
        "source": source,
        "manifestSnapshot": manifest_snapshot,
        "resources": resources,
        "runtimeSettings": runtime_settings,
        "debugPlan": debug_plan,
        "debugOverlay": debug_overlay,
    }))
}

pub(crate) fn draft_resource_snapshots_cover_definition(
    definition: &WorkflowDefinition,
    snapshots: &[RuntimeResourceSnapshot],
) -> bool {
    if snapshots.iter().any(|snapshot| {
        !definition
            .nodes
            .iter()
            .any(|node| node.id == snapshot.node_id)
    }) {
        return false;
    }
    definition.nodes.iter().all(|node| {
        node.resource_references.iter().all(|expected| {
            snapshots.iter().any(|snapshot| {
                snapshot.node_id == node.id
                    && resolved_resource_reference_matches(expected, &snapshot.reference)
            })
        })
    })
}

fn resolved_resource_reference_matches(
    expected: &ResourceReference,
    actual: &ResourceReference,
) -> bool {
    expected.binding_id == actual.binding_id
        && expected.binding_role == actual.binding_role
        && expected.resource_type == actual.resource_type
        && expected.resource_id == actual.resource_id
        && expected.operation == actual.operation
        && expected
            .resource_version_id
            .is_none_or(|version| actual.resource_version_id == Some(version))
}

pub(crate) fn parse_uuid(value: &Value, key: &str) -> Result<Uuid> {
    Uuid::parse_str(
        value
            .get(key)
            .and_then(Value::as_str)
            .context(format!("{key} is missing"))?,
    )
    .map_err(Into::into)
}

pub(crate) fn checkpoint_type(result: &TaskResult) -> &'static str {
    match result {
        TaskResult::Completed(_) | TaskResult::Failed { .. } => "node_completed",
        TaskResult::Suspended(_) => "node_suspended",
    }
}

pub(crate) fn capability_name(value: &NodeCapability) -> &'static str {
    value.as_str()
}

pub(crate) fn side_effect_name(value: &SideEffectLevel) -> &'static str {
    match value {
        SideEffectLevel::None => "none",
        SideEffectLevel::Idempotent => "idempotent",
        SideEffectLevel::Reversible => "reversible",
        SideEffectLevel::Irreversible => "irreversible",
    }
}

pub(crate) fn activation_status_name(value: ActivationStatus) -> &'static str {
    match value {
        ActivationStatus::Ready => "ready",
        ActivationStatus::Running => "queued",
        ActivationStatus::Waiting => "waiting",
        ActivationStatus::Succeeded => "succeeded",
        ActivationStatus::Failed => "failed",
        ActivationStatus::Skipped => "skipped",
        ActivationStatus::Cancelled => "cancelled",
    }
}

pub(crate) fn attempt_status_name(value: AttemptStatus) -> &'static str {
    match value {
        AttemptStatus::Running => "queued",
        AttemptStatus::Succeeded => "succeeded",
        AttemptStatus::Failed => "failed",
        AttemptStatus::Suspended => "suspended",
        AttemptStatus::Cancelled => "cancelled",
    }
}

pub(crate) fn execution_status_name(value: RuntimeExecutionStatus) -> &'static str {
    match value {
        RuntimeExecutionStatus::Created => "created",
        RuntimeExecutionStatus::Running => "running",
        RuntimeExecutionStatus::Waiting => "waiting",
        RuntimeExecutionStatus::Succeeded => "succeeded",
        RuntimeExecutionStatus::Failed => "failed",
        RuntimeExecutionStatus::Cancelled => "cancelled",
        RuntimeExecutionStatus::TimedOut => "timed_out",
    }
}
