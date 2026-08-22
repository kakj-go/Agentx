use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Immutable audit context captured when an Execution is created.
///
/// Control remains authoritative for current IAM and resource names. Runtime
/// stores this snapshot so later renames do not rewrite execution history.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionOriginV1 {
    pub initiator_user_id: Option<Uuid>,
    pub initiator_user_name: Option<String>,
    pub initiator_department_id: Option<Uuid>,
    pub initiator_department_name: Option<String>,
    pub trigger_source_id: Option<Uuid>,
    pub trigger_name: Option<String>,
}

impl ExecutionOriginV1 {
    #[must_use]
    pub const fn system(trigger_source_id: Option<Uuid>) -> Self {
        Self {
            initiator_user_id: None,
            initiator_user_name: None,
            initiator_department_id: None,
            initiator_department_name: None,
            trigger_source_id,
            trigger_name: None,
        }
    }
}
