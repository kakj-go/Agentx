use std::collections::BTreeMap;

use agentx_domain::ResourceType;
use agentx_node_protocol::{
    BindingSlot, CanvasAppearance, CanvasNodeRole, ExecutionStyle, NODE_PROTOCOL_VERSION,
    NodeCapability, NodeManifestLocalization, NodeManifestVersion, NodePort, NodeUiSchema,
    PortKind, ReadinessPolicy, SideEffectLevel,
};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Clone, Debug, Default)]
pub struct NodeRegistry {
    manifests: BTreeMap<(String, u32), NodeManifestVersion>,
}

#[derive(Debug, Error, PartialEq)]
pub enum RegistryError {
    #[error("node manifest {node_type}@{version} already exists")]
    Duplicate { node_type: String, version: u32 },
    #[error("node manifest {node_type}@{version} uses unsupported protocol {protocol_version}")]
    UnsupportedProtocol {
        node_type: String,
        version: u32,
        protocol_version: String,
    },
    #[error("node manifest {node_type}@{version} is invalid: {message}")]
    InvalidManifest {
        node_type: String,
        version: u32,
        message: String,
    },
}

impl NodeRegistry {
    pub fn register(&mut self, manifest: NodeManifestVersion) -> Result<(), RegistryError> {
        if manifest.protocol_version != NODE_PROTOCOL_VERSION {
            return Err(RegistryError::UnsupportedProtocol {
                node_type: manifest.node_type,
                version: manifest.version,
                protocol_version: manifest.protocol_version,
            });
        }
        if let Err(message) = manifest.validate_localizations() {
            return Err(RegistryError::InvalidManifest {
                node_type: manifest.node_type,
                version: manifest.version,
                message,
            });
        }
        let key = manifest.key();
        if self.manifests.insert(key.clone(), manifest).is_some() {
            return Err(RegistryError::Duplicate {
                node_type: key.0,
                version: key.1,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn get(&self, node_type: &str, version: u32) -> Option<&NodeManifestVersion> {
        self.manifests.get(&(node_type.to_owned(), version))
    }

    pub fn manifests(&self) -> impl Iterator<Item = &NodeManifestVersion> {
        self.manifests.values()
    }

    #[must_use]
    pub fn m4_defaults() -> Self {
        let mut registry = Self::default();
        for manifest in default_manifests() {
            registry
                .register(manifest)
                .expect("default manifests are valid");
        }
        registry
    }

    #[must_use]
    pub fn m5_defaults() -> Self {
        Self::m4_defaults()
    }
}

pub(crate) fn port(name: &str, kind: PortKind, required: bool, variadic: bool) -> NodePort {
    NodePort {
        name: name.into(),
        kind,
        required,
        variadic,
    }
}

fn localized_node_copy(
    node_type: &str,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match node_type {
        "set" => (
            "Edit Fields",
            "编辑字段",
            "Set or transform item fields.",
            "设置或转换数据字段。",
        ),
        "if" => (
            "Condition",
            "条件",
            "Route items by a true or false condition.",
            "根据条件将数据路由到满足或不满足分支。",
        ),
        "switch" => (
            "Switch",
            "多路条件",
            "Route items across multiple conditions.",
            "根据多条条件将数据路由到不同分支。",
        ),
        "merge" => (
            "Merge",
            "合并",
            "Combine multiple workflow branches.",
            "合并多个工作流分支的数据。",
        ),
        "loop_over_items" => (
            "Loop Over Items",
            "循环处理",
            "Process items in controlled batches.",
            "按批次循环处理数据项。",
        ),
        "wait" => (
            "Wait",
            "等待",
            "Suspend execution until a resume condition.",
            "暂停执行并等待恢复条件。",
        ),
        "approval" => (
            "Approval",
            "审批",
            "Suspend execution for a human decision.",
            "暂停执行并等待人工审批。",
        ),
        "sub_workflow" => (
            "Sub-workflow",
            "子流程",
            "Run an immutable workflow version.",
            "执行一个不可变的工作流版本。",
        ),
        "declarative_http" => (
            "HTTP Request",
            "HTTP 请求",
            "Call an HTTP endpoint.",
            "调用 HTTP 接口。",
        ),
        "remote_action" => (
            "Remote Action",
            "远程动作",
            "Execute a remote Agentx node action.",
            "执行远程 Agentx 节点动作。",
        ),
        "model" => (
            "Model",
            "模型",
            "Invoke an authorized model resource.",
            "调用已授权的模型资源。",
        ),
        "mcp_tool" => (
            "Tool",
            "工具",
            "Invoke an authorized MCP tool.",
            "调用已授权的 MCP 工具。",
        ),
        "skill" => (
            "Skill",
            "技能",
            "Load an authorized Agent skill.",
            "加载已授权的 Agent 技能。",
        ),
        "rag" => (
            "Knowledge",
            "知识",
            "Query or update an authorized knowledge base.",
            "查询或更新已授权的知识库。",
        ),
        "memory" => (
            "Memory",
            "记忆",
            "Read or write authorized memory.",
            "读取或写入已授权的记忆。",
        ),
        "agent" => (
            "Agent",
            "智能体",
            "Reason with models, tools, memory, knowledge, and skills.",
            "使用模型、工具、记忆、知识和技能完成推理。",
        ),
        "code" => (
            "Code",
            "代码",
            "Run code in an isolated sandbox.",
            "在隔离沙箱中运行代码。",
        ),
        "error_handler" => (
            "Error Handler",
            "错误处理",
            "Recover an error item or fail the workflow.",
            "恢复错误数据或终止工作流。",
        ),
        _ => (
            "Action",
            "动作",
            "Execute a workflow action.",
            "执行一个工作流动作。",
        ),
    }
}

fn english_port_label(name: &str, input: bool) -> &'static str {
    match name {
        "main" => {
            if input {
                "Input"
            } else {
                "Output"
            }
        }
        "error" => "Error",
        "true" => "True",
        "false" => "False",
        "case" => "Case",
        "fallback" => "Fallback",
        "loop" => "Loop",
        "done" => "Done",
        "resumed" => "Resumed",
        "timed_out" => "Timed out",
        "approved" => "Approved",
        "rejected" => "Rejected",
        "recovered" => "Recovered",
        _ => "Port",
    }
}

fn chinese_port_label(name: &str, input: bool) -> &'static str {
    match name {
        "main" => {
            if input {
                "输入"
            } else {
                "输出"
            }
        }
        "error" => "错误",
        "true" => "满足条件",
        "false" => "不满足条件",
        "case" => "条件分支",
        "fallback" => "默认分支",
        "loop" => "循环",
        "done" => "完成",
        "resumed" => "已恢复",
        "timed_out" => "已超时",
        "approved" => "已通过",
        "rejected" => "已拒绝",
        "recovered" => "已恢复",
        _ => "端口",
    }
}

pub(crate) fn manifest(
    node_type: &str,
    style: ExecutionStyle,
    capability: NodeCapability,
    readiness: ReadinessPolicy,
    inputs: Vec<NodePort>,
    outputs: Vec<NodePort>,
    side_effect_level: SideEffectLevel,
) -> NodeManifestVersion {
    let role = canvas_role(node_type, style.clone(), capability.clone());
    let input_port_labels = inputs
        .iter()
        .map(|port| {
            (
                port.name.clone(),
                english_port_label(&port.name, true).into(),
            )
        })
        .collect();
    let output_port_labels = outputs
        .iter()
        .map(|port| {
            (
                port.name.clone(),
                english_port_label(&port.name, false).into(),
            )
        })
        .collect();
    let zh_input_port_labels = inputs
        .iter()
        .map(|port| {
            (
                port.name.clone(),
                chinese_port_label(&port.name, true).into(),
            )
        })
        .collect();
    let zh_output_port_labels = outputs
        .iter()
        .map(|port| {
            (
                port.name.clone(),
                chinese_port_label(&port.name, false).into(),
            )
        })
        .collect();
    let (english_name, chinese_name, english_description, chinese_description) =
        localized_node_copy(node_type);
    let mut localizations = BTreeMap::new();
    localizations.insert(
        "en-US".into(),
        NodeManifestLocalization {
            display_name: english_name.into(),
            description: english_description.into(),
            keywords: node_type.split('_').map(str::to_owned).collect(),
            input_port_labels,
            output_port_labels,
            binding_slot_labels: BTreeMap::new(),
            parameter_labels: BTreeMap::new(),
            parameter_descriptions: BTreeMap::new(),
            parameter_placeholders: BTreeMap::new(),
            parameter_enum_options: BTreeMap::new(),
        },
    );
    localizations.insert(
        "zh-CN".into(),
        NodeManifestLocalization {
            display_name: chinese_name.into(),
            description: chinese_description.into(),
            keywords: vec![chinese_name.into()],
            input_port_labels: zh_input_port_labels,
            output_port_labels: zh_output_port_labels,
            binding_slot_labels: BTreeMap::new(),
            parameter_labels: BTreeMap::new(),
            parameter_descriptions: BTreeMap::new(),
            parameter_placeholders: BTreeMap::new(),
            parameter_enum_options: BTreeMap::new(),
        },
    );
    let output_cardinality = outputs
        .iter()
        .map(|port| (port.name.clone(), Default::default()))
        .collect();
    NodeManifestVersion {
        protocol_version: NODE_PROTOCOL_VERSION.into(),
        node_type: node_type.into(),
        version: 1,
        display_name: english_name.into(),
        description: english_description.into(),
        category: category(node_type).into(),
        keywords: node_type.split('_').map(str::to_owned).collect(),
        localizations,
        icon_key: icon_key(node_type).into(),
        execution_style: style,
        capability,
        readiness,
        input_ports: inputs,
        output_ports: outputs,
        binding_slots: Vec::new(),
        parameter_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        output_port_schemas: BTreeMap::new(),
        output_cardinality,
        expression_capabilities: agentx_node_protocol::ExpressionCapabilities {
            namespaces: vec!["inputs".into(), "outputs".into(), "contexts".into()],
            supports_current: true,
            supports_first_last: true,
            supports_all: true,
            supports_run_selection: true,
        },
        context_read_capability: true,
        context_write_capability: true,
        output_projection_schema: json!({"type":"object","additionalProperties":true}),
        artifact_output_schema: json!({"type":"array","items":{"type":"object"}}),
        ui_schema: NodeUiSchema {
            canvas: Some(CanvasAppearance { role }),
            ..Default::default()
        },
        providers: Vec::new(),
        lifecycle_operations: Vec::new(),
        credentials: Vec::new(),
        default_timeout_ms: Some(30_000),
        retry_policy: Default::default(),
        sandbox_required: false,
        supports_mock: true,
        side_effect_level,
    }
}

fn m5_manifest(
    node_type: &str,
    capability: NodeCapability,
    schema: serde_json::Value,
    side_effect_level: SideEffectLevel,
) -> NodeManifestVersion {
    let resource_selector = match &capability {
        NodeCapability::Model => Some(("model", "use")),
        NodeCapability::McpTool => Some(("mcp_tool", "use")),
        NodeCapability::Skill => Some(("skill", "use")),
        NodeCapability::Rag => Some(("rag", "read")),
        NodeCapability::Memory => Some(("memory", "read")),
        NodeCapability::Sandbox => Some(("sandbox_profile", "use")),
        _ => None,
    };
    let mut value = manifest(
        node_type,
        ExecutionStyle::Action,
        capability,
        ReadinessPolicy::Any,
        vec![port("main", PortKind::Main, true, false)],
        vec![
            port("main", PortKind::Main, false, false),
            port("error", PortKind::Error, false, false),
        ],
        side_effect_level,
    );
    value.parameter_schema = schema;
    let fields = match node_type {
        "model" => {
            json!({"prompt":{"control":"prompt"},"userQuestion":{"control":"text"}})
        }
        "rag" | "memory" => json!({"operation":{"control":"select"},"input":{"control":"json"}}),
        "agent" => json!({
            "systemPrompt":{"control":"prompt"},"userQuestion":{"control":"text"},
            "maxIterations":{"control":"number","unit":"calls"},"maxModelCalls":{"control":"number","unit":"calls"},
            "maxToolCalls":{"control":"number","unit":"calls"},"maxTotalTokens":{"control":"number","unit":"tokens"},
            "maxOutputTokens":{"control":"number","unit":"tokens"},"maxCostMicros":{"control":"number","unit":"micros"},
            "maxDurationMs":{"control":"number","unit":"milliseconds"},"limitAction":{"control":"select"}
        }),
        "code" => json!({
            "runner":{"control":"select"},"source":{"control":"code","languageField":"runner"},
            "arguments":{"control":"json"},"networkPolicy":{"control":"json"},
            "outputPaths":{"control":"json"},"credentialFiles":{"control":"json"}
        }),
        _ => json!({}),
    };
    value.ui_schema.fields = fields
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    if let Some((resource_type, operation)) = resource_selector {
        value.ui_schema.resource_selectors.push(json!({
            "resourceType":resource_type,
            "operation":operation,
            "required":true
        }));
    }
    if node_type == "code" {
        value.ui_schema.resource_selectors.push(json!({
            "resourceType":"credential",
            "operation":"use",
            "required":false,
            "label":"Credential"
        }));
    }
    value.default_timeout_ms = Some(300_000);
    value.supports_mock = false;
    value.sandbox_required = node_type == "code";
    value
}

fn canvas_role(
    node_type: &str,
    execution_style: ExecutionStyle,
    capability: NodeCapability,
) -> CanvasNodeRole {
    if execution_style == ExecutionStyle::Trigger {
        return CanvasNodeRole::Trigger;
    }
    if execution_style == ExecutionStyle::Suspend {
        return CanvasNodeRole::Suspend;
    }
    if execution_style == ExecutionStyle::SubWorkflow {
        return CanvasNodeRole::SubWorkflow;
    }
    if matches!(node_type, "if" | "switch") {
        return CanvasNodeRole::Branch;
    }
    if node_type == "merge" {
        return CanvasNodeRole::Merge;
    }
    if node_type == "loop_over_items" {
        return CanvasNodeRole::Loop;
    }
    if node_type == "approval" {
        return CanvasNodeRole::Approval;
    }
    if node_type == "error_handler" {
        return CanvasNodeRole::ErrorHandler;
    }
    if capability == NodeCapability::Agent {
        return CanvasNodeRole::Agent;
    }
    if capability == NodeCapability::Sandbox {
        return CanvasNodeRole::Code;
    }
    if category(node_type) == "flow" {
        return CanvasNodeRole::Flow;
    }
    CanvasNodeRole::Default
}

fn category(node_type: &str) -> &'static str {
    match node_type {
        "item_generator" => "triggers",
        "if" | "switch" | "merge" | "loop_over_items" | "wait" | "approval" | "sub_workflow"
        | "error_handler" | "no_op" | "stop_and_error" => "flow",
        "filter"
        | "limit"
        | "sort"
        | "remove_duplicates"
        | "split_out"
        | "aggregate"
        | "rename_fields"
        | "json_transform"
        | "date_time"
        | "base64"
        | "hash"
        | "compare_datasets"
        | "structured_validator" => "data",
        "agent" | "model" | "mcp_tool" | "skill" | "rag" | "memory" => "ai",
        "code" => "code",
        _ => "actions",
    }
}

fn icon_key(node_type: &str) -> &'static str {
    match node_type {
        "agent" => "bot",
        "model" => "brain-circuit",
        "mcp_tool" => "wrench",
        "skill" => "sparkles",
        "rag" => "database",
        "memory" => "memory-stick",
        "code" => "code-2",
        "approval" => "badge-check",
        "if" | "switch" => "split",
        "merge" => "git-merge",
        "loop_over_items" => "repeat-2",
        "wait" => "clock-3",
        "error_handler" => "shield-alert",
        "filter" => "filter",
        "limit" => "list-end",
        "sort" => "arrow-down-a-z",
        "remove_duplicates" => "copy-minus",
        "split_out" => "rows-3",
        "aggregate" => "sigma",
        "rename_fields" => "replace",
        "json_transform" => "braces",
        "no_op" => "circle",
        "stop_and_error" => "octagon-x",
        "item_generator" => "list-plus",
        "date_time" => "calendar-clock",
        "base64" => "binary",
        "hash" => "hash",
        "compare_datasets" => "git-compare",
        "structured_validator" => "shield-check",
        _ => "box",
    }
}

