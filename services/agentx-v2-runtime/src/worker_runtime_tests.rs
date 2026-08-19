use super::{
    WorkerExecution, effective_agent_budget, execute_builtin_node, mcp_tool_binding,
    openai_chat_completions_endpoint, openai_execution_output, provider_secret_header,
    provider_usage_detail, runtime_call_fingerprint, runtime_call_is_replayable,
    sandbox_execution_output,
};
use agentx_runtime_contracts::{
    ContentHash, RuntimeResourceBindingV1, RuntimeResourceConfigurationV1, RuntimeResourceKindV1,
    WorkerResultStatusV1,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn worker_operation_deadline_cancels_slow_provider_work() {
    let result = crate::worker_support::with_operation_deadline(
        time::OffsetDateTime::now_utc() + time::Duration::milliseconds(5),
        async {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            "late"
        },
    )
    .await;
    assert!(result.is_err());
}

fn mcp_binding(resource_id: Uuid, tool_name: &str) -> RuntimeResourceBindingV1 {
    RuntimeResourceBindingV1 {
        resource_kind: RuntimeResourceKindV1::Mcp,
        resource_id,
        resource_version: "1".into(),
        state_epoch: 1,
        content_hash: ContentHash::parse(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        configuration: RuntimeResourceConfigurationV1::Mcp {
            endpoint: "http://mcp.example/mcp".into(),
            tool_name: tool_name.into(),
            tool_version: "1".into(),
            input_schema_hash: ContentHash::parse(
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
            credential: None,
        },
        object_ids: Vec::new(),
    }
}

#[test]
fn mcp_server_closure_never_shadows_the_executable_tool_binding() {
    let server = mcp_binding(Uuid::now_v7(), "__server__");
    let tool_id = Uuid::now_v7();
    let resources = [server, mcp_binding(tool_id, "echo")];
    let selected = mcp_tool_binding(&resources).unwrap();
    assert_eq!(selected.resource_id, tool_id);
    assert!(
        matches!(&selected.configuration, RuntimeResourceConfigurationV1::Mcp { tool_name, .. } if tool_name == "echo")
    );
}

#[test]
fn current_agent_manifest_budget_fields_override_defaults() {
    assert_eq!(
        effective_agent_budget(
            &json!({"maxIterations":3,"maxTotalTokens":1000,"maxCostMicros":1000})
        ),
        json!({"maxIterations":3,"maxTokens":1000,"maxCostMicros":1000})
    );
}

#[test]
fn stop_and_error_is_a_failed_worker_result_with_frozen_parameters() {
    let result = execute_builtin_node(
        "stop_and_error",
        &json!({"code":"EXPECTED_STOP","message":"expected message"}),
        json!({"ignored":true}),
    );
    assert_eq!(result.status, WorkerResultStatusV1::Failed);
    assert_eq!(result.error_code.as_deref(), Some("EXPECTED_STOP"));
    assert_eq!(result.error_message.as_deref(), Some("expected message"));
}

#[test]
fn set_builtin_uses_resolved_values_and_keep_only_set() {
    let result = execute_builtin_node(
        "set",
        &json!({"values":{"answer":"resolved"},"keepOnlySet":true}),
        json!({"input":"not copied"}),
    );
    assert_eq!(result.status, WorkerResultStatusV1::Succeeded);
    assert_eq!(result.outputs["main"][0].json, json!({"answer":"resolved"}));
}

#[test]
fn explicit_nested_agent_budget_takes_precedence() {
    assert_eq!(
        effective_agent_budget(
            &json!({"budget":{"maxIterations":4,"maxTokens":2000,"maxCostMicros":3000},"maxIterations":2,"maxTotalTokens":500,"maxCostMicros":700})
        ),
        json!({"maxIterations":4,"maxTokens":2000,"maxCostMicros":3000})
    );
}

#[test]
fn sandbox_runtime_call_fingerprint_ignores_attempt_lease_identity() {
    let first = json!({"apiVersion":1,"attemptId":"018f0000-0000-7000-8000-000000000001","workerId":"018f0000-0000-7000-8000-000000000002","fencingToken":1,"idempotencyKey":"sandbox:execute:attempt","input":{"message":"stable"}});
    let replacement = json!({"apiVersion":1,"attemptId":"018f0000-0000-7000-8000-000000000001","workerId":"018f0000-0000-7000-8000-000000000003","fencingToken":2,"idempotencyKey":"sandbox:execute:attempt","input":{"message":"stable"}});
    assert_eq!(
        runtime_call_fingerprint("sandbox", &first),
        runtime_call_fingerprint("sandbox", &replacement)
    );
    let changed = json!({"apiVersion":1,"attemptId":"018f0000-0000-7000-8000-000000000001","workerId":"018f0000-0000-7000-8000-000000000003","fencingToken":2,"idempotencyKey":"sandbox:execute:attempt","input":{"message":"changed"}});
    assert_ne!(
        runtime_call_fingerprint("sandbox", &first),
        runtime_call_fingerprint("sandbox", &changed)
    );
}

#[test]
fn only_uncommitted_or_idempotent_sent_runtime_calls_are_replayable() {
    assert!(runtime_call_is_replayable("reserved", "irreversible"));
    assert!(runtime_call_is_replayable("sent", "none"));
    assert!(runtime_call_is_replayable("sent", "idempotent"));
    assert!(!runtime_call_is_replayable("sent", "irreversible"));
    assert!(!runtime_call_is_replayable("outcome_unknown", "none"));
    assert!(!runtime_call_is_replayable("failed", "none"));
}

#[test]
fn sandbox_manager_envelope_is_not_exposed_as_node_output() {
    let execution = sandbox_execution_output(WorkerExecution::succeeded(
        json!({"apiVersion":1,"leaseId":"018f0000-0000-7000-8000-000000000001","sandboxId":"sandbox-v2","replayed":true,"output":{"stdout":"agentx-v2-04","exitCode":0}}),
    ));
    assert_eq!(execution.status, WorkerResultStatusV1::Succeeded);
    assert_eq!(
        execution.outputs["main"][0].json,
        json!({"stdout":"agentx-v2-04","exitCode":0})
    );
}

#[test]
fn openai_compatible_endpoint_targets_chat_completions_once() {
    assert_eq!(
        openai_chat_completions_endpoint("https://provider.example/v1"),
        "https://provider.example/v1/chat/completions"
    );
    assert_eq!(
        openai_chat_completions_endpoint("https://provider.example/v1/chat/completions/"),
        "https://provider.example/v1/chat/completions"
    );
}

#[test]
fn provider_authorization_secret_adds_bearer_only_when_needed() {
    assert_eq!(
        provider_secret_header(b"raw-secret", "authorization"),
        b"Bearer raw-secret"
    );
    assert_eq!(
        provider_secret_header(b"Bearer token", "authorization"),
        b"Bearer token"
    );
    assert_eq!(
        provider_secret_header(b"Basic token", "authorization"),
        b"Basic token"
    );
    assert_eq!(
        provider_secret_header(b"raw-secret", "x-api-key"),
        b"raw-secret"
    );
}

#[test]
fn openai_tool_call_is_normalized_for_the_agent_loop() {
    let execution = openai_execution_output(WorkerExecution::succeeded(
        json!({"choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hello\"}"}}]}}],"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}),
    ));
    assert_eq!(
        execution.outputs["main"][0].json,
        json!({"toolCall":{"text":"hello"},"usage":{"inputTokens":11,"outputTokens":7,"tokens":18,"costMicros":0}})
    );
}

#[test]
fn openai_final_answer_and_usage_are_normalized() {
    let execution = openai_execution_output(WorkerExecution::succeeded(
        json!({"choices":[{"message":{"role":"assistant","content":"complete"}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}),
    ));
    assert_eq!(
        execution.outputs["main"][0].json,
        json!({"done":true,"answer":"complete","finalAnswer":"complete","usage":{"inputTokens":5,"outputTokens":3,"tokens":8,"costMicros":0}})
    );
}

#[test]
fn provider_usage_detail_accepts_raw_and_normalized_token_names() {
    assert_eq!(
        provider_usage_detail(
            &json!({"usage":{"prompt_tokens":5,"completion_tokens":3,"cost_micros":7}})
        ),
        (5, 3, 7)
    );
    assert_eq!(
        provider_usage_detail(&json!({"usage":{"inputTokens":11,"outputTokens":4,"costMicros":9}})),
        (11, 4, 9)
    );
}
