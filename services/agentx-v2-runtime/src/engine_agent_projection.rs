use agentx_runtime_contracts::{AGENT_BUNDLE_VERSION, AgentBundleContractV2, CompiledAgentNodeV2};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    engine_protocol::runtime_bad_request,
    error::{RuntimeError, RuntimeResult},
};

pub(super) fn inject_agent_runtime_parameters(
    raw_parameters: &mut Value,
    compiled_agent: &CompiledAgentNodeV2,
    agent_bundle: &AgentBundleContractV2,
    definition_hash: &str,
    node_id: &str,
    bundle_id: Uuid,
) -> RuntimeResult<()> {
    if agent_bundle.bundle_version != AGENT_BUNDLE_VERSION {
        return Err(runtime_bad_request(
            "AGENT_BUNDLE_VERSION_UNSUPPORTED",
            "Execution snapshot must contain Agent Bundle 2.0",
        ));
    }
    if agent_bundle.definition_hash != definition_hash {
        return Err(runtime_bad_request(
            "AGENT_BUNDLE_DEFINITION_MISMATCH",
            "Agent Bundle definition hash does not match the compiled Workflow snapshot",
        ));
    }
    let entry = agent_bundle
        .agents
        .iter()
        .find(|entry| entry.node_id == node_id)
        .ok_or_else(|| {
            runtime_bad_request(
                "AGENT_BUNDLE_NODE_MISSING",
                "Agent Bundle does not contain the compiled Agent node",
            )
        })?;
    if &entry.configuration != compiled_agent {
        return Err(runtime_bad_request(
            "AGENT_BUNDLE_CONFIGURATION_MISMATCH",
            "Agent Bundle configuration does not match the compiled Agent node",
        ));
    }
    let object = raw_parameters.as_object_mut().ok_or_else(|| {
        RuntimeError::Internal(anyhow::anyhow!(
            "compiled Agent parameters must be a JSON object"
        ))
    })?;
    object.insert(
        "_agent".into(),
        json!({
            "contractVersion": agent_bundle.core_contract_version,
            "sessionPolicy": entry.configuration.session_policy,
            "model": entry.configuration.model,
            "workspaceSandbox": entry.configuration.workspace_sandbox,
            "canvasAttachments": entry.configuration.canvas_attachments,
            "coreTools": entry.configuration.core_tools,
            "attachmentRegistry": entry.attachment_registry,
            "definitionHash": agent_bundle.definition_hash,
            "bundleHash": bundle_id,
            "stableAgentNodeKey": node_id,
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use agentx_runtime_contracts::{
        AGENT_CORE_CONTRACT_VERSION, AgentAttachmentRegistryV1, AgentBundleContractV2,
        CompiledAgentBundleEntryV2,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::inject_agent_runtime_parameters;

    fn compiled_agent() -> agentx_runtime_contracts::CompiledAgentNodeV2 {
        serde_json::from_value(json!({
            "contractVersion":"2.0",
            "sessionPolicy":"invocation",
            "model":{
                "bindingRole":"model",
                "resourceType":"model",
                "resourceId":Uuid::nil(),
                "resourceVersionId":Uuid::from_u128(1),
                "operation":"use"
            },
            "canvasAttachments":[],
            "coreTools":[]
        }))
        .unwrap()
    }

    fn bundle(agent: &agentx_runtime_contracts::CompiledAgentNodeV2) -> AgentBundleContractV2 {
        let mut bundle = AgentBundleContractV2::empty("definition-hash", "compiler-test");
        bundle.agents.push(CompiledAgentBundleEntryV2 {
            node_id: "agent-node".into(),
            configuration: agent.clone(),
            resource_closure: vec![],
            attachment_registry: AgentAttachmentRegistryV1::default(),
        });
        bundle
    }

    #[test]
    fn worker_projection_uses_frozen_core_contract_not_compiled_node_contract() {
        let agent = compiled_agent();
        let bundle = bundle(&agent);
        let mut parameters = json!({"prompt":"hello"});
        inject_agent_runtime_parameters(
            &mut parameters,
            &agent,
            &bundle,
            "definition-hash",
            "agent-node",
            Uuid::from_u128(2),
        )
        .unwrap();

        assert_eq!(
            parameters.pointer("/_agent/contractVersion"),
            Some(&Value::String(AGENT_CORE_CONTRACT_VERSION.into()))
        );
        assert_ne!(
            parameters.pointer("/_agent/contractVersion"),
            Some(&Value::String(agent.contract_version))
        );
    }

    #[test]
    fn worker_projection_rejects_bundle_configuration_drift() {
        let agent = compiled_agent();
        let mut bundle = bundle(&agent);
        bundle.agents[0].configuration.contract_version = "drift".into();
        let error = inject_agent_runtime_parameters(
            &mut json!({}),
            &agent,
            &bundle,
            "definition-hash",
            "agent-node",
            Uuid::from_u128(2),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("AGENT_BUNDLE_CONFIGURATION_MISMATCH")
        );
    }
}
