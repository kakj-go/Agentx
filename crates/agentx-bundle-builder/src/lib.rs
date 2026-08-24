use std::collections::{BTreeMap, BTreeSet};

use agentx_domain::WorkflowDefinition;
use agentx_node_protocol::NodeCapability;
use agentx_runtime::{COMPILER_VERSION, CompileContext, NodeRegistry, WorkflowCompiler};
use agentx_runtime_contracts::{
    BUNDLE_SCHEMA_VERSION, DependencyClosureEntryV1, DependencyClosureV1, DependencyKindV1,
    ExecutionOriginV1, ExecutionSpecBundleV1, ExecutionSpecPayloadV1,
    RuntimeAuthorizationSnapshotV1, RuntimeCallPurposeV1, RuntimeModelEvaluatorExecutionV1,
    RuntimeObjectReferenceV1, RuntimePolicyV1, RuntimeResourceBindingV1, RuntimeTriggerSpecV1,
    RuntimeWorkPackageOverlayV1, RuntimeWorkPackagePayloadV1, RuntimeWorkPackageV1,
    WorkPackagePurpose, WorkerCompatibilityV1,
};
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct BundleBuildSource {
    pub bundle_id: Uuid,
    pub tenant_id: Uuid,
    pub application_id: Uuid,
    pub deployment_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Uuid,
    pub workflow_name: String,
    pub workflow_version_number: u64,
    pub workflow_owner_department: Option<agentx_runtime_contracts::ExecutionDepartmentSnapshotV1>,
    pub sequence: u64,
    pub definition: WorkflowDefinition,
    pub dependency_versions: BTreeMap<Uuid, WorkflowDefinition>,
    pub supported_capabilities: BTreeSet<String>,
    pub input_contract: Value,
    pub output_contract: Value,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub authorization: RuntimeAuthorizationSnapshotV1,
    pub triggers: Vec<RuntimeTriggerSpecV1>,
    pub runtime_policy: RuntimePolicyV1,
    pub objects: Vec<RuntimeObjectReferenceV1>,
    pub created_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct WorkPackageBuildSource {
    pub package_id: Uuid,
    pub tenant_id: Uuid,
    pub workflow: agentx_runtime_contracts::ExecutionWorkflowSnapshotV1,
    pub origin: ExecutionOriginV1,
    pub purpose: WorkPackagePurpose,
    pub call_purpose: RuntimeCallPurposeV1,
    pub spec: agentx_runtime_contracts::RuntimeWorkPackageSpecV1,
    pub source_revision: String,
    pub definition: WorkflowDefinition,
    pub dependency_versions: BTreeMap<Uuid, WorkflowDefinition>,
    pub supported_capabilities: BTreeSet<String>,
    pub overlay: RuntimeWorkPackageOverlayV1,
    pub resources: Vec<RuntimeResourceBindingV1>,
    pub authorization: RuntimeAuthorizationSnapshotV1,
    pub objects: Vec<RuntimeObjectReferenceV1>,
    pub runtime_policy: RuntimePolicyV1,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("Workflow compilation failed: {0}")]
    Compilation(String),
    #[error("fixed sub-workflow version {0} is missing")]
    MissingDependency(Uuid),
    #[error("sub-workflow dependency cycle contains {0}")]
    DependencyCycle(Uuid),
    #[error("fixed sub-workflow version {0} has no immutable Runtime object")]
    MissingDependencyObject(Uuid),
    #[error("Runtime capability {0} is not supported by this publishing slice")]
    UnsupportedCapability(String),
    #[error("Runtime object uses a non-canonical key")]
    NonCanonicalObject,
    #[error("Bundle signing failed: {0}")]
    Signing(#[from] agentx_runtime_contracts::ContractError),
}

#[must_use]
pub fn composite_ir_object_id(workflow_version_id: Uuid) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"agentx-composite-ir-v1\0");
    digest.update(workflow_version_id.as_bytes());
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub fn compile_workflow_version(
    definition: &WorkflowDefinition,
    workflow_version_id: Uuid,
) -> Result<agentx_runtime_contracts::CompiledWorkflowV1, BuildError> {
    compile_workflow_version_with_dependencies(definition, workflow_version_id, &BTreeMap::new())
}