fn default_manifests() -> Vec<NodeManifestVersion> {
    let main_in = || vec![port("main", PortKind::Main, true, false)];
    let main_out = || {
        vec![
            port("main", PortKind::Main, false, false),
            port("error", PortKind::Error, false, false),
        ]
    };
    let configured = |mut manifest: NodeManifestVersion,
                      parameter_schema: serde_json::Value,
                      ui_schema: serde_json::Value| {
        manifest.parameter_schema = parameter_schema;
        let canvas = manifest.ui_schema.canvas.clone();
        manifest.ui_schema = serde_json::from_value(ui_schema).expect("valid UI schema");
        manifest.ui_schema.canvas = canvas;
        manifest
    };
    let mut manifests = vec![
        configured(
            manifest(
                "set",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                main_out(),
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"values":{"type":"object","default":{}},"keepOnlySet":{"type":"boolean","default":false}},"additionalProperties":false}),
            json!({"order":["values","keepOnlySet"],"fields":{"values":{"control":"json"},"keepOnlySet":{"control":"boolean"}}}),
        ),
        configured(
            manifest(
                "error_handler",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                vec![port("error", PortKind::Error, true, false)],
                vec![port("recovered", PortKind::Main, false, false)],
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"mode":{"type":"string","enum":["recover","fail"],"default":"recover"}},"additionalProperties":false}),
            json!({"order":["mode"],"fields":{"mode":{"control":"select"}}}),
        ),
        configured(
            manifest(
                "if",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("true", PortKind::Main, false, false),
                    port("false", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","required":["condition"],"properties":{"condition":{}},"additionalProperties":false}),
            json!({"fields":{"condition":{"control":"expression"}}}),
        ),
        configured(
            manifest(
                "switch",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("case", PortKind::Main, false, true),
                    port("fallback", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","required":["rules"],"properties":{"rules":{"type":"array","items":{"type":"object","required":["condition"],"properties":{"condition":{}}}},"sendToAllMatches":{"type":"boolean","default":false}},"additionalProperties":false}),
            json!({"order":["rules","sendToAllMatches"],"fields":{"rules":{"control":"json"},"sendToAllMatches":{"control":"boolean"}}}),
        ),
        configured(
            manifest(
                "merge",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Required,
                vec![
                    port("main", PortKind::Main, false, true),
                    port("left", PortKind::Main, false, false),
                    port("right", PortKind::Main, false, false),
                ],
                main_out(),
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"mode":{"type":"string","enum":["append","combine_by_position","combine_by_key"],"default":"append"},"leftField":{"type":"string","default":"id"},"rightField":{"type":"string","default":"id"},"joinType":{"type":"string","enum":["inner","left","right","full"],"default":"inner"},"conflictStrategy":{"type":"string","enum":["prefer_left","prefer_right","suffix"],"default":"prefer_right"}},"additionalProperties":false}),
            json!({"fields":{"mode":{"control":"select"},"leftField":{"control":"text"},"rightField":{"control":"text"},"joinType":{"control":"select"},"conflictStrategy":{"control":"select"}}}),
        ),
        configured(
            manifest(
                "loop_over_items",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("loop", PortKind::Main, false, false),
                    port("done", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"batchSize":{"type":"integer","minimum":1,"default":1}},"additionalProperties":false}),
            json!({"fields":{"batchSize":{"control":"number"}}}),
        ),
        configured(
            manifest(
                "wait",
                ExecutionStyle::Suspend,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("resumed", PortKind::Main, false, false),
                    port("timed_out", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"kind":{"type":"string","enum":["duration","datetime","webhook","form"],"default":"duration"},"durationMs":{"type":"integer","minimum":1,"default":1000},"resumeAt":{"type":"string"},"timeoutAt":{"type":"string"},"payloadSchema":{"type":"object"},"authenticationMode":{"type":"string","enum":["signed","none"],"default":"signed"}},"additionalProperties":false}),
            json!({"order":["kind","durationMs","resumeAt","timeoutAt","payloadSchema","authenticationMode"],"fields":{"kind":{"control":"select"},"durationMs":{"control":"number","visibleWhen":{"field":"kind","equals":"duration"}},"resumeAt":{"control":"text","visibleWhen":{"field":"kind","equals":"datetime"}},"timeoutAt":{"control":"text"},"payloadSchema":{"control":"json"},"authenticationMode":{"control":"select"}}}),
        ),
        configured(
            manifest(
                "approval",
                ExecutionStyle::Suspend,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("approved", PortKind::Main, false, false),
                    port("rejected", PortKind::Main, false, false),
                    port("timed_out", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"title":{"type":"string"},"description":{"type":"string"},"candidateUserId":{"type":"string"},"timeoutMs":{"type":"integer","minimum":1},"timeoutAt":{"type":"string"}},"additionalProperties":false}),
            json!({"order":["title","description","candidateUserId","timeoutMs","timeoutAt"],"fields":{"title":{"control":"text"},"description":{"control":"textarea"},"candidateUserId":{"control":"text"},"timeoutMs":{"control":"number"},"timeoutAt":{"control":"text"}}}),
        ),
        {
            let mut sub_workflow = configured(
                manifest(
                    "sub_workflow",
                    ExecutionStyle::SubWorkflow,
                    NodeCapability::Builtin,
                    ReadinessPolicy::Any,
                    main_in(),
                    main_out(),
                    SideEffectLevel::None,
                ),
                json!({"type":"object","required":["workflowVersionId"],"properties":{"workflowVersionId":{"type":"string","format":"uuid"}},"additionalProperties":false}),
                json!({"fields":{"workflowVersionId":{"control":"provider_options","provider":"workflow_versions"}}}),
            );
            sub_workflow.providers = vec!["workflow_versions".into()];
            sub_workflow
        },
        configured(
            manifest(
                "declarative_http",
                ExecutionStyle::Action,
                NodeCapability::DeclarativeHttp,
                ReadinessPolicy::Any,
                main_in(),
                main_out(),
                SideEffectLevel::Idempotent,
            ),
            json!({"type":"object","required":["url"],"properties":{"method":{"type":"string","enum":["GET","POST","PUT","PATCH","DELETE"],"default":"GET"},"url":{"type":"string"},"headers":{"type":"object"},"body":{}},"additionalProperties":false}),
            json!({"order":["method","url","headers","body"],"fields":{"method":{"control":"select"},"url":{"control":"expression"},"headers":{"control":"json"},"body":{"control":"json"}}}),
        ),
        {
            let mut remote_action = configured(
                manifest(
                    "remote_action",
                    ExecutionStyle::Action,
                    NodeCapability::RemoteAction,
                    ReadinessPolicy::Any,
                    main_in(),
                    main_out(),
                    SideEffectLevel::Irreversible,
                ),
                json!({"type":"object","required":["endpoint"],"properties":{"endpoint":{"type":"string"}},"additionalProperties":true}),
                json!({"fields":{"endpoint":{"control":"text"}}}),
            );
            remote_action.lifecycle_operations = vec![
                agentx_node_protocol::LifecycleOperation::Activate,
                agentx_node_protocol::LifecycleOperation::Deactivate,
                agentx_node_protocol::LifecycleOperation::Poll,
                agentx_node_protocol::LifecycleOperation::Webhook,
            ];
            remote_action
        },
        m5_manifest(
            "model",
            NodeCapability::Model,
            json!({"type":"object","properties":{"prompt":{"type":"string"},"userQuestion":{"type":"string","templatable":true,"allowedNamespaces":["inputs","outputs","contexts","execution","item","loop"],"expectedType":"string","multiline":false,"richText":false}},"additionalProperties":false}),
            SideEffectLevel::None,
        ),
        m5_manifest(
            "mcp_tool",
            NodeCapability::McpTool,
            json!({"type":"object","properties":{"resourceId":{"type":"string","format":"uuid"},"arguments":{}},"additionalProperties":false}),
            SideEffectLevel::Irreversible,
        ),
        m5_manifest(
            "skill",
            NodeCapability::Skill,
            json!({"type":"object","properties":{"resourceId":{"type":"string","format":"uuid"}},"additionalProperties":false}),
            SideEffectLevel::None,
        ),
        m5_manifest(
            "rag",
            NodeCapability::Rag,
            json!({"type":"object","required":["operation"],"properties":{"operation":{"enum":["query","retrieve","insert","delete","health_check"]},"input":{}},"additionalProperties":false}),
            SideEffectLevel::Reversible,
        ),
        m5_manifest(
            "memory",
            NodeCapability::Memory,
            json!({"type":"object","required":["operation"],"properties":{"operation":{"enum":["get","search","add","update","delete"]},"input":{}},"additionalProperties":false}),
            SideEffectLevel::Reversible,
        ),
        {
            let mut agent = m5_manifest(
                "agent",
                NodeCapability::Agent,
                json!({
                    "type":"object",
                    "properties":{
                        "systemPrompt":{"type":"string"},"userQuestion":{"type":"string","templatable":true,"allowedNamespaces":["inputs","outputs","contexts","execution","item","loop"],"expectedType":"string","multiline":false,"richText":false},
                        "maxIterations":{"type":"integer","minimum":1,"maximum":12,"default":12},
                        "maxModelCalls":{"type":"integer","minimum":1,"maximum":12,"default":12},
                        "maxToolCalls":{"type":"integer","minimum":0,"maximum":32,"default":32},
                        "maxTotalTokens":{"type":"integer","minimum":1,"maximum":64000,"default":64000},
                        "maxOutputTokens":{"type":"integer","minimum":1,"maximum":64000,"default":4096},
                        "maxCostMicros":{"type":"integer","minimum":0,"maximum":1000000,"default":1000000},
                        "maxDurationMs":{"type":"integer","minimum":1000,"maximum":300000,"default":300000},
                        "limitAction":{"enum":["fail","error_output","partial"],"default":"error_output"}
                    },
                    "additionalProperties":false
                }),
                SideEffectLevel::Irreversible,
            );
            agent.binding_slots = vec![
                BindingSlot {
                    name: "ai_model".into(),
                    resource_type: ResourceType::Model,
                    required: true,
                    multiple: false,
                },
                BindingSlot {
                    name: "ai_tool".into(),
                    resource_type: ResourceType::McpTool,
                    required: false,
                    multiple: true,
                },
                BindingSlot {
                    name: "ai_memory".into(),
                    resource_type: ResourceType::Memory,
                    required: false,
                    multiple: false,
                },
                BindingSlot {
                    name: "ai_retriever".into(),
                    resource_type: ResourceType::Rag,
                    required: false,
                    multiple: true,
                },
                BindingSlot {
                    name: "ai_skill".into(),
                    resource_type: ResourceType::Skill,
                    required: false,
                    multiple: true,
                },
            ];
            for (locale, labels) in [
                (
                    "en-US",
                    [
                        ("ai_model", "Model"),
                        ("ai_tool", "Tool"),
                        ("ai_memory", "Memory"),
                        ("ai_retriever", "Knowledge"),
                        ("ai_skill", "Skill"),
                    ],
                ),
                (
                    "zh-CN",
                    [
                        ("ai_model", "模型"),
                        ("ai_tool", "工具"),
                        ("ai_memory", "记忆"),
                        ("ai_retriever", "知识"),
                        ("ai_skill", "技能"),
                    ],
                ),
            ] {
                agent
                    .localizations
                    .get_mut(locale)
                    .expect("locale exists")
                    .binding_slot_labels = labels
                    .into_iter()
                    .map(|(name, label)| (name.into(), label.into()))
                    .collect();
            }
            agent
        },
        m5_manifest(
            "code",
            NodeCapability::Sandbox,
            json!({
                "type":"object","required":["runner","source"],
                "properties":{"runner":{"enum":["python","javascript","shell","browser"]},"source":{"type":"string"},"arguments":{"type":"array","items":{"type":"string"}},"networkPolicy":{"type":"object"},"outputPaths":{"type":"array","items":{"type":"string"}},"credentialFiles":{"type":"object","propertyNames":{"pattern":"^[A-Z_][A-Z0-9_]*$"},"additionalProperties":{"type":"string","format":"uuid"}}},
                "additionalProperties":false
            }),
            SideEffectLevel::Irreversible,
        ),
    ];
    manifests.extend(crate::builtin_catalog::manifests());
    for manifest in &mut manifests {
        configure_workflow_v4_capabilities(manifest);
        populate_parameter_localizations(manifest);
    }
    manifests
}

