use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_runtime_contracts::CompiledNodeV1;

pub(crate) fn validate_node_output_contract(
    node: &CompiledNodeV1,
    outputs: &BTreeMap<String, Vec<Item>>,
) -> Result<(), String> {
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

pub(crate) fn violation_code(node: &CompiledNodeV1) -> &'static str {
    if node.node_type == "model" {
        "MODEL_OUTPUT_CONTRACT_VIOLATION"
    } else {
        "NODE_OUTPUT_CONTRACT_VIOLATION"
    }
}
