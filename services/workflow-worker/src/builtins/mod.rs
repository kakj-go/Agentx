use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
use anyhow::Result;

mod data;
mod encoding;
mod validation;

pub fn execute(task: &RuntimeTask) -> Result<Option<TaskResult>> {
    let result = match task.node_type.as_str() {
        "filter" | "limit" | "sort" | "remove_duplicates" | "split_out" | "aggregate" | "merge"
        | "rename_fields" | "json_transform" | "no_op" | "item_generator" | "date_time"
        | "compare_datasets" => data::execute(task)?,
        "base64" | "hash" => encoding::execute(task)?,
        "structured_validator" | "stop_and_error" => validation::execute(task)?,
        _ => return Ok(None),
    };
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentx_domain::NodeExecutionId;
    use agentx_infrastructure::runtime_repository::{RuntimeTask, TaskResult};
    use agentx_node_protocol::{BinaryReference, Item, ItemSource};
    use serde_json::{Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::execute;

    fn item(json: Value) -> Item {
        Item {
            json,
            binary: BTreeMap::from([(
                "file".into(),
                BinaryReference {
                    artifact_handle: "artifact".into(),
                    file_name: Some("a.txt".into()),
                    content_type: Some("text/plain".into()),
                    size_bytes: 1,
                },
            )]),
            metadata: BTreeMap::from([("source".into(), json!("test"))]),
            lineage: vec![ItemSource {
                node_execution_id: NodeExecutionId::from_uuid(Uuid::nil()),
                node_id: "source".into(),
                run_index: 0,
                output_index: 0,
                item_index: 0,
            }],
        }
    }

    fn task(
        node_type: &str,
        parameters: Value,
        inputs: BTreeMap<String, Vec<Item>>,
    ) -> RuntimeTask {
        RuntimeTask {
            tenant_id: Uuid::nil(),
            workflow_id: Uuid::nil(),
            workflow_version_id: None,
            workflow_service_identity_id: Uuid::nil(),
            execution_id: Uuid::nil(),
            node_execution_id: Uuid::nil(),
            attempt_id: Uuid::nil(),
            attempt_number: 1,
            node_type: node_type.into(),
            node_version: 1,
            node_parameters: parameters,
            inputs,
            run_index: 0,
            iteration_index: 0,
            capability: "builtin".into(),
            idempotency_key: "test".into(),
            deadline: OffsetDateTime::now_utc() + time::Duration::minutes(1),
            mode: "manual".into(),
            trace_id: Uuid::nil(),
            linked_nodes: json!({}),
            resource_references: vec![],
            resource_snapshots: vec![],
        }
    }

    fn main_inputs(values: Vec<Value>) -> BTreeMap<String, Vec<Item>> {
        BTreeMap::from([("main".into(), values.into_iter().map(item).collect())])
    }

    fn completed(task: RuntimeTask) -> BTreeMap<String, Vec<Item>> {
        match execute(&task)
            .expect("built-in execution")
            .expect("handled built-in")
        {
            TaskResult::Completed(outputs) => outputs,
            other => panic!("expected completed result, got {other:?}"),
        }
    }

    fn execution_error(task: RuntimeTask) -> String {
        execute(&task)
            .expect_err("built-in execution must reject invalid input")
            .to_string()
    }

    #[test]
    fn executes_local_transform_nodes_and_preserves_item_envelope() {
        let cases = [
            (
                "filter",
                json!({"condition":"=$json.value > 1"}),
                vec![json!({"value":1}), json!({"value":2})],
            ),
            (
                "limit",
                json!({"maxItems":1}),
                vec![json!({"value":1}), json!({"value":2})],
            ),
            (
                "sort",
                json!({"fields":[{"field":"value","direction":"desc"}]}),
                vec![json!({"value":1}), json!({"value":2})],
            ),
            (
                "remove_duplicates",
                json!({"fields":["value"]}),
                vec![json!({"value":1}), json!({"value":1})],
            ),
            (
                "split_out",
                json!({"field":"values"}),
                vec![json!({"values":[1,2]})],
            ),
            (
                "rename_fields",
                json!({"mappings":[{"from":"old","to":"new"}]}),
                vec![json!({"old":1})],
            ),
            (
                "json_transform",
                json!({"operation":"parse","field":"body"}),
                vec![json!({"body":"{\"ok\":true}"})],
            ),
            ("no_op", json!({}), vec![json!({"value":1})]),
            (
                "date_time",
                json!({"operation":"add","field":"at","amount":1,"unit":"days"}),
                vec![json!({"at":"2024-01-01T00:00:00Z"})],
            ),
            (
                "base64",
                json!({"operation":"encode","field":"value"}),
                vec![json!({"value":"Agentx"})],
            ),
            (
                "hash",
                json!({"algorithm":"sha256","field":"value"}),
                vec![json!({"value":"Agentx"})],
            ),
        ];
        for (node_type, parameters, inputs) in cases {
            let outputs = completed(task(node_type, parameters, main_inputs(inputs)));
            let output = outputs
                .get("main")
                .and_then(|items| items.first())
                .expect(node_type);
            assert!(
                output.binary.contains_key("file"),
                "{node_type} dropped Binary"
            );
            assert_eq!(
                output.metadata.get("source"),
                Some(&json!("test")),
                "{node_type} dropped Metadata"
            );
            assert_eq!(output.lineage.len(), 1, "{node_type} dropped Lineage");
        }
    }

    #[test]
    fn executes_generator_aggregate_merge_compare_and_validator() {
        let generated = completed(task(
            "item_generator",
            json!({"start":1,"end":3}),
            BTreeMap::new(),
        ));
        assert_eq!(generated["main"].len(), 3);

        let aggregated = completed(task(
            "aggregate",
            json!({"groupBy":["group"],"operations":[{"operation":"sum","field":"value","outputField":"total"}]}),
            main_inputs(vec![
                json!({"group":"a","value":2}),
                json!({"group":"a","value":3}),
            ]),
        ));
        assert_eq!(aggregated["main"][0].json["total"], 5.0);
        assert_eq!(
            aggregated["main"][0].lineage.len(),
            1,
            "duplicate lineage must be removed"
        );

        let merged = completed(task(
            "merge",
            json!({"mode":"combine_by_key","leftField":"id","rightField":"id","joinType":"full","conflictStrategy":"prefer_right"}),
            BTreeMap::from([
                ("left".into(), vec![item(json!({"id":1,"left":true}))]),
                (
                    "right".into(),
                    vec![item(json!({"id":1,"right":true})), item(json!({"id":2}))],
                ),
            ]),
        ));
        assert_eq!(merged["main"].len(), 2);
        assert_eq!(
            merged["main"][0].json,
            json!({"id":1,"left":true,"right":true})
        );
        assert_eq!(merged["main"][0].lineage.len(), 1);

        let compared = completed(task(
            "compare_datasets",
            json!({"keyFields":["id"]}),
            BTreeMap::from([
                (
                    "left".into(),
                    vec![
                        item(json!({"id":1,"value":"same"})),
                        item(json!({"id":2,"value":"left"})),
                    ],
                ),
                (
                    "right".into(),
                    vec![
                        item(json!({"id":1,"value":"same"})),
                        item(json!({"id":2,"value":"right"})),
                        item(json!({"id":3})),
                    ],
                ),
            ]),
        ));
        assert_eq!(compared["same"].len(), 1);
        assert_eq!(compared["different"].len(), 1);
        assert_eq!(compared["different"][0].lineage.len(), 1);
        assert_eq!(compared["right_only"].len(), 1);

        let validated = completed(task(
            "structured_validator",
            json!({"schema":{"type":"object","required":["id"]},"mode":"route"}),
            main_inputs(vec![json!({"id":1}), json!({"name":"missing"})]),
        ));
        assert_eq!(validated["valid"].len(), 1);
        assert_eq!(validated["invalid"].len(), 1);
        assert!(
            validated["invalid"][0]
                .metadata
                .contains_key("validationErrors")
        );
    }

    #[test]
    fn stop_with_error_returns_a_formal_failure() {
        let result = execute(&task(
            "stop_and_error",
            json!({"code":"BAD_INPUT","message":"invalid value"}),
            main_inputs(vec![json!({})]),
        ))
        .expect("execution")
        .expect("handled");
        assert!(
            matches!(result, TaskResult::Failed { code, message, retryable: false } if code == "BAD_INPUT" && message == "invalid value")
        );

        let expression = execute(&task(
            "stop_and_error",
            json!({"code":"=$json.code","message":"=$json.message"}),
            main_inputs(vec![
                json!({"code":"EXPRESSION_ERROR","message":"from item"}),
            ]),
        ))
        .expect("expression execution")
        .expect("handled");
        assert!(
            matches!(expression, TaskResult::Failed { code, message, .. } if code == "EXPRESSION_ERROR" && message == "from item")
        );
    }

    #[test]
    fn empty_inputs_are_stable_for_every_applicable_builtin() {
        let cases = [
            ("filter", json!({"condition":true}), vec!["main"]),
            ("limit", json!({"maxItems":1}), vec!["main"]),
            ("sort", json!({"fields":[]}), vec!["main"]),
            ("remove_duplicates", json!({}), vec!["main"]),
            ("split_out", json!({"field":"values"}), vec!["main"]),
            ("aggregate", json!({}), vec!["main"]),
            ("merge", json!({}), vec!["main"]),
            ("rename_fields", json!({"mappings":[]}), vec!["main"]),
            ("json_transform", json!({"field":"value"}), vec!["main"]),
            ("no_op", json!({}), vec!["main"]),
            ("item_generator", json!({"items":[]}), vec!["main"]),
            ("date_time", json!({"field":"value"}), vec!["main"]),
            ("base64", json!({"field":"value"}), vec!["main"]),
            ("hash", json!({"field":"value"}), vec!["main"]),
            (
                "compare_datasets",
                json!({}),
                vec!["same", "different", "left_only", "right_only"],
            ),
            (
                "structured_validator",
                json!({"schema":{}}),
                vec!["valid", "invalid"],
            ),
        ];
        for (node_type, parameters, ports) in cases {
            let outputs = completed(task(node_type, parameters, BTreeMap::new()));
            for port in ports {
                assert!(
                    outputs.get(port).is_some_and(Vec::is_empty),
                    "{node_type}:{port} must be empty"
                );
            }
        }
    }

    #[test]
    fn invalid_fields_and_types_return_actionable_errors() {
        let cases = [
            (
                "filter",
                json!({"condition":"not-a-boolean"}),
                json!({"value":1}),
                "boolean",
            ),
            (
                "split_out",
                json!({"field":"value"}),
                json!({"value":1}),
                "array",
            ),
            (
                "aggregate",
                json!({"operations":[{"operation":"sum","field":"value","outputField":"sum"}]}),
                json!({"value":"text"}),
                "numeric",
            ),
            (
                "rename_fields",
                json!({"mappings":[{"from":"missing","to":"value"}],"missingField":"error"}),
                json!({}),
                "missing",
            ),
            (
                "json_transform",
                json!({"operation":"parse","field":"missing"}),
                json!({}),
                "missing",
            ),
            (
                "date_time",
                json!({"field":"value"}),
                json!({"value":"not-a-date"}),
                "RFC3339",
            ),
            (
                "base64",
                json!({"operation":"decode","field":"value"}),
                json!({"value":"%%%"}),
                "Base64",
            ),
            ("hash", json!({"field":"missing"}), json!({}), "missing"),
        ];
        for (node_type, parameters, input, expected) in cases {
            let error = execution_error(task(node_type, parameters, main_inputs(vec![input])));
            assert!(
                error.contains(expected),
                "{node_type} returned unexpected error: {error}"
            );
        }
        assert!(
            execution_error(task("item_generator", json!({"step":0}), BTreeMap::new()))
                .contains("zero")
        );

        let invalid_schema = execution_error(task(
            "structured_validator",
            json!({"schema":{"type":"unsupported"}}),
            main_inputs(vec![json!({})]),
        ));
        assert!(invalid_schema.contains("invalid JSON Schema"));
    }

    #[test]
    fn validator_fail_mode_returns_a_formal_non_retryable_failure() {
        let result = execute(&task(
            "structured_validator",
            json!({"schema":{"type":"object","required":["id"]},"mode":"fail"}),
            main_inputs(vec![json!({"name":"missing"})]),
        ))
        .expect("execution")
        .expect("handled");
        assert!(
            matches!(result, TaskResult::Failed { code, retryable: false, .. } if code == "SCHEMA_VALIDATION_FAILED")
        );
    }
}
