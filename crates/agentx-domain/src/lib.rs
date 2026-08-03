use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

pub mod resource;
pub mod workflow;

pub use resource::{
    MissingGrant, ResourceGrant, ResourceOperation, ResourceReference, ResourceType,
    ResourceVersionSnapshot,
};
pub use workflow::{
    DefinitionIssue, ExecutionOrder, NodeErrorPolicy, NodeSettings, WorkflowConnection,
    WorkflowDefinition, WorkflowNode, WorkflowPosition, WorkflowSettings, canonical_content_hash,
    validate_definition,
};

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Deserialize,
            Eq,
            Hash,
            JsonSchema,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

domain_id!(TenantId);
domain_id!(DepartmentId);
domain_id!(UserId);
domain_id!(RoleId);
domain_id!(WorkflowId);
domain_id!(WorkflowDraftId);
domain_id!(WorkflowRevisionId);
domain_id!(WorkflowVersionId);
domain_id!(EnvironmentId);
domain_id!(DeploymentId);
domain_id!(WorkflowServiceIdentityId);
domain_id!(ResourceId);
domain_id!(ResourceVersionId);
domain_id!(CredentialId);
domain_id!(ExecutionId);
domain_id!(NodeExecutionId);
domain_id!(AttemptId);
domain_id!(CheckpointId);
domain_id!(ResumeTokenId);
domain_id!(ArtifactId);
domain_id!(AuditEventId);
domain_id!(RefreshSessionId);
domain_id!(ApplicationId);
domain_id!(ApplicationDeploymentId);
domain_id!(ApiKeyId);
domain_id!(SessionId);
domain_id!(MessageId);
domain_id!(InvocationId);
domain_id!(DatasetId);
domain_id!(DatasetVersionId);
domain_id!(EvaluationProfileId);
domain_id!(EvaluationProfileVersionId);
domain_id!(EvaluationRunId);
domain_id!(ApprovalTaskId);
domain_id!(NotificationId);
domain_id!(TraceId);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PermissionKey(String);

impl PermissionKey {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b':' | b'_' | b'-')
            })
        {
            return Err("permission key must use lowercase ASCII letters, digits, ':', '_' or '-'");
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataScope {
    Company,
    DepartmentTree(DepartmentId),
    Own,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantContext {
    pub tenant_id: TenantId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorContext {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub username: String,
    pub token_version: u64,
    pub permissions: Vec<PermissionKey>,
    pub scopes: Vec<DataScope>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    pub request_id: Uuid,
    pub actor: Option<ActorContext>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedEntity {
    pub version: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowServiceIdentity {
    pub id: WorkflowServiceIdentityId,
    pub tenant_id: TenantId,
    pub workflow_id: WorkflowId,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub id: WorkflowId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionStatus {
    Created,
    Queued,
    Running,
    Waiting,
    WaitingApproval,
    Suspended,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub id: ExecutionId,
    pub tenant_id: TenantId,
    pub workflow_id: WorkflowId,
    pub workflow_version_id: WorkflowVersionId,
    pub invocation_id: Option<InvocationId>,
    pub session_id: Option<SessionId>,
    pub trace_id: TraceId,
    pub trigger_type: String,
    pub status: ExecutionStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub duration_ms: Option<u64>,
    pub cost_micros: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub trace_id: TraceId,
    pub span_id: Uuid,
    pub parent_span_id: Option<Uuid>,
    pub execution_id: ExecutionId,
    pub workflow_id: WorkflowId,
    pub workflow_version_id: WorkflowVersionId,
    pub node_execution_id: Option<NodeExecutionId>,
    pub event_type: String,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub event_time: OffsetDateTime,
    pub run_index: u32,
    pub iteration_index: u32,
    pub duration_ms: Option<u64>,
    pub model_name: Option<String>,
    pub provider_name: Option<String>,
    pub mcp_tool_name: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_micros: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub content_ref: Option<ArtifactId>,
    pub attributes: serde_json::Value,
}
