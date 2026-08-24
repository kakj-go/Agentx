use agentx_domain::{ContextScope, ContextWriteOperation, NodeExecutionId};
use agentx_node_protocol::Item;
use agentx_runtime::{
    ExecutionMachine, ExpressionContext, ExpressionEngine, RuntimeExecutionStatus,
};
use agentx_runtime_contracts::{RuntimeAuthorizationSnapshotV1, RuntimeResourceBindingV1};
use serde_json::{Map, Value, json};
use sqlx::{MySql, Row, Transaction};
use uuid::Uuid;

use crate::{
    engine_names::{activation_status, machine_status, side_effect_name},
    engine_protocol::runtime_bad_request,
    error::{RuntimeError, RuntimeResult},
};

pub(crate) fn apply_context_write(
    context: &mut Value,
    path: &str,
    operation: ContextWriteOperation,
    value: Value,
) -> RuntimeResult<()> {
    let segments = path
        .split('.')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(runtime_bad_request(
            "INVALID_CONTEXT_PATH",
            "Context path is empty",
        ));
    }
    let mut target = context;
    for segment in &segments[..segments.len() - 1] {
        let object = target.as_object_mut().ok_or_else(|| {
            runtime_bad_request("INVALID_CONTEXT_PATH", "Context path is not an object")
        })?;
        target = object
            .entry((*segment).to_owned())
            .or_insert_with(|| json!({}));
    }
    let object = target.as_object_mut().ok_or_else(|| {
        runtime_bad_request("INVALID_CONTEXT_PATH", "Context parent is not an object")
    })?;
    let key = segments[segments.len() - 1];
    let current = object.get(key).cloned().unwrap_or(Value::Null);
    let next = match operation {
        ContextWriteOperation::Set => Some(value),
        ContextWriteOperation::SetIfAbsent => current.is_null().then_some(value),
        ContextWriteOperation::Delete => None,
        ContextWriteOperation::Append => {
            let mut values = current.as_array().cloned().unwrap_or_default();
            match value {
                Value::Array(items) => values.extend(items),
                value => values.push(value),
            }
            Some(Value::Array(values))
        }
        ContextWriteOperation::MergeObject => {
            let mut values = current.as_object().cloned().unwrap_or_default();
            values.extend(value.as_object().cloned().unwrap_or_default());
            Some(Value::Object(values))
        }
        ContextWriteOperation::Increment => Some(json!(
            current.as_f64().unwrap_or(0.0) + value.as_f64().unwrap_or(0.0)
        )),
        ContextWriteOperation::Min => Some(json!(
            current
                .as_f64()
                .unwrap_or(f64::INFINITY)
                .min(value.as_f64().unwrap_or(f64::INFINITY))
        )),
        ContextWriteOperation::Max => Some(json!(
            current
                .as_f64()
                .unwrap_or(f64::NEG_INFINITY)
                .max(value.as_f64().unwrap_or(f64::NEG_INFINITY))
        )),
        ContextWriteOperation::CompareAndSet => {
            let expected = value.get("expected").cloned().unwrap_or(Value::Null);
            (current == expected).then(|| value.get("value").cloned().unwrap_or(Value::Null))
        }
    };
    if let Some(next) = next {
        object.insert(key.to_owned(), next);
    } else if operation == ContextWriteOperation::Delete {
        object.remove(key);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_checkpoint(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    bundle_id: Uuid,
    work_package_id: Option<Uuid>,
    state_version: u64,
    node_execution_id: Option<NodeExecutionId>,
    machine: &ExecutionMachine,
    context: &Value,
    checkpoint_type: &str,
) -> RuntimeResult<Uuid> {
    let sequence: u64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM checkpoints WHERE execution_id=?",
    )
    .bind(execution_id)
    .fetch_one(&mut **tx)
    .await?;
    let payload = serde_json::to_value(crate::fork_runtime::checkpoint_payload(machine, context))
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let hash = agentx_runtime_contracts::content_hash(&payload)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let checkpoint_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO checkpoints(id,tenant_id,execution_id,bundle_id,work_package_id,node_execution_id,sequence_number,checkpoint_type,state_hash,payload_hash,state_version,payload_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(checkpoint_id)
    .bind(tenant_id)
    .bind(execution_id)
    .bind(bundle_id)
    .bind(work_package_id)
    .bind(node_execution_id.map(NodeExecutionId::as_uuid))
    .bind(sequence)
    .bind(checkpoint_type)
    .bind(hash.as_str())
    .bind(hash.as_str())
    .bind(state_version)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT IGNORE INTO bundle_references(id,tenant_id,bundle_id,reference_kind,owner_id) VALUES(?,?,?,'checkpoint_fork_source',?)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(bundle_id)
    .bind(checkpoint_id)
    .execute(&mut **tx)
    .await?;
    Ok(checkpoint_id)
}

pub(super) async fn upsert_activation(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    activation: &agentx_runtime::NodeActivation,
    node: &agentx_runtime::CompiledNode,
) -> RuntimeResult<()> {
    let status = activation_status(activation.status);
    sqlx::query(
        "INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,iteration_index,status,capability,side_effect_level,input_json,started_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,IF(?='ready',NULL,UTC_TIMESTAMP(6))) ON DUPLICATE KEY UPDATE status=VALUES(status),input_json=VALUES(input_json),started_at=IF(started_at IS NULL AND VALUES(status)<>'ready',UTC_TIMESTAMP(6),started_at),updated_at=UTC_TIMESTAMP(6)",
    )
    .bind(activation.id.as_uuid())
    .bind(tenant_id)
    .bind(execution_id)
    .bind(&node.id)
    .bind(&node.key)
    .bind(&node.name)
    .bind(&node.node_type)
    .bind(node.type_version)
    .bind(activation.generation)
    .bind(activation.slot)
    .bind(activation.run_index)
    .bind(0_u32)
    .bind(status)
    .bind(node.capability.as_str())
    .bind(side_effect_name(&node.side_effect_level))
    .bind(serde_json::to_value(&activation.inputs).map_err(|error| RuntimeError::Internal(error.into()))?)
    .bind(status)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn upsert_failed_activation(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    activation: &agentx_runtime::NodeActivation,
    node: &agentx_runtime::CompiledNode,
    error_code: &str,
    error_message: &str,
) -> RuntimeResult<()> {
    upsert_activation(tx, tenant_id, execution_id, activation, node).await?;
    let updated = sqlx::query(
        "UPDATE node_executions SET error_code=?,error_message=?,ended_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND execution_id=? AND id=? AND status='failed'",
    )
    .bind(error_code)
    .bind(error_message)
    .bind(tenant_id)
    .bind(execution_id)
    .bind(activation.id.as_uuid())
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(RuntimeError::Internal(anyhow::anyhow!(
            "failed activation was not persisted"
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish_execution(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    work_package_id: Option<Uuid>,
    invocation_id: Option<Uuid>,
    state_version: u64,
    machine: &ExecutionMachine,
    context: &Value,
) -> RuntimeResult<()> {
    let (status, output, error, string_conversions) =
        match materialize_result(tx, tenant_id, execution_id, machine, context).await {
            Ok((output, error, conversions)) => {
                (machine_status(machine.status()), output, error, conversions)
            }
            // Definition/value errors are terminal workflow data. Storage errors
            // must still abort the transaction so recovery can retry them.
            Err(RuntimeError::Deterministic { code, message }) => (
                "failed",
                json!({}),
                Some(json!({"code":code,"message":message})),
                Vec::new(),
            ),
            Err(
                RuntimeError::BadRequest(_, message) | RuntimeError::InvalidRequest(_, message),
            ) => (
                "failed",
                json!({}),
                Some(json!({
                    "code":"END_OUTPUT_EVALUATION_FAILED",
                    "message":message,
                })),
                Vec::new(),
            ),
            Err(error) => return Err(error),
        };
    super::engine::string_conversion_trace::enqueue_end_records(
        tx,
        tenant_id,
        execution_id,
        &string_conversions,
    )
    .await;
    let error_code = error
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    let error_message = error
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str);
    let result_hash = agentx_runtime_contracts::content_hash(&output)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    sqlx::query(
        "UPDATE workflow_executions SET status=?,state_version=?,output_json=?,error_code=?,error_message=?,error_json=?,terminal_result_json=?,terminal_result_hash=?,ended_at=UTC_TIMESTAMP(6),duration_ms=TIMESTAMPDIFF(MICROSECOND,started_at,UTC_TIMESTAMP(6))/1000 WHERE tenant_id=? AND id=?",
    )
    .bind(status)
    .bind(state_version)
    .bind(&output)
    .bind(error_code)
    .bind(error_message)
    .bind(&error)
    .bind(&output)
    .bind(result_hash.as_str())
    .bind(tenant_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await?;
    let invocation_status = if status == "succeeded" {
        "completed"
    } else if machine.status() == RuntimeExecutionStatus::Cancelled {
        "cancelled"
    } else {
        "failed"
    };
    if invocation_id.is_some() {
        sqlx::query(
            "UPDATE application_invocations SET status=?,state_version=state_version+1,result_json=?,error_json=?,completed_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND execution_id=?",
        )
        .bind(invocation_status)
        .bind(&output)
        .bind(&error)
        .bind(tenant_id)
        .bind(execution_id)
        .execute(&mut **tx)
        .await?;
    }
    if let Some(invocation_id) = invocation_id.filter(|_| invocation_status == "completed") {
        append_session_assistant_message(tx, tenant_id, invocation_id, &output).await?;
    }
    sqlx::query(
        "UPDATE execution_snapshots SET output_json=?,state_version=? WHERE tenant_id=? AND execution_id=?",
    )
    .bind(&output)
    .bind(state_version)
    .bind(tenant_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE execution_runtime_state SET terminal_result_json=?,terminal_result_hash=? WHERE tenant_id=? AND execution_id=?",
    )
    .bind(&output)
    .bind(result_hash.as_str())
    .bind(tenant_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await?;
    crate::work_package_execution::converge_execution(
        tx,
        tenant_id,
        execution_id,
        work_package_id,
        invocation_status,
        &output,
        &error,
        result_hash.as_str(),
    )
    .await?;
    crate::composite_execution::converge_child(
        tx,
        tenant_id,
        execution_id,
        invocation_status,
        &output,
        &error,
        context,
    )
    .await?;
    sqlx::query(
        "UPDATE bundle_references SET released_at=UTC_TIMESTAMP(6) WHERE tenant_id=? AND reference_kind='active_execution' AND owner_id=? AND released_at IS NULL",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await?;
    crate::quota::release_execution(tx, tenant_id, execution_id, "execution_terminal").await?;
    if let Some(invocation_id) = invocation_id {
        insert_invocation_event(
            tx,
            tenant_id,
            invocation_id,
            if invocation_status == "completed" {
                "invocation.completed"
            } else {
                "invocation.failed"
            },
            json!({"executionId":execution_id,"status":invocation_status,"outputs":output,"error":error}),
        )
        .await?;
    }
    sqlx::query(
        "INSERT INTO execution_outbox(id,tenant_id,execution_id,message_type,payload_json,status) VALUES(?,?,?,'runtime_event',?,'pending')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(execution_id)
    .bind(json!({"type":format!("execution.{status}"),"resultHash":result_hash}))
    .execute(&mut **tx)
    .await?;
    let mut trace = crate::trace_delivery::TraceDraft::execution(
        tenant_id,
        execution_id,
        format!("execution.{status}"),
        status,
    );
    trace.error_code = error
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    trace.error_message = error
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    trace.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::WorkflowOutput);
    trace.content_preview = crate::trace_delivery::bounded_preview(&output);
    let mut end = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        crate::trace_delivery::boundary_entity_id(execution_id, "end"),
        Some((
            execution_id,
            agentx_runtime_contracts::TraceSpanKindV1::Execution,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Boundary,
        "End",
        agentx_runtime_contracts::TraceEventKindV1::Finished,
        "boundary.end.finished",
        status,
    );
    end.error_code = trace.error_code.clone();
    end.error_message = trace.error_message.clone();
    end.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::WorkflowOutput);
    end.content_preview = crate::trace_delivery::bounded_preview(&output);
    crate::trace_delivery::enqueue_best_effort(tx, end).await;
    crate::trace_delivery::enqueue_best_effort(tx, trace).await;
    Ok(())
}

async fn append_session_assistant_message(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    invocation_id: Uuid,
    output: &Value,
) -> RuntimeResult<()> {
    let invocation = sqlx::query(
        "SELECT session_id,chat_mapping_json FROM application_invocations WHERE tenant_id=? AND id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(invocation_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(invocation) = invocation else {
        return Ok(());
    };
    let session_id: Option<Uuid> = invocation.try_get("session_id")?;
    let Some(session_id) = session_id else {
        return Ok(());
    };
    let mapping = invocation
        .try_get::<Option<Value>, _>("chat_mapping_json")?
        .map(serde_json::from_value::<agentx_runtime_contracts::ChatMappingV1>)
        .transpose()
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let Some(mapping) = mapping else {
        return Ok(());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM application_messages WHERE tenant_id=? AND session_id=? AND invocation_id=? AND role='assistant')",
    )
    .bind(tenant_id)
    .bind(session_id)
    .bind(invocation_id)
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        return Ok(());
    }
    let sequence: u64 = sqlx::query_scalar(
        "SELECT next_message_sequence FROM application_sessions WHERE tenant_id=? AND id=? AND status='active' FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(session_id)
    .fetch_one(&mut **tx)
    .await?;
    let message_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO application_messages(id,tenant_id,session_id,invocation_id,sequence_number,role) VALUES(?,?,?,?,?,'assistant')",
    )
    .bind(message_id)
    .bind(tenant_id)
    .bind(session_id)
    .bind(invocation_id)
    .bind(sequence)
    .execute(&mut **tx)
    .await?;
    let parts = crate::output_projection::assistant_message_parts(output, &mapping)?;
    for (index, part) in parts.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO application_message_parts(id,tenant_id,message_id,part_index,part_type,content_json,artifact_id) VALUES(?,?,?,?,?,?,?)",
        )
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(message_id)
        .bind(index as u32)
        .bind(part.part_type)
        .bind(part.content)
        .bind(part.artifact_id)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE application_sessions SET next_message_sequence=next_message_sequence+1,version=version+1 WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(session_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn materialize_result(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    machine: &ExecutionMachine,
    context: &Value,
) -> RuntimeResult<(
    Value,
    Option<Value>,
    Vec<agentx_runtime::StringConversionRecord>,
)> {
    let execution =
        sqlx::query("SELECT input_json FROM workflow_executions WHERE tenant_id=? AND id=?")
            .bind(tenant_id)
            .bind(execution_id)
            .fetch_one(&mut **tx)
            .await?;
    let outputs = load_output_namespace(tx, tenant_id, execution_id).await?;
    let expression_context = ExpressionContext {
        inputs: execution
            .try_get::<Option<Value>, _>("input_json")?
            .unwrap_or(Value::Null),
        outputs,
        contexts: context.clone(),
        execution: crate::execution_context::load(tx, tenant_id, execution_id).await?,
        output_node_keys: machine
            .workflow()
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node.key.clone()))
            .collect(),
        ..ExpressionContext::default()
    };
    let engine = ExpressionEngine;
    let mut string_conversions = Vec::new();
    if machine.status() != RuntimeExecutionStatus::Succeeded {
        let mut errors = machine
            .end_deliveries()
            .iter()
            .filter(|delivery| delivery.target_port == "error")
            .flat_map(|delivery| &delivery.items)
            .map(|item| item.json.clone())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            let rows = sqlx::query("SELECT error_code,error_message,node_id,id FROM node_executions WHERE tenant_id=? AND execution_id=? AND error_code IS NOT NULL ORDER BY ended_at,id")
                .bind(tenant_id).bind(execution_id).fetch_all(&mut **tx).await?;
            errors.extend(rows.into_iter().map(|row| {
                json!({
                    "code":row.try_get::<String,_>("error_code").unwrap_or_else(|_| "WORKFLOW_FAILED".into()),
                    "message":row.try_get::<Option<String>,_>("error_message").ok().flatten().unwrap_or_else(|| "Workflow did not succeed".into()),
                    "sourceNodeId":row.try_get::<String,_>("node_id").ok(),
                    "nodeExecutionId":row.try_get::<Uuid,_>("id").ok()
                })
            }));
        }
        let primary = errors.first().cloned().unwrap_or_else(
            || json!({"code":"WORKFLOW_FAILED","message":"Workflow did not succeed"}),
        );
        let mut error_context = expression_context.clone();
        error_context.json = primary.clone();
        let mut error_outputs = Map::new();
        for (name, output) in &machine.workflow().end.error.outputs {
            let (value, mut conversions) = engine
                .resolve_dynamic_optional_with_conversions(
                    &output.value,
                    &error_context,
                    format!("end.error.outputs.{name}"),
                )
                .map_err(|error| RuntimeError::Deterministic {
                    code: "END_OUTPUT_EVALUATION_FAILED",
                    message: error.to_string(),
                })?;
            string_conversions.append(&mut conversions);
            let Some(value) = value else {
                if output.required {
                    return Err(RuntimeError::Deterministic {
                        code: "REQUIRED_END_ERROR_OUTPUT_OMITTED",
                        message: format!("required End error output {name} was omitted"),
                    });
                }
                continue;
            };
            if output.required && value.is_null() {
                return Err(runtime_bad_request(
                    "REQUIRED_END_ERROR_OUTPUT_NULL",
                    &format!("required End error output {name} resolved to null"),
                ));
            }
            jsonschema::validator_for(&output.schema)
                .map_err(|error| RuntimeError::Deterministic {
                    code: "END_OUTPUT_SCHEMA_INVALID",
                    message: error.to_string(),
                })?
                .validate(&value)
                .map_err(|error| RuntimeError::Deterministic {
                    code: "END_OUTPUT_SCHEMA_VALIDATION_FAILED",
                    message: error.to_string(),
                })?;
            error_outputs.insert(name.clone(), value);
        }
        let code = primary
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("WORKFLOW_FAILED");
        let message = primary
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Workflow did not succeed");
        return Ok((
            json!({}),
            Some(json!({
                "code":code,
                "message":message,
                "primaryError":primary,
                "errors":errors,
                "outputs":error_outputs
            })),
            string_conversions,
        ));
    }
    let mut result = Map::new();
    for (name, output) in &machine.workflow().end.outputs {
        let (value, mut conversions) = engine
            .resolve_dynamic_optional_with_conversions(
                &output.value,
                &expression_context,
                format!("end.outputs.{name}"),
            )
            .map_err(|error| RuntimeError::Deterministic {
                code: "END_OUTPUT_EVALUATION_FAILED",
                message: error.to_string(),
            })?;
        string_conversions.append(&mut conversions);
        let Some(value) = value else {
            if output.required {
                return Err(RuntimeError::Deterministic {
                    code: "REQUIRED_END_OUTPUT_OMITTED",
                    message: format!("required End output {name} was omitted"),
                });
            }
            continue;
        };
        if output.required && value.is_null() {
            return Err(runtime_bad_request(
                "REQUIRED_END_OUTPUT_NULL",
                &format!("required End output {name} resolved to null"),
            ));
        }
        jsonschema::validator_for(&output.schema)
            .map_err(|error| RuntimeError::Deterministic {
                code: "END_OUTPUT_SCHEMA_INVALID",
                message: error.to_string(),
            })?
            .validate(&value)
            .map_err(|error| RuntimeError::Deterministic {
                code: "END_OUTPUT_SCHEMA_VALIDATION_FAILED",
                message: error.to_string(),
            })?;
        result.insert(name.clone(), value);
    }
    if result.is_empty()
        && let Some(item) = machine
            .end_deliveries()
            .iter()
            .filter(|delivery| delivery.target_port == "main")
            .flat_map(|delivery| &delivery.items)
            .next()
    {
        return Ok((item.json.clone(), None, string_conversions));
    }
    Ok((Value::Object(result), None, string_conversions))
}

pub(super) async fn load_output_namespace(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> RuntimeResult<Value> {
    let rows = sqlx::query(
        "SELECT node_key,run_index,output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND output_json IS NOT NULL ORDER BY run_index,id",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut namespace = Map::new();
    for row in rows {
        merge_output_namespace(
            &mut namespace,
            &row.try_get::<String, _>("node_key")?,
            row.try_get("run_index")?,
            &row.try_get::<Value, _>("output_json")?,
        );
    }
    let deliveries = sqlx::query(
        "SELECT n.node_key,n.run_index,d.source_port,d.payload_json FROM execution_edge_deliveries d JOIN node_executions n ON n.tenant_id=d.tenant_id AND n.execution_id=d.execution_id AND n.id=d.source_node_execution_id WHERE d.tenant_id=? AND d.execution_id=? AND d.payload_json IS NOT NULL UNION ALL SELECT n.node_key,n.run_index,d.source_port,d.payload_json FROM execution_end_deliveries d JOIN node_executions n ON n.tenant_id=d.tenant_id AND n.execution_id=d.execution_id AND n.id=d.source_node_execution_id WHERE d.tenant_id=? AND d.execution_id=?",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_all(&mut **tx)
    .await?;
    for delivery in deliveries {
        let source_port: String = delivery.try_get("source_port")?;
        let payload: Value = delivery.try_get("payload_json")?;
        merge_output_namespace(
            &mut namespace,
            &delivery.try_get::<String, _>("node_key")?,
            delivery.try_get("run_index")?,
            &json!({source_port:payload}),
        );
    }
    Ok(Value::Object(namespace))
}

pub(super) fn merge_output_namespace(
    namespace: &mut Map<String, Value>,
    key: &str,
    run: u32,
    outputs: &Value,
) {
    let node = namespace
        .entry(key.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("node output namespace is an object");
    node.entry("runs")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("node runs namespace is an object")
        .insert(run.to_string(), outputs.clone());
    let Some(ports) = outputs.as_object() else {
        return;
    };
    for (port, value) in ports {
        let items = value.as_array().cloned().unwrap_or_default();
        let first = items.first().cloned().unwrap_or(Value::Null);
        let last = items.last().cloned().unwrap_or(Value::Null);
        node.insert(
            port.clone(),
            json!({"current":first,"first":first,"last":last,"all":items,"runIndex":run}),
        );
        if port == "main"
            && let Some(fields) = first.get("json").and_then(Value::as_object)
        {
            for (field, value) in fields {
                if !matches!(field.as_str(), "runs" | "main" | "error") {
                    node.insert(field.clone(), value.clone());
                }
            }
        }
    }
}

pub(super) fn single_port_output(port: &str, payload: Value) -> Value {
    let mut outputs = Map::new();
    outputs.insert(port.to_owned(), json!([{"json": payload}]));
    Value::Object(outputs)
}

pub(super) async fn authorize_snapshot(
    tx: &mut Transaction<'_, MySql>,
    authorization: &RuntimeAuthorizationSnapshotV1,
) -> RuntimeResult<()> {
    if authorization.maximum_policy_staleness_seconds
        > agentx_runtime_contracts::MAX_POLICY_STALENESS_SECONDS
    {
        return Err(runtime_bad_request(
            "POLICY_STALENESS_EXCEEDED",
            "Policy Last Known Good window exceeds 72 hours",
        ));
    }
    let identity_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM service_identity_projection i JOIN tenant_admission t ON t.tenant_id=i.tenant_id WHERE i.tenant_id=? AND i.identity_id=? AND i.workflow_id=? AND i.status='active' AND i.policy_epoch>=? AND t.status='active' AND i.updated_at>=DATE_SUB(UTC_TIMESTAMP(6),INTERVAL ? SECOND))",
    )
    .bind(authorization.tenant_id)
    .bind(authorization.service_identity_id)
    .bind(authorization.workflow_id)
    .bind(authorization.policy_epoch)
    .bind(authorization.maximum_policy_staleness_seconds)
    .fetch_one(&mut **tx)
    .await?;
    if !identity_ok {
        return Err(runtime_bad_request(
            "RUNTIME_AUTHORIZATION_STALE",
            "Runtime identity state is revoked, missing, or outside the Last Known Good window",
        ));
    }
    for grant_id in &authorization.grant_ids {
        let grant_ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM resource_grant_projection WHERE tenant_id=? AND grant_id=? AND subject_id=? AND status='active' AND policy_epoch>=?)",
        )
        .bind(authorization.tenant_id)
        .bind(grant_id)
        .bind(authorization.service_identity_id)
        .bind(authorization.policy_epoch)
        .fetch_one(&mut **tx)
        .await?;
        if !grant_ok {
            return Err(runtime_bad_request(
                "RUNTIME_GRANT_REVOKED",
                "Runtime grant is missing or revoked",
            ));
        }
    }
    Ok(())
}

pub(super) async fn authorize_resources(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    resources: &[RuntimeResourceBindingV1],
) -> RuntimeResult<()> {
    for resource in resources {
        let resource_kind = serde_json::to_value(resource.resource_kind)
            .map_err(|error| RuntimeError::Internal(error.into()))?
            .as_str()
            .ok_or_else(|| {
                RuntimeError::Internal(anyhow::anyhow!("resource kind is not a string"))
            })?
            .to_owned();
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runtime_resource_states WHERE tenant_id=? AND resource_kind=? AND resource_id=? AND resource_version=? AND state_epoch>=? AND status='active' AND content_hash=?)",
        )
        .bind(tenant_id)
        .bind(resource_kind)
        .bind(resource.resource_id)
        .bind(&resource.resource_version)
        .bind(resource.state_epoch)
        .bind(resource.content_hash.as_str())
        .fetch_one(&mut **tx)
        .await?;
        if !valid {
            return Err(runtime_bad_request(
                "RUNTIME_RESOURCE_DISABLED",
                "Runtime resource state is missing, stale, revoked, or does not match the immutable binding",
            ));
        }
    }
    Ok(())
}

pub(super) fn policy_timeout_from_snapshot(row: &sqlx::mysql::MySqlRow) -> RuntimeResult<u32> {
    let policy: agentx_runtime_contracts::RuntimePolicyV1 =
        serde_json::from_value(row.try_get("policy_snapshot_json")?)
            .map_err(|error| RuntimeError::Internal(error.into()))?;
    Ok(policy.timeout_seconds)
}

pub(super) async fn persist_lineage(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    delivery_id: Uuid,
    items: &[Item],
) -> RuntimeResult<()> {
    for (target_index, item) in items.iter().enumerate() {
        for source in &item.lineage {
            sqlx::query(
                "INSERT IGNORE INTO item_lineage(tenant_id,execution_id,delivery_id,target_item_index,source_node_execution_id,source_run_index,source_output_index,source_item_index) VALUES(?,?,?,?,?,?,?,?)",
            )
            .bind(tenant_id)
            .bind(execution_id)
            .bind(delivery_id)
            .bind(target_index as u32)
            .bind(source.node_execution_id.as_uuid())
            .bind(source.run_index)
            .bind(source.output_index)
            .bind(source.item_index)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn insert_invocation_event(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    invocation_id: Uuid,
    event_type: &str,
    payload: Value,
) -> RuntimeResult<()> {
    let next: u64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM invocation_events WHERE tenant_id=? AND invocation_id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(invocation_id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO invocation_events(tenant_id,invocation_id,event_id,sequence_number,event_type,payload_json) VALUES(?,?,?,?,?,?)",
    )
    .bind(tenant_id)
    .bind(invocation_id)
    .bind(Uuid::now_v7())
    .bind(next)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn initial_context_for_execution(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    workflow: &agentx_runtime::CompiledWorkflow,
) -> RuntimeResult<(Value, u64)> {
    let mut context = workflow
        .contexts
        .iter()
        .map(|(name, definition)| (name.clone(), definition.default.clone()))
        .collect::<Map<_, _>>();
    let session = sqlx::query("SELECT s.id,s.application_deployment_id FROM workflow_executions e JOIN application_invocations i ON i.tenant_id=e.tenant_id AND i.id=e.invocation_id JOIN application_sessions s ON s.tenant_id=i.tenant_id AND s.id=i.session_id WHERE e.tenant_id=? AND e.id=?")
        .bind(tenant_id).bind(execution_id).fetch_optional(&mut **tx).await?;
    let Some(session) = session else {
        return Ok((Value::Object(context), 0));
    };
    let session_id: Uuid = session.try_get("id")?;
    let deployment_id: Uuid = session.try_get("application_deployment_id")?;
    let session_defaults = Value::Object(
        workflow
            .contexts
            .iter()
            .filter(|(_, definition)| definition.scope == ContextScope::Session)
            .map(|(name, definition)| (name.clone(), definition.default.clone()))
            .collect(),
    );
    sqlx::query("INSERT IGNORE INTO application_session_contexts(tenant_id,application_deployment_id,session_id,context_json,context_version) VALUES(?,?,?,?,0)")
        .bind(tenant_id).bind(deployment_id).bind(session_id).bind(session_defaults)
        .execute(&mut **tx).await?;
    let stored = sqlx::query("SELECT context_json,context_version FROM application_session_contexts WHERE tenant_id=? AND application_deployment_id=? AND session_id=?")
        .bind(tenant_id).bind(deployment_id).bind(session_id).fetch_one(&mut **tx).await?;
    if let Some(values) = stored.try_get::<Value, _>("context_json")?.as_object() {
        context.extend(values.clone());
    }
    Ok((Value::Object(context), stored.try_get("context_version")?))
}