fn populate_parameter_localizations(manifest: &mut NodeManifestVersion) {
    let properties = parameter_schema_paths(&manifest.parameter_schema);
    for (path, property) in properties {
        let name = path
            .trim_end_matches("[]")
            .rsplit('.')
            .next()
            .unwrap_or(path.as_str())
            .trim_end_matches("[]");
        let english = humanize_protocol_name(name);
        let chinese = chinese_parameter_name(name);
        for (locale, label) in [("en-US", english), ("zh-CN", chinese)] {
            let localization = manifest
                .localizations
                .get_mut(locale)
                .expect("built-in locale exists");
            localization.parameter_labels.insert(path.clone(), label);
            if property.get("templatable").and_then(Value::as_bool) == Some(true) {
                localization
                    .parameter_placeholders
                    .entry(path.clone())
                    .or_insert_with(|| "${{ inputs.value }}".into());
            }
            if let Some(options) = property.get("enum").and_then(Value::as_array) {
                let labels = options
                    .iter()
                    .map(|option| {
                        let value = option.as_str().unwrap_or_default();
                        (
                            value.to_owned(),
                            if locale == "zh-CN" {
                                chinese_enum_label(value)
                            } else {
                                humanize_protocol_name(value)
                            },
                        )
                    })
                    .collect();
                localization
                    .parameter_enum_options
                    .insert(path.clone(), labels);
            }
        }
    }
}

