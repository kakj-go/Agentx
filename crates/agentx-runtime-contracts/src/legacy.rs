use agentx_domain::{ExecutionId, TenantId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

pub const RUNTIME_EVENT_SCHEMA_VERSION: u32 = crate::EVENT_SCHEMA_VERSION;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommandType {
    StartExecution,
    CancelExecution,
    ResumeExecution,
}

impl RuntimeCommandType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StartExecution => "start_execution",
            Self::CancelExecution => "cancel_execution",
            Self::ResumeExecution => "resume_execution",
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCommand {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub command_type: RuntimeCommandType,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub idempotency_key: String,
    pub payload: Value,
}

impl RuntimeCommand {
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        command_type: RuntimeCommandType,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: crate::EVENT_SCHEMA_VERSION,
            id: Uuid::now_v7(),
            tenant_id,
            command_type,
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            idempotency_key: idempotency_key.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartExecutionCommandPayload {
    pub workflow_version_id: Uuid,
    pub invocation_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub requested_by: Option<Uuid>,
    pub trigger_type: String,
    pub input: Value,
    #[serde(default = "empty_object")]
    pub runtime_settings: Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelExecutionCommandPayload {
    pub execution_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeExecutionCommandPayload {
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub resume_token: String,
    pub output_port: String,
    pub payload: Value,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeEventEnvelope {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub event_type: String,
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub execution_id: Option<ExecutionId>,
    pub sequence: Option<u64>,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
    pub payload: Value,
}

impl RuntimeEventEnvelope {
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        execution_id: Option<ExecutionId>,
        sequence: Option<u64>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            tenant_id,
            event_type: event_type.into(),
            schema_version: RUNTIME_EVENT_SCHEMA_VERSION,
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            execution_id,
            sequence,
            occurred_at: OffsetDateTime::now_utc(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionResult {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub outputs: Value,
    pub output_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}
