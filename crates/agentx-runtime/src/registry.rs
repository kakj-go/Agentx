use std::collections::BTreeMap;

use agentx_node_protocol::{
    ExecutionStyle, NODE_PROTOCOL_VERSION, NodeCapability, NodeManifestVersion, NodePort, PortKind,
    ReadinessPolicy, SideEffectLevel,
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
        execution_style: style,
        capability,
        readiness,
        input_ports: inputs,
        output_ports: outputs,
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

fn default_manifests() -> Vec<NodeManifestVersion> {
    let main_in = || vec![port("main", PortKind::Main, true, false)];
    let main_out = || {
        vec![
            port("main", PortKind::Main, false, false),
            port("error", PortKind::Error, false, false),
        ]
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
        manifest(
            "set",
            ExecutionStyle::Action,
            NodeCapability::Builtin,
            ReadinessPolicy::Any,
            main_in(),
            main_out(),
            SideEffectLevel::None,
        ),
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
        manifest(
            "merge",
            ExecutionStyle::Action,
            NodeCapability::Builtin,
            ReadinessPolicy::Required,
            vec![port("main", PortKind::Main, true, true)],
            main_out(),
            SideEffectLevel::None,
        ),
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
        manifest(
            "sub_workflow",
            ExecutionStyle::SubWorkflow,
            NodeCapability::Builtin,
            ReadinessPolicy::Any,
            main_in(),
            main_out(),
            SideEffectLevel::None,
        ),
        manifest(
            "declarative_http",
            ExecutionStyle::Action,
            NodeCapability::DeclarativeHttp,
            ReadinessPolicy::Any,
            main_in(),
            main_out(),
            SideEffectLevel::Idempotent,
        ),
        manifest(
            "remote_action",
            ExecutionStyle::Action,
            NodeCapability::RemoteAction,
            ReadinessPolicy::Any,
            main_in(),
            main_out(),
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
}