fn parameter_schema_paths(schema: &Value) -> Vec<(String, Value)> {
    let mut paths = Vec::new();
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            collect_parameter_schema_paths(name, property, &mut paths);
        }
    }
    paths
}

fn collect_parameter_schema_paths(path: &str, schema: &Value, paths: &mut Vec<(String, Value)>) {
    paths.push((path.to_owned(), schema.clone()));
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            collect_parameter_schema_paths(&format!("{path}.{name}"), property, paths);
        }
    }
    if let Some(items) = schema.get("items") {
        collect_parameter_schema_paths(&format!("{path}[]"), items, paths);
    }
}

fn humanize_protocol_name(value: &str) -> String {
    let mut label = String::with_capacity(value.len() + 4);
    for (index, character) in value.chars().enumerate() {
        if character == '_' {
            label.push(' ');
        } else {
            if index > 0 && character.is_ascii_uppercase() {
                label.push(' ');
            }
            label.push(character);
        }
    }
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}

fn chinese_parameter_name(value: &str) -> String {
    match value {
        "values" => "字段值".into(),
        "keepOnlySet" => "仅保留已设置字段".into(),
        "condition" => "条件".into(),
        "mode" => "模式".into(),
        "operation" => "操作".into(),
        "prompt" => "提示词".into(),
        "systemPrompt" => "系统提示词".into(),
        "userQuestion" => "用户问题".into(),
        "arguments" => "调用参数".into(),
        "endpoint" => "端点".into(),
        "url" => "地址".into(),
        "method" => "请求方法".into(),
        "headers" => "请求头".into(),
        "body" => "请求体".into(),
        "runner" => "运行器".into(),
        "source" => "源代码".into(),
        "networkPolicy" => "网络策略".into(),
        "outputPaths" => "输出路径".into(),
        "kind" => "类型".into(),
        "durationMs" => "持续时间".into(),
        "timeoutMs" => "超时时间".into(),
        "batchSize" => "批大小".into(),
        "limitAction" => "超限动作".into(),
        "maxIterations" => "最大迭代次数".into(),
        "maxModelCalls" => "最大模型调用数".into(),
        "maxToolCalls" => "最大工具调用数".into(),
        "maxTotalTokens" => "最大 Token 数".into(),
        "maxOutputTokens" => "最大输出 Token".into(),
        "maxCostMicros" => "最大成本".into(),
        "maxDurationMs" => "最大时长".into(),
        "workflowVersionId" => "工作流版本".into(),
        "authenticationMode" => "认证方式".into(),
        "statusCode" => "状态码".into(),
        "structuredJson" => "结构化 JSON".into(),
        "structuredOutputs" => "结构化输出".into(),
        "algorithm" => "算法".into(),
        "amount" => "数量".into(),
        "compareTo" => "比较目标".into(),
        "direction" => "排序方向".into(),
        "encoding" => "编码方式".into(),
        "field" => "字段".into(),
        "fields" => "字段列表".into(),
        "format" => "格式".into(),
        "from" => "来源字段".into(),
        "groupBy" => "分组字段".into(),
        "items" => "数据项".into(),
        "keep" => "保留策略".into(),
        "keyFields" => "键字段".into(),
        "mappings" => "字段映射".into(),
        "maxItems" => "最大项数".into(),
        "missingField" => "缺失字段策略".into(),
        "nulls" => "空值位置".into(),
        "operations" => "聚合操作".into(),
        "outputField" => "输出字段".into(),
        "properties" => "字段定义".into(),
        "schema" => "数据结构".into(),
        "step" => "步长".into(),
        "to" => "目标字段".into(),
        "unit" => "单位".into(),
        "candidateUserId" => "候选用户".into(),
        "conflictStrategy" => "冲突策略".into(),
        "joinType" => "连接类型".into(),
        "payloadSchema" => "载荷结构".into(),
        "resumeAt" => "恢复时间".into(),
        "timeoutAt" => "超时时间点".into(),
        "input" => "输入数据".into(),
        _ => "参数".into(),
    }
}

