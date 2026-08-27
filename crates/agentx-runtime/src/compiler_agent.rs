use agentx_domain::{ResourceReference, ResourceType, WorkflowNode};
use agentx_node_protocol::{BindingSlotPlacement, NodeManifestVersion};
use agentx_runtime_contracts::{
    AgentSessionPolicyModeV2, CompiledAgentAttachmentV2, CompiledAgentNodeV2,
    CompiledAgentResourceReferenceV2, CoreToolReplayPolicyV2, DerivedCoreToolV2,
};
use serde_json::Value;

use super::{CompileContext, CompileIssue};

const CORE_TOOL_NAMES: [&str; 4] = ["read", "write", "edit", "bash"];

pub(super) fn validate_binding_slots(
    definition_index: usize,
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
    context: &CompileContext,
    issues: &mut Vec<CompileIssue>,
) {
    // Manifest 2.0 placement slots are an Agent-only contract. Standalone
    // resource nodes keep their existing resource-reference semantics.
    if node.node_type != "agent" {
        return;
    }
    for slot in &manifest.binding_slots {
        let count = node
            .resource_references
            .iter()
            .filter(|reference| {
                reference.resource_type == slot.resource_type
                    && match slot.placement {
                        BindingSlotPlacement::Inspector => {
                            reference.binding_id.is_none() && reference.binding_role.is_none()
                        }
                        BindingSlotPlacement::Canvas => {
                            reference.binding_id.is_some()
                                && reference.binding_role.as_deref() == Some(slot.name.as_str())
                        }
                    }
            })
            .count();
        if slot.required && count == 0 {
            issues.push(CompileIssue {
                code: "BINDING_REQUIRED".into(),
                path: format!("nodes[{definition_index}].resourceReferences"),
                message: format!("Binding slot '{}' is required", slot.name),
            });
        }
        if !slot.multiple && count > 1 {
            issues.push(CompileIssue {
                code: "BINDING_MULTIPLE_NOT_ALLOWED".into(),
                path: format!("nodes[{definition_index}].resourceReferences"),
                message: format!("Binding slot '{}' accepts only one resource", slot.name),
            });
        }
    }
    let derives_core_tools = node.resource_references.iter().any(|candidate| {
        candidate.resource_type == ResourceType::SandboxProfile
            && candidate.binding_id.is_none()
            && candidate.binding_role.is_none()
    });
    for (reference_index, reference) in node.resource_references.iter().enumerate() {
        let slot = manifest.binding_slots.iter().find(|slot| {
            slot.resource_type == reference.resource_type
                && match slot.placement {
                    BindingSlotPlacement::Inspector => {
                        reference.binding_id.is_none() && reference.binding_role.is_none()
                    }
                    BindingSlotPlacement::Canvas => {
                        reference.binding_id.is_some()
                            && reference.binding_role.as_deref() == Some(slot.name.as_str())
                    }
                }
        });
        let Some(slot) = slot else {
            issues.push(CompileIssue {
                code: "INVALID_BINDING_SLOT".into(),
                path: format!("nodes[{definition_index}].resourceReferences[{reference_index}]"),
                message: "Agent resource reference does not match a Manifest 2.0 slot or placement"
                    .into(),
            });
            continue;
        };
        if slot.placement == BindingSlotPlacement::Canvas && reference.resource_version_id.is_none()
        {
            issues.push(CompileIssue {
                code: "RESOURCE_VERSION_REQUIRED".into(),
                path: format!(
                    "nodes[{definition_index}].resourceReferences[{reference_index}].resourceVersionId"
                ),
                message: "Agent attachments must resolve to an exact resource version".into(),
            });
        }
        if node.node_type == "agent"
            && derives_core_tools
            && reference.resource_type == ResourceType::McpTool
            && context
                .resource_tool_names
                .get(&reference.resource_id)
                .is_some_and(|name| CORE_TOOL_NAMES.contains(&name.as_str()))
        {
            issues.push(CompileIssue {
                code: "AGENT_CORE_TOOL_NAME_CONFLICT".into(),
                path: format!("nodes[{definition_index}].resourceReferences[{reference_index}]"),
                message: "Attachment tool name conflicts with a derived Agent core tool".into(),
            });
        }
    }
}

pub(super) fn compile_agent_node(
    node: &WorkflowNode,
    manifest: &NodeManifestVersion,
) -> Option<CompiledAgentNodeV2> {
    if manifest.capability != agentx_node_protocol::NodeCapability::Agent {
        return None;
    }
    let session_policy = match node
        .parameters
        .pointer("/sessionPolicy/mode")
        .and_then(Value::as_str)
        .expect("Agent session policy validated")
    {
        "application_session" => AgentSessionPolicyModeV2::ApplicationSession,
        "invocation" => AgentSessionPolicyModeV2::Invocation,
        _ => unreachable!("Agent session policy validated"),
    };
    let model = node
        .resource_references
        .iter()
        .find(|reference| {
            reference.resource_type == ResourceType::Model
                && reference.binding_id.is_none()
                && reference.binding_role.is_none()
        })
        .map(|reference| compiled_inspector_reference(reference, "model"))
        .expect("Agent model validated");
    let workspace_sandbox = node
        .resource_references
        .iter()
        .find(|reference| {
            reference.resource_type == ResourceType::SandboxProfile
                && reference.binding_id.is_none()
                && reference.binding_role.is_none()
        })
        .map(|reference| compiled_inspector_reference(reference, "workspace_sandbox"));
    let mut canvas_attachments = node
        .resource_references
        .iter()
        .filter(|reference| reference.binding_id.is_some())
        .map(|reference| CompiledAgentAttachmentV2 {
            binding_id: reference
                .binding_id
                .clone()
                .expect("Canvas binding validated"),
            binding_role: reference
                .binding_role
                .clone()
                .expect("Canvas binding role validated"),
            resource_type: reference.resource_type,
            resource_id: reference.resource_id,
            resource_version_id: reference
                .resource_version_id
                .expect("Agent attachment version validated"),
            operation: reference.operation,
        })
        .collect::<Vec<_>>();
    canvas_attachments.sort_by(|left, right| {
        (&left.binding_role, &left.binding_id, left.resource_id).cmp(&(
            &right.binding_role,
            &right.binding_id,
            right.resource_id,
        ))
    });
    let core_tools = workspace_sandbox
        .as_ref()
        .map(|sandbox| {
            CORE_TOOL_NAMES
                .into_iter()
                .map(|name| DerivedCoreToolV2 {
                    name: name.into(),
                    replay_policy: if name == "read" {
                        CoreToolReplayPolicyV2::Safe
                    } else {
                        CoreToolReplayPolicyV2::Never
                    },
                    workspace_sandbox_resource_id: sandbox.resource_id,
                    workspace_sandbox_version_id: sandbox.resource_version_id,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(CompiledAgentNodeV2 {
        contract_version: "2.0".into(),
        session_policy,
        model,
        workspace_sandbox,
        canvas_attachments,
        core_tools,
    })
}

fn compiled_inspector_reference(
    reference: &ResourceReference,
    binding_role: &str,
) -> CompiledAgentResourceReferenceV2 {
    CompiledAgentResourceReferenceV2 {
        binding_role: binding_role.into(),
        resource_type: reference.resource_type,
        resource_id: reference.resource_id,
        resource_version_id: reference
            .resource_version_id
            .expect("Inspector resource version validated"),
        operation: reference.operation,
    }
}
