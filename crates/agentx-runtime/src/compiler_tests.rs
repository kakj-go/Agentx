use super::*;
use agentx_domain::ValueCoercion;
use agentx_node_protocol::OutputCardinality;

fn reference(
    namespace: ValueNamespace,
    source_node_id: Option<&str>,
    port: Option<&str>,
    item: ValueSelection,
    path: &[&str],
) -> DynamicValue {
    DynamicValue::Reference {
        selector: ValueSelector {
            namespace,
            source_node_id: source_node_id.map(str::to_owned),
            port: port.map(str::to_owned),
            run: ValueSelection::Current,
            item,
            path: path
                .iter()
                .map(|value| ValuePathSegment::Key((*value).into()))
                .collect(),
        },
        missing_policy: MissingValuePolicy::Error,
        coerce: None,
    }
}

fn reference_json(
    namespace: ValueNamespace,
    source_node_id: Option<&str>,
    port: Option<&str>,
    item: ValueSelection,
    path: &[&str],
) -> serde_json::Value {
    serde_json::to_value(reference(namespace, source_node_id, port, item, path)).unwrap()
}

fn fixture() -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "settings":{"activationBudget":20,"executionOrder":"deterministic"},
            "nodes":[
                {"id":"root","key":"root","type":"no_op","typeVersion":1,"name":"Root","outputProjection":{},"contextWrites":[]},
                {"id":"if","key":"condition","type":"if","typeVersion":1,"name":"IF","parameters":{"condition":reference_json(ValueNamespace::Item,None,None,ValueSelection::Current,&["ok"])},"outputProjection":{},"contextWrites":[]},
                {"id":"merge","key":"merge","type":"merge","typeVersion":1,"name":"Merge","outputProjection":{},"contextWrites":[]},
                {"id":"loop","key":"loop","type":"loop_over_items","typeVersion":1,"name":"Loop","outputProjection":{},"contextWrites":[]}
            ],
            "connections":[
                {"id":"__start__-root","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"root","targetHandle":"main","order":0},
                {"id":"a","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"if","targetHandle":"main","order":0},
                {"id":"b","sourceNodeId":"if","sourceHandle":"true","targetNodeId":"merge","targetHandle":"main:0","order":0},
                {"id":"c","sourceNodeId":"if","sourceHandle":"false","targetNodeId":"merge","targetHandle":"main:1","order":1},
                {"id":"d","sourceNodeId":"merge","sourceHandle":"main","targetNodeId":"loop","targetHandle":"main","order":0},
                {"id":"e","sourceNodeId":"loop","sourceHandle":"loop","targetNodeId":"merge","targetHandle":"main:2","order":0},
                {"id":"loop-end","sourceNodeId":"loop","sourceHandle":"done","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        })).unwrap()
}

#[test]
fn compilation_is_deterministic_and_marks_cycles() {
    let registry = NodeRegistry::m4_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let first = compiler
        .compile(&fixture(), &CompileContext::default())
        .unwrap();
    let second = compiler
        .compile(&fixture(), &CompileContext::default())
        .unwrap();
    assert_eq!(first.canonical_hash, second.canonical_hash);
    assert!(
        first
            .connections
            .iter()
            .any(|connection| connection.back_edge)
    );
    assert_eq!(first.connections[1].branch_order, 0);
    assert_eq!(first.connections[2].branch_order, 1);
}

#[test]
fn rejects_unknown_ports_and_recursive_subworkflows() {
    let registry = NodeRegistry::m4_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition = fixture();
    definition
        .connections
        .iter_mut()
        .find(|connection| connection.id == "a")
        .expect("fixture edge")
        .source_handle = "missing".into();
    definition.nodes.push(serde_json::from_value(serde_json::json!({
            "id":"sub","key":"sub","type":"sub_workflow","typeVersion":1,"name":"Sub","parameters":{"workflowVersionId":"version-a"},"outputProjection":{},"contextWrites":[]
        })).unwrap());
    definition.connections.push(serde_json::from_value(serde_json::json!({
            "id":"sub-edge","sourceNodeId":"loop","sourceHandle":"done","targetNodeId":"sub","targetHandle":"main","order":1
        })).unwrap());
    let error = compiler
        .compile(
            &definition,
            &CompileContext {
                current_workflow_version_id: Some("version-a".into()),
                ancestor_workflow_version_ids: BTreeSet::new(),
            },
        )
        .unwrap_err();
    let codes = error
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"UNKNOWN_SOURCE_PORT"));
    assert!(codes.contains(&"RECURSIVE_SUBWORKFLOW"));
}

#[test]
fn rejects_undeclared_parameters_and_accepts_empty_loop_configuration() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition = fixture();
    definition.nodes[3].parameters = serde_json::json!({"batchSize":0});
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "INVALID_NODE_PARAMETERS"
                && issue.path.starts_with("nodes[3].parameters"))
    );

    definition.nodes[3].parameters = serde_json::json!({});
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
}

