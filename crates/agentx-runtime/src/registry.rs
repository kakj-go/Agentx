use std::collections::BTreeMap;

use agentx_domain::ResourceType;
use agentx_node_protocol::{
    BindingSlot, BindingSlotPlacement, CanvasAppearance, CanvasNodeRole, ExecutionStyle,
    NODE_PROTOCOL_VERSION, NodeCapability, NodeManifestLocalization, NodeManifestVersion, NodePort,
    NodeUiSchema, PortKind, ReadinessPolicy, SideEffectLevel,
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
    #[must_use]
    pub fn is_definition_node_type(node_type: &str) -> bool {
        matches!(
            node_type,
            "set"
                | "list"
                | "if"
                | "merge"
                | "loop_over_items"
                | "approval"
                | "sub_workflow"
                | "declarative_http"
                | "model"
                | "agent"
                | "code"
        )
    }

    pub fn register(&mut self, manifest: NodeManifestVersion) -> Result<(), RegistryError> {
        if manifest.protocol_version != NODE_PROTOCOL_VERSION {
            return Err(RegistryError::UnsupportedProtocol {
                node_type: manifest.node_type,
                version: manifest.version,
                protocol_version: manifest.protocol_version,
            });
        }
        if let Err(message) = manifest
            .validate_binding_slots()
            .and_then(|()| manifest.validate_localizations())
            .and_then(|()| validate_agent_manifest(&manifest))
        {
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

    /// Resolves the generic Studio Sub-workflow node to the immutable
    /// version-derived Manifest when that dependency snapshot is available.
    #[must_use]
    pub fn resolve_definition_manifest(
        &self,
        node_type: &str,
        version: u32,
        parameters: &Value,
    ) -> Option<&NodeManifestVersion> {
        if node_type == "sub_workflow"
            && let Some(version_id) = parameters
                .get("workflowVersionId")
                .and_then(Value::as_str)
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
        {
            let derived = format!("workflow.{}", version_id.simple());
            if let Some(manifest) = self.get(&derived, version) {
                return Some(manifest);
            }
        }
        self.get(node_type, version)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &NodeManifestVersion> {
        self.manifests.values()
    }

    /// The only runtime node types users may create in Workflow Studio.
    /// Resource executors remain registered for Agent attachments, but never
    /// become a second public node catalog.
    pub fn studio_manifests(&self) -> impl Iterator<Item = &NodeManifestVersion> {
        self.manifests.values().filter(|manifest| {
            Self::is_definition_node_type(&manifest.node_type)
                && !manifest.node_type.starts_with("workflow.")
        })
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

fn validate_agent_manifest(manifest: &NodeManifestVersion) -> Result<(), String> {
    if manifest.node_type != "agent" {
        return Ok(());
    }
    if manifest.version != 2 {
        return Err("Manifest 2.0 requires Agent node version 2".into());
    }
    let expected = [
        (
            "model",
            ResourceType::Model,
            BindingSlotPlacement::Inspector,
            true,
            false,
        ),
        (
            "workspace_sandbox",
            ResourceType::SandboxProfile,
            BindingSlotPlacement::Inspector,
            false,
            false,
        ),
        (
            "mcp_tools",
            ResourceType::McpTool,
            BindingSlotPlacement::Inspector,
            false,
            true,
        ),
        (
            "skills",
            ResourceType::Skill,
            BindingSlotPlacement::Inspector,
            false,
            true,
        ),
        (
            "knowledge",
            ResourceType::Rag,
            BindingSlotPlacement::Inspector,
            false,
            true,
        ),
        (
            "long_term_memory",
            ResourceType::Memory,
            BindingSlotPlacement::Inspector,
            false,
            false,
        ),
    ];
    if manifest.binding_slots.len() != expected.len() {
        return Err("Agent manifest must declare exactly the six frozen resource slots".into());
    }
    for (name, resource_type, placement, required, multiple) in expected {
        let Some(slot) = manifest.binding_slots.iter().find(|slot| slot.name == name) else {
            return Err(format!(
                "Agent manifest is missing frozen resource slot '{name}'"
            ));
        };
        if slot.resource_type != resource_type
            || slot.placement != placement
            || slot.required != required
            || slot.multiple != multiple
        {
            return Err(format!(
                "Agent resource slot '{name}' does not match its frozen Manifest 2.0 contract"
            ));
        }
    }
    Ok(())
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
        "list" => (
            "List Operator",
            "列表操作",
            "Filter, sort and truncate the item stream in one node.",
            "在单个节点内完成过滤、排序与截断。",
        ),
        "if" => (
            "Condition",
            "条件分支",
            "Route items to the first matching IF/ELIF branch, else to ELSE.",
            "按序命中 IF/ELIF 分支，未命中走 ELSE 分支。",
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
        "case" => "Case",
        "else" => "Else",
        "decision" => "Decision",
        "timed_out" => "Timed out",
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
        "case" => "条件分支",
        "else" => "否则分支",
        "decision" => "审批决策",
        "timed_out" => "已超时",
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
        selector_capabilities: agentx_node_protocol::SelectorCapabilities {
            namespaces: vec!["inputs".into(), "outputs".into(), "contexts".into()],
            supports_current: true,
            supports_first_last: true,
            supports_all: true,
            supports_run_selection: true,
        },
        context_read_capability: true,
        context_write_capability: true,
        artifact_output_schema: json!({"type":"array","items":{"type":"object"}}),
        ui_schema: NodeUiSchema {
            canvas: Some(CanvasAppearance { role }),
            ..Default::default()
        },
        providers: Vec::new(),
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
            json!({"prompt":{"control":"prompt"},"userQuestion":{"control":"template"},"responseMode":{"control":"select"},"structuredSchema":{"control":"schema_editor"}})
        }
        "mcp_tool" => json!({"arguments":{"control":"json"}}),
        "rag" | "memory" => json!({"operation":{"control":"select"},"input":{"control":"json"}}),
        "agent" => json!({
            "systemPrompt":{"control":"prompt"},"userQuestion":{"control":"template"},
            "sessionPolicy":{"control":"json"},
            "maxIterations":{"control":"number","unit":"calls"},"maxModelCalls":{"control":"number","unit":"calls"},
            "maxToolCalls":{"control":"number","unit":"calls"},"maxTotalTokens":{"control":"number","unit":"tokens"},
            "maxOutputTokens":{"control":"number","unit":"tokens"},"maxCostMicros":{"control":"number","unit":"micros"},
            "maxDurationMs":{"control":"number","unit":"milliseconds"},"limitAction":{"control":"select"}
        }),
        "code" => json!({
            "runner":{"control":"select"},"source":{"control":"code","languageField":"runner"},
            "inputs":{"control":"mapper"},"outputExample":{"control":"json5_example"},"networkPolicy":{"control":"network_policy"}
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
    if node_type == "if" {
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
        "if" | "merge" | "loop_over_items" | "approval" => "logic",
        "set" | "list" | "code" => "transform",
        "declarative_http" | "sub_workflow" => "integration",
        "agent" | "model" | "mcp_tool" | "skill" | "rag" | "memory" => "ai",
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
        "if" => "split",
        "list" => "filter",
        "merge" => "git-merge",
        "loop_over_items" => "repeat-2",
        _ => "box",
    }
}

fn binding_contract(kind: &str, recursive: bool) -> Value {
    let accepted_kinds = match kind {
        "reference" => json!(["reference"]),
        "template" => json!(["literal", "reference", "template"]),
        "value" | "structured" => {
            json!(["literal", "reference", "template", "array", "object"])
        }
        _ => json!([]),
    };
    json!({
        "acceptedKinds": accepted_kinds,
        "allowedNamespaces": ["inputs", "outputs", "contexts", "execution", "item", "loop"],
        "acceptedCardinality": ["single"],
        "missingPolicies": ["error", "null", "omit"],
        "recursive": recursive
    })
}

fn condition_spec_schema() -> Value {
    json!({
        "type": "object",
        "required": ["left", "operator"],
        "properties": {
            "left": {"x-agentx-binding": binding_contract("value", false)},
            "operator": {"type":"string","enum":["eq","ne","gt","gte","lt","lte","in","contains","not_contains","ends_with","starts_with","matches","is_empty","is_not_empty"]},
            "right": {"x-agentx-binding": binding_contract("value", false)}
        },
        "additionalProperties": false
    })
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
            json!({"type":"object","properties":{"values":{"type":"object","default":{"kind":"object","fields":{}}},"keepOnlySet":{"type":"boolean","default":false}},"additionalProperties":false}),
            json!({"order":["values","keepOnlySet"],"fields":{"values":{"control":"structured"},"keepOnlySet":{"control":"boolean"}}}),
        ),
        configured(
            manifest(
                "list",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                main_out(),
                SideEffectLevel::None,
            ),
            json!({
                "type":"object",
                "required":["input"],
                "properties":{
                    "input":{"type":"array","items":{}},
                    "filter":{"type":"object","properties":{"conditions":{"type":"array","items":{"type":"object","required":["condition"],"properties":{"condition":condition_spec_schema(),"label":{"type":"string"}},"additionalProperties":false}},"logicalOp":{"type":"string","enum":["and","or"],"default":"and"}},"additionalProperties":false,"default":{"conditions":[],"logicalOp":"and"}},
                    "sort":{"type":"array","items":{"type":"object","required":["selector","direction","nulls"],"properties":{"selector":{"x-agentx-binding":binding_contract("reference",false)},"direction":{"type":"string","enum":["asc","desc"],"default":"asc"},"nulls":{"type":"string","enum":["first","last"],"default":"last"}},"additionalProperties":false},"default":[]},
                    "takeN":{"type":"integer","minimum":0}
                },
                "additionalProperties":false
            }),
            json!({"order":["input","filter","sort","takeN"],"fields":{"input":{"control":"value"},"filter":{"control":"condition_builder"},"sort":{"control":"sort_builder"},"takeN":{"control":"number"}}}),
        ),
        configured(
            manifest(
                "if",
                ExecutionStyle::Action,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("case", PortKind::Main, false, true),
                    port("else", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","required":["cases"],"properties":{"cases":{"type":"array","minItems":1,"default":[{"id":"case_1","name":"","conditions":[{"condition":{"left":{"kind":"literal","value":""},"operator":"eq","right":{"kind":"literal","value":""}}}],"logicalOp":"and"}],"items":{"type":"object","required":["id","conditions"],"properties":{"id":{"type":"string","minLength":1},"name":{"type":"string"},"conditions":{"type":"array","minItems":1,"items":{"type":"object","required":["condition"],"properties":{"condition":condition_spec_schema(),"label":{"type":"string"}},"additionalProperties":false}},"logicalOp":{"type":"string","enum":["and","or"],"default":"and"}},"additionalProperties":false}}},"additionalProperties":false}),
            json!({"fields":{"cases":{"control":"condition_builder"}}}),
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
                    port("main", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","required":["input","outputSelector"],"properties":{"input":{"type":"array","items":{}},"outputSelector":{"x-agentx-binding":binding_contract("reference",false)},"errorMode":{"type":"string","enum":["terminate","continue","remove"],"default":"terminate"},"parallelism":{"type":"integer","minimum":1,"maximum":10,"default":1}},"additionalProperties":false}),
            json!({"order":["input","outputSelector","errorMode","parallelism"],"fields":{"input":{"control":"value"},"outputSelector":{"control":"reference"},"errorMode":{"control":"select"},"parallelism":{"control":"number"}}}),
        ),
        configured(
            manifest(
                "approval",
                ExecutionStyle::Suspend,
                NodeCapability::Builtin,
                ReadinessPolicy::Any,
                main_in(),
                vec![
                    port("decision", PortKind::Main, false, true),
                    port("timed_out", PortKind::Main, false, false),
                    port("error", PortKind::Error, false, false),
                ],
                SideEffectLevel::None,
            ),
            json!({"type":"object","required":["candidateUserId"],"properties":{"title":{"type":"string"},"description":{"type":"string"},"candidateUserId":{"type":"string","format":"uuid"},"buttons":{"type":"array","minItems":1,"items":{"type":"object","required":["id","label"],"properties":{"id":{"type":"string","minLength":1},"label":{"type":"string"}},"additionalProperties":false}},"timeoutMs":{"type":"integer","minimum":1,"maximum":31_536_000_000_i64}},"additionalProperties":false}),
            json!({"order":["title","description","candidateUserId","buttons","timeoutMs"],"fields":{"title":{"control":"template"},"description":{"control":"template"},"candidateUserId":{"control":"provider_options","provider":"users"},"buttons":{"control":"buttons_editor"},"timeoutMs":{"control":"number"}}}),
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
                json!({"type":"object","required":["workflowVersionId","inputs"],"properties":{"workflowVersionId":{"type":"string","format":"uuid"},"inputs":{"type":"object","additionalProperties":{},"default":{"kind":"object","fields":{}}}},"additionalProperties":false}),
                json!({"order":["workflowVersionId","inputs"],"fields":{"workflowVersionId":{"control":"provider_options","provider":"workflow_versions"},"inputs":{"control":"mapper"}}}),
            );
            sub_workflow.providers = vec!["workflow_versions".into()];
            sub_workflow.default_timeout_ms = Some(300_000);
            sub_workflow
        },
        {
            let mut http = configured(
                manifest(
                    "declarative_http",
                    ExecutionStyle::Action,
                    NodeCapability::DeclarativeHttp,
                    ReadinessPolicy::Any,
                    main_in(),
                    main_out(),
                    SideEffectLevel::Idempotent,
                ),
                json!({"type":"object","required":["url"],"properties":{"method":{"type":"string","enum":["GET","POST","PUT","PATCH","DELETE"],"default":"GET"},"url":{},"query":{"type":"array","items":{"type":"object","required":["name","value"],"properties":{"name":{"type":"string","minLength":1},"value":{"x-agentx-binding":binding_contract("template",false)}},"additionalProperties":false},"default":[]},"headers":{"type":"array","items":{"type":"object","required":["name","value"],"properties":{"name":{"type":"string","minLength":1},"value":{"x-agentx-binding":binding_contract("template",false)}},"additionalProperties":false},"default":[]},"body":{},"apiKeyPlacement":{"type":"object","required":["in","name"],"properties":{"in":{"type":"string","enum":["header","query"]},"name":{"type":"string","minLength":1}},"additionalProperties":false}},"additionalProperties":false}),
                json!({"order":["method","url","query","headers","body","apiKeyPlacement"],"fields":{"method":{"control":"select"},"url":{"control":"template"},"query":{"control":"kv_builder"},"headers":{"control":"kv_builder"},"body":{"control":"structured"},"apiKeyPlacement":{"control":"api_key_placement"}}}),
            );
            http.binding_slots = vec![BindingSlot {
                name: "credential".into(),
                resource_type: ResourceType::Credential,
                placement: BindingSlotPlacement::Inspector,
                required: false,
                multiple: false,
            }];
            http.ui_schema.resource_selectors.push(json!({
                "bindingRole":"credential",
                "resourceType":"credential",
                "operation":"use",
                "required":false,
                "label":"Credential"
            }));
            http
        },
        m5_manifest(
            "model",
            NodeCapability::Model,
            json!({"type":"object","properties":{"prompt":{"type":"string"},"userQuestion":{"type":"string","templatable":true,"allowedNamespaces":["inputs","outputs","contexts","execution","item","loop"],"expectedType":"string","multiline":false,"richText":false},"responseMode":{"type":"string","enum":["text","json_schema"],"default":"text"},"structuredSchema":{"type":"object"}},"additionalProperties":false}),
            SideEffectLevel::None,
        ),
        m5_manifest(
            "mcp_tool",
            NodeCapability::McpTool,
            json!({"type":"object","properties":{"arguments":{}},"additionalProperties":false}),
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
            json!({"type":"object","required":["operation"],"properties":{"operation":{"enum":["query","insert"]},"input":{}},"additionalProperties":false}),
            SideEffectLevel::Reversible,
        ),
        m5_manifest(
            "memory",
            NodeCapability::Memory,
            json!({"type":"object","required":["operation"],"properties":{"operation":{"enum":["search","add"]},"input":{}},"additionalProperties":false}),
            SideEffectLevel::Reversible,
        ),
        {
            let mut agent = m5_manifest(
                "agent",
                NodeCapability::Agent,
                json!({
                    "type":"object",
                    "required":["sessionPolicy"],
                    "properties":{
                        "systemPrompt":{"type":"string"},"userQuestion":{"type":"string","templatable":true,"allowedNamespaces":["inputs","outputs","contexts","execution","item","loop"],"expectedType":"string","multiline":false,"richText":false},
                        "sessionPolicy":{"type":"object","required":["mode"],"properties":{"mode":{"enum":["application_session","invocation"]},"retentionPolicyId":{"type":"string","minLength":1}},"additionalProperties":false},
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
            agent.version = 2;
            agent.binding_slots = vec![
                BindingSlot {
                    name: "model".into(),
                    resource_type: ResourceType::Model,
                    placement: BindingSlotPlacement::Inspector,
                    required: true,
                    multiple: false,
                },
                BindingSlot {
                    name: "workspace_sandbox".into(),
                    resource_type: ResourceType::SandboxProfile,
                    placement: BindingSlotPlacement::Inspector,
                    required: false,
                    multiple: false,
                },
                BindingSlot {
                    name: "mcp_tools".into(),
                    resource_type: ResourceType::McpTool,
                    placement: BindingSlotPlacement::Inspector,
                    required: false,
                    multiple: true,
                },
                BindingSlot {
                    name: "skills".into(),
                    resource_type: ResourceType::Skill,
                    placement: BindingSlotPlacement::Inspector,
                    required: false,
                    multiple: true,
                },
                BindingSlot {
                    name: "knowledge".into(),
                    resource_type: ResourceType::Rag,
                    placement: BindingSlotPlacement::Inspector,
                    required: false,
                    multiple: true,
                },
                BindingSlot {
                    name: "long_term_memory".into(),
                    resource_type: ResourceType::Memory,
                    placement: BindingSlotPlacement::Inspector,
                    required: false,
                    multiple: false,
                },
            ];
            agent.ui_schema.resource_selectors = vec![
                json!({
                    "bindingRole":"model",
                    "resourceType":"model",
                    "operation":"use",
                    "required":true,
                    "label":"Model"
                }),
                json!({
                    "bindingRole":"workspace_sandbox",
                    "resourceType":"sandbox_profile",
                    "operation":"use",
                    "required":false,
                    "label":"Workspace Sandbox"
                }),
                json!({"bindingRole":"mcp_tools","resourceType":"mcp_tool","operation":"use","required":false,"multiple":true,"label":"MCP Tools"}),
                json!({"bindingRole":"skills","resourceType":"skill","operation":"use","required":false,"multiple":true,"label":"Skills"}),
                json!({"bindingRole":"knowledge","resourceType":"rag","operation":"read","required":false,"multiple":true,"label":"Knowledge"}),
                json!({"bindingRole":"long_term_memory","resourceType":"memory","operation":"read","required":false,"multiple":false,"label":"Long-term Memory"}),
            ];
            for (locale, labels) in [
                (
                    "en-US",
                    [
                        ("model", "Model"),
                        ("workspace_sandbox", "Workspace Sandbox"),
                        ("mcp_tools", "MCP Tools"),
                        ("skills", "Skills"),
                        ("knowledge", "Knowledge"),
                        ("long_term_memory", "Long-term Memory"),
                    ],
                ),
                (
                    "zh-CN",
                    [
                        ("model", "模型"),
                        ("workspace_sandbox", "工作区沙箱"),
                        ("mcp_tools", "MCP 工具"),
                        ("skills", "技能"),
                        ("knowledge", "知识库"),
                        ("long_term_memory", "长期记忆"),
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
                "type":"object","required":["runner","inputs","source","outputExample","networkPolicy"],
                "properties":{
                    "runner":{"type":"string","enum":["python","javascript","shell"],"default":"python"},
                    "inputs":{"type":"object","additionalProperties":{},"default":{"kind":"object","fields":{}}},
                    "source":{"type":"string","default":"def main(**inputs):\n    return {}"},
                    "outputExample":{"type":"object","default":{},"additionalProperties":true},
                    "networkPolicy":{
                        "type":"object","required":["mode","destinations"],
                        "properties":{
                            "mode":{"type":"string","enum":["deny","allowlist"],"default":"deny"},
                            "destinations":{
                                "type":"array","maxItems":32,"default":[],
                                "items":{
                                    "type":"object","required":["target","ports"],
                                    "properties":{
                                        "target":{"type":"string","minLength":1,"maxLength":253},
                                        "ports":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","required":["from","to"],"properties":{"from":{"type":"integer","minimum":1,"maximum":65535},"to":{"type":"integer","minimum":1,"maximum":65535}},"additionalProperties":false}}
                                    },
                                    "additionalProperties":false
                                }
                            }
                        },
                        "additionalProperties":false,
                        "default":{"mode":"deny","destinations":[]}
                    }
                },
                "additionalProperties":false
            }),
            SideEffectLevel::Irreversible,
        ),
    ];
    for manifest in &mut manifests {
        if manifest.node_type == "approval" {
            manifest.providers.push("users".into());
            manifest.default_timeout_ms = None;
        }
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
                    .or_insert_with(|| "选择变量或输入固定值".into());
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
        "id" => "稳定标识".into(),
        "name" => "名称".into(),
        "label" => "显示名称".into(),
        "title" => "标题".into(),
        "description" => "描述".into(),
        "value" => "值".into(),
        "buttons" => "决策按钮".into(),
        "cases" => "分支".into(),
        "conditions" => "条件组".into(),
        "logicalOp" => "条件关系".into(),
        "selector" => "字段选择".into(),
        "in" => "注入位置".into(),
        "egressMode" => "出站网络".into(),
        "leftField" => "左侧键".into(),
        "rightField" => "右侧键".into(),
        "values" => "字段值".into(),
        "keepOnlySet" => "仅保留已设置字段".into(),
        "condition" => "条件".into(),
        "mode" => "模式".into(),
        "operation" => "操作".into(),
        "prompt" => "提示词".into(),
        "systemPrompt" => "系统提示词".into(),
        "userQuestion" => "用户问题".into(),
        "sessionPolicy" => "会话策略".into(),
        "retentionPolicyId" => "保留策略".into(),
        "inputs" => "命名输入".into(),
        "outputSelector" => "每轮输出".into(),
        "outputSchema" => "输出结构".into(),
        "outputExample" => "返回 JSON 示例".into(),
        "responseMode" => "响应模式".into(),
        "structuredSchema" => "结构化输出结构".into(),
        "filter" => "过滤条件".into(),
        "sort" => "排序规则".into(),
        "takeN" => "取前 N 项".into(),
        "parallelism" => "并发数".into(),
        "errorMode" => "失败处理".into(),
        "url" => "地址".into(),
        "method" => "请求方法".into(),
        "headers" => "请求头".into(),
        "query" => "查询参数".into(),
        "apiKeyPlacement" => "API Key 位置".into(),
        "body" => "请求体".into(),
        "runner" => "运行器".into(),
        "source" => "源代码".into(),
        "networkPolicy" => "网络策略".into(),
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
        "input" => "输入数据".into(),
        _ => humanize_protocol_name(value),
    }
}

fn chinese_enum_label(value: &str) -> String {
    match value {
        "text" => "文本".into(),
        "json_schema" => "JSON Schema".into(),
        "and" => "且".into(),
        "or" => "或".into(),
        "terminate" => "终止".into(),
        "continue" => "保留失败位置".into(),
        "remove" => "移除失败项".into(),
        "tcp_proxy" => "允许 TCP 代理".into(),
        "application_session" => "应用会话".into(),
        "invocation" => "本次调用".into(),
        "header" => "请求头".into(),
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
        "sync" => "同步".into(),
        "async" => "异步".into(),
        "approved" => "通过".into(),
        "rejected" => "拒绝".into(),
        "decided" => "已决策".into(),
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
        _ => humanize_protocol_name(value),
    }
}

fn configure_workflow_v4_capabilities(manifest: &mut NodeManifestVersion) {
    match manifest.node_type.as_str() {
        "if" => {
            for port in &manifest.output_ports {
                manifest.output_cardinality.insert(
                    port.name.clone(),
                    agentx_node_protocol::OutputCardinality::ZeroOrMany,
                );
            }
        }
        "loop_over_items" => {
            manifest.output_schema = array_items_response_schema();
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
            manifest.output_cardinality.insert(
                "error".into(),
                agentx_node_protocol::OutputCardinality::ZeroOrOne,
            );
        }
        "list" => {
            manifest.output_schema = array_items_response_schema();
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
            manifest.output_cardinality.insert(
                "error".into(),
                agentx_node_protocol::OutputCardinality::ZeroOrOne,
            );
        }
        "approval" => {
            for port in &manifest.output_ports {
                manifest.output_cardinality.insert(
                    port.name.clone(),
                    agentx_node_protocol::OutputCardinality::ZeroOrOne,
                );
            }
            manifest.output_port_schemas.insert(
                "decision".into(),
                json!({
                    "type":"object",
                    "properties":{"taskId":{"type":"string","format":"uuid"},"decision":{"type":"string"},"reason":{"type":["string","null"]},"decidedBy":{"type":["string","null"],"format":"uuid"},"input":{}},
                    "required":["taskId","decision","decidedBy","reason","input"],
                    "additionalProperties":false
                }),
            );
            manifest.output_port_schemas.insert("timed_out".into(), json!({
                "type":"object",
                "properties":{"taskId":{"type":"string","format":"uuid"},"decision":{"type":"string","enum":["timed_out"]},"decidedBy":{"type":"null"},"reason":{"type":"null"},"input":{}},
                "required":["taskId","decision","decidedBy","reason","input"],
                "additionalProperties":false
            }));
        }
        "declarative_http" | "http_request" => {
            manifest.output_schema = json!({"type":"object","properties":{"statusCode":{"type":"integer"},"headers":{"type":"object"},"body":{"type":"string"},"files":{"type":"array","items":artifact_ref_schema()}},"required":["statusCode","headers","body","files"],"additionalProperties":false});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "model" => {
            manifest.output_schema = ai_response_schema();
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "agent" => {
            manifest.output_schema = ai_response_schema();
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "mcp_tool" => {
            manifest.output_schema = tool_response_schema();
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "rag" => {
            manifest.output_schema = json!({"type":"object","properties":{"text":{"type":"string"},"documents":{"type":"array"},"citations":{"type":"array","items":citation_schema()},"recordIds":{"type":"array","items":{"type":"string"}}},"required":["text","documents","citations","recordIds"],"additionalProperties":false});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "memory" => {
            manifest.output_schema = json!({"type":"object","properties":{"text":{"type":"string"},"records":{"type":"array"},"recordIds":{"type":"array","items":{"type":"string"}}},"required":["text","records","recordIds"],"additionalProperties":false});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        "code" => {
            manifest.output_schema = json!({"type":"object","properties":{"stdout":{"type":"string"},"stderr":{"type":"string"},"exitCode":{"type":"integer"},"structuredOutput":{"type":["object","null"]},"files":{"type":"array","items":artifact_ref_schema()},"partial":{"type":"boolean"}},"required":["stdout","stderr","exitCode","structuredOutput","files","partial"],"additionalProperties":false});
            manifest.output_cardinality.insert(
                "main".into(),
                agentx_node_protocol::OutputCardinality::ExactlyOne,
            );
        }
        _ => {}
    }
    if manifest.node_type == "skill" {
        manifest.output_schema = tool_response_schema();
    }
    let error_schema = error_response_schema();
    for port in &manifest.output_ports {
        if port.kind == PortKind::Error {
            manifest
                .output_port_schemas
                .insert(port.name.clone(), error_schema.clone());
        }
    }
    if matches!(
        manifest.node_type.as_str(),
        "if" | "merge" | "loop_over_items" | "approval"
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
            let binding = match control.as_str() {
                "reference" => Some(binding_contract("reference", false)),
                "value" => Some(binding_contract("value", false)),
                "template" | "prompt" | "textarea" => Some(binding_contract("template", false)),
                "mapper" | "structured" => Some(binding_contract("structured", true)),
                _ => None,
            };
            if let Some(binding) = binding
                && let Some(property) = properties.get_mut(&name).and_then(Value::as_object_mut)
            {
                property.insert("x-agentx-binding".into(), binding);
                property.remove("templatable");
                property.remove("allowedNamespaces");
                property.remove("expectedType");
                property.remove("multiline");
                property.remove("richText");
            }
        }
    }
}

fn ai_response_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "text":{"type":"string"},
            "reasoningContent":{"type":["string","null"]},
            "structuredOutput":{"type":["object","null"]},
            "files":{"type":"array","items":artifact_ref_schema()},
            "citations":{"type":"array","items":citation_schema()},
            "usage":{"type":"object","properties":{"inputTokens":{"type":"integer"},"outputTokens":{"type":"integer"},"totalTokens":{"type":"integer"},"costMicros":{"type":"integer"}},"required":["inputTokens","outputTokens","totalTokens","costMicros"],"additionalProperties":false},
            "finishReason":{"type":["string","null"]},
            "partial":{"type":"boolean"}
        },
        "required":["text","reasoningContent","structuredOutput","files","citations","usage","finishReason","partial"],
        "additionalProperties":false
    })
}

fn tool_response_schema() -> Value {
    json!({"type":"object","properties":{"text":{"type":"string"},"structuredOutput":{"type":["object","null"]},"files":{"type":"array","items":artifact_ref_schema()}},"required":["text","structuredOutput","files"],"additionalProperties":false})
}

fn array_items_response_schema() -> Value {
    json!({
        "type":"object",
        "properties":{"items":{"type":"array","items":{}}},
        "required":["items"],
        "additionalProperties":false
    })
}

fn artifact_ref_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "artifactId":{"type":"string","format":"uuid"},
            "fileName":{"type":"string","minLength":1},
            "contentType":{"type":"string","minLength":1},
            "sizeBytes":{"type":"integer","minimum":0},
            "sha256":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"}
        },
        "required":["artifactId","fileName","contentType","sizeBytes","sha256"],
        "additionalProperties":false
    })
}

fn citation_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "sourceId":{"type":"string","minLength":1},
            "text":{"type":"string"},
            "title":{"type":["string","null"]},
            "uri":{"type":["string","null"]},
            "recordId":{"type":["string","null"]},
            "metadata":{"type":"object"}
        },
        "required":["sourceId","text","metadata"],
        "additionalProperties":false
    })
}

fn error_response_schema() -> Value {
    json!({"type":"object","properties":{"code":{"type":"string"},"message":{"type":"string"},"retryable":{"type":"boolean"},"details":{"type":"object"},"sourceNodeId":{"type":"string"},"nodeExecutionId":{"type":"string"}},"required":["code","message","retryable","details","sourceNodeId","nodeExecutionId"],"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn unknown_versions_are_not_coerced() {
        let registry = NodeRegistry::m4_defaults();
        assert!(registry.get("set", 1).is_some());
        assert!(registry.get("set", 2).is_none());

        let mut manifest_v1 = registry.get("set", 1).expect("set manifest").clone();
        manifest_v1.protocol_version = "1.0".into();
        assert!(matches!(
            NodeRegistry::default().register(manifest_v1),
            Err(RegistryError::UnsupportedProtocol { .. })
        ));
    }

    #[test]
    fn approval_manifest_declares_decision_output_contracts() {
        let registry = NodeRegistry::m5_defaults();
        let manifest = registry.get("approval", 1).expect("approval manifest");

        let decision = manifest
            .output_port_schemas
            .get("decision")
            .expect("decision port schema");
        assert_eq!(
            decision["required"],
            json!(["taskId", "decision", "decidedBy", "reason", "input"])
        );
        assert!(decision["properties"]["decision"].get("enum").is_none());
        assert_eq!(
            manifest.output_port_schemas["timed_out"]["properties"]["decision"]["enum"],
            json!(["timed_out"])
        );
        assert!(
            manifest
                .output_ports
                .iter()
                .any(|port| port.name == "decision" && port.variadic)
        );
        assert_eq!(
            manifest.output_port_schemas["error"]["properties"]["code"]["type"],
            "string"
        );
    }

    #[test]
    fn specialized_controls_own_bindings_only_at_editable_value_leaves() {
        let registry = NodeRegistry::m5_defaults();
        let if_schema = &registry.get("if", 1).unwrap().parameter_schema;
        assert!(
            if_schema["properties"]["cases"]
                .get("x-agentx-binding")
                .is_none()
        );
        assert_eq!(
            if_schema["properties"]["cases"]["items"]["properties"]["conditions"]["items"]["properties"]
                ["condition"]["properties"]["left"]["x-agentx-binding"]["acceptedKinds"],
            json!(["literal", "reference", "template", "array", "object"])
        );

        let list_schema = &registry.get("list", 1).unwrap().parameter_schema;
        assert!(
            list_schema["properties"]["filter"]
                .get("x-agentx-binding")
                .is_none()
        );
        assert!(
            list_schema["properties"]["sort"]
                .get("x-agentx-binding")
                .is_none()
        );
        assert_eq!(
            list_schema["properties"]["sort"]["items"]["properties"]["selector"]["x-agentx-binding"]
                ["acceptedKinds"],
            json!(["reference"])
        );

        let http_schema = &registry
            .get("declarative_http", 1)
            .unwrap()
            .parameter_schema;
        assert!(
            http_schema["properties"]["query"]
                .get("x-agentx-binding")
                .is_none()
        );
        assert_eq!(
            http_schema["properties"]["query"]["items"]["properties"]["value"]["x-agentx-binding"]
                ["acceptedKinds"],
            json!(["literal", "reference", "template"])
        );
    }

    #[test]
    fn every_error_port_uses_the_standard_error_contract() {
        let registry = NodeRegistry::m5_defaults();
        for manifest in registry.manifests() {
            for port in manifest
                .output_ports
                .iter()
                .filter(|port| port.kind == PortKind::Error)
            {
                let schema = manifest
                    .output_port_schemas
                    .get(&port.name)
                    .expect("error port schema");
                assert_eq!(
                    schema["required"],
                    json!([
                        "code",
                        "message",
                        "retryable",
                        "details",
                        "sourceNodeId",
                        "nodeExecutionId"
                    ]),
                    "{}",
                    manifest.node_type
                );
            }
        }
    }

    #[test]
    fn model_and_agent_publish_only_the_ai_response_contract() {
        let registry = NodeRegistry::m5_defaults();
        for node_type in ["model", "agent"] {
            let version = if node_type == "agent" { 2 } else { 1 };
            let properties = registry.get(node_type, version).unwrap().output_schema["properties"]
                .as_object()
                .unwrap();
            assert_eq!(
                properties.keys().cloned().collect::<BTreeSet<_>>(),
                BTreeSet::from(
                    [
                        "text",
                        "reasoningContent",
                        "structuredOutput",
                        "files",
                        "citations",
                        "usage",
                        "finishReason",
                        "partial"
                    ]
                    .map(str::to_owned)
                )
            );
            for removed in [
                "message",
                "messages",
                "toolCalls",
                "iterations",
                "providerRawResponse",
            ] {
                assert!(!properties.contains_key(removed));
            }
        }
    }

    #[test]
    fn semantic_file_and_citation_outputs_use_strong_contracts() {
        let registry = NodeRegistry::m5_defaults();
        let ai = &registry.get("model", 1).unwrap().output_schema;
        assert_eq!(
            ai["properties"]["files"]["items"]["required"],
            json!([
                "artifactId",
                "fileName",
                "contentType",
                "sizeBytes",
                "sha256"
            ])
        );
        assert_eq!(
            ai["properties"]["citations"]["items"]["required"],
            json!(["sourceId", "text", "metadata"])
        );
        let tool = &registry.get("mcp_tool", 1).unwrap().output_schema;
        assert_eq!(
            tool["properties"]["files"]["items"]["properties"]["artifactId"]["format"],
            "uuid"
        );
    }

    #[test]
    fn ui_fields_are_declared_and_agent_parameters_are_editable() {
        let registry = NodeRegistry::m5_defaults();
        for manifest in registry.manifests() {
            let parameters = manifest.parameter_schema["properties"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            for field in manifest.ui_schema.fields.keys() {
                assert!(
                    parameters.contains_key(field),
                    "{} exposes undeclared UI parameter {field}",
                    manifest.node_type
                );
            }
        }
        let agent = registry.get("agent", 2).expect("Agent v2 manifest");
        for parameter in agent.parameter_schema["properties"]
            .as_object()
            .expect("Agent parameters")
            .keys()
        {
            assert!(
                agent.ui_schema.fields.contains_key(parameter),
                "Agent parameter {parameter} has no UI field"
            );
        }
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

        let branch = registry.get("if", 1).expect("if manifest");
        let english = branch.localizations.get("en-US").expect("English locale");
        assert!(
            english
                .parameter_labels
                .contains_key("cases[].conditions[].condition")
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

        let mut invalid_nested = branch.clone();
        invalid_nested
            .localizations
            .get_mut("en-US")
            .expect("English localization")
            .parameter_labels
            .insert("cases[].missing".into(), "Missing".into());
        assert!(invalid_nested.validate_localizations().is_err());
    }

    #[test]
    fn agent_resources_remain_internal_and_studio_catalog_is_exact() {
        let registry = NodeRegistry::m5_defaults();
        assert_eq!(
            registry
                .get("mcp_tool", 1)
                .expect("MCP manifest")
                .capability,
            NodeCapability::McpTool
        );
        assert_eq!(
            registry
                .studio_manifests()
                .map(|manifest| manifest.node_type.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "agent",
                "approval",
                "code",
                "declarative_http",
                "if",
                "list",
                "loop_over_items",
                "merge",
                "model",
                "set",
                "sub_workflow",
            ])
        );
        for resource_only in ["mcp_tool", "skill", "rag", "memory"] {
            assert!(
                registry
                    .studio_manifests()
                    .all(|manifest| manifest.node_type != resource_only),
                "{resource_only} must stay out of the Studio Catalog"
            );
        }
        assert_eq!(
            registry.get("skill", 1).expect("Skill manifest").capability,
            NodeCapability::Skill
        );

        let agent = registry.get("agent", 2).expect("Agent v2 manifest");
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
                .any(|slot| slot.name == "mcp_tools"
                    && slot.placement == BindingSlotPlacement::Inspector)
        );
        assert!(
            agent
                .binding_slots
                .iter()
                .any(|slot| slot.name == "skills"
                    && slot.placement == BindingSlotPlacement::Inspector)
        );
        assert!(agent.binding_slots.iter().any(|slot| {
            slot.name == "model"
                && slot.required
                && slot.placement == BindingSlotPlacement::Inspector
        }));
        assert!(agent.binding_slots.iter().any(|slot| {
            slot.name == "workspace_sandbox"
                && !slot.required
                && slot.placement == BindingSlotPlacement::Inspector
        }));
        assert!(registry.get("agent", 1).is_none());

        let mut old_agent = agent.clone();
        old_agent.version = 1;
        assert!(matches!(
            NodeRegistry::default().register(old_agent),
            Err(RegistryError::InvalidManifest { .. })
        ));
        let mut invalid_agent = agent.clone();
        invalid_agent
            .binding_slots
            .iter_mut()
            .find(|slot| slot.name == "model")
            .expect("model slot")
            .placement = BindingSlotPlacement::Canvas;
        assert!(matches!(
            NodeRegistry::default().register(invalid_agent),
            Err(RegistryError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn generated_studio_catalog_fixture_does_not_drift() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../apps/web/src/features/workflow-designer/testing/studio-catalog.fixture.json",
        );
        let committed: Value = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
        )
        .expect("Studio Catalog fixture is valid JSON");
        let registry = NodeRegistry::m5_defaults();
        let generated = serde_json::to_value(registry.studio_manifests().collect::<Vec<_>>())
            .expect("Studio Catalog serializes");
        assert_eq!(
            committed, generated,
            "Studio Catalog drift: run `cargo run -p agentx-runtime --bin generate-studio-catalog -- apps/web/src/features/workflow-designer/testing/studio-catalog.fixture.json`"
        );
    }

    #[test]
    fn merge_manifest_ports_and_modes_are_declared() {
        let registry = NodeRegistry::m5_defaults();
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
