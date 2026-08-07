use std::collections::BTreeMap;

use agentx_domain::ResourceType;
use agentx_node_protocol::{
    BindingSlot, CanvasAppearance, CanvasNodeRole, ExecutionStyle, LifecycleOperation,
    NODE_PROTOCOL_VERSION, NodeCapability, NodeManifestLocalization, NodeManifestVersion, NodePort,
    NodeUiSchema, PortKind, ReadinessPolicy, SideEffectLevel,
};
use serde_json::json;
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

fn port(name: &str, kind: PortKind, required: bool, variadic: bool) -> NodePort {
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
        "manual_trigger" => (
            "Manual Trigger",
            "手动触发",
            "Start a workflow manually.",
            "手动启动工作流。",
        ),
        "remote_trigger" => (
            "Remote Trigger",
            "远程触发",
            "Start from a remote lifecycle event.",
            "通过远程生命周期事件启动工作流。",
        ),
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

fn manifest(
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
        },
    );
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
            json!({"prompt":{"control":"prompt"},"messages":{"control":"collection"},"parameters":{"control":"json"}})
        }
        "mcp_tool" => json!({"arguments":{"control":"json"}}),
        "rag" | "memory" => json!({"operation":{"control":"select"},"input":{"control":"json"}}),
        "agent" => json!({
            "systemPrompt":{"control":"prompt"},"messages":{"control":"collection"},
            "maxIterations":{"control":"number"},"maxModelCalls":{"control":"number"},
            "maxToolCalls":{"control":"number"},"maxTotalTokens":{"control":"number"},
            "maxOutputTokens":{"control":"number"},"maxCostMicros":{"control":"number"},
            "maxDurationMs":{"control":"number"},"limitAction":{"control":"select"}
        }),
        "code" => json!({
            "runner":{"control":"select"},"source":{"control":"code","languageField":"runner"},
            "arguments":{"control":"collection"},"networkPolicy":{"control":"json"},
            "outputPaths":{"control":"collection"},"credentialFiles":{"control":"fixed_collection"}
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
        "manual_trigger" | "remote_trigger" => "triggers",
        "if" | "switch" | "merge" | "loop_over_items" | "wait" | "approval" | "sub_workflow"
        | "error_handler" => "flow",
        "agent" | "model" | "mcp_tool" | "skill" | "rag" | "memory" => "ai",
        "code" => "code",
        _ => "actions",
    }
}

fn icon_key(node_type: &str) -> &'static str {
    match node_type {
        "manual_trigger" => "mouse-pointer-click",
        "remote_trigger" => "radio-tower",
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
    vec![
        manifest(
            "manual_trigger",
            ExecutionStyle::Trigger,
            NodeCapability::Builtin,
            ReadinessPolicy::Any,
            vec![],
            vec![port("main", PortKind::Main, false, false)],
            SideEffectLevel::None,
        ),
        {
            let mut trigger = configured(
                manifest(
                    "remote_trigger",
                    ExecutionStyle::Trigger,
                    NodeCapability::RemoteAction,
                    ReadinessPolicy::Any,
                    vec![],
                    vec![port("main", PortKind::Main, false, false)],
                    SideEffectLevel::None,
                ),
                json!({"type":"object","required":["endpoint"],"properties":{"endpoint":{"type":"string"},"pollIntervalSeconds":{"type":"integer","minimum":1,"maximum":86400,"default":60}},"additionalProperties":true}),
                json!({"order":["endpoint","pollIntervalSeconds"],"fields":{"endpoint":{"control":"text"},"pollIntervalSeconds":{"control":"number"}}}),
            );
            trigger.lifecycle_operations = vec![
                LifecycleOperation::Activate,
                LifecycleOperation::Deactivate,
                LifecycleOperation::Poll,
            ];
            trigger
        },
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
            json!({"order":["values","keepOnlySet"],"fields":{"values":{"control":"mapper"},"keepOnlySet":{"control":"boolean"}}}),
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
            json!({"order":["rules","sendToAllMatches"],"fields":{"rules":{"control":"collection"},"sendToAllMatches":{"control":"boolean"}}}),
        ),
        configured(
            manifest(
                "merge",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Required,
                vec![port("main", PortKind::Main, true, true)],
                main_out(),
                SideEffectLevel::None,
            ),
            json!({"type":"object","properties":{"mode":{"type":"string","enum":["append","combine_by_position"],"default":"append"}},"additionalProperties":false}),
            json!({"fields":{"mode":{"control":"select"}}}),
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
            json!({"order":["method","url","headers","body"],"fields":{"method":{"control":"select"},"url":{"control":"expression"},"headers":{"control":"fixed_collection"},"body":{"control":"json"}}}),
        ),
        configured(
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
        ),
        m5_manifest(
            "model",
            NodeCapability::Model,
            json!({"type":"object","properties":{"messages":{"type":"array"},"prompt":{"type":"string"},"parameters":{"type":"object"}},"additionalProperties":false}),
            SideEffectLevel::None,
        ),
        m5_manifest(
            "mcp_tool",
            NodeCapability::McpTool,
            json!({"type":"object","properties":{"arguments":{"type":"object"}},"additionalProperties":false}),
            SideEffectLevel::Irreversible,
        ),
        m5_manifest(
            "skill",
            NodeCapability::Skill,
            json!({"type":"object","additionalProperties":false}),
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
                        "systemPrompt":{"type":"string"},"messages":{"type":"array"},
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
    ]
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
    fn remote_trigger_declares_poll_and_deployment_lifecycle() {
        let registry = NodeRegistry::m5_defaults();
        let operations = &registry
            .get("remote_trigger", 1)
            .expect("remote trigger manifest")
            .lifecycle_operations;
        assert_eq!(
            operations,
            &[
                LifecycleOperation::Activate,
                LifecycleOperation::Deactivate,
                LifecycleOperation::Poll,
            ]
        );
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
    }
}