#[test]
fn validates_nested_parameter_expressions_against_the_leaf_schema() {
    let mut registry = NodeRegistry::m5_defaults();
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","required":["question"],"properties":{"question":{"type":"string"}},"additionalProperties":false},"contexts":{}},
        "nodes":[{
            "id":"model","key":"model","type":"model","typeVersion":1,"name":"Model",
            "parameters":{"prompt":"Answer the question","userQuestion":reference_json(ValueNamespace::Inputs,None,None,ValueSelection::Current,&["question"])},
            "outputProjection":{},"contextWrites":[]
        }],
        "connections":[
            {"id":"start-model","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"model","targetHandle":"main","order":0},
            {"id":"model-end","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{}}
    }))
    .unwrap();

    WorkflowCompiler::new(&registry)
        .compile(&definition, &CompileContext::default())
        .unwrap();

    let mut restricted = registry.get("model", 1).unwrap().clone();
    restricted.node_type = "restricted_model".into();
    restricted.parameter_schema["properties"]["userQuestion"]
        .as_object_mut()
        .unwrap()
        .remove("x-agentx-dynamicValue");
    registry.register(restricted).unwrap();
    definition.nodes[0].node_type = "restricted_model".into();
    let error = WorkflowCompiler::new(&registry)
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(error.issues.iter().any(|issue| {
        issue.code == "PARAMETER_NOT_TEMPLATABLE"
            && issue.path == "nodes[0].parameters.userQuestion"
    }));
}

#[test]
fn rejects_structured_parameter_references_from_disallowed_namespaces() {
    let mut registry = NodeRegistry::m5_defaults();
    let mut restricted = registry.get("model", 1).unwrap().clone();
    restricted.node_type = "output_only_model".into();
    restricted.parameter_schema["properties"]["userQuestion"]["x-agentx-dynamicValue"]["allowedNamespaces"] =
        serde_json::json!(["outputs"]);
    registry.register(restricted).unwrap();

    let mut definition = fixture();
    definition.nodes[0].node_type = "output_only_model".into();
    definition.nodes[0].parameters["userQuestion"] = reference_json(
        ValueNamespace::Inputs,
        None,
        None,
        ValueSelection::Current,
        &["question"],
    );

    let error = WorkflowCompiler::new(&registry)
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(error.issues.iter().any(|issue| {
        issue.code == "EXPRESSION_NAMESPACE_NOT_ALLOWED"
            && issue.path == "nodes[0].parameters.userQuestion"
    }));
}

#[test]
fn roots_are_declared_by_start_connections() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[
                {"id":"root","key":"root","type":"no_op","typeVersion":1,"name":"Root","outputProjection":{},"contextWrites":[]},
                {"id":"set","key":"set","type":"set","typeVersion":1,"name":"Set","parameters":{"values":{"ok":true}},"outputProjection":{},"contextWrites":[]}
            ],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"root","targetHandle":"main","order":0},
                {"id":"root-set","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"set","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"set","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        }))
        .unwrap();

    let compiled = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
    assert_eq!(compiled.start_nodes, vec![0]);
}

