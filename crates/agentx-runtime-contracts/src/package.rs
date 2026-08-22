use ed25519_dalek::{SigningKey, VerifyingKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    BUNDLE_SCHEMA_VERSION, CompiledWorkflowV1, ContentHash, Ed25519Signature,
    RuntimeAuthorizationSnapshotV1, RuntimeCallPurposeV1, RuntimePolicyV1,
    RuntimeResourceBindingV1, RuntimeResourceConfigurationV1, RuntimeResourceKindV1,
    RuntimeWorkPackageOverlayV1, WorkerCompatibilityV1,
};

pub const RUNTIME_OBJECT_KEY_PREFIX: &str = "runtime";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeObjectReferenceV1 {
    pub tenant_id: Uuid,
    pub storage_domain: StorageDomain,
    pub object_id: Uuid,
    pub object_key: String,
    pub content_hash: ContentHash,
    pub size_bytes: u64,
    pub media_type: String,
}

impl RuntimeObjectReferenceV1 {
    #[must_use]
    pub fn canonical_key(tenant_id: Uuid, object_id: Uuid, content_hash: &ContentHash) -> String {
        format!(
            "{RUNTIME_OBJECT_KEY_PREFIX}/{tenant_id}/{object_id}/{}",
            content_hash
                .as_str()
                .strip_prefix("sha256:")
                .expect("ContentHash always carries the sha256 prefix")
        )
    }

