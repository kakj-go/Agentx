use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_runtime::{ExpressionContext, ExpressionEngine, ExpressionError};
use serde_json::{Value, json};

pub(super) fn resolve(
    raw_parameters: &Value,
    inputs_by_port: &BTreeMap<String, Vec<Item>>,
    inputs: Value,
    outputs: Value,
    contexts: Value,
    execution: Value,
    output_node_keys: BTreeMap<String, String>,
) -> Result<(Value, Vec<Value>, Value), ExpressionError> {
    let items = inputs_by_port
        .values()
        .flat_map(|items| items.iter())
        .collect::<Vec<_>>();
    let base = ExpressionContext {
        inputs,
        outputs,
        contexts,
        execution,
        output_node_keys,
        ..ExpressionContext::default()
    };
    let common_item = items.first().map_or(Value::Null, |item| item.json.clone());
    let (common, common_conversions) = ExpressionEngine.resolve_parameters_with_conversions(
        raw_parameters,
        &ExpressionContext {
            json: common_item.clone(),
            input: common_item,
            ..base.clone()
        },
    )?;
    let per_item = items
        .into_iter()
        .enumerate()
        .map(|(item_index, item)| {
            ExpressionEngine.resolve_parameters_with_conversions(
                raw_parameters,
                &ExpressionContext {
                    json: item.json.clone(),
                    input: item.json.clone(),
                    item_index,
                    ..base.clone()
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (per_item_values, per_item_conversions): (Vec<_>, Vec<_>) = per_item.into_iter().unzip();
    Ok((
        common,
        per_item_values,
        json!({"common":common_conversions,"perItem":per_item_conversions}),
    ))
}

#[cfg(test)]
mod tests {
    use agentx_domain::{
        DynamicValue, MissingValuePolicy, ValueNamespace, ValuePathSegment, ValueSelection,
        ValueSelector,
    };

    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_item_parameters_for_each_input_item() {
        let parameters = serde_json::to_value(DynamicValue::Reference {
            selector: ValueSelector {
                namespace: ValueNamespace::Item,
                source_node_id: None,
                port: None,
                run: ValueSelection::Current,
                item: ValueSelection::Current,
                path: vec![ValuePathSegment::Key("keep".into())],
            },
            missing_policy: MissingValuePolicy::Error,
            coerce: None,
        })
        .unwrap();
        let items = BTreeMap::from([(
            "main".into(),
            vec![
                Item {
                    json: json!({"keep":true}),
                    ..Item::default()
                },
                Item {
                    json: json!({"keep":false}),
                    ..Item::default()
                },
            ],
        )]);
        let (common, per_item, conversions) = resolve(
            &json!({"condition":parameters}),
            &items,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(common, json!({"condition":true}));
        assert_eq!(
            per_item,
            vec![json!({"condition":true}), json!({"condition":false})]
        );
        assert_eq!(conversions, json!({"common":[],"perItem":[[],[]]}));
    }

    #[test]
    fn records_canonical_string_conversions_without_exposing_values() {
        let parameters = json!({
            "bodyText":{
                "kind":"reference",
                "selector":{"namespace":"item","run":{"kind":"current"},"item":{"kind":"current"},"path":["body"]},
                "missingPolicy":{"kind":"error"},
                "coerce":"string"
            }
        });
        let items = BTreeMap::from([(
            "main".into(),
            vec![Item {
                json: json!({"body":{"b":2,"a":1}}),
                ..Item::default()
            }],
        )]);
        let (common, _, conversions) = resolve(
            &parameters,
            &items,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(common["bodyText"], "{\"a\":1,\"b\":2}");
        assert_eq!(conversions["common"][0]["sourceType"], "object");
        assert_eq!(
            conversions["common"][0]["targetPath"],
            "parameters.bodyText"
        );
        assert!(conversions.to_string().find("\"a\":1").is_none());
    }
}