#[test]
fn rejects_a_node_without_an_explicit_start_or_end() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[{"id":"set","key":"set","type":"set","typeVersion":1,"name":"Set","parameters":{"values":{}},"outputProjection":{},"contextWrites":[]}],
            "connections":[],
            "end":{"outputs":{}}
        }))
        .unwrap();

    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "START_REQUIRED")
    );
}

#[test]
fn error_connections_do_not_change_the_explicit_end_contract() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
            "nodes":[
                {"id":"if","key":"condition","type":"if","typeVersion":1,"name":"Condition","parameters":{"condition":true},"outputProjection":{},"contextWrites":[],"settings":{"onError":"continue_error_output"}},
                {"id":"handler","key":"handler","type":"error_handler","typeVersion":1,"name":"Error Handler","parameters":{"mode":"recover"},"outputProjection":{},"contextWrites":[]}
            ],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"if","targetHandle":"main","order":0},
                {"id":"normal","sourceNodeId":"if","sourceHandle":"true","targetNodeId":"__end__","targetHandle":"main","order":0},
                {"id":"error","sourceNodeId":"if","sourceHandle":"error","targetNodeId":"handler","targetHandle":"error","order":0},
                {"id":"end","sourceNodeId":"handler","sourceHandle":"recovered","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"answer":{"schema":{"type":"object"},"value":reference_json(ValueNamespace::Outputs,Some("if"),Some("true"),ValueSelection::First,&[]),"required":false}}}
        }))
        .unwrap();

    let compiled = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
    assert_eq!(compiled.end.outputs.len(), 1);
    assert_eq!(
        compiled
            .connections
            .iter()
            .find(|edge| edge.id == "error")
            .unwrap()
            .source_port_kind,
        PortKind::Error
    );
}

#[test]
fn error_only_nodes_do_not_require_a_main_path_to_end() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
        "nodes":[
            {"id":"stop","key":"stop","type":"stop_and_error","typeVersion":1,"name":"Stop","parameters":{},"outputProjection":{},"contextWrites":[]},
            {"id":"success","key":"success","type":"no_op","typeVersion":1,"name":"Success","parameters":{},"outputProjection":{},"contextWrites":[]}
        ],
        "connections":[
            {"id":"start-stop","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"stop","targetHandle":"main","order":0},
            {"id":"start-success","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"success","targetHandle":"main","order":1},
            {"id":"success-end","sourceNodeId":"success","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0},
            {"id":"stop-error","sourceNodeId":"stop","sourceHandle":"error","targetNodeId":"__end__","targetHandle":"error","order":0}
        ],
        "end":{"outputs":{},"error":{"strategy":"fail_fast","outputs":{}}}
    })).unwrap();

    let compiled = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
    assert_eq!(compiled.start_nodes.len(), 2);
    assert!(
        compiled
            .terminal_connections
            .iter()
            .any(|connection| connection.target_port == "error")
    );
}

#[test]
fn end_error_outputs_accept_fixed_error_item_fields() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
            "nodes":[{"id":"branch","key":"branch","type":"if","typeVersion":1,"name":"Branch","parameters":{"condition":true},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"branch","targetHandle":"main","order":0},
                {"id":"main","sourceNodeId":"branch","sourceHandle":"true","targetNodeId":"__end__","targetHandle":"main","order":0},
                {"id":"error","sourceNodeId":"branch","sourceHandle":"error","targetNodeId":"__end__","targetHandle":"error","order":0}
            ],
            "end":{"outputs":{},"error":{"strategy":"fail_fast","outputs":{"message":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Item,None,None,ValueSelection::Current,&["message"]),"required":true}}}}
        })).unwrap();
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
    definition
        .end
        .error
        .outputs
        .get_mut("message")
        .unwrap()
        .schema = serde_json::json!({"type":"number"});
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(error.issues.iter().any(|issue| {
        issue.code == "END_OUTPUT_TYPE_MISMATCH" && issue.path == "end.error.outputs.message.value"
    }));
}

