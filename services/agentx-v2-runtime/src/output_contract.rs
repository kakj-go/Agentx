use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_node_protocol::OutputCardinality;
use agentx_runtime_contracts::CompiledNodeV1;

pub(crate) fn validate_node_output_contract(
    node: &CompiledNodeV1,
    outputs: &BTreeMap<String, Vec<Item>>,
) -> Result<(), String> {
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
    use agentx_node_protocol::OutputCardinality;

    use super::validate_cardinality;

    #[test]
    fn frozen_output_cardinality_is_enforced() {
        assert!(validate_cardinality("model", "main", OutputCardinality::ExactlyOne, 1).is_ok());
        assert!(validate_cardinality("model", "main", OutputCardinality::ExactlyOne, 0).is_err());
        assert!(validate_cardinality("wait", "resumed", OutputCardinality::ZeroOrOne, 2).is_err());
        assert!(validate_cardinality("set", "main", OutputCardinality::Many, 3).is_ok());
    }
}
