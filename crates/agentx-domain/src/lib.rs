use std::{fmt, str::FromStr};

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
    DefinitionIssue, WorkflowConnection, WorkflowDefinition, WorkflowNode, WorkflowPosition,
    canonical_content_hash, validate_definition,
};

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
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
domain_id!(ArtifactId);
domain_id!(AuditEventId);
domain_id!(RefreshSessionId);

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