fn chinese_enum_label(value: &str) -> String {
    match value {
        "fail_fast" | "fail" | "stop" => "失败并停止".into(),
        "collect" => "收集".into(),
        "recover" => "恢复".into(),
        "append" => "追加".into(),
        "replace" => "覆盖".into(),
        "merge_object" => "合并对象".into(),
        "increment" => "递增".into(),
        "python" => "Python".into(),
        "javascript" => "JavaScript".into(),
        "shell" => "Shell".into(),
        "browser" => "浏览器".into(),
        "sync" => "同步".into(),
        "async" => "异步".into(),
        "approved" => "通过".into(),
        "rejected" => "拒绝".into(),
        "first" => "第一个".into(),
        "last" => "最后一个".into(),
        "asc" => "升序".into(),
        "desc" => "降序".into(),
        "count" => "计数".into(),
        "sum" => "求和".into(),
        "avg" => "平均值".into(),
        "ignore" => "忽略".into(),
        "error" => "报错".into(),
        "parse" => "解析".into(),
        "stringify" => "转为文本".into(),
        "format" => "格式化".into(),
        "add" => "增加".into(),
        "subtract" => "减少".into(),
        "difference" => "差值".into(),
        "rfc3339" => "RFC3339".into(),
        "unix" => "Unix 时间戳".into(),
        "seconds" => "秒".into(),
        "minutes" => "分钟".into(),
        "hours" => "小时".into(),
        "days" => "天".into(),
        "encode" => "编码".into(),
        "decode" => "解码".into(),
        "hex" => "十六进制".into(),
        "base64" => "Base64".into(),
        "sha256" => "SHA-256".into(),
        "sha512" => "SHA-512".into(),
        "route" => "路由".into(),
        "combine_by_position" => "按位置合并".into(),
        "combine_by_key" => "按键合并".into(),
        "inner" => "内连接".into(),
        "left" => "左连接".into(),
        "right" => "右连接".into(),
        "full" => "全连接".into(),
        "prefer_left" => "优先左侧".into(),
        "prefer_right" => "优先右侧".into(),
        "suffix" => "添加后缀".into(),
        "duration" => "持续时间".into(),
        "datetime" => "指定时间".into(),
        "webhook" => "Webhook".into(),
        "form" => "表单".into(),
        "signed" => "签名".into(),
        "none" => "无".into(),
        "GET" => "GET".into(),
        "POST" => "POST".into(),
        "PUT" => "PUT".into(),
        "PATCH" => "PATCH".into(),
        "DELETE" => "DELETE".into(),
        "query" => "查询".into(),
        "retrieve" => "读取".into(),
        "insert" => "写入".into(),
        "health_check" => "健康检查".into(),
        "get" => "获取".into(),
        "search" => "搜索".into(),
        "update" => "更新".into(),
        "error_output" => "输出错误".into(),
        "partial" => "部分结果".into(),
        _ => "选项".into(),
    }
}

