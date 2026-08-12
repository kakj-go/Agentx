use std::collections::BTreeSet;

use agentx_application::{ArtifactStore, ArtifactWrite, ExecutionResult};
use agentx_domain::TenantId;
use agentx_runtime::{CompiledWorkflow, ExpressionContext, ExpressionEngine};
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

const RESULT_SCHEMA_VERSION: &str = "4.0";

pub async fn materialize_execution_result(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> Result<(Value, String)> {
    let snapshot = sqlx::query(
        "SELECT compiled_ir_json,debug_plan_json FROM execution_snapshots WHERE tenant_id=? AND execution_id=?",
    )
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_one(&mut **transaction)
        .await?;
    let compiled_json: Value = snapshot.try_get("compiled_ir_json")?;
    let debug_plan: Value = snapshot.try_get("debug_plan_json")?;
    let compiled: CompiledWorkflow =
        serde_json::from_value(compiled_json).context("Execution compiled IR is invalid")?;
    let execution = sqlx::query("SELECT status,input_json,context_json FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_one(&mut **transaction)
        .await?;
    let inputs = execution
        .try_get::<Option<Value>, _>("input_json")?
        .unwrap_or(Value::Null);
    let contexts = execution.try_get::<Value, _>("context_json")?;
    let node_outputs =
        crate::runtime_context::load_output_namespace(transaction, tenant_id, execution_id).await?;
    let expression_context = ExpressionContext {
        inputs,
        outputs: node_outputs,
        contexts,
        execution: serde_json::json!({"id": execution_id}),
        ..ExpressionContext::default()
    };
    let engine = ExpressionEngine;
    let mut outputs = serde_json::Map::new();
    let mut error_result = None;
    let materialize_end_outputs = should_materialize_end_outputs(&debug_plan);
    let execution_status: String = execution.try_get("status")?;
    if execution_status == "failed" {
        let terminal_rows = sqlx::query("SELECT payload_json FROM execution_end_deliveries WHERE tenant_id=? AND execution_id=? AND target_port='error' ORDER BY sequence_number")
            .bind(tenant_id)
            .bind(execution_id)
            .fetch_all(&mut **transaction)
            .await?;
        let mut errors = Vec::new();
        let mut seen = BTreeSet::new();
        for row in terminal_rows {
            let payload: Value = row.try_get("payload_json")?;
            for error in payload
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("json"))
            {
                let identity = format!(
                    "{}:{}:{}:{}",
                    error
                        .get("nodeExecutionId")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    error.get("runIndex").and_then(Value::as_u64).unwrap_or(0),
                    error
                        .get("iterationIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    error
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                if seen.insert(identity) {
                    errors.push(error.clone());
                }
            }
        }
        if errors.is_empty() {
            let rows = sqlx::query("SELECT node_executions.id,node_executions.error_code,node_executions.error_message,node_executions.node_id,node_executions.node_key,node_executions.run_index,node_executions.iteration_index FROM node_executions WHERE node_executions.tenant_id=? AND node_executions.execution_id=? AND node_executions.status='failed' ORDER BY node_executions.ended_at,node_executions.id")
                .bind(tenant_id)
                .bind(execution_id)
                .fetch_all(&mut **transaction)
                .await?;
            errors = rows
                .into_iter()
                .map(|row| fallback_error_item(&row))
                .collect::<Result<Vec<Value>, sqlx::Error>>()?;
        }
        let primary = errors.first().cloned().unwrap_or_else(|| serde_json::json!({"code":"WORKFLOW_FAILED","message":"Workflow failed","details":{}}));
        let error_item = ExpressionContext {
            json: primary.clone(),
            ..expression_context.clone()
        };
        for (name, output) in &compiled.end.error.outputs {
            let value = engine
                .resolve_parameters(&Value::String(output.expression.clone()), &error_item)
                .with_context(|| format!("End error output '{name}' expression failed"))?;
            if output.required && value.is_null() {
                anyhow::bail!("Required End error output '{name}' resolved to null");
            }
            let validator = jsonschema::validator_for(&output.schema)
                .with_context(|| format!("End error output '{name}' schema is invalid"))?;
            validator.validate(&value).map_err(|error| {
                anyhow::anyhow!("End error output '{name}' does not match its schema: {error}")
            })?;
            outputs.insert(name.clone(), value);
        }
        error_result = Some(
            serde_json::json!({"primaryError": primary, "errors": errors, "outputs": outputs.clone()}),
        );
    } else if materialize_end_outputs {
        for (name, output) in &compiled.end.outputs {
            let value = engine
                .resolve_parameters(
                    &Value::String(output.expression.clone()),
                    &expression_context,
                )
                .with_context(|| {
                    let output_keys = expression_context
                        .outputs
                        .as_object()
                        .map(|value| value.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    format!(
                        "End output '{name}' expression failed: {} (available outputs: {:?})",
                        output.expression, output_keys
                    )
                })?;
            if output.required && value.is_null() {
                anyhow::bail!("Required End output '{name}' resolved to null");
            }
            let validator = jsonschema::validator_for(&output.schema)
                .with_context(|| format!("End output '{name}' schema is invalid"))?;
            validator.validate(&value).map_err(|error| {
                anyhow::anyhow!("End output '{name}' does not match its schema: {error}")
            })?;
            outputs.insert(name.clone(), value);
        }
    }
    // Partial debug runs intentionally do not expose formal End outputs: their
    // selected subgraph may not execute the nodes referenced by those outputs.
    let outputs = Value::Object(outputs);
    let output_hash = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&outputs)?));
    let result = ExecutionResult {
        schema_version: RESULT_SCHEMA_VERSION.into(),
        outputs,
        output_hash: output_hash.clone(),
        error: error_result,
    };
    Ok((serde_json::to_value(result)?, output_hash))
}

fn fallback_error_item(row: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    Ok(serde_json::json!({
        "code": row.try_get::<Option<String>, _>("error_code")?.unwrap_or_else(|| "WORKFLOW_FAILED".into()),
        "message": row.try_get::<Option<String>, _>("error_message")?.unwrap_or_else(|| "Workflow failed".into()),
        "details": {},
        "sourceNodeId": row.try_get::<String, _>("node_id")?,
        "sourceNodeKey": row.try_get::<String, _>("node_key")?,
        "nodeExecutionId": row.try_get::<Uuid, _>("id")?,
        "runIndex": row.try_get::<u32, _>("run_index")?,
        "iterationIndex": row.try_get::<u32, _>("iteration_index")?,
        "retryable": false
    }))
}

fn should_materialize_end_outputs(debug_plan: &Value) -> bool {
    !matches!(
        debug_plan.get("mode").and_then(Value::as_str),
        Some("single_node" | "to_node" | "from_node")
    )
}

pub async fn externalize_execution_results(
    pool: &MySqlPool,
    store: &dyn ArtifactStore,
    threshold_bytes: usize,
    limit: u32,
) -> Result<u64> {
    let rows = sqlx::query("SELECT id,tenant_id,result_hash,result_json FROM workflow_executions WHERE status='succeeded' AND result_json IS NOT NULL AND result_artifact_id IS NULL ORDER BY ended_at,id LIMIT ?")
        .bind(limit.clamp(1, 500))
        .fetch_all(pool)
        .await?;
    let mut externalized = 0;
    for row in rows {
        let execution_id: Uuid = row.try_get("id")?;
        let tenant_id: Uuid = row.try_get("tenant_id")?;
        let result_hash: String = row.try_get("result_hash")?;
        let result: Value = row.try_get("result_json")?;
        let bytes = serde_json::to_vec(&result)?;
        if bytes.len() <= threshold_bytes {
            continue;
        }
        let artifact = store
            .put(ArtifactWrite {
                tenant_id: TenantId::from_uuid(tenant_id),
                content_type: "application/vnd.agentx.execution-result+json".into(),
                content: bytes,
            })
            .await?;
        let mut transaction = pool.begin().await?;
        let changed = sqlx::query("UPDATE workflow_executions SET result_json=NULL,result_artifact_id=? WHERE tenant_id=? AND id=? AND result_hash=? AND result_json IS NOT NULL AND result_artifact_id IS NULL")
            .bind(artifact.id.as_uuid())
            .bind(tenant_id)
            .bind(execution_id)
            .bind(&result_hash)
            .execute(&mut *transaction)
            .await?;
        if changed.rows_affected() == 1 {
            sqlx::query("INSERT IGNORE INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'execution',?,'result')")
                .bind(tenant_id)
                .bind(artifact.id.as_uuid())
                .bind(execution_id.to_string())
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            externalized += 1;
        } else {
            transaction.rollback().await?;
            store
                .delete(TenantId::from_uuid(tenant_id), artifact.id)
                .await?;
        }
    }
    Ok(externalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_result_contract_is_camel_case() {
        let value = serde_json::to_value(ExecutionResult {
            schema_version: RESULT_SCHEMA_VERSION.into(),
            outputs: serde_json::json!({"answer": 42}),
            output_hash: "sha256:test".into(),
            error: None,
        })
        .unwrap();
        assert_eq!(value["outputs"]["answer"], 42);
        assert_eq!(value["outputHash"], "sha256:test");
    }

    #[test]
    fn partial_debug_runs_do_not_materialize_formal_end_outputs() {
        for mode in ["single_node", "to_node", "from_node"] {
            assert!(!should_materialize_end_outputs(
                &serde_json::json!({"mode": mode})
            ));
        }
        assert!(should_materialize_end_outputs(
            &serde_json::json!({"mode": "full"})
        ));
        assert!(should_materialize_end_outputs(&serde_json::json!({})));
    }
}
