use agentx_runtime::PartialExecutionMode;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::runtime_repository::{
    CreateExecution, CreatedExecution, ForkExecution, ResumeExecution, RuntimeExecutionSource,
    RuntimeRepository, insert_execution_event, persist_machine, queue_ready_attempts,
    sync_execution_status,
};

impl RuntimeRepository {
    pub async fn fork_execution(&self, command: ForkExecution) -> Result<CreatedExecution> {
        anyhow::ensure!(
            matches!(
                command.mode.as_str(),
                "whole" | "node" | "to_node" | "from_node"
            ),
            "INVALID_FORK_MODE"
        );
        if command.mode != "whole" {
            anyhow::ensure!(
                command
                    .node_id
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
                "FORK_NODE_REQUIRED"
            );
        }
        let row=sqlx::query("SELECT e.workflow_version_id,e.source_kind,e.source_id,e.source_revision,e.input_json,c.state_hash,s.resource_snapshot_json,s.debug_overlay_snapshot_json FROM workflow_executions e JOIN checkpoints c ON c.execution_id=e.id AND c.tenant_id=e.tenant_id JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=? AND c.id=?")
            .bind(command.tenant_id).bind(command.source_execution_id).bind(command.checkpoint_id)
            .fetch_optional(&self.pool).await?.context("Fork checkpoint was not found")?;
        let mut input: Value = row
            .try_get::<Option<Value>, _>("input_json")?
            .unwrap_or_else(|| json!({}));
        merge_json(&mut input, &command.input_overrides);
        let source_kind: String = row.try_get("source_kind")?;
        let source = match source_kind.as_str() {
            "version" => RuntimeExecutionSource::Version(row.try_get("source_id")?),
            "draft_revision" => RuntimeExecutionSource::DraftRevision {
                workflow_id: row.try_get("source_id")?,
                revision: row
                    .try_get::<Option<u64>, _>("source_revision")?
                    .context("Draft revision is missing")?,
            },
            value => anyhow::bail!("Unsupported execution source {value}"),
        };
        let source_machine = self
            .load_checkpoint_machine(command.tenant_id, command.checkpoint_id)
            .await
            .context("Fork checkpoint state is invalid")?;
        let mode = match command.mode.as_str() {
            "whole" => PartialExecutionMode::Whole,
            "node" => PartialExecutionMode::Node,
            "to_node" => PartialExecutionMode::ToNode,
            "from_node" => PartialExecutionMode::FromNode,
            _ => unreachable!("fork mode validated"),
        };
        let initial_machine = source_machine.fork_from_checkpoint(
            mode,
            command.node_id.as_deref(),
            crate::runtime_repository::invocation_items(&input),
            &command.input_overrides,
        )?;
        let source_state_hash: String = row.try_get("state_hash")?;
        let draft_resource_snapshots = if source_kind == "draft_revision" {
            row.try_get::<Value, _>("resource_snapshot_json")?
                .get("resources")
                .and_then(Value::as_array)
                .context("Draft execution resource snapshot is invalid")?
                .iter()
                .cloned()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let debug_overlay_snapshot: Value = row.try_get("debug_overlay_snapshot_json")?;
        let key = command
            .idempotency_key
            .as_ref()
            .map(|value| format!("fork:{}:{value}", command.source_execution_id));
        let created = self
            .create_execution(CreateExecution {
                tenant_id: command.tenant_id,
                source,
                invocation_id: None,
                session_id: None,
                requested_by: Some(command.actor_user_id),
                trigger_type: "fork".into(),
                input,
                idempotency_key: key,
                caller_execution_id: None,
                execution_type: command.mode.clone(),
                parent_execution_id: Some(command.source_execution_id),
                fork_checkpoint_id: Some(command.checkpoint_id),
                fork_mode: Some(command.mode.clone()),
                runtime_settings: json!({"mode":command.mode,"nodeId":command.node_id,"sourceExecutionId":command.source_execution_id,"checkpointId":command.checkpoint_id,"sourceStateHash":source_state_hash,"sideEffectDecisions":command.side_effect_decisions}),
                debug_plan: json!({"mode":command.mode,"nodeId":command.node_id,"sourceExecutionId":command.source_execution_id}),
                debug_overlay_snapshot,
                draft_resource_snapshots,
                initial_machine: Some(initial_machine),
            })
            .await?;
        if !created.replayed {
            let mut transaction = self.pool.begin().await?;
            sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,?,'execution.fork','execution',?,?,?)")
                .bind(Uuid::now_v7()).bind(command.tenant_id).bind(command.actor_user_id).bind(created.execution_id.to_string()).bind(Uuid::now_v7())
                .bind(json!({"sourceExecutionId":command.source_execution_id,"checkpointId":command.checkpoint_id,"mode":command.mode,"nodeId":command.node_id}))
                .execute(&mut *transaction).await?;
            transaction.commit().await?;
        }
        Ok(created)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn confirm_side_effect(
        &self,
        tenant_id: Uuid,
        execution_id: Uuid,
        node_execution_id: Uuid,
        checkpoint_id: Option<Uuid>,
        decision: &str,
        actor_user_id: Uuid,
        idempotency_key: &str,
    ) -> Result<bool> {
        anyhow::ensure!(
            matches!(decision, "execute" | "reuse_output" | "dry_run"),
            "INVALID_SIDE_EFFECT_DECISION"
        );
        let mut transaction = self.pool.begin().await?;
        let existing=sqlx::query("SELECT decision,idempotency_key FROM side_effect_confirmations WHERE tenant_id=? AND execution_id=? AND node_execution_id=? FOR UPDATE")
            .bind(tenant_id).bind(execution_id).bind(node_execution_id).fetch_optional(&mut *transaction).await?;
        if let Some(existing) = existing {
            let same = existing.try_get::<String, _>("decision")? == decision
                && existing.try_get::<String, _>("idempotency_key")? == idempotency_key;
            transaction.rollback().await?;
            anyhow::ensure!(same, "SIDE_EFFECT_DECISION_CONFLICT");
            return Ok(true);
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM node_executions n JOIN workflow_executions e ON e.id=n.execution_id AND e.tenant_id=n.tenant_id WHERE n.tenant_id=? AND n.execution_id=? AND n.id=? AND n.side_effect_level='irreversible' AND e.parent_execution_id IS NOT NULL)")
            .bind(tenant_id).bind(execution_id).bind(node_execution_id).fetch_one(&mut *transaction).await?;
        anyhow::ensure!(valid, "SIDE_EFFECT_CONFIRMATION_NOT_REQUIRED");
        sqlx::query("INSERT INTO side_effect_confirmations(id,tenant_id,execution_id,node_execution_id,checkpoint_id,actor_user_id,decision,idempotency_key,detail_json) VALUES(?,?,?,?,?,?,?,?,JSON_OBJECT())")
            .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(checkpoint_id).bind(actor_user_id).bind(decision).bind(idempotency_key)
            .execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,?,'execution.side_effect_confirm','node_execution',?,?,?)")
            .bind(Uuid::now_v7()).bind(tenant_id).bind(actor_user_id).bind(node_execution_id.to_string()).bind(Uuid::now_v7()).bind(json!({"decision":decision,"executionId":execution_id}))
            .execute(&mut *transaction).await?;
        let mut machine = self
            .load_machine(&mut transaction, tenant_id, execution_id)
            .await?;
        machine.resume_confirmation();
        queue_ready_attempts(&mut transaction, tenant_id, execution_id, &mut machine).await?;
        persist_machine(
            &mut transaction,
            tenant_id,
            execution_id,
            &machine,
            "manual",
        )
        .await?;
        sync_execution_status(
            &mut transaction,
            tenant_id,
            execution_id,
            machine.status(),
            None,
        )
        .await?;
        insert_execution_event(
            &mut transaction,
            tenant_id,
            execution_id,
            "execution.side_effect_confirmed",
            match machine.status() {
                agentx_runtime::RuntimeExecutionStatus::Waiting => "waiting",
                agentx_runtime::RuntimeExecutionStatus::Succeeded => "succeeded",
                agentx_runtime::RuntimeExecutionStatus::Failed => "failed",
                _ => "running",
            },
            json!({"nodeExecutionId":node_execution_id,"decision":decision}),
        )
        .await?;
        transaction.commit().await?;
        Ok(false)
    }

