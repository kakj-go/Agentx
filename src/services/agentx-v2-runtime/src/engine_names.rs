use agentx_domain::ContextWriteOperation;
use agentx_node_protocol::SideEffectLevel;
use agentx_runtime::{ActivationStatus, RuntimeExecutionStatus};
use agentx_runtime_contracts::{
    PartialExecutionModeV1, SideEffectResolutionV1, WorkerResultStatusV1,
};
use serde_json::Value;
use uuid::Uuid;

pub(crate) fn context_value<'a>(context: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(context, |value, segment| value.get(segment))
}

pub(crate) const fn context_operation_name(operation: ContextWriteOperation) -> &'static str {
    match operation {
        ContextWriteOperation::Set => "set",
        ContextWriteOperation::SetIfAbsent => "set_if_absent",
        ContextWriteOperation::Delete => "delete",
        ContextWriteOperation::Append => "append",
        ContextWriteOperation::MergeObject => "merge_object",
        ContextWriteOperation::Increment => "increment",
        ContextWriteOperation::Min => "min",
        ContextWriteOperation::Max => "max",
        ContextWriteOperation::CompareAndSet => "compare_and_set",
    }
}

pub(crate) const fn partial_mode(mode: PartialExecutionModeV1) -> &'static str {
    match mode {
        PartialExecutionModeV1::Whole => "whole",
        PartialExecutionModeV1::Node => "node",
        PartialExecutionModeV1::ToNode => "to_node",
        PartialExecutionModeV1::FromNode => "from_node",
    }
}

pub(crate) const fn side_effect_resolution_name(
    resolution: SideEffectResolutionV1,
) -> &'static str {
    match resolution {
        SideEffectResolutionV1::Execute => "execute",
        SideEffectResolutionV1::ReuseOutput => "reuse_output",
        SideEffectResolutionV1::DryRun => "dry_run",
    }
}

pub(crate) fn deterministic_uuid(namespace: Uuid, label: &[u8]) -> Uuid {
    agentx_runtime_contracts::deterministic_uuid(namespace, label)
}

pub(crate) const fn activation_status(status: ActivationStatus) -> &'static str {
    match status {
        ActivationStatus::Ready => "ready",
        ActivationStatus::Running => "running",
        ActivationStatus::Waiting => "waiting",
        ActivationStatus::Succeeded => "succeeded",
        ActivationStatus::Failed => "failed",
        ActivationStatus::Skipped => "skipped",
        ActivationStatus::Cancelled => "cancelled",
    }
}

pub(crate) const fn worker_result_status(status: WorkerResultStatusV1) -> &'static str {
    match status {
        WorkerResultStatusV1::Succeeded => "succeeded",
        WorkerResultStatusV1::Failed | WorkerResultStatusV1::OutcomeUnknown => "failed",
        WorkerResultStatusV1::Suspended => "suspended",
        WorkerResultStatusV1::Cancelled => "cancelled",
    }
}

pub(crate) const fn machine_status(status: RuntimeExecutionStatus) -> &'static str {
    match status {
        RuntimeExecutionStatus::Created => "created",
        RuntimeExecutionStatus::Running => "running",
        RuntimeExecutionStatus::Waiting => "waiting",
        RuntimeExecutionStatus::Succeeded => "succeeded",
        RuntimeExecutionStatus::Failed => "failed",
        RuntimeExecutionStatus::Cancelled => "cancelled",
        RuntimeExecutionStatus::TimedOut => "timed_out",
    }
}

pub(crate) const fn side_effect_name(level: &SideEffectLevel) -> &'static str {
    match level {
        SideEffectLevel::None => "none",
        SideEffectLevel::Idempotent => "idempotent",
        SideEffectLevel::Reversible => "reversible",
        SideEffectLevel::Irreversible => "irreversible",
    }
}

pub(crate) const fn is_terminal(status: RuntimeExecutionStatus) -> bool {
    matches!(
        status,
        RuntimeExecutionStatus::Succeeded
            | RuntimeExecutionStatus::Failed
            | RuntimeExecutionStatus::Cancelled
            | RuntimeExecutionStatus::TimedOut
    )
}
