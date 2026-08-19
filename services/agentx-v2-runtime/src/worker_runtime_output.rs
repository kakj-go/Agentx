use agentx_runtime_contracts::{RuntimeResourceConfigurationV1, WorkerResultStatusV1};
use serde_json::{Value, json};

use super::{ClaimedWorkerAttempt, WorkerExecution, mcp_tool_binding, successful_value};

pub(super) fn openai_chat_request(
    claim: &ClaimedWorkerAttempt,
    model: &str,
    price_version: &str,
    input: &Value,
) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = claim
        .node_parameters
        .get("systemPrompt")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({"role":"system","content":system}));
    }
    let content = claim
        .node_parameters
        .get("userQuestion")
        .cloned()
        .or_else(|| input.get("question").cloned())
        .unwrap_or_else(|| input.clone());
    messages.push(json!({"role":"user","content":json_text(&content)}));
    if let Some(tool) = input.get("tool") {
        messages.push(json!({
            "role":"tool",
            "tool_call_id":"agentx-runtime-tool",
            "content":json_text(tool),
        }));
    }
    let mut request = json!({
        "model":model,
        "messages":messages,
        "stream":false,
        "metadata":{"priceVersion":price_version},
    });
    if let Some(tool) = mcp_tool_binding(&claim.resources)
        && let RuntimeResourceConfigurationV1::Mcp { tool_name, .. } = &tool.configuration
    {
        request["tools"] = json!([{
            "type":"function",
            "function":{
                "name":tool_name,
                "description":"Runtime-pinned MCP tool",
                "parameters":{"type":"object","additionalProperties":true},
            }
        }]);
    }
    request
}

pub(super) fn openai_execution_output(execution: WorkerExecution) -> WorkerExecution {
    if execution.status != WorkerResultStatusV1::Succeeded {
        return execution;
    }
    let Some(response) = successful_value(&execution) else {
        return WorkerExecution::failed(
            "PROVIDER_RESPONSE_INVALID",
            "OpenAI-compatible response is empty",
            false,
        );
    };
    let Some(message) = response.pointer("/choices/0/message") else {
        return WorkerExecution::failed(
            "PROVIDER_RESPONSE_INVALID",
            "OpenAI-compatible response has no assistant message",
            false,
        );
    };
    let usage = response.get("usage").cloned().unwrap_or_else(|| json!({}));
    let normalized_usage = json!({
        "inputTokens":usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
        "outputTokens":usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
        "tokens":usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(0),
        "costMicros":0,
    });
    if let Some(arguments) = message
        .pointer("/tool_calls/0/function/arguments")
        .and_then(Value::as_str)
    {
        let arguments = serde_json::from_str(arguments).unwrap_or_else(|_| {
            json!({
                "value":arguments,
            })
        });
        return WorkerExecution::succeeded(json!({
            "toolCall":arguments,
            "usage":normalized_usage,
        }));
    }
    let content = message.get("content").cloned().unwrap_or(Value::Null);
    WorkerExecution::succeeded(json!({
        "done":true,
        "answer":content,
        "finalAnswer":content,
        "usage":normalized_usage,
    }))
}

fn json_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

pub(super) fn provider_usage(value: &Value) -> (u64, u64) {
    let (input, output, cost) = provider_usage_detail(value);
    let usage = value.get("usage").unwrap_or(value);
    let total = usage
        .get("tokens")
        .or_else(|| usage.get("totalTokens"))
        .or_else(|| usage.get("total_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input.saturating_add(output));
    (total, cost)
}

pub(super) fn provider_usage_detail(value: &Value) -> (u64, u64, u64) {
    let usage = value.get("usage").unwrap_or(value);
    let input = usage
        .get("inputTokens")
        .or_else(|| usage.get("input_tokens"))
        .or_else(|| usage.get("promptTokens"))
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("outputTokens")
        .or_else(|| usage.get("output_tokens"))
        .or_else(|| usage.get("completionTokens"))
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cost = usage
        .get("costMicros")
        .or_else(|| usage.get("cost_micros"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (input, output, cost)
}

pub(super) fn effective_agent_budget(parameters: &Value) -> Value {
    let nested = parameters.get("budget");
    let maximum_iterations = nested
        .and_then(|budget| budget.get("maxIterations"))
        .or_else(|| parameters.get("maxIterations"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 100);
    let maximum_tokens = nested
        .and_then(|budget| budget.get("maxTokens"))
        .or_else(|| parameters.get("maxTotalTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(4096);
    let maximum_cost = nested
        .and_then(|budget| budget.get("maxCostMicros"))
        .or_else(|| parameters.get("maxCostMicros"))
        .and_then(Value::as_u64)
        .unwrap_or(1_000_000);
    json!({
        "maxIterations": maximum_iterations,
        "maxTokens": maximum_tokens,
        "maxCostMicros": maximum_cost,
    })
}

pub(super) fn runtime_call_is_replayable(status: &str, side_effect: &str) -> bool {
    status == "reserved" || (status == "sent" && matches!(side_effect, "none" | "idempotent"))
}

pub(super) fn sandbox_execution_output(execution: WorkerExecution) -> WorkerExecution {
    if execution.status != WorkerResultStatusV1::Succeeded {
        return execution;
    }
    let Some(response) = successful_value(&execution) else {
        return WorkerExecution::failed(
            "PROVIDER_RESPONSE_INVALID",
            "Sandbox Manager returned no response payload",
            false,
        );
    };
    let Some(output) = response.get("output").cloned() else {
        return WorkerExecution::failed(
            "PROVIDER_RESPONSE_INVALID",
            "Sandbox Manager response has no output",
            false,
        );
    };
    WorkerExecution::succeeded(output)
}
