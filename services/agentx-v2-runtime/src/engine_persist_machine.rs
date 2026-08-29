use agentx_runtime::{ActivationStatus, ExecutionMachine};
use serde_json::Value;
use sqlx::{MySql, Transaction};
use uuid::Uuid;

use crate::{
    engine_persistence::{persist_checkpoint, persist_lineage, upsert_activation},
    engine_protocol::lease_conflict,
    error::{RuntimeError, RuntimeResult},
};

pub(super) struct PersistMachineRequest<'a> {
    pub(super) tenant_id: Uuid,
    pub(super) execution_id: Uuid,
    pub(super) bundle_id: Uuid,
    pub(super) work_package_id: Option<Uuid>,
    pub(super) state_version: u64,
    pub(super) context_version: u64,
    pub(super) context: &'a Value,
    pub(super) machine: &'a ExecutionMachine,
    pub(super) checkpoint_type: &'a str,
}

pub(super) async fn persist_machine(
    tx: &mut Transaction<'_, MySql>,
    request: PersistMachineRequest<'_>,
) -> RuntimeResult<()> {
    let PersistMachineRequest {
        tenant_id,
        execution_id,
        bundle_id,
        work_package_id,
        state_version,
        context_version,
        context,
        machine,
        checkpoint_type,
    } = request;
    for activation in machine.activations() {
        let node = &machine.workflow().nodes[activation.node_index];
        upsert_activation(tx, tenant_id, execution_id, activation, node).await?;
    }
    for delivery in machine.deliveries() {
        let connection = &machine.workflow().connections[delivery.connection_index];
        let (kind, items) = match &delivery.kind {
            agentx_runtime::DeliveryKind::Data(items) => ("data", Some(items)),
            agentx_runtime::DeliveryKind::ClosedWithoutData => ("closed_without_data", None),
        };
        let inserted = sqlx::query(
            "INSERT IGNORE INTO execution_edge_deliveries(id,tenant_id,execution_id,sequence_number,connection_id,source_node_execution_id,source_port,target_node_id,target_port,target_generation,delivery_kind,item_count,payload_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(delivery.id)
        .bind(tenant_id)
        .bind(execution_id)
        .bind(delivery.sequence)
        .bind(&connection.id)
        .bind(delivery.source_node_execution_id.as_uuid())
        .bind(&connection.source_port)
        .bind(&machine.workflow().nodes[connection.target_node].id)
        .bind(&connection.target_port)
        .bind(delivery.target_generation)
        .bind(kind)
        .bind(items.map_or(0, Vec::len) as u32)
        .bind(items.map(serde_json::to_value).transpose().map_err(|error| RuntimeError::Internal(error.into()))?)
        .execute(&mut **tx)
        .await?
        .rows_affected()
            == 1;
        if inserted && let Some(items) = items {
            persist_lineage(tx, tenant_id, execution_id, delivery.id, items).await?;
        }
    }
    for delivery in machine.end_deliveries() {
        sqlx::query(
            "INSERT IGNORE INTO execution_end_deliveries(tenant_id,execution_id,sequence_number,source_node_execution_id,source_node_id,source_port,target_port,target_exit_id,payload_json) VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(tenant_id)
        .bind(execution_id)
        .bind(delivery.sequence)
        .bind(delivery.source_node_execution_id.as_uuid())
        .bind(&machine.workflow().nodes[delivery.source_node].id)
        .bind(&delivery.source_port)
        .bind(&delivery.target_port)
        .bind(&delivery.target_exit)
        .bind(serde_json::to_value(&delivery.items).map_err(|error| RuntimeError::Internal(error.into()))?)
        .execute(&mut **tx)
        .await?;
    }
    let machine_json =
        serde_json::to_value(machine).map_err(|error| RuntimeError::Internal(error.into()))?;
    let machine_hash = agentx_runtime_contracts::content_hash(&machine_json)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let frontier = machine
        .activations()
        .filter(|activation| {
            matches!(
                activation.status,
                ActivationStatus::Ready | ActivationStatus::Running | ActivationStatus::Waiting
            )
        })
        .map(|activation| activation.id.as_uuid())
        .collect::<Vec<_>>();
    let activation_count = machine.activations().count() as u64;
    let delivery_sequence = machine
        .deliveries()
        .iter()
        .map(|delivery| delivery.sequence)
        .chain(
            machine
                .end_deliveries()
                .iter()
                .map(|delivery| delivery.sequence),
        )
        .max()
        .unwrap_or(0);
    let frontier_json =
        serde_json::to_value(frontier).map_err(|error| RuntimeError::Internal(error.into()))?;
    let current_version: Option<u64> = sqlx::query_scalar(
        "SELECT state_version FROM execution_runtime_state WHERE tenant_id=? AND execution_id=? FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(execution_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(current_version) = current_version {
        if current_version.checked_add(1) != Some(state_version) {
            return Err(lease_conflict("Execution Runtime state CAS failed"));
        }
        let changed = sqlx::query(
            "UPDATE execution_runtime_state SET state_version=?,context_version=?,delivery_sequence=?,activation_count=?,activation_budget=?,current_frontier_json=?,context_json=?,machine_state_json=?,machine_state_hash=? WHERE tenant_id=? AND execution_id=? AND state_version=?",
        )
        .bind(state_version)
        .bind(context_version)
        .bind(delivery_sequence)
        .bind(activation_count)
        .bind(machine.workflow().activation_budget)
        .bind(&frontier_json)
        .bind(context)
        .bind(&machine_json)
        .bind(machine_hash.as_str())
        .bind(tenant_id)
        .bind(execution_id)
        .bind(current_version)
        .execute(&mut **tx)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(lease_conflict("Execution Runtime state CAS failed"));
        }
    } else {
        sqlx::query(
            "INSERT INTO execution_runtime_state(execution_id,tenant_id,state_version,context_version,delivery_sequence,activation_count,activation_budget,current_frontier_json,context_json,machine_state_json,machine_state_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(execution_id)
        .bind(tenant_id)
        .bind(state_version)
        .bind(context_version)
        .bind(delivery_sequence)
        .bind(activation_count)
        .bind(machine.workflow().activation_budget)
        .bind(&frontier_json)
        .bind(context)
        .bind(&machine_json)
        .bind(machine_hash.as_str())
        .execute(&mut **tx)
        .await?;
    }
    persist_checkpoint(
        tx,
        tenant_id,
        execution_id,
        bundle_id,
        work_package_id,
        state_version,
        None,
        machine,
        context,
        checkpoint_type,
    )
    .await?;
    Ok(())
}
