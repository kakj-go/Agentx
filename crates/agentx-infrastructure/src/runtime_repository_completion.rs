use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn complete_without_worker(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    machine: &mut ExecutionMachine,
    node_execution_id: NodeExecutionId,
    node: &agentx_runtime::CompiledNode,
    outputs: BTreeMap<String, Vec<Item>>,
    decision: &str,
) -> Result<()> {
    let attempt_id = machine.start_attempt(node_execution_id)?;
    let activation = machine
        .activation(node_execution_id)
        .expect("activation exists");
    upsert_activation(transaction, tenant_id, execution_id, activation, node).await?;
    let output = serde_json::to_value(&outputs)?;
    sqlx::query("INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,input_json,output_json,started_at,ended_at) VALUES(?,?,?,?,1,'succeeded',?,?,?,CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6))")
        .bind(attempt_id.as_uuid()).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
        .bind(format!("{execution_id}:{node_execution_id}:side_effect:{decision}"))
        .bind(serde_json::to_value(&activation.inputs)?).bind(&output).execute(&mut **transaction).await?;
    machine.complete(node_execution_id, outputs)?;
    sqlx::query(
        "UPDATE node_executions SET output_json=?,ended_at=CURRENT_TIMESTAMP(6) WHERE id=?",
    )
    .bind(output)
    .bind(node_execution_id.as_uuid())
    .execute(&mut **transaction)
    .await?;
    let (event_type, summary_key) = if matches!(
        decision,
        "pin_data" | "mock_output" | "history_output" | "artifact"
    ) {
        ("node.debug_overlay_applied", "overlayKind")
    } else {
        ("node.side_effect_resolved", "decision")
    };
    insert_execution_event(
        transaction,
        tenant_id,
        execution_id,
        event_type,
        "succeeded",
        json!({"nodeId":node.id,"nodeExecutionId":node_execution_id,(summary_key):decision}),
    )
    .await?;
    Ok(())
}

pub(super) fn debug_overlay_for_node<'a>(
    snapshot: &'a Value,
    node_id: &str,
) -> Option<(&'a str, &'a Value)> {
    snapshot
        .get("items")?
        .as_array()?
        .iter()
        .find(|item| item.get("nodeId").and_then(Value::as_str) == Some(node_id))
        .and_then(|item| Some((item.get("kind")?.as_str()?, item.get("payload")?)))
}

pub(super) fn overlay_items(payload: &Value) -> BTreeMap<String, Vec<Item>> {
    serde_json::from_value(payload.clone()).unwrap_or_else(|_| {
        BTreeMap::from([(
            "main".into(),
            vec![Item {
                json: payload.clone(),
                ..Item::default()
            }],
        )])
    })
}

pub(super) async fn upsert_activation(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    activation: &agentx_runtime::NodeActivation,
    node: &agentx_runtime::CompiledNode,
) -> Result<()> {
    let status = activation_status_name(activation.status);
    sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,iteration_index,status,capability,side_effect_level,input_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE status=VALUES(status),input_json=VALUES(input_json),updated_at=CURRENT_TIMESTAMP(6)")
        .bind(activation.id.as_uuid()).bind(tenant_id).bind(execution_id).bind(&node.id).bind(&node.key).bind(&node.name).bind(&node.node_type).bind(node.type_version)
        .bind(activation.generation).bind(activation.slot).bind(activation.run_index).bind(0_u32).bind(status)
        .bind(capability_name(&node.capability)).bind(side_effect_name(&node.side_effect_level)).bind(serde_json::to_value(&activation.inputs)?)
        .execute(&mut **transaction).await?;
    Ok(())
}

pub(crate) async fn persist_machine(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    machine: &ExecutionMachine,
    checkpoint_type: &str,
) -> Result<()> {
    for activation in machine.activations() {
        let node = &machine.workflow().nodes[activation.node_index];
        upsert_activation(transaction, tenant_id, execution_id, activation, node).await?;
        for attempt in &activation.attempts {
            let db_status = attempt_status_name(attempt.status);
            sqlx::query("UPDATE node_attempts SET status=IF(status='running' AND ?='queued','running',?),error_code=?,error_message=?,ended_at=IF(? IN ('succeeded','failed','cancelled'),CURRENT_TIMESTAMP(6),ended_at) WHERE id=?")
                .bind(db_status).bind(db_status).bind(&attempt.error_code).bind(&attempt.error_message).bind(db_status).bind(attempt.id.as_uuid())
                .execute(&mut **transaction).await?;
        }
    }
    for delivery in machine.deliveries() {
        let connection = &machine.workflow().connections[delivery.connection_index];
        let (kind, items) = match &delivery.kind {
            agentx_runtime::DeliveryKind::Data(items) => ("data", Some(items)),
            agentx_runtime::DeliveryKind::ClosedWithoutData => ("closed_without_data", None),
        };
        let inserted=sqlx::query("INSERT IGNORE INTO execution_edge_deliveries(id,tenant_id,execution_id,sequence_number,connection_id,source_node_execution_id,source_port,target_node_id,target_port,target_generation,delivery_kind,item_count,payload_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(delivery.id).bind(tenant_id).bind(execution_id).bind(delivery.sequence).bind(&connection.id)
            .bind(delivery.source_node_execution_id.as_uuid()).bind(&connection.source_port).bind(&machine.workflow().nodes[connection.target_node].id)
            .bind(&connection.target_port).bind(delivery.target_generation).bind(kind).bind(items.map_or(0,Vec::len) as u32).bind(items.map(serde_json::to_value).transpose()?)
            .execute(&mut **transaction).await?.rows_affected()==1;
        if inserted && let Some(items) = items {
            for (target_index, item) in items.iter().enumerate() {
                for source in &item.lineage {
                    sqlx::query("INSERT IGNORE INTO item_lineage(tenant_id,execution_id,delivery_id,target_item_index,source_node_execution_id,source_run_index,source_output_index,source_item_index) VALUES(?,?,?,?,?,?,?,?)")
                        .bind(tenant_id).bind(execution_id).bind(delivery.id).bind(target_index as u32).bind(source.node_execution_id.as_uuid())
                        .bind(source.run_index).bind(source.output_index).bind(source.item_index).execute(&mut **transaction).await?;
                }
            }
        }
    }
    for delivery in machine.end_deliveries() {
        sqlx::query("INSERT IGNORE INTO execution_end_deliveries(tenant_id,execution_id,sequence_number,source_node_execution_id,source_node_id,source_port,target_port,payload_json) VALUES(?,?,?,?,?,?,?,?)")
            .bind(tenant_id)
            .bind(execution_id)
            .bind(delivery.sequence)
            .bind(delivery.source_node_execution_id.as_uuid())
            .bind(&machine.workflow().nodes[delivery.source_node].id)
            .bind(&delivery.source_port)
            .bind(&delivery.target_port)
            .bind(serde_json::to_value(&delivery.items)?)
            .execute(&mut **transaction)
            .await?;
    }
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM checkpoints WHERE execution_id=?")
        .bind(execution_id).fetch_one(&mut **transaction).await?;
    let payload = serde_json::to_value(machine)?;
    let hash = hash_json(&payload)?;
    sqlx::query("INSERT INTO checkpoints(id,tenant_id,execution_id,node_execution_id,sequence_number,checkpoint_type,state_hash,payload_json) VALUES(?,?,?,NULL,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(sequence).bind(checkpoint_type).bind(hash).bind(payload)
        .execute(&mut **transaction).await?;
    Ok(())
}

#[cfg(test)]
#[path = "runtime_repository_tests.rs"]
mod tests;