pub fn compile_workflow_version_with_dependencies(
    definition: &WorkflowDefinition,
    workflow_version_id: Uuid,
    dependency_versions: &BTreeMap<Uuid, WorkflowDefinition>,
) -> Result<agentx_runtime_contracts::CompiledWorkflowV1, BuildError> {
    let registry = node_registry_with_composites(dependency_versions)?;
    WorkflowCompiler::new(&registry)
        .compile(
            definition,
            &CompileContext {
                current_workflow_version_id: Some(workflow_version_id.to_string()),
                ancestor_workflow_version_ids: BTreeSet::new(),
            },
        )
        .map_err(|error| {
            BuildError::Compilation(
                error
                    .issues
                    .into_iter()
                    .map(|issue| issue.code)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
}

/// Builds the immutable node registry used by Control validation, Bundle/Work Package
/// compilation and Runtime composite snapshots. Each fixed Workflow Version gets a
/// version-addressed Manifest whose input, output and mutable Context contract comes
/// from that version's immutable Definition.
pub fn node_registry_with_composites(
    dependency_versions: &BTreeMap<Uuid, WorkflowDefinition>,
) -> Result<NodeRegistry, BuildError> {
    let mut registry = NodeRegistry::m5_defaults();
    let base = registry
        .get("sub_workflow", 1)
        .expect("the built-in composite manifest exists")
        .clone();
    for (version_id, definition) in dependency_versions {
        let mut manifest = base.clone();
        manifest.node_type = format!("workflow.{}", version_id.simple());
        manifest.display_name = format!("Workflow Version {version_id}");
        manifest.description = "Immutable composite Workflow Version".into();
        manifest.parameter_schema = json!({
            "type": "object",
            "x-agentx-contextContract": definition.start.contexts,
            "required": ["workflowVersionId", "inputs"],
            "properties": {
                "workflowVersionId": {
                    "type": "string",
                    "format": "uuid",
                    "const": version_id.to_string()
                },
                "inputs": {
                    "allOf": [definition.start.inputs],
                    "x-agentx-dynamicValue": {
                        "modes": ["literal", "reference"],
                        "allowedNamespaces": ["inputs", "outputs", "contexts", "execution"],
                        "acceptedCardinality": ["single"],
                        "missingPolicies": ["error", "null", "default", "omit"],
                        "recursive": true
                    }
                }
            },
            "additionalProperties": false
        });
        let required = definition
            .end
            .outputs
            .iter()
            .filter(|(_, output)| output.required)
            .map(|(name, _)| Value::String(name.clone()))
            .collect::<Vec<_>>();
        let properties = definition
            .end
            .outputs
            .iter()
            .map(|(name, output)| (name.clone(), output.schema.clone()))
            .collect::<serde_json::Map<_, _>>();
        manifest.output_schema = json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        });
        registry.register(manifest).map_err(|error| {
            BuildError::Compilation(format!("INVALID_COMPOSITE_MANIFEST:{error}"))
        })?;
    }
    Ok(registry)
}

pub fn build_bundle(
    mut source: BundleBuildSource,
    key_id: &str,
    signing_key: &SigningKey,
) -> Result<ExecutionSpecBundleV1, BuildError> {
    if source
        .objects
        .iter()
        .any(|object| !object.has_canonical_key())
    {
        return Err(BuildError::NonCanonicalObject);
    }
    let registry = node_registry_with_composites(&source.dependency_versions)?;
    source.triggers.extend(workflow_runtime_triggers(
        &registry,
        &source.definition,
        source.tenant_id,
        source.application_id,
        source.workflow_version_id,
        source.sequence,
    )?);
    source.triggers.sort_by_key(|trigger| trigger.trigger_id);
    if source
        .triggers
        .windows(2)
        .any(|pair| pair[0].trigger_id == pair[1].trigger_id)
    {
        return Err(BuildError::Compilation(
            "DUPLICATE_RUNTIME_TRIGGER_ID".into(),
        ));
    }
    let compiled = WorkflowCompiler::new(&registry)
        .compile(
            &source.definition,
            &CompileContext {
                current_workflow_version_id: Some(source.workflow_version_id.to_string()),
                ancestor_workflow_version_ids: BTreeSet::new(),
            },
        )
        .map_err(|error| {
            BuildError::Compilation(
                error
                    .issues
                    .into_iter()
                    .map(|issue| issue.code)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })?;
    let mut closure = Vec::new();
    let mut visiting = BTreeSet::from([source.workflow_version_id]);
    collect_dependencies(
        &registry,
        &source.dependency_versions,
        &compiled.subworkflow_version_ids,
        &mut visiting,
        &mut closure,
    )?;
    let available_objects = source
        .objects
        .iter()
        .map(|object| object.object_id)
        .collect::<BTreeSet<_>>();
    for entry in &closure {
        for object_id in &entry.object_ids {
            if !available_objects.contains(object_id) {
                return Err(BuildError::MissingDependencyObject(*object_id));
            }
        }
    }
    if !available_objects.contains(&source.workflow_version_id) {
        return Err(BuildError::MissingDependencyObject(
            source.workflow_version_id,
        ));
    }
    let node_manifests = compiled
        .nodes
        .iter()
        .filter_map(|node| registry.get(&node.node_type, node.type_version))
        .map(|manifest| serde_json::to_value(manifest).expect("Node Manifest serializes"))
        .collect::<Vec<_>>();
    let capabilities = compiled
        .nodes
        .iter()
        .map(|node| capability_name(&node.capability).to_owned())
        .collect::<BTreeSet<_>>();
    if let Some(capability) = capabilities
        .iter()
        .find(|capability| !source.supported_capabilities.contains(*capability))
    {
        return Err(BuildError::UnsupportedCapability(capability.clone()));
    }
    let context_contract = serde_json::to_value(&source.definition.start.contexts)
        .expect("Workflow contexts serialize");
    ExecutionSpecBundleV1::signed(
        ExecutionSpecPayloadV1 {
            schema_version: BUNDLE_SCHEMA_VERSION,
            bundle_id: source.bundle_id,
            tenant_id: source.tenant_id,
            application_id: source.application_id,
            deployment_id: source.deployment_id,
            workflow_id: source.workflow_id,
            workflow_version_id: source.workflow_version_id,
            workflow_name: source.workflow_name,
            workflow_version_number: source.workflow_version_number,
            workflow_owner_department: source.workflow_owner_department,
            bundle_sequence: source.sequence,
            definition: serde_json::to_value(source.definition)
                .expect("Workflow Definition serializes"),
            compiled_ir: compiled,
            node_manifests,
            input_contract: source.input_contract,
            output_contract: source.output_contract,
            context_contract,
            resources: source.resources,
            authorization: source.authorization,
            dependency_closure: DependencyClosureV1 { entries: closure },
            triggers: source.triggers,
            runtime_policy: source.runtime_policy,
            objects: source.objects,
            worker_compatibility: WorkerCompatibilityV1 {
                protocol_version: 1,
                ir_versions: BTreeSet::from([1]),
                compiler_versions: BTreeSet::from([COMPILER_VERSION.into()]),
                capabilities,
                manifest_versions: BTreeSet::from([
                    agentx_node_protocol::NODE_PROTOCOL_VERSION.into()
                ]),
            },
            created_at: source.created_at,
        },
        key_id,
        signing_key,
    )
    .map_err(BuildError::from)
}

pub fn build_work_package(
    source: WorkPackageBuildSource,
    key_id: &str,
    signing_key: &SigningKey,
) -> Result<RuntimeWorkPackageV1, BuildError> {
    if source
        .objects
        .iter()
        .any(|object| !object.has_canonical_key())
    {
        return Err(BuildError::NonCanonicalObject);
    }
    let registry = node_registry_with_composites(&source.dependency_versions)?;
    let compiled = WorkflowCompiler::new(&registry)
        .compile(
            &source.definition,
            &CompileContext {
                current_workflow_version_id: Some(source.package_id.to_string()),
                ancestor_workflow_version_ids: BTreeSet::new(),
            },
        )
        .map_err(|error| {
            BuildError::Compilation(
                error
                    .issues
                    .into_iter()
                    .map(|issue| issue.code)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })?;
    let mut closure = Vec::new();
    let mut visiting = BTreeSet::from([source.package_id]);
    collect_dependencies(
        &registry,
        &source.dependency_versions,
        &compiled.subworkflow_version_ids,
        &mut visiting,
        &mut closure,
    )?;
    let available_objects = source
        .objects
        .iter()
        .map(|object| object.object_id)
        .collect::<BTreeSet<_>>();
    for entry in &closure {
        for object_id in &entry.object_ids {
            if !available_objects.contains(object_id) {
                return Err(BuildError::MissingDependencyObject(*object_id));
            }
        }
    }
    let node_manifests = compiled
        .nodes
        .iter()
        .filter_map(|node| registry.get(&node.node_type, node.type_version))
        .map(|manifest| serde_json::to_value(manifest).expect("Node Manifest serializes"))
        .collect::<Vec<_>>();
    let mut capabilities = compiled
        .nodes
        .iter()
        .map(|node| capability_name(&node.capability).to_owned())
        .collect::<BTreeSet<_>>();
    if matches!(
        &source.spec,
        agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Evaluation { evaluators, .. }
            if evaluators.iter().any(|evaluator| matches!(evaluator, agentx_runtime_contracts::RuntimeEvaluatorV1::Model { .. }))
    ) {
        capabilities.insert("model".into());
    }
    if let Some(capability) = capabilities
        .iter()
        .find(|capability| !source.supported_capabilities.contains(*capability))
    {
        return Err(BuildError::UnsupportedCapability(capability.clone()));
    }
    let model_evaluator_executions = build_model_evaluators(&source.spec, &registry)?;
    RuntimeWorkPackageV1::signed(
        RuntimeWorkPackagePayloadV1 {
            schema_version: BUNDLE_SCHEMA_VERSION,
            package_id: source.package_id,
            tenant_id: source.tenant_id,
            workflow: source.workflow,
            origin: source.origin,
            purpose: source.purpose,
            call_purpose: source.call_purpose,
            spec: source.spec,
            model_evaluator_executions,
            source_revision: source.source_revision,
            definition: serde_json::to_value(source.definition)
                .expect("Workflow Definition serializes"),
            compiled_ir: compiled,
            node_manifests,
            overlay: source.overlay,
            resource_closure: DependencyClosureV1 { entries: closure },
            resources: source.resources,
            authorization: source.authorization,
            objects: source.objects,
            runtime_policy: source.runtime_policy,
            worker_compatibility: WorkerCompatibilityV1 {
                protocol_version: 1,
                ir_versions: BTreeSet::from([1]),
                compiler_versions: BTreeSet::from([COMPILER_VERSION.into()]),
                capabilities,
                manifest_versions: BTreeSet::from([
                    agentx_node_protocol::NODE_PROTOCOL_VERSION.into()
                ]),
            },
            created_at: source.created_at,
            expires_at: source.expires_at,
        },
        key_id,
        signing_key,
    )
    .map_err(BuildError::from)
}

fn build_model_evaluators(
    spec: &agentx_runtime_contracts::RuntimeWorkPackageSpecV1,
    registry: &NodeRegistry,
) -> Result<Vec<RuntimeModelEvaluatorExecutionV1>, BuildError> {
    let agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Evaluation { evaluators, .. } = spec
    else {
        return Ok(Vec::new());
    };
    evaluators
        .iter()
        .filter_map(|evaluator| match evaluator {
            agentx_runtime_contracts::RuntimeEvaluatorV1::Model {
                evaluator_id,
                resource_id,
                prompt_object_id,
            } => Some((*evaluator_id, *resource_id, *prompt_object_id)),
            agentx_runtime_contracts::RuntimeEvaluatorV1::DeterministicRule { .. } => None,
        })
        .map(|(evaluator_id, resource_id, prompt_object_id)| {
            let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
                "schemaVersion":"5.0",
                "start":{"inputs":{},"contexts":{}},
                "nodes":[{
                    "id":"evaluate",
                    "key":"evaluate",
                    "type":"model",
                    "typeVersion":1,
                    "name":"Model Evaluator",
                    "parameters":{"prompt":"runtime_object"},
                    "outputProjection":{"main":{"evaluation":{"value":{"kind":"reference","selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["structuredOutput"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"object"},"sensitive":false}}},
                    "contextWrites":[],
                    "resourceReferences":[{
                        "resourceType":"model",
                        "resourceId":resource_id,
                        "operation":"use"
                    }]
                }],
                "connections":[
                    {"id":"start-evaluate","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"evaluate","targetHandle":"main","order":0},
                    {"id":"evaluate-end","sourceNodeId":"evaluate","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
                ],
                "end":{"outputs":{"evaluation":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"evaluate","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["evaluation"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"object"},"required":true}}},
                "settings":{"activationBudget":4,"executionOrder":"deterministic"}
            }))
            .map_err(|error| BuildError::Compilation(error.to_string()))?;
            let compiled_ir = WorkflowCompiler::new(registry)
                .compile(
                    &definition,
                    &CompileContext {
                        current_workflow_version_id: Some(evaluator_id.to_string()),
                        ancestor_workflow_version_ids: BTreeSet::new(),
                    },
                )
                .map_err(|error| {
                    BuildError::Compilation(
                        error
                            .issues
                            .into_iter()
                            .map(|issue| issue.code)
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })?;
            let node_manifests = compiled_ir
                .nodes
                .iter()
                .filter_map(|node| registry.get(&node.node_type, node.type_version))
                .map(|manifest| serde_json::to_value(manifest).expect("Node Manifest serializes"))
                .collect();
            Ok(RuntimeModelEvaluatorExecutionV1 {
                evaluator_id,
                resource_id,
                prompt_object_id,
                definition: serde_json::to_value(definition)
                    .expect("model evaluator Definition serializes"),
                compiled_ir,
                node_manifests,
            })
        })
        .collect()
}

fn workflow_runtime_triggers(
    registry: &NodeRegistry,
    definition: &WorkflowDefinition,
    tenant_id: Uuid,
    application_id: Uuid,
    workflow_version_id: Uuid,
    revision: u64,
) -> Result<Vec<RuntimeTriggerSpecV1>, BuildError> {
    use agentx_node_protocol::LifecycleOperation;
    use agentx_runtime_contracts::{LifecycleOperationV1, RuntimeTriggerConfigurationV1};

    let context = RuntimeTriggerContext {
        tenant_id,
        application_id,
        workflow_version_id,
        revision,
    };
    let mut triggers = Vec::new();
    for node in definition.nodes.iter().filter(|node| !node.disabled) {
        let Some(manifest) = registry.get(&node.node_type, node.type_version) else {
            continue;
        };
        if manifest.lifecycle_operations.is_empty() {
            continue;
        }
        let endpoint = node
            .parameters
            .get("endpoint")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                BuildError::Compilation(format!("RUNTIME_TRIGGER_ENDPOINT_REQUIRED:{}", node.id))
            })?
            .trim_end_matches('/');
        let protocol_input = |operation: &str| {
            serde_json::json!({
                "protocolVersion": agentx_node_protocol::NODE_PROTOCOL_VERSION,
                "operation": operation,
                "nodeType": node.node_type,
                "nodeVersion": node.type_version,
                "tenantId": tenant_id,
                "workflowVersionId": workflow_version_id,
                "configuration": node.parameters,
            })
        };
        if manifest
            .lifecycle_operations
            .contains(&LifecycleOperation::Poll)
        {
            let interval_seconds = node
                .parameters
                .get("pollIntervalSeconds")
                .and_then(Value::as_u64)
                .unwrap_or(60)
                .clamp(1, 86_400) as u32;
            let configuration = RuntimeTriggerConfigurationV1::Poll {
                interval_seconds,
                provider_endpoint: format!("{endpoint}/agentx/node/v1/lifecycle/poll"),
                input: protocol_input("poll"),
            };
            triggers.push(runtime_trigger(
                &context,
                &node.id,
                &node.name,
                "poll",
                true,
                configuration,
            )?);
        }
        for (operation, operation_name) in [
            (LifecycleOperation::Activate, "activate"),
            (LifecycleOperation::Deactivate, "deactivate"),
        ] {
            if !manifest.lifecycle_operations.contains(&operation) {
                continue;
            }
            let configuration = RuntimeTriggerConfigurationV1::Lifecycle {
                operation: if operation == LifecycleOperation::Activate {
                    LifecycleOperationV1::Activate
                } else {
                    LifecycleOperationV1::Deactivate
                },
                provider_endpoint: format!("{endpoint}/agentx/node/v1/lifecycle/{operation_name}"),
                input: protocol_input(operation_name),
            };
            triggers.push(runtime_trigger(
                &context,
                &node.id,
                &node.name,
                &format!("lifecycle:{operation_name}"),
                operation == LifecycleOperation::Activate,
                configuration,
            )?);
        }
    }
    triggers.sort_by_key(|trigger| trigger.trigger_id);
    Ok(triggers)
}

struct RuntimeTriggerContext {
    tenant_id: Uuid,
    application_id: Uuid,
    workflow_version_id: Uuid,
    revision: u64,
}

fn runtime_trigger(
    context: &RuntimeTriggerContext,
    node_id: &str,
    trigger_name: &str,
    kind: &str,
    enabled: bool,
    configuration: agentx_runtime_contracts::RuntimeTriggerConfigurationV1,
) -> Result<RuntimeTriggerSpecV1, BuildError> {
    let trigger_id = deterministic_trigger_id(
        context.tenant_id,
        context.application_id,
        context.workflow_version_id,
        node_id,
        kind,
        kind.starts_with("lifecycle:").then_some(context.revision),
    );
    let configuration_hash = agentx_runtime_contracts::content_hash(&configuration)?;
    Ok(RuntimeTriggerSpecV1 {
        schema_version: 1,
        trigger_id,
        trigger_name: trigger_name.to_owned(),
        application_id: context.application_id,
        node_id: format!("{node_id}:{kind}"),
        revision: context.revision,
        configuration_hash,
        enabled,
        configuration,
    })
}

fn deterministic_trigger_id(
    tenant_id: Uuid,
    application_id: Uuid,
    workflow_version_id: Uuid,
    node_id: &str,
    kind: &str,
    revision: Option<u64>,
) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"agentx-runtime-trigger-v1\0");
    digest.update(tenant_id.as_bytes());
    digest.update(application_id.as_bytes());
    digest.update(node_id.as_bytes());
    digest.update([0]);
    digest.update(kind.as_bytes());
    if let Some(revision) = revision {
        digest.update(workflow_version_id.as_bytes());
        digest.update(revision.to_be_bytes());
    }
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 UUIDv8 marks this as an application-defined deterministic ID.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn collect_dependencies(
    registry: &NodeRegistry,
    definitions: &BTreeMap<Uuid, WorkflowDefinition>,
    version_ids: &[String],
    visiting: &mut BTreeSet<Uuid>,
    closure: &mut Vec<DependencyClosureEntryV1>,
) -> Result<(), BuildError> {
    let mut ids = version_ids
        .iter()
        .map(|value| {
            Uuid::parse_str(value)
                .map_err(|_| BuildError::Compilation("INVALID_SUBWORKFLOW_VERSION".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        if !visiting.insert(id) {
            return Err(BuildError::DependencyCycle(id));
        }
        let definition = definitions
            .get(&id)
            .ok_or(BuildError::MissingDependency(id))?;
        for referenced_id in direct_dependency_ids(definition)? {
            if visiting.contains(&referenced_id) {
                return Err(BuildError::DependencyCycle(referenced_id));
            }
        }
        let compiled = WorkflowCompiler::new(registry)
            .compile(
                definition,
                &CompileContext {
                    current_workflow_version_id: Some(id.to_string()),
                    ancestor_workflow_version_ids: visiting.iter().map(Uuid::to_string).collect(),
                },
            )
            .map_err(|error| {
                BuildError::Compilation(
                    error
                        .issues
                        .into_iter()
                        .map(|issue| issue.code)
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })?;
        collect_dependencies(
            registry,
            definitions,
            &compiled.subworkflow_version_ids,
            visiting,
            closure,
        )?;
        let hash = agentx_runtime_contracts::content_hash(definition)?;
        closure.push(DependencyClosureEntryV1 {
            kind: DependencyKindV1::CompositeWorkflow,
            object_id: id,
            version: id.to_string(),
            content_hash: hash,
            object_ids: vec![id, composite_ir_object_id(id)],
        });
        visiting.remove(&id);
    }
    closure.sort_by_key(|entry| (entry.kind, entry.object_id));
    closure.dedup_by_key(|entry| (entry.kind, entry.object_id));
    Ok(())
}

fn direct_dependency_ids(definition: &WorkflowDefinition) -> Result<Vec<Uuid>, BuildError> {
    definition
        .nodes
        .iter()
        .filter(|node| node.node_type == "sub_workflow" || node.node_type.starts_with("workflow."))
        .map(|node| {
            node.parameters
                .get("workflowVersionId")
                .and_then(Value::as_str)
                .ok_or_else(|| BuildError::Compilation("SUBWORKFLOW_VERSION_REQUIRED".to_owned()))
                .and_then(|value| {
                    Uuid::parse_str(value).map_err(|_| {
                        BuildError::Compilation("INVALID_SUBWORKFLOW_VERSION".to_owned())
                    })
                })
        })
        .collect()
}

fn capability_name(capability: &NodeCapability) -> &'static str {
    capability.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentx_runtime_contracts::{RuntimeAuthorizationSnapshotV1, StorageDomain};
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use serde_json::json;

    fn definition() -> WorkflowDefinition {
        serde_json::from_value(json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false},"contexts":{}},
            "nodes":[{"id":"pass","key":"pass","type":"no_op","typeVersion":1,"name":"Pass","parameters":{},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start-pass","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"pass","targetHandle":"main","order":0},
                {"id":"pass-end","sourceNodeId":"pass","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"message":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"pass","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["message"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true}}},
            "settings":{}
        })).unwrap()
    }

    fn source() -> BundleBuildSource {
        let tenant_id = Uuid::from_u128(1);
        let object_id = Uuid::from_u128(6);
        let hash =
            agentx_runtime_contracts::ContentHash::parse(format!("sha256:{}", "a".repeat(64)))
                .unwrap();
        BundleBuildSource {
            bundle_id: Uuid::from_u128(2),
            tenant_id,
            application_id: Uuid::from_u128(3),
            deployment_id: Uuid::from_u128(4),
            workflow_id: Uuid::from_u128(5),
            workflow_version_id: Uuid::from_u128(6),
            workflow_name: "Workflow".into(),
            workflow_version_number: 1,
            workflow_owner_department: None,
            sequence: 1,
            definition: definition(),
            dependency_versions: BTreeMap::new(),
            supported_capabilities: BTreeSet::from(["builtin".into()]),
            input_contract: json!({"type":"object"}),
            output_contract: json!({"type":"object"}),
            resources: vec![],
            authorization: RuntimeAuthorizationSnapshotV1 {
                schema_version: 1,
                tenant_id,
                service_identity_id: Uuid::from_u128(7),
                workflow_id: Uuid::from_u128(5),
                policy_epoch: 1,
                capabilities: BTreeSet::from(["builtin".into()]),
                grant_ids: vec![],
                maximum_policy_staleness_seconds: 72 * 60 * 60,
                captured_at: OffsetDateTime::UNIX_EPOCH,
            },
            triggers: vec![],
            runtime_policy: RuntimePolicyV1 {
                timeout_seconds: 30,
                operation_deadline_seconds: 30,
                ..RuntimePolicyV1::default()
            },
            objects: vec![RuntimeObjectReferenceV1 {
                tenant_id,
                storage_domain: StorageDomain::Runtime,
                object_id,
                object_key: RuntimeObjectReferenceV1::canonical_key(tenant_id, object_id, &hash),
                content_hash: hash,
                size_bytes: 2,
                media_type: "application/json".into(),
            }],
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn work_package_source() -> WorkPackageBuildSource {
        let tenant_id = Uuid::from_u128(1);
        let package_id = Uuid::from_u128(30);
        let definition = definition();
        let compiled = compile_workflow_version(&definition, package_id).unwrap();
        WorkPackageBuildSource {
            package_id,
            tenant_id,
            workflow: agentx_runtime_contracts::ExecutionWorkflowSnapshotV1 {
                id: Uuid::from_u128(5),
                name: "Workflow".into(),
                version_id: package_id,
                version_number: 7,
                owner_department: None,
            },
            origin: agentx_runtime_contracts::ExecutionOriginV1::system(None),
            purpose: WorkPackagePurpose::Debug,
            call_purpose: RuntimeCallPurposeV1::Debug,
            spec: agentx_runtime_contracts::RuntimeWorkPackageSpecV1::Debug {
                draft_revision: 7,
                debug_plan: agentx_runtime_contracts::RuntimeDebugPlanV1::whole(&compiled),
            },
            source_revision: "draft:7".into(),
            definition,
            dependency_versions: BTreeMap::new(),
            supported_capabilities: BTreeSet::from(["builtin".into()]),
            overlay: RuntimeWorkPackageOverlayV1 {
                input: json!({"message":"debug"}),
                ..RuntimeWorkPackageOverlayV1::default()
            },
            resources: vec![],
            authorization: RuntimeAuthorizationSnapshotV1 {
                schema_version: 1,
                tenant_id,
                service_identity_id: Uuid::from_u128(7),
                workflow_id: Uuid::from_u128(5),
                policy_epoch: 1,
                capabilities: BTreeSet::from(["builtin".into()]),
                grant_ids: vec![],
                maximum_policy_staleness_seconds: 72 * 60 * 60,
                captured_at: OffsetDateTime::UNIX_EPOCH,
            },
            objects: vec![],
            runtime_policy: RuntimePolicyV1::default(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            expires_at: OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1),
        }
    }

    #[test]
    fn rebuild_is_deterministic_and_bundle_is_signed() {
        let key = SigningKey::generate(&mut OsRng);
        let first = build_bundle(source(), "bundle-current", &key).unwrap();
        let second = build_bundle(source(), "bundle-current", &key).unwrap();
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(
            first.signature.signature_base64,
            second.signature.signature_base64
        );
        first.verify(&key.verifying_key()).unwrap();
    }

    #[test]
    fn work_package_build_is_deterministic_and_uses_an_independent_key() {
        let bundle_key = SigningKey::generate(&mut OsRng);
        let work_package_key = SigningKey::generate(&mut OsRng);
        let first = build_work_package(
            work_package_source(),
            "work-package-current",
            &work_package_key,
        )
        .unwrap();
        let second = build_work_package(
            work_package_source(),
            "work-package-current",
            &work_package_key,
        )
        .unwrap();
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(
            first.signature.signature_base64,
            second.signature.signature_base64
        );
        first.verify(&work_package_key.verifying_key()).unwrap();
        assert!(first.verify(&bundle_key.verifying_key()).is_err());
    }

    #[test]
    fn non_canonical_object_key_is_rejected() {
        let mut source = source();
        source.objects[0].object_key = "wrong".into();
        assert!(matches!(
            build_bundle(source, "bundle-current", &SigningKey::generate(&mut OsRng)),
            Err(BuildError::NonCanonicalObject)
        ));
    }

    #[test]
    fn missing_fixed_dependency_is_rejected() {
        let child = Uuid::from_u128(20);
        let mut source = source();
        source.definition = composite_definition(&[child]);
        let result = build_bundle(source, "bundle-current", &SigningKey::generate(&mut OsRng));
        assert!(
            matches!(
                result,
                Err(BuildError::MissingDependency(id)) if id == child
            ),
            "{result:?}"
        );
    }

    #[test]
    fn mutable_subworkflow_head_and_unknown_node_version_are_rejected() {
        let mut mutable_head_source = source();
        mutable_head_source.definition.nodes[0].node_type = "sub_workflow".into();
        mutable_head_source.definition.nodes[0].parameters =
            json!({"workflowId": Uuid::from_u128(20)});
        assert!(matches!(
            build_bundle(
                mutable_head_source,
                "bundle-current",
                &SigningKey::generate(&mut OsRng)
            ),
            Err(BuildError::Compilation(code)) if code.contains("SUBWORKFLOW_VERSION_REQUIRED")
        ));

        let mut source = source();
        source.definition.nodes[0].type_version = 999;
        assert!(matches!(
            build_bundle(source, "bundle-current", &SigningKey::generate(&mut OsRng)),
            Err(BuildError::Compilation(code)) if code.contains("UNKNOWN_NODE_VERSION")
        ));
    }

    #[test]
    fn dependency_cycles_are_rejected_before_signing() {
        let root = Uuid::from_u128(6);
        let child = Uuid::from_u128(20);
        let mut source = source();
        source.definition = composite_definition(&[child]);
        source
            .dependency_versions
            .insert(child, composite_definition(&[root]));
        source.objects.push(object(source.tenant_id, child));
        let result = build_bundle(source, "bundle-current", &SigningKey::generate(&mut OsRng));
        assert!(
            matches!(
                result,
                Err(BuildError::DependencyCycle(id)) if id == root
            ),
            "{result:?}"
        );
    }

    #[test]
    fn unsupported_runtime_capability_is_rejected_by_the_builder() {
        let mut source = source();
        source.definition.nodes[0].node_type = "declarative_http".into();
        source.definition.nodes[0].parameters = json!({"url":"https://example.invalid"});
        source.definition.end = Default::default();
        let result = build_bundle(source, "bundle-current", &SigningKey::generate(&mut OsRng));
        assert!(
            matches!(
                result,
                Err(BuildError::UnsupportedCapability(ref capability)) if capability == "declarative_http"
            ),
            "{result:?}"
        );
    }

    #[test]
    fn remote_action_produces_deterministic_poll_and_lifecycle_triggers() {
        let mut source = source();
        source.supported_capabilities.insert("remote_action".into());
        source.definition.nodes[0].node_type = "remote_action".into();
        source.definition.nodes[0].parameters = json!({
            "endpoint":"http://echo-node:8080/",
            "pollIntervalSeconds":7,
            "eventId":"evt-7",
            "pollInput":{"source":"poll"}
        });
        source.definition.end = Default::default();
        let key = SigningKey::generate(&mut OsRng);
        let first = build_bundle(source.clone(), "bundle-current", &key).unwrap();
        let second = build_bundle(source, "bundle-current", &key).unwrap();
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.payload.triggers.len(), 3);
        assert_eq!(
            first
                .payload
                .triggers
                .iter()
                .map(|trigger| trigger.trigger_id)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let poll = first
            .payload
            .triggers
            .iter()
            .find(|trigger| {
                matches!(
                    trigger.configuration,
                    agentx_runtime_contracts::RuntimeTriggerConfigurationV1::Poll { .. }
                )
            })
            .unwrap();
        match &poll.configuration {
            agentx_runtime_contracts::RuntimeTriggerConfigurationV1::Poll {
                interval_seconds,
                provider_endpoint,
                input,
            } => {
                assert_eq!(*interval_seconds, 7);
                assert_eq!(
                    provider_endpoint,
                    "http://echo-node:8080/agentx/node/v1/lifecycle/poll"
                );
                assert_eq!(input["workflowVersionId"], Uuid::from_u128(6).to_string());
            }
            _ => unreachable!(),
        }
        let lifecycle_enabled = first
            .payload
            .triggers
            .iter()
            .filter_map(|trigger| match &trigger.configuration {
                agentx_runtime_contracts::RuntimeTriggerConfigurationV1::Lifecycle {
                    operation,
                    ..
                } => Some((*operation, trigger.enabled)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(lifecycle_enabled.contains(&(
            agentx_runtime_contracts::LifecycleOperationV1::Activate,
            true
        )));
        assert!(lifecycle_enabled.contains(&(
            agentx_runtime_contracts::LifecycleOperationV1::Deactivate,
            false
        )));
    }

    #[test]
    fn transitive_dependency_closure_is_sorted_complete_and_deterministic() {
        let first_child = Uuid::from_u128(20);
        let grandchild = Uuid::from_u128(10);
        let mut source = source();
        source.definition = composite_definition(&[first_child]);
        source
            .dependency_versions
            .insert(first_child, composite_definition(&[grandchild]));
        source.dependency_versions.insert(grandchild, definition());
        source.objects.extend([
            object(source.tenant_id, first_child),
            object(source.tenant_id, composite_ir_object_id(first_child)),
            object(source.tenant_id, grandchild),
            object(source.tenant_id, composite_ir_object_id(grandchild)),
        ]);
        let key = SigningKey::generate(&mut OsRng);
        let first = build_bundle(source.clone(), "bundle-current", &key).unwrap();
        let second = build_bundle(source, "bundle-current", &key).unwrap();
        let entries = &first.payload.dependency_closure.entries;
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.object_id)
                .collect::<Vec<_>>(),
            vec![grandchild, first_child]
        );
        assert!(
            entries.iter().all(|entry| entry.object_ids
                == [entry.object_id, composite_ir_object_id(entry.object_id)])
        );
        assert_eq!(first.content_hash, second.content_hash);
    }

    #[test]
    fn immutable_composite_registry_pins_version_io_and_context_contracts() {
        let child_id = Uuid::from_u128(42);
        let child: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"5.0",
            "start":{
                "inputs":{"type":"object","required":["question"],"properties":{"question":{"type":"string"}},"additionalProperties":false},
                "contexts":{"counter":{"schema":{"type":"number"},"default":0,"mutable":true,"sensitive":false,"clientWritable":false,"scope":"execution_tree","mergePolicy":"increment"}}
            },
            "nodes":[],
            "connections":[{"id":"direct","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}],
            "end":{"outputs":{"answer":{"value":{"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["question"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true,"sensitive":false}}},
            "settings":{}
        }))
        .unwrap();
        let definitions = BTreeMap::from([(child_id, child)]);
        let registry = node_registry_with_composites(&definitions).unwrap();
        let node_type = format!("workflow.{}", child_id.simple());
        let manifest = registry.get(&node_type, 1).unwrap();
        assert_eq!(
            manifest.parameter_schema["properties"]["workflowVersionId"]["const"],
            child_id.to_string()
        );
        assert_eq!(
            manifest.parameter_schema["properties"]["inputs"]["allOf"][0]["required"],
            json!(["question"])
        );
        assert_eq!(
            manifest.parameter_schema["properties"]["inputs"]["x-agentx-dynamicValue"]["allowedNamespaces"],
            json!(["inputs", "outputs", "contexts", "execution"])
        );
        assert_eq!(manifest.output_schema["required"], json!(["answer"]));
        assert_eq!(
            manifest.parameter_schema["x-agentx-contextContract"]["counter"]["mergePolicy"],
            "increment"
        );

        let parent: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"5.0",
            "start":{
                "inputs":{"type":"object","required":["question"],"properties":{"question":{"type":"string"}},"additionalProperties":false},
                "contexts":{"counter":{"schema":{"type":"number"},"default":0,"mutable":true,"sensitive":false,"clientWritable":false,"scope":"execution_tree","mergePolicy":"increment"}}
            },
            "nodes":[
                {"id":"child","key":"child","type":node_type,"typeVersion":1,"name":"Child","parameters":{"workflowVersionId":child_id,"inputs":{"question":{"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["question"]},"missingPolicy":{"kind":"error"}}}},"outputProjection":{},"contextWrites":[],"resourceReferences":[]},
                {"id":"summary","key":"summary","type":"set","typeVersion":1,"name":"Summary","parameters":{"values":{"answer":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"child","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["answer"]},"missingPolicy":{"kind":"error"}},"counter":{"kind":"reference","selector":{"namespace":"contexts","run":{"kind":"current"},"item":{"kind":"current"},"path":["counter"]},"missingPolicy":{"kind":"error"}}},"keepOnlySet":true},"outputProjection":{"main":{"answer_text":{"value":{"kind":"reference","selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["answer"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"sensitive":false},"counter_value":{"value":{"kind":"reference","selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["counter"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"number"},"sensitive":false}}},"contextWrites":[],"resourceReferences":[]}
            ],
            "connections":[
                {"id":"start-child","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"child","targetHandle":"main","order":0},
                {"id":"child-summary","sourceNodeId":"child","sourceHandle":"main","targetNodeId":"summary","targetHandle":"main","order":0},
                {"id":"summary-end","sourceNodeId":"summary","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"answer":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"summary","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["answer_text"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"string"},"required":true,"sensitive":false},"counter":{"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"summary","port":"main","run":{"kind":"current"},"item":{"kind":"current"},"path":["counter_value"]},"missingPolicy":{"kind":"error"}},"schema":{"type":"number"},"required":true,"sensitive":false}}},
            "settings":{}
        }))
        .unwrap();
        compile_workflow_version_with_dependencies(&parent, Uuid::from_u128(43), &definitions)
            .unwrap();
    }

    #[test]
    fn immutable_composite_alias_rejects_missing_and_mismatched_versions() {
        let child_id = Uuid::from_u128(44);
        let node_type = format!("workflow.{}", child_id.simple());
        let mut parent = composite_definition(&[child_id]);
        parent.nodes[0].node_type = node_type;
        parent.nodes[0].parameters["inputs"] = json!({"message":{"kind":"reference","selector":{"namespace":"inputs","run":{"kind":"current"},"item":{"kind":"current"},"path":["message"]},"missingPolicy":{"kind":"error"}}});

        let missing = compile_workflow_version_with_dependencies(
            &parent,
            Uuid::from_u128(45),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(
            matches!(missing, BuildError::Compilation(ref code) if code.contains("UNKNOWN_NODE_VERSION"))
        );

        let definitions = BTreeMap::from([(child_id, definition())]);
        parent.nodes[0].parameters["workflowVersionId"] =
            Value::String(Uuid::from_u128(46).to_string());
        let mismatch =
            compile_workflow_version_with_dependencies(&parent, Uuid::from_u128(45), &definitions)
                .unwrap_err();
        assert!(
            matches!(mismatch, BuildError::Compilation(ref code) if code.contains("COMPOSITE_VERSION_MISMATCH"))
        );
    }

    fn composite_definition(children: &[Uuid]) -> WorkflowDefinition {
        let mut value = serde_json::to_value(definition()).unwrap();
        let nodes = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                json!({
                    "id": format!("child_{index}"),
                    "key": format!("child_{index}"),
                    "type": "sub_workflow",
                    "typeVersion": 1,
                    "name": format!("Child {index}"),
                    "parameters": {"workflowVersionId": child},
                    "outputProjection": {},
                    "contextWrites": [],
                    "resourceReferences": []
                })
            })
            .collect::<Vec<_>>();
        let mut connections = Vec::new();
        for index in 0..children.len() {
            connections.push(json!({
                "id": format!("edge-{index}"),
                "sourceNodeId": if index == 0 { "__start__".to_owned() } else { format!("child_{}", index - 1) },
                "sourceHandle": "main",
                "targetNodeId": format!("child_{index}"),
                "targetHandle": "main",
                "order": 0
            }));
        }
        if !children.is_empty() {
            connections.push(json!({
                "id": "edge-end",
                "sourceNodeId": format!("child_{}", children.len() - 1),
                "sourceHandle": "main",
                "targetNodeId": "__end__",
                "targetHandle": "main",
                "order": 0
            }));
        }
        value["nodes"] = Value::Array(nodes);
        value["connections"] = Value::Array(connections);
        value["end"] = json!({"outputs": {}});
        serde_json::from_value(value).unwrap()
    }

    fn object(tenant_id: Uuid, object_id: Uuid) -> RuntimeObjectReferenceV1 {
        let hash =
            agentx_runtime_contracts::ContentHash::parse(format!("sha256:{}", "b".repeat(64)))
                .unwrap();
        RuntimeObjectReferenceV1 {
            tenant_id,
            storage_domain: StorageDomain::Runtime,
            object_id,
            object_key: RuntimeObjectReferenceV1::canonical_key(tenant_id, object_id, &hash),
            content_hash: hash,
            size_bytes: 2,
            media_type: "application/json".into(),
        }
    }
}
