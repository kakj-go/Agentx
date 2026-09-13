use std::net::IpAddr;

use agentx_domain::WorkflowNode;
use ipnet::IpNet;
use serde_json::Value;

use super::CompileIssue;

pub(super) fn validate_code_node(
    definition_index: usize,
    node: &WorkflowNode,
    issues: &mut Vec<CompileIssue>,
) {
    validate_network_policy(definition_index, node, issues);
    let Some(example) = node.parameters.get("outputExample") else {
        issues.push(CompileIssue {
            code: "CODE_OUTPUT_EXAMPLE_REQUIRED".into(),
            path: format!("nodes[{definition_index}].parameters.outputExample"),
            message: "Code outputExample is required".into(),
        });
        return;
    };
    if !example.is_object() {
        issues.push(CompileIssue {
            code: "CODE_OUTPUT_EXAMPLE_OBJECT_REQUIRED".into(),
            path: format!("nodes[{definition_index}].parameters.outputExample"),
            message: "Code outputExample must be a JSON object".into(),
        });
    }
}

fn validate_network_policy(
    definition_index: usize,
    node: &WorkflowNode,
    issues: &mut Vec<CompileIssue>,
) {
    let path = format!("nodes[{definition_index}].parameters.networkPolicy");
    let policy = node
        .parameters
        .get("networkPolicy")
        .and_then(Value::as_object);
    let mode = policy
        .and_then(|policy| policy.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("deny");
    let destinations = policy
        .and_then(|policy| policy.get("destinations"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if (mode == "deny" && !destinations.is_empty())
        || (mode == "allowlist" && destinations.is_empty())
    {
        issues.push(CompileIssue {
            code: "CODE_NETWORK_POLICY_INVALID".into(),
            path: path.clone(),
            message:
                "deny requires no destinations and allowlist requires at least one destination"
                    .into(),
        });
    }
    for (index, destination) in destinations.iter().enumerate() {
        let target = destination
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !valid_egress_target(target) {
            issues.push(CompileIssue {
                code: "CODE_NETWORK_TARGET_FORBIDDEN".into(),
                path: format!("{path}.destinations[{index}].target"),
                message: "Destination must be a valid domain, wildcard domain, IP, or CIDR and cannot target protected infrastructure".into(),
            });
        }
        for (port_index, range) in destination
            .get("ports")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let from = range
                .get("from")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let to = range.get("to").and_then(Value::as_u64).unwrap_or_default();
            if from == 0 || to == 0 || from > to || to > u64::from(u16::MAX) {
                issues.push(CompileIssue {
                    code: "CODE_NETWORK_PORT_RANGE_INVALID".into(),
                    path: format!("{path}.destinations[{index}].ports[{port_index}]"),
                    message: "Port ranges must stay within 1..=65535 and from must not exceed to"
                        .into(),
                });
            }
        }
    }
}

fn valid_egress_target(value: &str) -> bool {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || value == "localhost"
        || value.ends_with(".localhost")
        || value.ends_with(".svc")
        || value.ends_with(".cluster.local")
        || matches!(
            value.as_str(),
            "kubernetes" | "kubernetes.default" | "kubernetes.default.svc"
        )
    {
        return false;
    }
    if let Ok(network) = value.parse::<IpNet>() {
        return !permanently_blocked_ip(network.network())
            && !permanently_blocked_ip(network.broadcast());
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return !permanently_blocked_ip(ip);
    }
    let domain = value.strip_prefix("*.").unwrap_or(&value);
    domain.len() <= 253
        && domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn permanently_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || (octets[0] == 169 && octets[1] == 254)
                || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        }
        IpAddr::V6(ip) => ip.is_unspecified() || ip.is_loopback() || ip.is_multicast(),
    }
}
