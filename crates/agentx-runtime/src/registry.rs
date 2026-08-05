use std::collections::BTreeMap;

use agentx_domain::ResourceType;
use agentx_node_protocol::{
    BindingSlot, ExecutionStyle, NODE_PROTOCOL_VERSION, NodeCapability, NodeManifestVersion,
    NodePort, PortKind, ReadinessPolicy, SideEffectLevel,
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

fn manifest(
    node_type: &str,
    style: ExecutionStyle,
    capability: NodeCapability,
    readiness: ReadinessPolicy,
    inputs: Vec<NodePort>,
    outputs: Vec<NodePort>,
    side_effect_level: SideEffectLevel,
) -> NodeManifestVersion {
    NodeManifestVersion {
        protocol_version: NODE_PROTOCOL_VERSION.into(),
        node_type: node_type.into(),
        version: 1,
        display_name: node_type.replace('_', " "),
        description: String::new(),
        category: category(node_type).into(),
        keywords: node_type.split('_').map(str::to_owned).collect(),
        icon_key: icon_key(node_type).into(),
        execution_style: style,
        capability,
        readiness,
        input_ports: inputs,
        output_ports: outputs,
        binding_slots: Vec::new(),
        parameter_schema: json!({"type":"object"}),
        ui_schema: json!({}),
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
    value.ui_schema = json!({"fields":fields});
    if let Some((resource_type, operation)) = resource_selector {
        value
            .ui_schema
            .as_object_mut()
            .expect("UI schema is an object")
            .insert(
                "resourceSelectors".into(),
                json!([{
                    "resourceType":resource_type,
                    "operation":operation,
                    "required":true
                }]),
            );
    }
    if node_type == "code" {
        value
            .ui_schema
            .get_mut("resourceSelectors")
            .and_then(serde_json::Value::as_array_mut)
            .expect("Code has resource selectors")
            .push(json!({"resourceType":"credential","operation":"use","required":false,"label":"Credential"}));
    }
    value.default_timeout_ms = Some(300_000);
    value.supports_mock = false;
    value.sandbox_required = node_type == "code";
    value
}

fn category(node_type: &str) -> &'static str {
    match node_type {
        "manual_trigger" => "triggers",
        "if" | "switch" | "merge" | "loop_over_items" | "wait" | "approval" | "sub_workflow" => {
            "flow"
        }
        "agent" | "model" | "mcp_tool" | "skill" | "rag" | "memory" => "ai",
        "code" => "code",
        _ => "actions",
    }
}

fn icon_key(node_type: &str) -> &'static str {
    match node_type {
        "manual_trigger" => "mouse-pointer-click",
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
        manifest.ui_schema = ui_schema;
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
}
