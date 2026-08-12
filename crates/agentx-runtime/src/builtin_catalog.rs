use agentx_node_protocol::{
    ExecutionStyle, NodeCapability, NodeManifestVersion, NodeUiSchema, PortKind, ReadinessPolicy,
    SideEffectLevel,
};
use serde_json::{Value, json};

use crate::registry::{manifest, port};

pub(crate) const NODE_TYPES: &[&str] = &[
    "filter",
    "limit",
    "sort",
    "remove_duplicates",
    "split_out",
    "aggregate",
    "rename_fields",
    "json_transform",
    "no_op",
    "stop_and_error",
    "item_generator",
    "date_time",
    "base64",
    "hash",
    "compare_datasets",
    "structured_validator",
];

pub(crate) fn manifests() -> Vec<NodeManifestVersion> {
    let main_in = || vec![port("main", PortKind::Main, true, false)];
    let main_out = || {
        vec![
            port("main", PortKind::Main, false, false),
            port("error", PortKind::Error, false, false),
        ]
    };
    let values = vec![
        configured(
            action(
                "filter",
                main_in(),
                main_out(),
                "Filter",
                "过滤",
                "Keep items matching an expression.",
                "保留满足表达式的数据项。",
            ),
            json!({"type":"object","required":["condition"],"properties":{"condition":{}},"additionalProperties":false}),
            json!({"fields":{"condition":{"control":"expression"}}}),
        ),
        configured(
            action(
                "limit",
                main_in(),
                main_out(),
                "Limit",
                "限制数量",
                "Keep the first or last N items.",
                "保留前 N 项或后 N 项。",
            ),
            json!({"type":"object","properties":{"maxItems":{"type":"integer","minimum":0,"default":1},"keep":{"type":"string","enum":["first","last"],"default":"first"}},"additionalProperties":false}),
            json!({"order":["maxItems","keep"],"fields":{"maxItems":{"control":"number"},"keep":{"control":"select"}}}),
        ),
        configured(
            action(
                "sort",
                main_in(),
                main_out(),
                "Sort",
                "排序",
                "Sort items by one or more fields.",
                "按一个或多个字段排序数据项。",
            ),
            json!({"type":"object","properties":{"fields":{"type":"array","items":{"type":"object","required":["field"],"properties":{"field":{"type":"string"},"direction":{"enum":["asc","desc"],"default":"asc"},"nulls":{"enum":["first","last"],"default":"last"}}}}},"additionalProperties":false}),
            json!({"fields":{"fields":{"control":"json"}}}),
        ),
        configured(
            action(
                "remove_duplicates",
                main_in(),
                main_out(),
                "Remove Duplicates",
                "移除重复项",
                "Remove duplicate items using all or selected fields.",
                "按全部或指定字段移除重复数据项。",
            ),
            json!({"type":"object","properties":{"fields":{"type":"array","items":{"type":"string"}},"keep":{"enum":["first","last"],"default":"first"}},"additionalProperties":false}),
            json!({"fields":{"fields":{"control":"json"},"keep":{"control":"select"}}}),
        ),
        configured(
            action(
                "split_out",
                main_in(),
                main_out(),
                "Split Out",
                "拆分数组",
                "Split an array field into individual items.",
                "将数组字段拆分为独立数据项。",
            ),
            json!({"type":"object","required":["field"],"properties":{"field":{"type":"string"}},"additionalProperties":false}),
            json!({"fields":{"field":{"control":"text"}}}),
        ),
        configured(
            action(
                "aggregate",
                main_in(),
                main_out(),
                "Aggregate",
                "聚合",
                "Group items and calculate aggregate values.",
                "分组数据项并计算聚合值。",
            ),
            json!({"type":"object","properties":{"groupBy":{"type":"array","items":{"type":"string"}},"operations":{"type":"array","items":{"type":"object","required":["operation","outputField"],"properties":{"operation":{"enum":["count","sum","avg","min","max","first","last","collect"]},"field":{"type":"string"},"outputField":{"type":"string"}}}}},"additionalProperties":false}),
            json!({"fields":{"groupBy":{"control":"json"},"operations":{"control":"json"}}}),
        ),
        configured(
            action(
                "rename_fields",
                main_in(),
                main_out(),
                "Rename Fields",
                "重命名字段",
                "Rename nested item fields.",
                "重命名数据项中的嵌套字段。",
            ),
            json!({"type":"object","required":["mappings"],"properties":{"mappings":{"type":"array","items":{"type":"object","required":["from","to"],"properties":{"from":{"type":"string"},"to":{"type":"string"}}}},"missingField":{"enum":["ignore","error"],"default":"ignore"}},"additionalProperties":false}),
            json!({"fields":{"mappings":{"control":"json"},"missingField":{"control":"select"}}}),
        ),
        configured(
            action(
                "json_transform",
                main_in(),
                main_out(),
                "JSON Transform",
                "JSON 转换",
                "Parse or stringify a JSON field.",
                "解析或序列化 JSON 字段。",
            ),
            json!({"type":"object","required":["field"],"properties":{"operation":{"enum":["parse","stringify"],"default":"parse"},"field":{"type":"string"},"outputField":{"type":"string"}},"additionalProperties":false}),
            json!({"fields":{"operation":{"control":"select"},"field":{"control":"text"},"outputField":{"control":"text"}}}),
        ),
        configured(
            action(
                "no_op",
                main_in(),
                main_out(),
                "No Operation",
                "无操作",
                "Pass items through unchanged.",
                "原样透传数据项。",
            ),
            json!({"type":"object","additionalProperties":false}),
            json!({}),
        ),
        configured(
            action(
                "stop_and_error",
                main_in(),
                vec![port("error", PortKind::Error, false, false)],
                "Stop With Error",
                "错误终止",
                "Stop execution with an expression-driven error.",
                "使用表达式化错误终止执行。",
            ),
            json!({"type":"object","properties":{"code":{"type":"string","default":"WORKFLOW_STOPPED"},"message":{"type":"string","default":"Workflow stopped"}},"additionalProperties":false}),
            json!({"fields":{"code":{"control":"expression"},"message":{"control":"expression"}}}),
        ),
        configured(
            action(
                "item_generator",
                main_in(),
                vec![
                    port("main", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                "Item Generator",
                "数据生成器",
                "Generate local test items.",
                "生成本地测试数据项。",
            ),
            json!({"type":"object","properties":{"items":{"type":"array"},"start":{"type":"integer","default":1},"end":{"type":"integer","default":10},"step":{"type":"integer","default":1},"field":{"type":"string","default":"value"}},"additionalProperties":false}),
            json!({"fields":{"items":{"control":"json"},"start":{"control":"number"},"end":{"control":"number"},"step":{"control":"number"},"field":{"control":"text"}}}),
        ),
        configured(
            action(
                "date_time",
                main_in(),
                main_out(),
                "Date & Time",
                "日期与时间",
                "Format, offset, or compare RFC3339 dates.",
                "格式化、增减或比较 RFC3339 日期。",
            ),
            json!({"type":"object","required":["field"],"properties":{"operation":{"enum":["format","add","subtract","difference"],"default":"format"},"field":{"type":"string"},"outputField":{"type":"string"},"format":{"enum":["rfc3339","unix"],"default":"rfc3339"},"amount":{"type":"integer","default":0},"unit":{"enum":["seconds","minutes","hours","days"],"default":"seconds"},"compareTo":{"type":"string"}},"additionalProperties":false}),
            json!({"fields":{"operation":{"control":"select"},"field":{"control":"text"},"outputField":{"control":"text"},"format":{"control":"select"},"amount":{"control":"number"},"unit":{"control":"select"},"compareTo":{"control":"expression"}}}),
        ),
        configured(
            action(
                "base64",
                main_in(),
                main_out(),
                "Base64",
                "Base64 编解码",
                "Encode or decode a UTF-8 field.",
                "编码或解码 UTF-8 字段。",
            ),
            json!({"type":"object","required":["field"],"properties":{"operation":{"enum":["encode","decode"],"default":"encode"},"field":{"type":"string"},"outputField":{"type":"string"}},"additionalProperties":false}),
            json!({"fields":{"operation":{"control":"select"},"field":{"control":"text"},"outputField":{"control":"text"}}}),
        ),
        configured(
            action(
                "hash",
                main_in(),
                main_out(),
                "Hash",
                "哈希",
                "Hash a field with SHA-256 or SHA-512.",
                "使用 SHA-256 或 SHA-512 计算字段哈希。",
            ),
            json!({"type":"object","required":["field"],"properties":{"algorithm":{"enum":["sha256","sha512"],"default":"sha256"},"encoding":{"enum":["hex","base64"],"default":"hex"},"field":{"type":"string"},"outputField":{"type":"string"}},"additionalProperties":false}),
            json!({"fields":{"algorithm":{"control":"select"},"encoding":{"control":"select"},"field":{"control":"text"},"outputField":{"control":"text"}}}),
        ),
        configured(
            action(
                "compare_datasets",
                vec![
                    port("left", PortKind::Main, true, false),
                    port("right", PortKind::Main, true, false),
                ],
                vec![
                    port("same", PortKind::Main, false, false),
                    port("different", PortKind::Main, false, false),
                    port("left_only", PortKind::Main, false, false),
                    port("right_only", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                "Compare Datasets",
                "比较数据集",
                "Compare two local datasets by key fields.",
                "按关键字段比较两个本地数据集。",
            ),
            json!({"type":"object","properties":{"keyFields":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}),
            json!({"fields":{"keyFields":{"control":"json"}}}),
        ),
        configured(
            action(
                "structured_validator",
                main_in(),
                vec![
                    port("valid", PortKind::Main, false, false),
                    port("invalid", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                "Structured Validator",
                "结构化校验",
                "Validate items with JSON Schema.",
                "使用 JSON Schema 校验数据项。",
            ),
            json!({"type":"object","required":["schema"],"properties":{"schema":{"type":"object"},"mode":{"enum":["route","fail"],"default":"route"}},"additionalProperties":false}),
            json!({"fields":{"schema":{"control":"json"},"mode":{"control":"select"}}}),
        ),
    ];
    debug_assert_eq!(values.len(), NODE_TYPES.len());
    values
}

fn action(
    node_type: &str,
    inputs: Vec<agentx_node_protocol::NodePort>,
    outputs: Vec<agentx_node_protocol::NodePort>,
    en: &str,
    zh: &str,
    en_description: &str,
    zh_description: &str,
) -> NodeManifestVersion {
    localized(
        manifest(
            node_type,
            ExecutionStyle::Action,
            NodeCapability::Builtin,
            if node_type == "compare_datasets" {
                ReadinessPolicy::Required
            } else {
                ReadinessPolicy::Any
            },
            inputs,
            outputs,
            SideEffectLevel::None,
        ),
        en,
        zh,
        en_description,
        zh_description,
    )
}

fn localized(
    mut value: NodeManifestVersion,
    en: &str,
    zh: &str,
    en_description: &str,
    zh_description: &str,
) -> NodeManifestVersion {
    value.display_name = en.into();
    value.description = en_description.into();
    if let Some(locale) = value.localizations.get_mut("en-US") {
        locale.display_name = en.into();
        locale.description = en_description.into();
    }
    if let Some(locale) = value.localizations.get_mut("zh-CN") {
        locale.display_name = zh.into();
        locale.description = zh_description.into();
        locale.keywords = vec![zh.into()];
    }
    value
}

fn configured(
    mut value: NodeManifestVersion,
    schema: Value,
    ui_schema: Value,
) -> NodeManifestVersion {
    value.parameter_schema = schema;
    let canvas = value.ui_schema.canvas.clone();
    value.ui_schema =
        serde_json::from_value::<NodeUiSchema>(ui_schema).expect("valid built-in UI schema");
    value.ui_schema.canvas = canvas;
    value
}