#[test]
fn end_error_outputs_reject_unknown_error_item_fields() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
            "nodes":[{"id":"branch","key":"branch","type":"if","typeVersion":1,"name":"Branch","parameters":{"condition":true},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"branch","targetHandle":"main","order":0},
                {"id":"main","sourceNodeId":"branch","sourceHandle":"true","targetNodeId":"__end__","targetHandle":"main","order":0},
                {"id":"error","sourceNodeId":"branch","sourceHandle":"error","targetNodeId":"__end__","targetHandle":"error","order":0}
            ],
            "end":{"outputs":{},"error":{"outputs":{"bad":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Item,None,None,ValueSelection::Current,&["notAField"])}}}}
        })).unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "UNKNOWN_ERROR_ITEM_REFERENCE")
    );

    definition.end.error.outputs.get_mut("bad").unwrap().value = reference(
        ValueNamespace::Item,
        None,
        None,
        ValueSelection::Current,
        &["code"],
    );
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();

    for removed in ["sourceNodeKey", "runIndex", "iterationIndex"] {
        definition.end.error.outputs.get_mut("bad").unwrap().value = reference(
            ValueNamespace::Item,
            None,
            None,
            ValueSelection::Current,
            &[removed],
        );
        let error = compiler
            .compile(&definition, &CompileContext::default())
            .unwrap_err();
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "UNKNOWN_ERROR_ITEM_REFERENCE"),
            "removed error field {removed} must not compile"
        );
    }
}

#[test]
fn end_error_output_can_use_only_common_error_predecessor_outputs() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let base = serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
        "nodes":[
            {"id":"root","key":"root","type":"no_op","typeVersion":1,"name":"Root","outputProjection":{},"contextWrites":[]},
            {"id":"a","key":"a","type":"no_op","typeVersion":1,"name":"A","outputProjection":{},"contextWrites":[]},
            {"id":"b","key":"b","type":"no_op","typeVersion":1,"name":"B","outputProjection":{},"contextWrites":[]},
            {"id":"merge","key":"merge","type":"merge","typeVersion":1,"name":"Merge","outputProjection":{},"contextWrites":[]}
        ],
        "connections":[
            {"id":"start-root","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"root","targetHandle":"main","order":0},
            {"id":"root-a","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"a","targetHandle":"main","order":0},
            {"id":"root-b","sourceNodeId":"root","sourceHandle":"main","targetNodeId":"b","targetHandle":"main","order":1},
            {"id":"a-merge","sourceNodeId":"a","sourceHandle":"main","targetNodeId":"merge","targetHandle":"main:0","order":0},
            {"id":"b-merge","sourceNodeId":"b","sourceHandle":"main","targetNodeId":"merge","targetHandle":"main:1","order":1},
            {"id":"merge-main","sourceNodeId":"merge","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0},
            {"id":"a-error","sourceNodeId":"a","sourceHandle":"error","targetNodeId":"__end__","targetHandle":"error","order":0},
            {"id":"b-error","sourceNodeId":"b","sourceHandle":"error","targetNodeId":"__end__","targetHandle":"error","order":1}
        ],
        "end":{"outputs":{},"error":{"outputs":{"value":{"schema":{"type":"object"},"value":reference_json(ValueNamespace::Outputs,Some("root"),Some("main"),ValueSelection::First,&[])}}}}
    });
    let mut definition: WorkflowDefinition = serde_json::from_value(base.clone()).unwrap();
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();

    definition.end.error.outputs.get_mut("value").unwrap().value = reference(
        ValueNamespace::Outputs,
        Some("a"),
        Some("main"),
        ValueSelection::First,
        &[],
    );
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "ERROR_OUTPUT_NOT_COMMON_PREDECESSOR")
    );
}