    pub async fn resume_due_waits(&self) -> Result<u64> {
        let rows=sqlx::query("SELECT w.id,w.tenant_id,w.execution_id,w.node_execution_id,w.wake_at,w.timeout_at FROM wait_subscriptions w WHERE w.status='waiting' AND ((w.wake_at IS NOT NULL AND w.wake_at<=CURRENT_TIMESTAMP(6)) OR (w.timeout_at IS NOT NULL AND w.timeout_at<=CURRENT_TIMESTAMP(6))) ORDER BY COALESCE(w.wake_at,w.timeout_at) LIMIT 100")
            .fetch_all(&self.pool).await?;
        let mut resumed = 0;
        for row in rows {
            let wait_id: Uuid = row.try_get("id")?;
            let wake_at: Option<time::OffsetDateTime> = row.try_get("wake_at")?;
            let timeout_at: Option<time::OffsetDateTime> = row.try_get("timeout_at")?;
            let timed_out = timeout_at
                .is_some_and(|timeout| timeout <= time::OffsetDateTime::now_utc())
                && wake_at.is_none_or(|wake| timeout_at <= Some(wake));
            let result = self
                .resume_execution(ResumeExecution {
                    tenant_id: row.try_get("tenant_id")?,
                    execution_id: row.try_get("execution_id")?,
                    node_execution_id: row.try_get("node_execution_id")?,
                    resume_token: wait_id.to_string(),
                    output_port: if timed_out { "timed_out" } else { "resumed" }.into(),
                    payload: json!({"timedOut":timed_out}),
                    idempotency_key: format!("timer:{wait_id}"),
                })
                .await;
            if result.is_ok() {
                resumed += 1;
            }
        }
        Ok(resumed)
    }
}

fn merge_json(target: &mut Value, overrides: &Value) {
    if let (Some(target), Some(overrides)) = (target.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            target.insert(key.clone(), value.clone());
        }
    } else if !overrides.is_null() {
        *target = overrides.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_are_shallow_and_explicit() {
        let mut value = json!({"a":1,"b":2});
        merge_json(&mut value, &json!({"b":3}));
        assert_eq!(value, json!({"a":1,"b":3}));
    }
}
