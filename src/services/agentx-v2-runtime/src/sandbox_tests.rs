use agentx_runtime_contracts::SandboxEgressModeV1;
use base64::Engine as _;

use super::{
    SANDBOX_EGRESS_CA_CONTENT_TYPE, SANDBOX_EGRESS_CA_METADATA_FILENAME, SANDBOX_EGRESS_CA_MODE,
    SANDBOX_EGRESS_CA_PATH, STANDARD, opensandbox_network_policy, parse_command_stream,
    sandbox_command, server_proxy_endpoint,
};
use serde_json::json;

fn decoded_program(command: &str) -> String {
    let encoded = command
        .split("printf '%s' '")
        .nth(2)
        .expect("encoded source command")
        .split('\'')
        .next()
        .expect("encoded source value");
    String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap()
}

#[test]
fn egress_ca_upload_uses_the_pinned_execd_file_contract() {
    assert_eq!(SANDBOX_EGRESS_CA_PATH, "/tmp/agentx-egress-ca.pem");
    assert_eq!(SANDBOX_EGRESS_CA_MODE, 600);
    assert_eq!(SANDBOX_EGRESS_CA_CONTENT_TYPE, "application/octet-stream");
    assert_eq!(SANDBOX_EGRESS_CA_METADATA_FILENAME, "metadata.json");
}

#[test]
fn python_sandbox_command_uses_the_pinned_image_interpreter() {
    let command = sandbox_command(
        &json!({"runner":"python","inputs":{"name":"Ada"},"source":"def main(**inputs): return inputs"}),
        "sandbox:test",
    )
    .unwrap();
    assert!(command.contains("python3 '/tmp/agentx-v2.py'"));
    assert!(command.contains("AGENTX_INPUT_PATH='/tmp/agentx-input.json'"));
    assert!(command.contains("AGENTX_OUTPUT_PATH='/tmp/agentx-output.json'"));
    let wrapper = decoded_program(&command);
    assert!(wrapper.contains("main(**_agentx_inputs)"));
    assert!(!command.contains(" python '/tmp/agentx-v2.py'"));
}

#[test]
fn javascript_and_shell_use_one_named_json_input_object() {
    let javascript = sandbox_command(
        &json!({"runner":"javascript","inputs":{"name":"Ada","count":2},"source":"async function main({ name, count }) { return { name, count }; }"}),
        "sandbox:javascript",
    )
    .unwrap();
    assert!(javascript.contains("AGENTX_INPUT_PATH='/tmp/agentx-input.json'"));
    assert!(decoded_program(&javascript).contains("main(inputs)"));
    let shell = sandbox_command(
        &json!({"runner":"shell","inputs":{"name":"Ada"},"source":"cat \"$AGENTX_INPUT_PATH\" > \"$AGENTX_OUTPUT_PATH\""}),
        "sandbox:shell",
    )
    .unwrap();
    assert!(shell.contains("AGENTX_INPUT_PATH='/tmp/agentx-input.json'"));
    assert!(shell.contains("AGENTX_OUTPUT_PATH='/tmp/agentx-output.json'"));
}

#[test]
fn sandbox_command_rejects_removed_browser_runner() {
    let error = sandbox_command(
        &json!({"runner":"browser","inputs":{},"source":"console.log('nope')"}),
        "sandbox:browser",
    )
    .unwrap_err();
    assert!(error.message.contains("unsupported Sandbox runner browser"));
}

#[test]
fn opensandbox_policy_only_allows_the_dedicated_gateway_host() {
    assert_eq!(
        opensandbox_network_policy(
            SandboxEgressModeV1::TcpProxy,
            Some("egress.internal.example")
        ),
        json!({
            "defaultAction":"deny",
            "egress":[{"action":"allow","target":"egress.internal.example"}]
        })
    );
    assert_eq!(
        opensandbox_network_policy(SandboxEgressModeV1::None, None),
        json!({"defaultAction":"deny","egress":[]})
    );
}

