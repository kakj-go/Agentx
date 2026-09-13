use agentx_agent_core::*;

fn reference(resource_type: &str, id: &str) -> ResourceReferenceV1 {
    ResourceReferenceV1 {
        resource_type: resource_type.into(),
        resource_id: id.into(),
        resource_version_id: format!("{id}-version"),
        operation: "use".into(),
    }
}

fn definition(sandbox: bool) -> AgentDefinitionV6 {
    AgentDefinitionV6 {
        api_version: "6.0".into(),
        model: reference("model", "model-1"),
        workspace_sandbox: sandbox.then(|| reference("sandbox_profile", "sandbox-1")),
        session_policy: SessionPolicyV1 {
            mode: SessionPolicyModeV1::Invocation,
        },
        mcp_tools: vec![reference("mcp_tool", "tool-1")],
        skills: vec![],
        knowledge: vec![],
        long_term_memory: None,
    }
}

#[test]
fn definition_deterministically_derives_bundle_and_core_tools() {
    let without = derive_agent_bundle_v2(&definition(false)).expect("valid definition");
    assert!(without.core_tools.is_empty());
    assert!(without.workspace_sandbox.is_none());

    let with = derive_agent_bundle_v2(&definition(true)).expect("valid definition");
    assert_eq!(
        with.core_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["read", "write", "edit", "bash"]
    );
    assert_eq!(with, derive_agent_bundle_v2(&definition(true)).unwrap());
}

#[test]
fn model_is_an_inspector_only_required_slot() {
    let manifest = frozen_agent_manifest_v2();
    let model = manifest
        .resource_slots
        .iter()
        .find(|slot| slot.name == "model")
        .expect("model slot");
    assert_eq!(model.placement, SlotPlacementV2::Inspector);
    assert_eq!((model.minimum, model.maximum), (1, Some(1)));
}

#[test]
fn stdio_requires_its_own_runtime_sandbox_and_http_forbids_one() {
    let mut stdio = McpServerVersionV1 {
        server_version_id: "server-version-1".into(),
        transport: McpTransportV1::Stdio {
            command: "/opt/mcp/server".into(),
            args: vec!["--stdio".into()],
            environment_credential_refs: vec![],
        },
        runtime_sandbox: None,
    };
    assert_eq!(
        stdio.validate(),
        Err(ContractValidationError::McpStdioSandboxRequired)
    );
    stdio.runtime_sandbox = Some(reference("sandbox_profile", "mcp-sandbox"));
    assert!(stdio.validate().is_ok());

    let http = McpServerVersionV1 {
        server_version_id: "server-version-2".into(),
        transport: McpTransportV1::StreamableHttp {
            endpoint: "https://mcp.example/mcp".into(),
            credential: None,
        },
        runtime_sandbox: Some(reference("sandbox_profile", "wrong")),
    };
    assert_eq!(
        http.validate(),
        Err(ContractValidationError::McpRuntimeSandboxForbidden)
    );
}

#[test]
fn contracts_round_trip_and_reject_unknown_fields_or_enums() {
    let value = serde_json::to_value(definition(true)).expect("serialize definition");
    let decoded: AgentDefinitionV6 =
        serde_json::from_value(value.clone()).expect("round-trip definition");
    assert_eq!(decoded, definition(true));

    let mut unknown_field = value.clone();
    unknown_field["legacyAiModel"] = serde_json::json!("forbidden");
    assert!(serde_json::from_value::<AgentDefinitionV6>(unknown_field).is_err());

    let mut unknown_enum = value;
    unknown_enum["sessionPolicy"]["mode"] = serde_json::json!("implicit_user");
    assert!(serde_json::from_value::<AgentDefinitionV6>(unknown_enum).is_err());
}
