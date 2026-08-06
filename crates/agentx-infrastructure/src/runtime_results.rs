use agentx_application::{ArtifactStore, ArtifactWrite, ExecutionResult, TerminalNodeResult};
use agentx_domain::TenantId;
use agentx_runtime::CompiledWorkflow;
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

const RESULT_SCHEMA_VERSION: &str = "1.0";

pub async fn materialize_execution_result(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> Result<(Value, String)> {
    let compiled_json: Value = sqlx::query_scalar(
        "SELECT compiled_ir_json FROM execution_snapshots WHERE tenant_id=? AND execution_id=?",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_one(&mut **transaction)
    .await?;
    let compiled: CompiledWorkflow =
        serde_json::from_value(compiled_json).context("Execution compiled IR is invalid")?;
    let terminal_nodes = compiled
        .nodes
        .iter()
        .filter(|node| node.outgoing_connections.is_empty())
        .map(|node| (node.id.clone(), node.index))
        .collect::<std::collections::BTreeMap<_, _>>();

    let rows = sqlx::query("SELECT id,node_id,generation,activation_slot,run_index,output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND status='succeeded' AND output_json IS NOT NULL ORDER BY run_index,generation,activation_slot,id")
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_all(&mut **transaction)
        .await?;
    let mut outputs = Vec::new();
    for row in rows {
        let node_id: String = row.try_get("node_id")?;
        let Some(&node_index) = terminal_nodes.get(&node_id) else {
            continue;
        };
        outputs.push(TerminalNodeResult {
            node_index,
            node_id,
            node_execution_id: row.try_get("id")?,
            activation_generation: row.try_get("generation")?,
            activation_slot: row.try_get("activation_slot")?,
            run_index: row.try_get("run_index")?,
            outputs: row.try_get("output_json")?,
        });
    }
    outputs.sort_by_key(|item| {
        (
            item.node_index,
            item.activation_generation,
            item.activation_slot,
            item.run_index,
            item.node_execution_id,
        )
    });
    let output_hash = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&outputs)?));
    let result = ExecutionResult {
        schema_version: RESULT_SCHEMA_VERSION.into(),
        terminal_nodes: outputs,
        output_hash: output_hash.clone(),
    };
    Ok((serde_json::to_value(result)?, output_hash))
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
            terminal_nodes: vec![TerminalNodeResult {
                node_index: 1,
                node_id: "output".into(),
                node_execution_id: Uuid::nil(),
                activation_generation: 2,
                activation_slot: 3,
                run_index: 4,
                outputs: serde_json::json!({"main": []}),
            }],
            output_hash: "sha256:test".into(),
        })
        .unwrap();
        assert_eq!(value["terminalNodes"][0]["activationSlot"], 3);
        assert_eq!(value["outputHash"], "sha256:test");
    }
}