#[test]
fn command_stream_accepts_the_pinned_execd_json_frames() {
    let body = concat!(
        "{\"type\":\"init\",\"text\":\"command-id\"}\n\n",
        "{\"type\":\"stdout\",\"text\":\"m6-studio-ok\\n\"}\n\n",
        "{\"type\":\"execution_complete\",\"execution_time\":2}\n\n"
    );
    let (stdout, stderr, exit_code) = parse_command_stream(body).unwrap();
    assert_eq!(stdout, "m6-studio-ok\n");
    assert!(stderr.is_empty());
    assert_eq!(exit_code, 0);
}

#[test]
fn command_stream_accepts_spec_sse_and_rejects_incomplete_success() {
    let (stdout, stderr, exit_code) = parse_command_stream(concat!(
        "data: {\"type\":\"stdout\",\"text\":\"ok\"}\n\n",
        "data: {\"type\":\"result\",\"exit_code\":0}\n\n"
    ))
    .unwrap();
    assert_eq!((stdout.as_str(), stderr.as_str(), exit_code), ("ok", "", 0));

    let error = parse_command_stream("{\"type\":\"stdout\",\"text\":\"partial\"}\n").unwrap_err();
    assert!(error.message.contains("without a terminal event"));
    assert!(error.outcome_unknown);
}

#[test]
fn command_stream_reads_nested_execd_errors() {
    let error = parse_command_stream(
        "{\"type\":\"error\",\"error\":{\"ename\":\"ExitError\",\"evalue\":\"command failed\"}}\n",
    )
    .unwrap_err();
    assert_eq!(error.message, "command failed");
    assert!(!error.outcome_unknown);

    let error = parse_command_stream(concat!(
        "{\"type\":\"stderr\",\"text\":\"traceback details\"}\n",
        "{\"type\":\"error\",\"error\":{\"ename\":\"ExitError\",\"evalue\":\"1\"}}\n"
    ))
    .unwrap_err();
    assert_eq!(error.message, "1: traceback details");
}

#[test]
fn server_proxy_endpoint_adds_trailing_slash_and_rewrites_loopback_host() {
    let endpoint = server_proxy_endpoint(
        "http://host.docker.internal:18080",
        "127.0.0.1:18080/v1/sandboxes/sbx-1/proxy/44772",
        "sbx-1",
    )
    .unwrap();
    assert_eq!(
        endpoint.as_str(),
        "http://host.docker.internal:18080/v1/sandboxes/sbx-1/proxy/44772/"
    );
    assert_eq!(
        endpoint.join("command").unwrap().as_str(),
        "http://host.docker.internal:18080/v1/sandboxes/sbx-1/proxy/44772/command"
    );
}

#[test]
fn server_proxy_endpoint_accepts_the_exact_execd_path() {
    let endpoint = server_proxy_endpoint(
        "https://opensandbox.example.test",
        "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/44772/",
        "sbx-1",
    )
    .unwrap();
    assert_eq!(endpoint.path(), "/v1/sandboxes/sbx-1/proxy/44772/");
}

#[test]
fn server_proxy_endpoint_cannot_escape_the_lifecycle_origin() {
    let error = server_proxy_endpoint(
        "https://opensandbox.example.test",
        "https://attacker.example.test/v1/sandboxes/sbx-1/proxy/",
        "sbx-1",
    )
    .unwrap_err();
    assert!(
        error
            .message
            .contains("outside the lifecycle provider origin")
    );

    let error = server_proxy_endpoint(
        "https://opensandbox.example.test:8443",
        "https://opensandbox.example.test:9443/v1/sandboxes/sbx-1/proxy/",
        "sbx-1",
    )
    .unwrap_err();
    assert!(
        error
            .message
            .contains("outside the lifecycle provider origin")
    );
}

#[test]
fn server_proxy_endpoint_rejects_path_and_query_injection() {
    for endpoint in [
        "https://opensandbox.example.test/v1/sandboxes/other/proxy/",
        "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/",
        "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/22/",
        "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/command/",
        "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/?target=metadata",
    ] {
        assert!(
            server_proxy_endpoint("https://opensandbox.example.test", endpoint, "sbx-1").is_err(),
            "endpoint should be rejected: {endpoint}"
        );
    }
}
