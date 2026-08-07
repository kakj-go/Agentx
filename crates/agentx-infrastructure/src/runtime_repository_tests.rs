use super::*;

#[test]
fn invocation_input_preserves_n8n_item_shape() {
    assert_eq!(invocation_items(&json!([{"a":1},{"a":2}])).len(), 2);
    assert_eq!(invocation_items(&json!({"a":1}))[0].json["a"], 1)
}

#[test]
fn approval_resume_payload_is_persisted_as_its_protocol_output() {
    let outputs = resumed_output_map("approved", &json!({"decision":"approved"}));
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs["approved"][0].json["decision"], "approved");
    assert!(!outputs.contains_key("main"));
}

#[test]
fn dispatch_message_round_trips() {
    let value = DispatchMessage {
        tenant_id: Uuid::nil(),
        execution_id: Uuid::nil(),
        node_execution_id: Uuid::nil(),
        attempt_id: Uuid::nil(),
        capability: "builtin".into(),
        node_protocol_version: agentx_node_protocol::NODE_PROTOCOL_VERSION.into(),
        compiler_version: "agentx-workflow-3.0.0".into(),
        ir_schema_version: "3.0".into(),
    };
    assert_eq!(
        serde_json::from_value::<DispatchMessage>(serde_json::to_value(value).unwrap())
            .unwrap()
            .capability,
        "builtin"
    )
}

#[test]
fn debug_overlay_accepts_port_maps_and_plain_json() {
    let ports = overlay_items(&json!({"main":[{"json":{"value":1}}]}));
    assert_eq!(ports["main"][0].json["value"], 1);
    let plain = overlay_items(&json!([{"value":2}]));
    assert_eq!(plain["main"][0].json[0]["value"], 2);
    let snapshot = json!({"items":[{"nodeId":"agent","kind":"mock_output","payload":{"ok":true}}]});
    let (kind, payload) = debug_overlay_for_node(&snapshot, "agent").unwrap();
    assert_eq!(kind, "mock_output");
    assert_eq!(payload["ok"], true);
}

#[test]
fn draft_revision_snapshot_hash_is_stable_and_covers_debug_inputs() {
    let definition = json!({"schemaVersion":"3.0","nodes":[],"connections":[],"settings":{}});
    let source = json!({"kind":"draft_revision","id":Uuid::nil(),"revision":7});
    let manifest = json!([{"nodeType":"manual_trigger","version":1}]);
    let resources = json!({"schemaVersion":"1.0","resources":[]});
    let base = execution_snapshot_hash(
        &definition,
        "sha256:compiled",
        &source,
        &manifest,
        &resources,
        &json!({"sideEffectDecisions":{}}),
        &json!({"mode":"full"}),
        &json!({"items":[]}),
    )
    .unwrap();
    let repeated = execution_snapshot_hash(
        &definition,
        "sha256:compiled",
        &source,
        &manifest,
        &resources,
        &json!({"sideEffectDecisions":{}}),
        &json!({"mode":"full"}),
        &json!({"items":[]}),
    )
    .unwrap();
    assert_eq!(base, repeated);

    let changed_overlay = execution_snapshot_hash(
        &definition,
        "sha256:compiled",
        &source,
        &manifest,
        &resources,
        &json!({"sideEffectDecisions":{}}),
        &json!({"mode":"full"}),
        &json!({"items":[{"nodeId":"agent","kind":"mock_output"}]}),
    )
    .unwrap();
    assert_ne!(base, changed_overlay);
}

#[test]
fn draft_resource_snapshots_allow_resolved_versions_and_transitive_dependencies() {
    let resource_id = Uuid::from_u128(1);
    let version_id = Uuid::from_u128(2);
    let expected = ResourceReference {
        binding_id: Some("model-binding".into()),
        binding_role: Some("ai_model".into()),
        resource_type: agentx_domain::ResourceType::Model,
        resource_id,
        resource_version_id: None,
        operation: agentx_domain::ResourceOperation::Use,
    };
    let mut definition = WorkflowDefinition::empty();
    definition.nodes[0].resource_references = vec![expected.clone()];
    let direct = RuntimeResourceSnapshot {
        node_id: "manual-trigger".into(),
        reference: ResourceReference {
            resource_version_id: Some(version_id),
            ..expected
        },
        snapshot_hash: "sha256:direct".into(),
        snapshot: json!({}),
    };
    let dependency = RuntimeResourceSnapshot {
        node_id: "manual-trigger".into(),
        reference: ResourceReference {
            binding_id: None,
            binding_role: None,
            resource_type: agentx_domain::ResourceType::Credential,
            resource_id: Uuid::from_u128(3),
            resource_version_id: None,
            operation: agentx_domain::ResourceOperation::Use,
        },
        snapshot_hash: "sha256:dependency".into(),
        snapshot: json!({}),
    };
    assert!(draft_resource_snapshots_cover_definition(
        &definition,
        &[direct.clone(), dependency.clone()]
    ));
    assert!(!draft_resource_snapshots_cover_definition(
        &definition,
        &[
            RuntimeResourceSnapshot {
                node_id: "unknown".into(),
                ..dependency
            },
            direct
        ]
    ));
}