#[test]
fn projection_fields_are_available_to_downstream_and_sensitive_fields_stay_private() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
            "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":"https://example.invalid"},"outputProjection":{"main":{"customer_name":{"value":reference_json(ValueNamespace::Item,None,None,ValueSelection::Current,&["body"]),"schema":{"type":"string"},"sensitive":false}}},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"name":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Outputs,Some("http"),Some("main"),ValueSelection::First,&["customer_name"]),"required":true}}}
        })).unwrap();
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();

    definition.nodes[0]
        .output_projection
        .get_mut("main")
        .unwrap()
        .get_mut("customer_name")
        .unwrap()
        .sensitive = true;
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "SENSITIVE_OUTPUT_EXPOSURE")
    );
}

#[test]
fn context_writes_can_read_current_projection_and_validate_operation_type() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{"answer":{"schema":{"type":"string"},"default":"","mutable":true,"scope":"execution_tree"}}},
            "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":"https://example.invalid"},"outputProjection":{"main":{"answer_text":{"value":reference_json(ValueNamespace::Item,None,None,ValueSelection::Current,&["body"]),"schema":{"type":"string"}}}},"contextWrites":[{"operation":"set","path":"answer","value":reference_json(ValueNamespace::Outputs,Some("http"),Some("main"),ValueSelection::Current,&["answer_text"])}]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        })).unwrap();
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();

    definition.nodes[0].context_writes[0].operation = ContextWriteOperation::Append;
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| { issue.code == "CONTEXT_WRITE_OPERATION_TYPE_MISMATCH" })
    );
}

#[test]
fn rejects_end_outputs_that_reference_unknown_nodes() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition = fixture();
    definition.end = serde_json::from_value(serde_json::json!({
            "outputs":{"answer":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Outputs,Some("missing"),Some("main"),ValueSelection::First,&["answer"]),"required":true}}
        })).unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "OUTPUT_REFERENCE_NOT_FOUND")
    );
}

#[test]
fn validates_run_item_and_output_schema_paths() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
            "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":"https://example.invalid"},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"answer":{"schema":{"type":"string"},"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"http","port":"main","run":{"kind":"index","index":0},"item":{"kind":"index","index":0},"path":["missing"]},"missingPolicy":{"kind":"error"}},"required":false}}}
        }))
        .unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "UNKNOWN_OUTPUT_FIELD")
    );
}

#[test]
fn removed_ai_output_fields_require_reselecting_text() {
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{},"additionalProperties":false},"contexts":{}},
        "nodes":[{"id":"model","key":"model","type":"model","typeVersion":1,"name":"Model","parameters":{"prompt":"system","userQuestion":"question"},"outputProjection":{},"contextWrites":[]}],
        "connections":[
            {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"model","targetHandle":"main","order":0},
            {"id":"end","sourceNodeId":"model","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{"answer":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Outputs,Some("model"),Some("main"),ValueSelection::First,&["message"]),"required":true}}}
    }))
    .unwrap();
    let error = WorkflowCompiler::new(&NodeRegistry::m5_defaults())
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    let issue = error
        .issues
        .iter()
        .find(|issue| issue.code == "UNKNOWN_OUTPUT_FIELD")
        .expect("removed AI field issue");
    assert!(issue.message.contains("no longer exists"));
    assert!(issue.message.contains("'text'"));
}

#[test]
fn end_string_output_freezes_reference_coercion_in_ir() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
        "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":"https://example.invalid"},"outputProjection":{},"contextWrites":[]}],
        "connections":[{"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},{"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}],
        "end":{"outputs":{"answer":{"schema":{"type":"string"},"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"http","port":"main","run":{"kind":"current"},"item":{"kind":"first"},"path":["body"]},"missingPolicy":{"kind":"error"}},"required":true}}}
    })).unwrap();
    let compiled = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();
    assert!(matches!(
        compiled.end.outputs["answer"].value,
        DynamicValue::Reference {
            coerce: Some(ValueCoercion::String),
            ..
        }
    ));
}

