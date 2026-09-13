use super::*;

fn compiled() -> agentx_runtime_contracts::CompiledWorkflowV1 {
    let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion":"8.0",
            "start":{"inputs":{"type":"object","additionalProperties":true},"contexts":{}},
            "nodes":[
                {"id":"root","key":"root","type":"set","typeVersion":1,"name":"Root","disabled":false,"parameters":{},"contextWrites":[],"resourceReferences":[],"settings":{}},
                {"id":"target","key":"target","type":"set","typeVersion":1,"name":"Target","disabled":false,"parameters":{},"contextWrites":[],"resourceReferences":[],"settings":{}},
                {"id":"tail","key":"tail","type":"set","typeVersion":1,"name":"Tail","disabled":false,"parameters":{},"contextWrites":[],"resourceReferences":[],"settings":{}},
                {"id":"__exit__","key":"__exit__","type":"exit","typeVersion":1,"name":"End","disabled":false,"protected":true,"parameters":{"outputs":{},"errorOutputs":{}},"contextWrites":[],"resourceReferences":[],"settings":{}}
            ],
            "connections":[
                {"id":"start-root","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"root","targetHandle":"main","order":0},
                {"id":"root-target","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"target","targetHandle":"main","order":0},
                {"id":"target-tail","sourceNodeId":"target","sourceHandle":"main","targetNodeId":"tail","targetHandle":"main","order":0},
                {"id":"tail-end","sourceNodeId":"tail","sourceHandle":"main","targetNodeId":"__exit__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}},
            "settings":{"activationBudget":8,"executionOrder":"deterministic"}
        }))
        .expect("test Workflow parses");
    compile_workflow_version_with_dependencies(&definition, Uuid::nil(), &BTreeMap::new())
        .expect("test Workflow compiles")
}

#[test]
fn partial_debug_plan_freezes_graph_reachability() {
    let compiled = compiled();
    let manual = || RuntimeDebugInputSourceV1::Manual {
        value: json!({"value":1}),
    };
    let node = build_debug_plan(
        &compiled,
        PartialExecutionModeV1::Node,
        Some("target".into()),
        Some(manual()),
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(node.included_node_ids, ["target"]);

    let to = build_debug_plan(
        &compiled,
        PartialExecutionModeV1::ToNode,
        Some("target".into()),
        None,
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(to.included_node_ids, ["root", "target"]);

    let from = build_debug_plan(
        &compiled,
        PartialExecutionModeV1::FromNode,
        Some("target".into()),
        Some(manual()),
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(from.included_node_ids, ["target", "tail"]);
}

#[test]
fn whole_published_execution_authorizes_all_irreversible_nodes() {
    let mut compiled = compiled();
    compiled.nodes[1].side_effect_level = agentx_node_protocol::SideEffectLevel::Irreversible;
    let plan = build_whole_execution_plan(&compiled).unwrap();
    assert_eq!(plan.included_node_ids, ["root", "target", "tail"]);
    assert_eq!(
        plan.side_effect_decisions.get("target"),
        Some(&SideEffectResolutionV1::Execute)
    );
}
