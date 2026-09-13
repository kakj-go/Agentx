use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_node_protocol::OutputCardinality;
use agentx_runtime_contracts::CompiledNodeV1;

pub(crate) fn validate_node_output_contract(
    node: &CompiledNodeV1,
    outputs: &BTreeMap<String, Vec<Item>>,
) -> Result<(), String> {
    // The Loop worker returns an internal seed envelope. The state machine
    // consumes it to start body rounds and later emits the public aggregate,
    // whose schema is derived from outputSelector. Those two shapes may differ.
    if node.loop_body.is_some() {
        return validate_loop_seed_outputs(outputs);
    }
    for port in node.effective_output_contract.port_schemas.keys() {
        let count = outputs.get(port).map_or(0, Vec::len);
        let cardinality = node
            .effective_output_contract
            .cardinalities
            .get(port)
            .copied()
            .unwrap_or_default();
        validate_cardinality(&node.node_type, port, cardinality, count)?;
    }
    for (port, items) in outputs {
        let schema = node
            .effective_output_contract
            .port_schemas
            .get(port)
            .ok_or_else(|| {
                format!(
                    "{}.{} is not declared by the frozen output contract",
                    node.node_type, port
                )
            })?;
        let validator = jsonschema::validator_for(schema).map_err(|error| {
            format!(
                "invalid output schema for {}.{port}: {error}",
                node.node_type
            )
        })?;
        for (index, item) in items.iter().enumerate() {
            if let Err(error) = validator.validate(&item.json) {
                return Err(format!(
                    "{}.{port}[{index}] violates its output contract: {error}",
                    node.node_type
                ));
            }
        }
    }
    Ok(())
}

fn validate_loop_seed_outputs(outputs: &BTreeMap<String, Vec<Item>>) -> Result<(), String> {
    if outputs.len() != 1 {
        return Err("loop_over_items must emit only its internal main seed".into());
    }
    let items = outputs
        .get("main")
        .ok_or_else(|| "loop_over_items internal seed is missing main".to_owned())?;
    if items.len() != 1 {
        return Err("loop_over_items internal seed must contain exactly one item".into());
    }
    let object = items[0]
        .json
        .as_object()
        .ok_or_else(|| "loop_over_items internal seed must be an object".to_owned())?;
    if object.len() != 1 || !object.get("items").is_some_and(serde_json::Value::is_array) {
        return Err("loop_over_items internal seed must be exactly {items: [...]}".into());
    }
    Ok(())
}

fn validate_cardinality(
    node_type: &str,
    port: &str,
    cardinality: OutputCardinality,
    count: usize,
) -> Result<(), String> {
    let valid = match cardinality {
        OutputCardinality::ZeroOrOne => count <= 1,
        OutputCardinality::ExactlyOne => count == 1,
        OutputCardinality::ZeroOrMany | OutputCardinality::Many => true,
    };
    valid.then_some(()).ok_or_else(|| {
        format!(
            "{node_type}.{port} emitted {count} items but its frozen cardinality is {cardinality:?}"
        )
    })
}

pub(crate) fn violation_code(_node: &CompiledNodeV1) -> &'static str {
    "NODE_OUTPUT_SCHEMA_VALIDATION_FAILED"
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentx_node_protocol::{Item, OutputCardinality};
    use serde_json::json;

    use super::{validate_cardinality, validate_loop_seed_outputs};

    #[test]
    fn frozen_output_cardinality_is_enforced() {
        assert!(validate_cardinality("model", "main", OutputCardinality::ExactlyOne, 1).is_ok());
        assert!(validate_cardinality("model", "main", OutputCardinality::ExactlyOne, 0).is_err());
        assert!(
            validate_cardinality(
                "approval",
                "decision:approved",
                OutputCardinality::ZeroOrOne,
                2
            )
            .is_err()
        );
        assert!(validate_cardinality("set", "main", OutputCardinality::Many, 3).is_ok());
    }

    #[test]
    fn loop_seed_is_validated_independently_from_its_public_aggregate_schema() {
        let valid = BTreeMap::from([(
            "main".into(),
            vec![Item {
                json: json!({"items":[{"input":1},{"input":2}]}),
                ..Item::default()
            }],
        )]);
        assert!(validate_loop_seed_outputs(&valid).is_ok());
        assert!(
            validate_loop_seed_outputs(&BTreeMap::from([(
                "main".into(),
                vec![Item {
                    json: json!({"items":[],"extra":true}),
                    ..Item::default()
                }],
            )]))
            .is_err()
        );
    }
}
