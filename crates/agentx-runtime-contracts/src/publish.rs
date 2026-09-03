use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApplyReceiptV1, ContentHash, ExecutionSpecBundleV2, INTERNAL_API_VERSION,
    RuntimeObjectReferenceV1, RuntimeWorkPackageV1,
};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeObjectUploadMetadataV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub tenant_id: Uuid,
    pub object_id: Uuid,
    pub content_hash: ContentHash,
    pub size_bytes: u64,
    pub media_type: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeObjectUploadReceiptV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub object: RuntimeObjectReferenceV1,
    pub replayed: bool,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub accepted_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareBundleRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub bundle: ExecutionSpecBundleV2,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationManifestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub tenant_id: Uuid,
    pub application_id: Uuid,
    pub deployment_id: Uuid,
    pub bundle_id: Uuid,
    pub expected_head_version: Option<u64>,
    pub activation_sequence: u64,
    pub minimum_admission_epoch: u64,
    pub runtime_config_revision: u64,
    pub runtime_policy: crate::ApplicationRuntimePolicyV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivateDeploymentRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub manifest: ActivationManifestV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollbackDeploymentRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub manifest: ActivationManifestV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisableDeploymentRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub tenant_id: Uuid,
    pub application_id: Uuid,
    pub admission_epoch: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareWorkPackageRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub work_package: RuntimeWorkPackageV1,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteWorkPackageRequestV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub idempotency_key: String,
    pub package_id: Uuid,
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishReceiptV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub api_version: u32,
    pub receipt: ApplyReceiptV1,
    pub bundle_id: Uuid,
    pub head_version: Option<u64>,
    pub activation_sequence: Option<u64>,
    pub status: PublishReceiptStatusV1,
    pub rejection: Option<PublishRejectionV1>,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub accepted_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishReceiptStatusV1 {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimePublishErrorCodeV1 {
    UnsupportedApiVersion,
    UnsupportedBundleVersion,
    UnsupportedIrVersion,
    InvalidSignature,
    ContentHashMismatch,
    ObjectMissing,
    ObjectHashMismatch,
    ObjectSizeMismatch,
    ObjectMediaTypeMismatch,
    UnsupportedCapability,
    AdmissionPrerequisiteMissing,
    HeadVersionConflict,
    ActivationSequenceConflict,
    IdempotencyConflict,
    BundleReferenceConflict,
    TenantMismatch,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishRejectionV1 {
    pub code: RuntimePublishErrorCodeV1,
    pub message: String,
    pub details: serde_json::Value,
}

#[must_use]
pub const fn current_internal_api_version() -> u32 {
    INTERNAL_API_VERSION
}