#[test]
fn unknown_reference_type_is_rejected_for_non_string_end_output() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
        "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":"https://example.invalid"},"outputProjection":{},"contextWrites":[]}],
        "connections":[{"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},{"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}],
        "end":{"outputs":{"answer":{"schema":{"type":"object"},"value":{"kind":"reference","selector":{"namespace":"outputs","sourceNodeId":"http","port":"main","run":{"kind":"current"},"item":{"kind":"first"},"path":["body"]},"missingPolicy":{"kind":"error"}},"required":true}}}
    })).unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "END_OUTPUT_TYPE_UNKNOWN")
    );
}

#[test]
fn freezes_approval_decision_port_schema_and_accepts_decision_reference() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "schemaVersion":"5.0",
        "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
        "nodes":[{
            "id":"approval","key":"approval","type":"approval","typeVersion":1,
            "name":"Approval","parameters":{"title":"Review"},
            "outputProjection":{},"contextWrites":[]
        }],
        "connections":[
            {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"approval","targetHandle":"main","order":0},
            {"id":"end","sourceNodeId":"approval","sourceHandle":"approved","targetNodeId":"__end__","targetHandle":"main","order":0}
        ],
        "end":{"outputs":{"decision":{
            "schema":{"type":"string","enum":["approved"]},
            "value":reference_json(ValueNamespace::Outputs,Some("approval"),Some("approved"),ValueSelection::Current,&["decision"]),
            "required":false
        }}}
    }))
    .unwrap();

    let compiled = compiler
        .compile(&definition, &CompileContext::default())
        .expect("approval decision reference compiles");
    let schema = &compiled.nodes[0].effective_output_contract.port_schemas["approved"];
    assert_eq!(
        schema["properties"]["decision"]["enum"],
        serde_json::json!(["approved"])
    );
    assert_eq!(
        schema["required"],
        serde_json::json!(["taskId", "decision", "decidedBy", "reason", "input"])
    );
    assert_eq!(
        compiled.nodes[0].effective_output_contract.cardinalities["approved"],
        OutputCardinality::ZeroOrOne
    );
}

#[test]
fn rejects_nullable_branch_selection_for_required_end_output() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{}},
            "nodes":[{"id":"branch","key":"branch","type":"if","typeVersion":1,"name":"Branch","parameters":{"condition":true},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"branch","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"branch","sourceHandle":"true","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{"answer":{"schema":{"type":"object"},"value":reference_json(ValueNamespace::Outputs,Some("branch"),Some("true"),ValueSelection::First,&[]),"required":true}}}
        }))
        .unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "REQUIRED_OUTPUT_MAY_BE_EMPTY")
    );

    definition.end.outputs.get_mut("answer").unwrap().required = false;
    assert!(
        compiler
            .compile(&definition, &CompileContext::default())
            .is_ok()
    );
}

#[test]
fn sensitive_context_is_readable_but_cannot_be_projected_or_returned() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","properties":{}},"contexts":{"secret":{"schema":{"type":"string"},"default":"","mutable":false,"sensitive":true,"scope":"execution_tree","mergePolicy":"replace","clientWritable":false}}},
            "nodes":[{"id":"http","key":"http","type":"declarative_http","typeVersion":1,"name":"HTTP","parameters":{"url":reference_json(ValueNamespace::Contexts,None,None,ValueSelection::Current,&["secret"])},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"http","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"http","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],
            "end":{"outputs":{}}
        }))
        .unwrap();
    assert!(
        compiler
            .compile(&definition, &CompileContext::default())
            .is_ok()
    );
    definition.end = serde_json::from_value(serde_json::json!({
            "outputs":{"secret":{"schema":{"type":"string"},"value":reference_json(ValueNamespace::Contexts,None,None,ValueSelection::Current,&["secret"]),"required":true,"sensitive":true}}
        }))
        .unwrap();
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "SENSITIVE_CONTEXT_EXPOSURE")
    );
}

