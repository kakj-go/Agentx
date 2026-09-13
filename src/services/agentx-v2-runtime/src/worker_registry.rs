use sqlx::MySqlPool;
use uuid::Uuid;

use crate::{engine_protocol::lease_conflict, error::RuntimeResult};

pub async fn register_worker(
    pool: &MySqlPool,
    worker_id: Uuid,
    capability: &str,
    compiler_version: &str,
) -> RuntimeResult<()> {
    sqlx::query(
        "INSERT INTO worker_capabilities(instance_id,capability,node_protocol_version,ir_schema_versions_json,compiler_version_min,compiler_version_max,manifest_hashes_json,status,heartbeat_at) VALUES(?,?,?,JSON_ARRAY(1),?,?,JSON_ARRAY(?),'ready',UTC_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE node_protocol_version=VALUES(node_protocol_version),ir_schema_versions_json=VALUES(ir_schema_versions_json),compiler_version_min=VALUES(compiler_version_min),compiler_version_max=VALUES(compiler_version_max),manifest_hashes_json=VALUES(manifest_hashes_json),status='ready',heartbeat_at=UTC_TIMESTAMP(6)",
    )
    .bind(worker_id.to_string())
    .bind(capability)
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .bind(compiler_version)
    .bind(compiler_version)
    .bind(agentx_node_protocol::NODE_PROTOCOL_VERSION)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn heartbeat_worker(
    pool: &MySqlPool,
    worker_id: Uuid,
    capability: &str,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE worker_capabilities SET heartbeat_at=UTC_TIMESTAMP(6) WHERE instance_id=? AND capability=? AND status='ready'",
    )
    .bind(worker_id.to_string())
    .bind(capability)
    .execute(pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(lease_conflict("Worker registration was lost"));
    }
    Ok(())
}

pub async fn mark_worker_draining(pool: &MySqlPool, worker_id: Uuid) -> RuntimeResult<()> {
    sqlx::query(
        "UPDATE worker_capabilities SET status='draining',heartbeat_at=UTC_TIMESTAMP(6) WHERE instance_id=? AND status='ready'",
    )
    .bind(worker_id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}