fn configure_workflow_v4_capabilities(manifest: &mut NodeManifestVersion) {
    let dynamic_projection = matches!(
        manifest.node_type.as_str(),
        "code"
            | "declarative_http"
            | "http_request"
            | "remote_action"
            | "model"
            | "agent"
            | "rag"
            | "mcp_tool"
            | "memory"
            | "set"
            | "json_transform"
            | "json_parse"
    );
    if !dynamic_projection {
        manifest.output_projection_schema = Value::Null;
    }
    match manifest.node_type.as_str() {
        "if" | "switch" => {
            for port in &manifest.output_ports {
                manifest.output_cardinality.insert(
                    port.name.clone(),
                    agentx_node_protocol::OutputCardinality::ZeroOrMany,
                );
            }
        }
        "loop_over_items" => {
            for port in &manifest.output_ports {
                manifest.output_cardinality.insert(
                    port.name.clone(),
                    agentx_node_protocol::OutputCardinality::ZeroOrMany,
                );
            }
        }
        "wait" | "approval" => {
            for port in &manifest.output_ports {
                manifest.output_cardinality.insert(
                    port.name.clone(),
                    agentx_node_protocol::OutputCardinality::ZeroOrOne,
                );
            }
        }
        "declarative_http" | "http_request" => {
            manifest.output_schema = json!({"type":"object","properties":{"status":{"type":"integer"},"statusCode":{"type":"integer"},"headers":{"type":"object"},"body":{},"responseArtifact":{"type":["object","null"]}},"required":["status","statusCode","headers","body"]});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "model" => {
            manifest.output_schema = json!({"type":"object","properties":{"text":{"type":"string"},"message":{},"structuredJson":{},"citations":{"type":"array"},"toolCalls":{"type":"array"},"usage":{"type":"object"},"finishReason":{"type":["string","null"]},"stopReason":{"type":["string","null"]},"partial":{"type":"boolean"}},"required":["text","toolCalls","usage","partial"]});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "agent" => {
            manifest.output_schema = json!({"type":"object","properties":{"finalAnswer":{"type":"string"},"message":{},"messages":{"type":"array"},"toolCalls":{"type":"integer"},"artifacts":{"type":"array"},"citations":{"type":"array"},"usage":{"type":"object"},"stopReason":{"type":["string","null"]}},"required":["finalAnswer","messages","artifacts","citations","usage"]});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "mcp_tool" => {
            manifest.output_schema = json!({"type":"object","properties":{"structuredContent":{},"textContent":{"type":"array"},"content":{"type":"array"},"isError":{"type":"boolean"}},"required":["content","textContent","isError"]});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "rag" => {
            manifest.output_schema = json!({"type":"object","properties":{"documents":{"type":"array"},"chunks":{"type":"array"},"citations":{"type":"array"},"text":{"type":"string"},"recordIds":{"type":"array"}},"additionalProperties":true});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "memory" => {
            manifest.output_schema = json!({"type":"object","properties":{"records":{"type":"array"},"recordIds":{"type":"array"},"text":{"type":"string"}},"additionalProperties":true});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "code" => {
            manifest.output_schema = json!({"type":"object","properties":{"stdout":{"type":"string"},"stderr":{"type":"string"},"exitCode":{"type":"integer"},"partial":{"type":"boolean"},"sandboxId":{"type":"string"},"downloadedArtifacts":{"type":"array","items":{"type":"object"}},"structuredOutputs":{"type":"object"}},"required":["stdout","stderr","exitCode","partial","downloadedArtifacts"]});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        _ => {}
    }
    if matches!(
        manifest.node_type.as_str(),
        "if" | "switch"
            | "merge"
            | "loop_over_items"
            | "wait"
            | "approval"
            | "error_handler"
            | "stop_and_error"
    ) {
        manifest.context_write_capability = false;
    }
    let controls = manifest
        .ui_schema
        .fields
        .iter()
        .filter_map(|(name, field)| {
            field
                .get("control")
                .and_then(Value::as_str)
                .map(|control| (name.clone(), control.to_owned()))
        })
        .collect::<Vec<_>>();
    if let Some(properties) = manifest
        .parameter_schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        for (name, control) in controls {
            if matches!(
                control.as_str(),
                "text"
                    | "textarea"
                    | "prompt"
                    | "expression"
                    | "json"
                    | "mapper"
                    | "fixed_collection"
                    | "number"
                    | "boolean"
                    | "select"
            ) && let Some(property) = properties.get_mut(&name).and_then(Value::as_object_mut)
            {
                property.insert("templatable".into(), Value::Bool(true));
                property.insert(
                    "allowedNamespaces".into(),
                    json!(["inputs", "outputs", "contexts", "execution", "item", "loop"]),
                );
                property.insert(
                    "expectedType".into(),
                    property
                        .get("type")
                        .cloned()
                        .unwrap_or_else(|| json!("any")),
                );
                property.insert(
                    "multiline".into(),
                    Value::Bool(matches!(
                        control.as_str(),
                        "textarea" | "prompt" | "expression" | "json"
                    )),
                );
                property.insert("richText".into(), Value::Bool(false));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_versions_are_not_coerced() {
        let registry = NodeRegistry::m4_defaults();
        assert!(registry.get("set", 1).is_some());
        assert!(registry.get("set", 2).is_none());
    }

    #[test]
    fn wait_manifest_accepts_form_resume() {
        let registry = NodeRegistry::m4_defaults();
        let schema = &registry
            .get("wait", 1)
            .expect("wait manifest")
            .parameter_schema;
        let kinds = schema["properties"]["kind"]["enum"]
            .as_array()
            .expect("wait kinds");
        assert!(kinds.iter().any(|kind| kind == "form"));
    }

    #[test]
    fn built_in_localizations_reference_declared_protocol_names() {
        let registry = NodeRegistry::m5_defaults();
        for manifest in registry.manifests() {
            manifest
                .validate_localizations()
                .expect("valid localization");
            assert!(manifest.localizations.contains_key("zh-CN"));
            assert!(manifest.localizations.contains_key("en-US"));
        }

        let aggregate = registry.get("aggregate", 1).expect("aggregate manifest");
        let english = aggregate
            .localizations
            .get("en-US")
            .expect("English locale");
        assert!(
            english
                .parameter_labels
                .contains_key("operations[].operation")
        );
        assert!(
            english
                .parameter_enum_options
                .contains_key("operations[].operation")
        );
        assert!(english.parameter_descriptions.is_empty());

        let mut invalid = registry.get("if", 1).expect("if manifest").clone();
        invalid
            .localizations
            .get_mut("zh-CN")
            .expect("Chinese localization")
            .output_port_labels
            .insert("missing".into(), "不存在".into());
        assert!(invalid.validate_localizations().is_err());
        let mut target = NodeRegistry::default();
        assert!(matches!(
            target.register(invalid),
            Err(RegistryError::InvalidManifest { .. })
        ));

        let mut invalid_nested = aggregate.clone();
        invalid_nested
            .localizations
            .get_mut("en-US")
            .expect("English localization")
            .parameter_labels
            .insert("operations[].missing".into(), "Missing".into());
        assert!(invalid_nested.validate_localizations().is_err());
    }

    #[test]
    fn agent_and_standalone_nodes_share_skill_and_tool_resources() {
        let registry = NodeRegistry::m5_defaults();
        assert_eq!(
            registry
                .get("mcp_tool", 1)
                .expect("MCP manifest")
                .capability,
            NodeCapability::McpTool
        );
        assert_eq!(
            registry.get("skill", 1).expect("Skill manifest").capability,
            NodeCapability::Skill
        );

        let agent = registry.get("agent", 1).expect("agent manifest");
        assert_eq!(
            agent.parameter_schema["properties"]["userQuestion"]["type"],
            "string"
        );
        assert!(
            agent.parameter_schema["properties"]
                .get("messages")
                .is_none()
        );
        assert!(
            agent
                .binding_slots
                .iter()
                .any(|slot| slot.name == "ai_tool")
        );
        assert!(
            agent
                .binding_slots
                .iter()
                .any(|slot| slot.name == "ai_skill")
        );
    }

    #[test]
    fn local_data_catalog_contains_all_new_capabilities() {
        let registry = NodeRegistry::m5_defaults();
        for node_type in crate::builtin_catalog::NODE_TYPES {
            let manifest = registry.get(node_type, 1).expect("local data manifest");
            assert_eq!(manifest.capability, NodeCapability::Builtin);
            assert!(manifest.providers.is_empty());
            assert!(manifest.credentials.is_empty());
            assert!(manifest.localizations.contains_key("en-US"));
            assert!(manifest.localizations.contains_key("zh-CN"));
        }
        let merge = registry.get("merge", 1).expect("merge manifest");
        assert_eq!(
            merge
                .input_ports
                .iter()
                .map(|port| port.name.as_str())
                .collect::<Vec<_>>(),
            ["main", "left", "right"]
        );
        assert!(
            merge.parameter_schema["properties"]["mode"]["enum"]
                .as_array()
                .expect("merge modes")
                .iter()
                .any(|value| value == "combine_by_key")
        );
    }
}