    #[must_use]
    pub fn has_canonical_key(&self) -> bool {
        self.storage_domain == StorageDomain::Runtime
            && self.object_key
                == Self::canonical_key(self.tenant_id, self.object_id, &self.content_hash)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDomain {
    Runtime,
    Observability,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKindV1 {
    CompositeWorkflow,
    Skill,
    Model,
    Mcp,
    Rag,
    Memory,
    Credential,
    SandboxProfile,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyClosureEntryV1 {
    pub kind: DependencyKindV1,
    pub object_id: Uuid,
    pub version: String,
    pub content_hash: ContentHash,
    pub object_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyClosureV1 {
    pub entries: Vec<DependencyClosureEntryV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionSpecPayloadV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub bundle_id: Uuid,
    pub tenant_id: Uuid,
    pub application_id: Uuid,
    pub deployment_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Uuid,
    pub bundle_sequence: u64,
    pub definition: Value,
    pub compiled_ir: CompiledWorkflowV1,
    pub node_manifests: Vec<Value>,
    pub input_contract: Value,
    pub output_contract: Value,
    pub context_contract: Value,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub authorization: RuntimeAuthorizationSnapshotV1,
    pub dependency_closure: DependencyClosureV1,
    pub triggers: Vec<crate::RuntimeTriggerSpecV1>,
    pub runtime_policy: RuntimePolicyV1,
    pub objects: Vec<RuntimeObjectReferenceV1>,
    pub worker_compatibility: WorkerCompatibilityV1,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl ExecutionSpecPayloadV1 {
    #[must_use]
    pub fn current_schema_version() -> u32 {
        BUNDLE_SCHEMA_VERSION
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionSpecBundleV1 {
    pub payload: ExecutionSpecPayloadV1,
    pub content_hash: ContentHash,
    pub signature: Ed25519Signature,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPackagePurpose {
    Debug,
    Evaluation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeWorkPackagePayloadV1 {
    #[serde(deserialize_with = "crate::deserialize_v1")]
    pub schema_version: u32,
    pub package_id: Uuid,
    pub tenant_id: Uuid,
    pub origin: crate::ExecutionOriginV1,
    pub purpose: WorkPackagePurpose,
    pub call_purpose: RuntimeCallPurposeV1,
    pub spec: crate::RuntimeWorkPackageSpecV1,
    pub model_evaluator_executions: Vec<crate::RuntimeModelEvaluatorExecutionV1>,
    pub source_revision: String,
    pub definition: Value,
    pub compiled_ir: CompiledWorkflowV1,
    pub node_manifests: Vec<Value>,
    pub overlay: RuntimeWorkPackageOverlayV1,
    pub resource_closure: DependencyClosureV1,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub authorization: RuntimeAuthorizationSnapshotV1,
    pub objects: Vec<RuntimeObjectReferenceV1>,
    pub runtime_policy: RuntimePolicyV1,
    pub worker_compatibility: WorkerCompatibilityV1,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[schemars(with = "String")]
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeWorkPackageV1 {
    pub payload: RuntimeWorkPackagePayloadV1,
    pub content_hash: ContentHash,
    pub signature: Ed25519Signature,
}

impl ExecutionSpecBundleV1 {
    pub fn signed(
        payload: ExecutionSpecPayloadV1,
        key_id: impl Into<String>,
        key: &SigningKey,
    ) -> Result<Self, crate::ContractError> {
        validate_bundle_payload(&payload)?;
        let content_hash = crate::content_hash(&payload)?;
        let signature = crate::sign_canonical(key_id, key, &payload)?;
        Ok(Self {
            payload,
            content_hash,
            signature,
        })
    }

    pub fn verify(&self, key: &VerifyingKey) -> Result<(), crate::ContractError> {
        validate_bundle_payload(&self.payload)?;
        if crate::content_hash(&self.payload)? != self.content_hash {
            return Err(crate::ContractError::ContentHashMismatch);
        }
        crate::verify_canonical(key, &self.payload, &self.signature)
    }
}

impl RuntimeWorkPackageV1 {
    pub fn signed(
        payload: RuntimeWorkPackagePayloadV1,
        key_id: impl Into<String>,
        key: &SigningKey,
    ) -> Result<Self, crate::ContractError> {
        validate_work_package_payload(&payload)?;
        let content_hash = crate::content_hash(&payload)?;
        let signature = crate::sign_canonical(key_id, key, &payload)?;
        Ok(Self {
            payload,
            content_hash,
            signature,
        })
    }

    pub fn verify(&self, key: &VerifyingKey) -> Result<(), crate::ContractError> {
        validate_work_package_payload(&self.payload)?;
        if crate::content_hash(&self.payload)? != self.content_hash {
            return Err(crate::ContractError::ContentHashMismatch);
        }
        crate::verify_canonical(key, &self.payload, &self.signature)
    }
}

fn validate_bundle_payload(payload: &ExecutionSpecPayloadV1) -> Result<(), crate::ContractError> {
    payload
        .runtime_policy
        .validate()
        .map_err(crate::ContractError::InvalidRuntimePolicy)?;
    if payload.authorization.tenant_id != payload.tenant_id
        || payload.authorization.workflow_id != payload.workflow_id
        || payload
            .objects
            .iter()
            .any(|object| object.tenant_id != payload.tenant_id || !object.has_canonical_key())
        || !valid_resource_bindings(&payload.resources, &payload.objects)
    {
        return Err(crate::ContractError::InvalidImmutableReference);
    }
    Ok(())
}

fn validate_work_package_payload(
    payload: &RuntimeWorkPackagePayloadV1,
) -> Result<(), crate::ContractError> {
    let maximum_ttl = match payload.purpose {
        WorkPackagePurpose::Debug => time::Duration::hours(1),
        WorkPackagePurpose::Evaluation => time::Duration::hours(24),
    };
    if payload.expires_at <= payload.created_at
        || payload.expires_at - payload.created_at > maximum_ttl
    {
        return Err(crate::ContractError::InvalidWorkPackageTtl);
    }
    let spec_valid = match (&payload.purpose, &payload.call_purpose, &payload.spec) {
        (
            WorkPackagePurpose::Debug,
            RuntimeCallPurposeV1::Debug,
            crate::RuntimeWorkPackageSpecV1::Debug {
                draft_revision,
                debug_plan,
            },
        ) => *draft_revision > 0 && valid_debug_plan(payload, debug_plan),
        (
            WorkPackagePurpose::Evaluation,
            RuntimeCallPurposeV1::Evaluation,
            crate::RuntimeWorkPackageSpecV1::Evaluation {
                cases, evaluators, ..
            },
        ) => {
            let case_ids = cases
                .iter()
                .map(|case| case.case_id)
                .collect::<std::collections::BTreeSet<_>>();
            let evaluator_ids = evaluators
                .iter()
                .map(|evaluator| match evaluator {
                    crate::RuntimeEvaluatorV1::DeterministicRule { evaluator_id, .. }
                    | crate::RuntimeEvaluatorV1::Model { evaluator_id, .. } => *evaluator_id,
                })
                .collect::<std::collections::BTreeSet<_>>();
            let resource_ids = payload
                .resources
                .iter()
                .map(|resource| (resource.resource_kind, resource.resource_id))
                .collect::<std::collections::BTreeSet<_>>();
            let object_ids = payload
                .objects
                .iter()
                .map(|object| object.object_id)
                .collect::<std::collections::BTreeSet<_>>();
            let model_evaluators = evaluators
                .iter()
                .filter_map(|evaluator| match evaluator {
                    crate::RuntimeEvaluatorV1::Model {
                        evaluator_id,
                        resource_id,
                        prompt_object_id,
                    } => Some((*evaluator_id, *resource_id, *prompt_object_id)),
                    crate::RuntimeEvaluatorV1::DeterministicRule { .. } => None,
                })
                .collect::<std::collections::BTreeSet<_>>();
            let evaluator_executions = payload
                .model_evaluator_executions
                .iter()
                .map(|execution| {
                    (
                        execution.evaluator_id,
                        execution.resource_id,
                        execution.prompt_object_id,
                    )
                })
                .collect::<std::collections::BTreeSet<_>>();
            let evaluators_resolve = evaluators.iter().all(|evaluator| match evaluator {
                crate::RuntimeEvaluatorV1::DeterministicRule { expression, .. } => {
                    !expression.trim().is_empty()
                }
                crate::RuntimeEvaluatorV1::Model {
                    resource_id,
                    prompt_object_id,
                    ..
                } => {
                    resource_ids.contains(&(RuntimeResourceKindV1::Model, *resource_id))
                        && object_ids.contains(prompt_object_id)
                }
            });
            !cases.is_empty()
                && case_ids.len() == cases.len()
                && !evaluators.is_empty()
                && evaluator_ids.len() == evaluators.len()
                && evaluators_resolve
                && model_evaluators == evaluator_executions
                && payload.model_evaluator_executions.len() == evaluator_executions.len()
        }
        _ => false,
    };
    if !spec_valid {
        return Err(crate::ContractError::InvalidImmutableReference);
    }
    if !matches!(
        payload.spec,
        crate::RuntimeWorkPackageSpecV1::Evaluation { .. }
    ) && !payload.model_evaluator_executions.is_empty()
    {
        return Err(crate::ContractError::InvalidImmutableReference);
    }
    payload
        .runtime_policy
        .validate()
        .map_err(crate::ContractError::InvalidRuntimePolicy)?;
    if payload.authorization.tenant_id != payload.tenant_id
        || payload
            .objects
            .iter()
            .any(|object| object.tenant_id != payload.tenant_id || !object.has_canonical_key())
        || !valid_resource_bindings(&payload.resources, &payload.objects)
    {
        return Err(crate::ContractError::InvalidImmutableReference);
    }
    Ok(())
}

fn valid_debug_plan(
    payload: &RuntimeWorkPackagePayloadV1,
    plan: &crate::RuntimeDebugPlanV1,
) -> bool {
    use crate::{PartialExecutionModeV1, RuntimeDebugInputSourceV1};

    let all = payload
        .compiled_ir
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let included = plan
        .included_node_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let skipped = plan
        .skipped_node_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if included.len() != plan.included_node_ids.len()
        || skipped.len() != plan.skipped_node_ids.len()
        || !included.is_disjoint(&skipped)
        || included
            .union(&skipped)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != all
        || plan
            .side_effect_decisions
            .keys()
            .any(|node_id| !included.contains(node_id))
    {
        return false;
    }

    let expected = match (plan.mode, plan.target_node_id.as_deref()) {
        (PartialExecutionModeV1::Whole, None) => all.clone(),
        (PartialExecutionModeV1::Whole, Some(_)) | (_, None) => return false,
        (mode, Some(target)) => {
            let Some(selected) = payload
                .compiled_ir
                .nodes
                .iter()
                .position(|node| node.id == target)
            else {
                return false;
            };
            let mut indexes = std::collections::BTreeSet::from([selected]);
            let mut frontier = std::collections::VecDeque::from([selected]);
            while let Some(node) = frontier.pop_front() {
                let connections = match mode {
                    PartialExecutionModeV1::Node => continue,
                    PartialExecutionModeV1::ToNode => {
                        &payload.compiled_ir.nodes[node].incoming_connections
                    }
                    PartialExecutionModeV1::FromNode => {
                        &payload.compiled_ir.nodes[node].outgoing_connections
                    }
                    PartialExecutionModeV1::Whole => unreachable!(),
                };
                for connection in connections {
                    let connection = &payload.compiled_ir.connections[*connection];
                    let next = if mode == PartialExecutionModeV1::ToNode {
                        connection.source_node
                    } else {
                        connection.target_node
                    };
                    if indexes.insert(next) {
                        frontier.push_back(next);
                    }
                }
            }
            indexes
                .into_iter()
                .map(|index| payload.compiled_ir.nodes[index].id.clone())
                .collect()
        }
    };
    if included != expected {
        return false;
    }

    let input_source_valid = match (&plan.mode, &plan.input_source) {
        (PartialExecutionModeV1::Node | PartialExecutionModeV1::FromNode, Some(source)) => {
            match source {
                RuntimeDebugInputSourceV1::Manual { value } => value == &payload.overlay.input,
                RuntimeDebugInputSourceV1::HistoryOutput { output_port, .. } => {
                    !output_port.trim().is_empty()
                }
                RuntimeDebugInputSourceV1::Artifact { .. } => true,
            }
        }
        (PartialExecutionModeV1::Whole | PartialExecutionModeV1::ToNode, None) => true,
        _ => false,
    };
    input_source_valid
        && payload.compiled_ir.nodes.iter().all(|node| {
            node.side_effect_level != agentx_node_protocol::SideEffectLevel::Irreversible
                || !included.contains(&node.id)
                || plan.side_effect_decisions.contains_key(&node.id)
        })
}

fn valid_resource_bindings(
    resources: &[RuntimeResourceBindingV1],
    objects: &[RuntimeObjectReferenceV1],
) -> bool {
    let available_objects = objects
        .iter()
        .map(|object| object.object_id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut identities = std::collections::BTreeSet::new();
    resources.iter().all(|resource| {
        identities.insert((
            resource.resource_kind,
            resource.resource_id,
            resource.resource_version.clone(),
        )) && resource
            .object_ids
            .iter()
            .all(|object_id| available_objects.contains(object_id))
            && configuration_kind(&resource.configuration) == resource.resource_kind
    })
}

const fn configuration_kind(
    configuration: &RuntimeResourceConfigurationV1,
) -> RuntimeResourceKindV1 {
    match configuration {
        RuntimeResourceConfigurationV1::Model { .. } => RuntimeResourceKindV1::Model,
        RuntimeResourceConfigurationV1::Mcp { .. } => RuntimeResourceKindV1::Mcp,
        RuntimeResourceConfigurationV1::Rag { .. } => RuntimeResourceKindV1::Rag,
        RuntimeResourceConfigurationV1::Memory { .. } => RuntimeResourceKindV1::Memory,
        RuntimeResourceConfigurationV1::Skill { .. } => RuntimeResourceKindV1::Skill,
        RuntimeResourceConfigurationV1::Credential { .. } => RuntimeResourceKindV1::Credential,
        RuntimeResourceConfigurationV1::SandboxProfile { .. } => {
            RuntimeResourceKindV1::SandboxProfile
        }
        RuntimeResourceConfigurationV1::Composite { .. } => RuntimeResourceKindV1::Composite,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleLifecycleState {
    Building,
    Prepared,
    Active,
    Superseded,
    Disabled,
    Retained,
    GarbageCollectable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleReferenceKind {
    ActiveExecution,
    CheckpointForkSource,
    PinnedSession,
    PendingWait,
    RetentionHold,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleReferenceV1 {
    pub bundle_id: Uuid,
    pub kind: BundleReferenceKind,
    pub owner_id: Uuid,
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub retained_until: Option<OffsetDateTime>,
}