#[test]
fn loop_namespace_is_only_available_inside_a_loop_component() {
    let registry = NodeRegistry::m5_defaults();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition = fixture();
    definition.nodes[1].parameters = serde_json::json!({"condition":{"kind":"expression","root":{"kind":"binary","operator":"gt","left":{"kind":"reference","selector":{"namespace":"loop","run":{"kind":"current"},"item":{"kind":"current"},"path":["iteration"]},"missingPolicy":{"kind":"error"}},"right":{"kind":"literal","value":0}}}});
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "LOOP_REFERENCE_OUTSIDE_ITERATION")
    );
}

#[test]
fn composite_manifest_pins_version_and_mutable_context_contract() {
    let version = uuid::Uuid::now_v7();
    let node_type = format!("workflow.{}", version.simple());
    let child_context = serde_json::json!({
        "history":{
            "schema":{"type":"array","items":{"type":"string"}},
            "default":[],"mutable":true,"sensitive":false,"scope":"session",
            "mergePolicy":"append","clientWritable":false
        }
    });
    let mut registry = NodeRegistry::m5_defaults();
    let mut manifest = registry.get("sub_workflow", 1).unwrap().clone();
    manifest.node_type = node_type.clone();
    manifest.parameter_schema = serde_json::json!({
        "type":"object",
        "x-agentx-contextContract":child_context.clone(),
        "required":["workflowVersionId","inputs"],
        "properties":{
            "workflowVersionId":{"type":"string","const":version.to_string()},
            "inputs":{
                "allOf":[{"type":"object","required":["question"],"properties":{"question":{"type":"string"}}}],
                "x-agentx-dynamicValue":{"modes":["literal","reference"],"allowedNamespaces":["inputs","outputs","contexts"],"acceptedCardinality":["single"],"missingPolicies":["error","null","default","omit"],"recursive":true}
            }
        },
        "additionalProperties":false
    });
    registry.register(manifest).unwrap();
    let compiler = WorkflowCompiler::new(&registry);
    let mut definition: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "schemaVersion":"5.0",
            "start":{"inputs":{"type":"object","required":["question"],"properties":{"question":{"type":"string"}}},"contexts":child_context},
            "nodes":[{"id":"child","key":"child","type":node_type,"typeVersion":1,"name":"Child","parameters":{"workflowVersionId":version.to_string(),"inputs":{"question":reference_json(ValueNamespace::Inputs,None,None,ValueSelection::Current,&["question"])}},"outputProjection":{},"contextWrites":[]}],
            "connections":[
                {"id":"start","sourceNodeId":"__start__","sourceHandle":"main","targetNodeId":"child","targetHandle":"main","order":0},
                {"id":"end","sourceNodeId":"child","sourceHandle":"main","targetNodeId":"__end__","targetHandle":"main","order":0}
            ],"end":{"outputs":{}}
        }))
        .unwrap();
    compiler
        .compile(&definition, &CompileContext::default())
        .unwrap();

    definition
        .start
        .contexts
        .get_mut("history")
        .unwrap()
        .merge_policy = agentx_domain::ContextMergePolicy::Replace;
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "COMPOSITE_CONTEXT_CONTRACT_MISMATCH")
    );

    definition
        .start
        .contexts
        .get_mut("history")
        .unwrap()
        .merge_policy = agentx_domain::ContextMergePolicy::Append;
    definition.nodes[0].parameters["workflowVersionId"] =
        Value::String(uuid::Uuid::now_v7().to_string());
    let error = compiler
        .compile(&definition, &CompileContext::default())
        .unwrap_err();
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.code == "COMPOSITE_VERSION_MISMATCH")
    );
}
