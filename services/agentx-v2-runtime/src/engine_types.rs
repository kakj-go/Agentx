use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_runtime::StringConversionRecord;
use agentx_runtime_contracts::{RuntimeResourceBindingV1, WorkerAttemptLeaseV1, WorkerTaskV1};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct ClaimedWorkerAttempt {
    pub lease: WorkerAttemptLeaseV1,
    pub task: WorkerTaskV1,
    pub node_type: String,
    pub node_version: u32,
    pub run_index: u32,
    pub iteration_index: u32,
    pub node_parameters: Value,
    pub per_item_parameters: Vec<Value>,
    pub string_conversions: Value,
    pub inputs: BTreeMap<String, Vec<Item>>,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub context: Value,
}

pub(super) enum ContextWriteOutcome {
    Applied {
        context: Value,
        version: u64,
        conversions: Vec<StringConversionRecord>,
    },
    SessionConflict,
}
