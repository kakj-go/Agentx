use std::{collections::BTreeMap, sync::Arc};

use agentx_application::{ArtifactStore, ArtifactWrite};
use agentx_domain::{
    ArtifactId, ExecutionOrder, NodeExecutionId, ResourceReference, TenantId, WorkflowDefinition,
};
use agentx_node_protocol::{Item, NodeCapability, SideEffectLevel};
use agentx_runtime::{
    AttemptStatus, CompileContext, CompiledWorkflow, ExecutionMachine, MachineError, NodeRegistry,
    RuntimeExecutionStatus, WorkflowCompiler,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct RuntimeRepository {
    pub(crate) pool: MySqlPool,
    checkpoint_artifacts: Option<Arc<dyn ArtifactStore>>,
    checkpoint_artifact_threshold: usize,
}

#[derive(Clone, Debug)]
pub struct CreateExecution {
    pub tenant_id: Uuid,
    pub workflow_version_id: Uuid,
    pub invocation_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub requested_by: Option<Uuid>,
    pub trigger_type: String,
    pub input: Value,
    pub idempotency_key: Option<String>,
    pub caller_execution_id: Option<Uuid>,
    pub execution_type: String,
    pub parent_execution_id: Option<Uuid>,
    pub fork_checkpoint_id: Option<Uuid>,
    pub fork_mode: Option<String>,
    pub runtime_settings: Value,
    pub initial_machine: Option<ExecutionMachine>,
}

#[derive(Clone, Debug)]
pub struct CreatedExecution {
    pub execution_id: Uuid,
    pub status: String,
    pub replayed: bool,
}

#[derive(Clone, Debug)]
pub struct ForkExecution {
    pub tenant_id: Uuid,
    pub source_execution_id: Uuid,
    pub checkpoint_id: Uuid,
    pub mode: String,
    pub node_id: Option<String>,
    pub input_overrides: Value,
    pub side_effect_decisions: Value,
    pub actor_user_id: Uuid,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ResumeExecution {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub resume_token: String,
    pub output_port: String,
    pub payload: Value,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchMessage {
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub capability: String,
}

#[derive(Clone, Debug)]
pub struct ClaimedTask {
    pub lease_token: Uuid,
    pub task: RuntimeTask,
}

#[derive(Clone, Debug)]
pub struct RuntimeTask {
    pub tenant_id: Uuid,
    pub workflow_version_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub attempt_number: u16,
    pub node_type: String,
    pub node_version: u32,
    pub node_parameters: Value,
    pub inputs: BTreeMap<String, Vec<Item>>,
    pub run_index: u32,
    pub iteration_index: u32,
    pub capability: String,
    pub idempotency_key: String,
    pub deadline: OffsetDateTime,
    pub mode: String,
    pub trace_id: Uuid,
    pub linked_nodes: Value,
    pub resource_references: Vec<ResourceReference>,
}

#[derive(Clone, Debug)]
pub enum TaskResult {
    Completed(BTreeMap<String, Vec<Item>>),
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
    Suspended(Value),
}

#[derive(Clone, Debug)]
pub struct OutboxDelivery {
    pub id: Uuid,
    pub capability: String,
    pub payload: DispatchMessage,
    pub lease_id: Uuid,
}

impl RuntimeRepository {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self {
            pool,
            checkpoint_artifacts: None,
            checkpoint_artifact_threshold: 64 * 1024,
        }
    }

    #[must_use]
    pub fn with_checkpoint_artifacts(
        mut self,
        artifacts: Arc<dyn ArtifactStore>,
        threshold_bytes: usize,
    ) -> Self {
        self.checkpoint_artifacts = Some(artifacts);
        self.checkpoint_artifact_threshold = threshold_bytes.clamp(1024, 16 * 1024 * 1024);
        self
    }

    #[must_use]
    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    pub async fn create_execution(&self, mut command: CreateExecution) -> Result<CreatedExecution> {
        let request_hash = hash_json(&json!({
            "workflowVersionId": command.workflow_version_id,
            "invocationId": command.invocation_id,
            "input": command.input,
            "triggerType": command.trigger_type,
            "executionType": command.execution_type,
            "parentExecutionId": command.parent_execution_id,
            "forkCheckpointId": command.fork_checkpoint_id,
            "forkMode": command.fork_mode,
        }))?;
        let mut transaction = self.pool.begin().await?;
        if let Some(key) = &command.idempotency_key {
            if let Some(row) = sqlx::query("SELECT request_hash,response_json FROM runtime_idempotency_keys WHERE tenant_id=? AND scope='request_execution' AND idempotency_key=? FOR UPDATE")
                .bind(command.tenant_id).bind(key).fetch_optional(&mut *transaction).await?
            {
                let stored_hash: String = row.try_get("request_hash")?;
                anyhow::ensure!(stored_hash == request_hash, "IDEMPOTENCY_KEY_REUSED");
                let response: Value = row.try_get("response_json")?;
                return Ok(CreatedExecution {
                    execution_id: parse_uuid(&response, "executionId")?,
                    status: response.get("status").and_then(Value::as_str).unwrap_or("queued").into(),
                    replayed: true,
                });
            }
            sqlx::query("INSERT INTO runtime_idempotency_keys(tenant_id,scope,idempotency_key,request_hash,status,expires_at) VALUES(?,'request_execution',?,?,'processing',DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 1 DAY))")
                .bind(command.tenant_id).bind(key).bind(&request_hash).execute(&mut *transaction).await?;
        }

        let version = sqlx::query("SELECT workflow_id,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version FROM workflow_versions WHERE tenant_id=? AND id=? FOR UPDATE")
            .bind(command.tenant_id).bind(command.workflow_version_id).fetch_optional(&mut *transaction).await?
            .context("Workflow Version was not found")?;
        let workflow_id: Uuid = version.try_get("workflow_id")?;
        let definition_value: Value = version.try_get("definition_json")?;
        let definition: WorkflowDefinition = serde_json::from_value(definition_value.clone())
            .context("Workflow Definition is invalid")?;
        let compiled = match version.try_get::<Option<Value>, _>("compiled_ir_json")? {
            Some(value) => {
                serde_json::from_value(value).context("Compiled Workflow IR is invalid")?
            }
            None => {
                let registry = NodeRegistry::m4_defaults();
                let compiler = WorkflowCompiler::new(&registry);
                let compiled = compiler
                    .compile(
                        &definition,
                        &CompileContext {
                            current_workflow_version_id: Some(
                                command.workflow_version_id.to_string(),
                            ),
                            ancestor_workflow_version_ids: Default::default(),
                        },
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            serde_json::to_string(&error.issues)
                                .unwrap_or_else(|_| error.to_string())
                        )
                    })?;
                sqlx::query("UPDATE workflow_versions SET compiled_ir_json=?,compiled_ir_hash=?,compiler_version=?,compiled_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND compiled_ir_json IS NULL")
                    .bind(serde_json::to_value(&compiled)?).bind(&compiled.canonical_hash).bind(&compiled.compiler_version)
                    .bind(command.tenant_id).bind(command.workflow_version_id).execute(&mut *transaction).await?;
                compiled
            }
        };

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,invocation_id,session_id,parent_execution_id,caller_execution_id,fork_checkpoint_id,trace_id,trigger_type,execution_type,fork_mode,requested_by,input_json,status,started_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'queued',CURRENT_TIMESTAMP(6))")
            .bind(execution_id).bind(command.tenant_id).bind(workflow_id).bind(command.workflow_version_id)
            .bind(command.invocation_id).bind(command.session_id).bind(command.parent_execution_id).bind(command.caller_execution_id).bind(command.fork_checkpoint_id)
            .bind(trace_id).bind(&command.trigger_type).bind(&command.execution_type).bind(&command.fork_mode)
            .bind(command.requested_by).bind(&command.input).execute(&mut *transaction).await?;
        let items = invocation_items(&command.input);
        let mut machine = command
            .initial_machine
            .take()
            .map_or_else(|| ExecutionMachine::new(compiled.clone(), items), Ok)?;
        let snapshot_hash = hash_json(&json!({
            "definition": definition_value,
            "compiledIrHash": machine.workflow().canonical_hash,
            "workflowVersionId": command.workflow_version_id,
        }))?;
        sqlx::query("INSERT INTO execution_snapshots(execution_id,tenant_id,workflow_version_id,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,resource_snapshot_json,runtime_settings_json,state_hash) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(execution_id).bind(command.tenant_id).bind(command.workflow_version_id).bind(&definition_value)
            .bind(serde_json::to_value(machine.workflow())?).bind(&machine.workflow().canonical_hash).bind(&machine.workflow().compiler_version)
            .bind(json!({})).bind(&command.runtime_settings).bind(snapshot_hash).execute(&mut *transaction).await?;
        queue_ready_attempts(
            &mut transaction,
            command.tenant_id,
            execution_id,
            &mut machine,
        )
        .await?;
        persist_machine(
            &mut transaction,
            command.tenant_id,
            execution_id,
            &machine,
            "execution_start",
        )
        .await?;
        let created_status = execution_status_name(machine.status());
        insert_execution_event(
            &mut transaction,
            command.tenant_id,
            execution_id,
            "execution.started",
            created_status,
            json!({"triggerType":command.trigger_type}),
        )
        .await?;
        insert_trace(
            &mut transaction,
            TraceInsert {
                tenant_id: command.tenant_id,
                execution_id,
                workflow_id,
                workflow_version_id: command.workflow_version_id,
                trace_id,
                node_execution_id: None,
                node_id: None,
                event_type: "execution.started",
                status: created_status,
                run_index: 0,
                attributes: json!({"triggerType":command.trigger_type}),
                error_code: None,
                error_message: None,
            },
        )
        .await?;
        sync_execution_status(
            &mut transaction,
            command.tenant_id,
            execution_id,
            machine.status(),
        )
        .await?;
        if let Some(key) = &command.idempotency_key {
            sqlx::query("UPDATE runtime_idempotency_keys SET status='completed',response_json=? WHERE tenant_id=? AND scope='request_execution' AND idempotency_key=?")
                .bind(json!({"executionId":execution_id,"status":created_status})).bind(command.tenant_id).bind(key)
                .execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        Ok(CreatedExecution {
            execution_id,
            status: created_status.into(),
            replayed: false,
        })
    }

    pub async fn claim_task(
        &self,
        message: &DispatchMessage,
        worker_instance_id: &str,
        lease_seconds: u64,
    ) -> Result<Option<ClaimedTask>> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT a.status attempt_status,n.status node_status,e.cancellation_requested_at,e.status execution_status FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.id=? AND a.node_execution_id=? FOR UPDATE")
            .bind(message.tenant_id).bind(message.attempt_id).bind(message.node_execution_id)
            .fetch_optional(&mut *transaction).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let attempt_status: String = row.try_get("attempt_status")?;
        let execution_status: String = row.try_get("execution_status")?;
        if attempt_status != "queued"
            || row
                .try_get::<Option<OffsetDateTime>, _>("cancellation_requested_at")?
                .is_some()
            || matches!(
                execution_status.as_str(),
                "cancelled" | "failed" | "timed_out" | "succeeded"
            )
        {
            transaction.rollback().await?;
            return Ok(None);
        }
        let lease_token = Uuid::now_v7();
        let lease_seconds = lease_seconds.clamp(5, 300);
        sqlx::query("INSERT INTO worker_leases(node_attempt_id,tenant_id,node_execution_id,lease_token,worker_instance_id,capability,acquired_at,heartbeat_at,expires_at) VALUES(?,?,?,?,?,?,CURRENT_TIMESTAMP(6),CURRENT_TIMESTAMP(6),DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND))")
            .bind(message.attempt_id).bind(message.tenant_id).bind(message.node_execution_id).bind(lease_token)
            .bind(worker_instance_id).bind(&message.capability).bind(lease_seconds).execute(&mut *transaction).await?;
        sqlx::query("UPDATE node_attempts SET status='running',lease_token=?,worker_instance_id=?,started_at=COALESCE(started_at,CURRENT_TIMESTAMP(6)) WHERE id=? AND status='queued'")
            .bind(lease_token).bind(worker_instance_id).bind(message.attempt_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE node_executions SET status='running',started_at=COALESCE(started_at,CURRENT_TIMESTAMP(6)) WHERE id=? AND status='queued'")
            .bind(message.node_execution_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        let task = self.load_task(message).await?;
        Ok(Some(ClaimedTask { lease_token, task }))
    }

    pub async fn heartbeat(
        &self,
        tenant_id: Uuid,
        attempt_id: Uuid,
        lease_token: Uuid,
        worker: &str,
        lease_seconds: u64,
    ) -> Result<(bool, bool)> {
        let result = sqlx::query("UPDATE worker_leases SET heartbeat_at=CURRENT_TIMESTAMP(6),expires_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND) WHERE tenant_id=? AND node_attempt_id=? AND lease_token=? AND worker_instance_id=? AND released_at IS NULL AND expires_at>CURRENT_TIMESTAMP(6)")
            .bind(lease_seconds.clamp(5,300)).bind(tenant_id).bind(attempt_id).bind(lease_token).bind(worker)
            .execute(&self.pool).await?;
        let cancelled = sqlx::query_scalar::<_, bool>("SELECT e.cancellation_requested_at IS NOT NULL OR e.status='cancelled' FROM node_attempts a JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.id=?")
            .bind(tenant_id).bind(attempt_id).fetch_optional(&self.pool).await?.unwrap_or(true);
        Ok((result.rows_affected() == 1, cancelled))
    }

    pub async fn report_task(
        &self,
        tenant_id: Uuid,
        execution_id: Uuid,
        node_execution_id: Uuid,
        attempt_id: Uuid,
        lease_token: Uuid,
        result: TaskResult,
    ) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let row=sqlx::query("SELECT a.status,a.lease_token,n.node_id,n.run_index,e.workflow_id,e.workflow_version_id,e.trace_id,e.status execution_status FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.execution_id=? AND a.node_execution_id=? AND a.id=? FOR UPDATE")
            .bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(attempt_id).fetch_optional(&mut *transaction).await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let stored_lease: Option<Uuid> = row.try_get("lease_token")?;
        let status: String = row.try_get("status")?;
        if stored_lease != Some(lease_token) || status != "running" {
            transaction.rollback().await?;
            return Ok(false);
        }
        let mut machine = self
            .load_machine(&mut transaction, tenant_id, execution_id)
            .await?;
        let domain_node = NodeExecutionId::from_uuid(node_execution_id);
        let transition = match &result {
            TaskResult::Completed(outputs) => machine.complete(domain_node, outputs.clone()),
            TaskResult::Failed {
                code,
                message,
                retryable,
            } => machine.fail(domain_node, code, message, *retryable),
            TaskResult::Suspended(_) => machine.suspend(domain_node),
        };
        let transition_error = match transition {
            Ok(()) => None,
            Err(MachineError::ActivationBudgetExceeded(budget)) => Some((
                "ACTIVATION_BUDGET_EXCEEDED",
                format!("Workflow activation budget {budget} was exhausted"),
            )),
            Err(error) => return Err(error.into()),
        };
        match &result {
            TaskResult::Completed(outputs) => {
                let value = serde_json::to_value(outputs)?;
                sqlx::query("UPDATE node_attempts SET output_json=? WHERE id=?")
                    .bind(&value)
                    .bind(attempt_id)
                    .execute(&mut *transaction)
                    .await?;
                sqlx::query("UPDATE node_executions SET output_json=? WHERE id=?")
                    .bind(value)
                    .bind(node_execution_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            TaskResult::Failed { code, message, .. } => {
                sqlx::query("UPDATE node_attempts SET error_code=?,error_message=? WHERE id=?")
                    .bind(code)
                    .bind(message)
                    .bind(attempt_id)
                    .execute(&mut *transaction)
                    .await?;
                sqlx::query("UPDATE node_executions SET error_code=?,error_message=? WHERE id=?")
                    .bind(code)
                    .bind(message)
                    .bind(node_execution_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            TaskResult::Suspended(contract) => {
                sqlx::query("UPDATE node_attempts SET output_json=? WHERE id=?")
                    .bind(contract)
                    .bind(attempt_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        sqlx::query("UPDATE worker_leases SET released_at=CURRENT_TIMESTAMP(6) WHERE node_attempt_id=? AND lease_token=? AND released_at IS NULL")
            .bind(attempt_id).bind(lease_token).execute(&mut *transaction).await?;
        if let TaskResult::Suspended(contract) = &result {
            create_wait(
                &mut transaction,
                tenant_id,
                execution_id,
                node_execution_id,
                contract,
            )
            .await?;
        }
        queue_ready_attempts(&mut transaction, tenant_id, execution_id, &mut machine).await?;
        persist_machine(
            &mut transaction,
            tenant_id,
            execution_id,
            &machine,
            checkpoint_type(&result),
        )
        .await?;
        let node_id: String = row.try_get("node_id")?;
        let run_index: u32 = row.try_get("run_index")?;
        let workflow_id: Uuid = row.try_get("workflow_id")?;
        let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
        let trace_id: Uuid = row.try_get("trace_id")?;
        let (event_type, event_status, error_code, error_message) = match &result {
            TaskResult::Completed(_) => ("node.completed", "succeeded", None, None),
            TaskResult::Failed { code, message, .. } => (
                "node.failed",
                "failed",
                Some(code.clone()),
                Some(message.clone()),
            ),
            TaskResult::Suspended(_) => ("node.suspended", "waiting", None, None),
        };
        insert_execution_event(
            &mut transaction,
            tenant_id,
            execution_id,
            event_type,
            event_status,
            json!({"nodeId":node_id,"nodeExecutionId":node_execution_id}),
        )
        .await?;
        insert_trace(
            &mut transaction,
            TraceInsert {
                tenant_id,
                execution_id,
                workflow_id,
                workflow_version_id,
                trace_id,
                node_execution_id: Some(node_execution_id),
                node_id: Some(&node_id),
                event_type,
                status: event_status,
                run_index,
                attributes: json!({}),
                error_code,
                error_message,
            },
        )
        .await?;
        sync_execution_status(&mut transaction, tenant_id, execution_id, machine.status()).await?;
        if let Some((code, message)) = transition_error {
            sqlx::query("UPDATE workflow_executions SET error_code=?,error_message=? WHERE tenant_id=? AND id=?")
                .bind(code).bind(&message).bind(tenant_id).bind(execution_id)
                .execute(&mut *transaction).await?;
            insert_execution_event(
                &mut transaction,
                tenant_id,
                execution_id,
                "execution.activation_budget_exceeded",
                "failed",
                json!({"nodeId":node_id,"nodeExecutionId":node_execution_id,"code":code,"message":message}),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn cancel_execution(&self, tenant_id: Uuid, execution_id: Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let current = sqlx::query_scalar::<_, String>(
            "SELECT status FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(current) = current else {
            return Ok(false);
        };
        if matches!(
            current.as_str(),
            "succeeded" | "failed" | "cancelled" | "timed_out"
        ) {
            transaction.rollback().await?;
            return Ok(true);
        }
        let mut machine = self
            .load_machine(&mut transaction, tenant_id, execution_id)
            .await?;
        machine.cancel();
        persist_machine(
            &mut transaction,
            tenant_id,
            execution_id,
            &machine,
            "manual",
        )
        .await?;
        sqlx::query("UPDATE workflow_executions SET status='cancelled',cancellation_requested_at=CURRENT_TIMESTAMP(6),ended_at=CURRENT_TIMESTAMP(6),duration_ms=TIMESTAMPDIFF(MICROSECOND,started_at,CURRENT_TIMESTAMP(6))/1000 WHERE tenant_id=? AND id=?")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE execution_resume_tokens SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='active'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE wait_subscriptions SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='waiting'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE resume_webhook_bindings b JOIN wait_subscriptions w ON w.id=b.wait_subscription_id AND w.tenant_id=b.tenant_id SET b.status='cancelled' WHERE w.tenant_id=? AND w.execution_id=? AND b.status='active'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE approval_tasks SET status='cancelled',resume_status='succeeded',version=version+1 WHERE tenant_id=? AND execution_id=? AND status IN ('pending','claimed')")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        insert_execution_event(
            &mut transaction,
            tenant_id,
            execution_id,
            "execution.cancelled",
            "cancelled",
            json!({}),
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn resume_execution(&self, command: ResumeExecution) -> Result<bool> {
        let token_hash = format!("{:x}", Sha256::digest(command.resume_token.as_bytes()));
        let timed_out = command.output_port == "timed_out";
        let mut transaction = self.pool.begin().await?;
        let token=sqlx::query("SELECT id,status,expires_at FROM execution_resume_tokens WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND token_hash=? FOR UPDATE")
            .bind(command.tenant_id).bind(command.execution_id).bind(command.node_execution_id).bind(token_hash).fetch_optional(&mut *transaction).await?;
        let Some(token) = token else {
            anyhow::bail!("RESUME_TOKEN_INVALID");
        };
        let status: String = token.try_get("status")?;
        match status.as_str() {
            "used" => {
                transaction.rollback().await?;
                return Ok(true);
            }
            "expired" if timed_out => {
                transaction.rollback().await?;
                return Ok(true);
            }
            "expired" => {
                transaction.rollback().await?;
                anyhow::bail!("RESUME_TOKEN_EXPIRED");
            }
            "active" => {}
            _ => {
                transaction.rollback().await?;
                anyhow::bail!("RESUME_TOKEN_INACTIVE");
            }
        }
        if !timed_out
            && token
                .try_get::<Option<OffsetDateTime>, _>("expires_at")?
                .is_some_and(|value| value <= OffsetDateTime::now_utc())
        {
            transaction.rollback().await?;
            anyhow::bail!("RESUME_TOKEN_EXPIRED");
        }
        let token_id: Uuid = token.try_get("id")?;
        let mut machine = self
            .load_machine(&mut transaction, command.tenant_id, command.execution_id)
            .await?;
        machine.resume(
            NodeExecutionId::from_uuid(command.node_execution_id),
            &command.output_port,
            invocation_items(&command.payload),
        )?;
        queue_ready_attempts(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            &mut machine,
        )
        .await?;
        persist_machine(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            &machine,
            "node_completed",
        )
        .await?;
        sqlx::query("UPDATE execution_resume_tokens SET status=IF(?,'expired','used'),used_at=CURRENT_TIMESTAMP(6),idempotency_key=? WHERE id=? AND status='active'")
            .bind(timed_out).bind(&command.idempotency_key).bind(token_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE wait_subscriptions SET status=IF(?,'timed_out','resumed'),resumed_at=CURRENT_TIMESTAMP(6) WHERE resume_token_id=? AND status='waiting'")
            .bind(timed_out).bind(token_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE resume_webhook_bindings b JOIN wait_subscriptions w ON w.id=b.wait_subscription_id SET b.status=IF(?,'expired','used') WHERE w.resume_token_id=? AND b.status='active'")
            .bind(timed_out).bind(token_id).execute(&mut *transaction).await?;
        if timed_out {
            sqlx::query("UPDATE approval_tasks SET status='timed_out',resume_status='succeeded',version=version+1 WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND status IN ('pending','claimed')")
                .bind(command.tenant_id).bind(command.execution_id).bind(command.node_execution_id).execute(&mut *transaction).await?;
        }
        sync_execution_status(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            machine.status(),
        )
        .await?;
        insert_execution_event(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            "execution.resumed",
            "running",
            json!({"nodeExecutionId":command.node_execution_id,"outputPort":command.output_port}),
        )
        .await?;
        transaction.commit().await?;
        Ok(false)
    }

    pub async fn claim_outbox(
        &self,
        limit: u32,
        lease_seconds: u64,
    ) -> Result<Vec<OutboxDelivery>> {
        let lease_id = Uuid::now_v7();
        let mut transaction = self.pool.begin().await?;
        let rows=sqlx::query("SELECT id FROM execution_outbox WHERE status='pending' AND available_at<=CURRENT_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6)) ORDER BY created_at,id LIMIT ? FOR UPDATE SKIP LOCKED")
            .bind(limit.clamp(1,500)).fetch_all(&mut *transaction).await?;
        for row in rows {
            let id: Uuid = row.try_get("id")?;
            sqlx::query("UPDATE execution_outbox SET locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),attempt_count=attempt_count+1 WHERE id=? AND status='pending'")
                .bind(lease_id).bind(lease_seconds.clamp(5,300)).bind(id).execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        let rows=sqlx::query("SELECT id,capability,payload_json FROM execution_outbox WHERE locked_by=? AND status='pending' ORDER BY created_at,id")
            .bind(lease_id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(OutboxDelivery {
                    id: row.try_get("id")?,
                    capability: row.try_get("capability")?,
                    payload: serde_json::from_value(row.try_get("payload_json")?)?,
                    lease_id,
                })
            })
            .collect()
    }

    pub async fn mark_outbox_published(&self, delivery: &OutboxDelivery) -> Result<bool> {
        Ok(sqlx::query("UPDATE execution_outbox SET status='published',published_at=CURRENT_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,last_error=NULL WHERE id=? AND locked_by=? AND status='pending'")
            .bind(delivery.id).bind(delivery.lease_id).execute(&self.pool).await?.rows_affected() == 1)
    }

    pub async fn mark_outbox_failed(&self, delivery: &OutboxDelivery, error: &str) -> Result<()> {
        sqlx::query("UPDATE execution_outbox SET available_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 2 SECOND),locked_by=NULL,locked_until=NULL,last_error=? WHERE id=? AND locked_by=? AND status='pending'")
            .bind(error.chars().take(1000).collect::<String>()).bind(delivery.id).bind(delivery.lease_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn reap_expired_leases(&self) -> Result<u64> {
        let rows=sqlx::query("SELECT l.tenant_id,a.execution_id,l.node_execution_id,l.node_attempt_id,l.lease_token FROM worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id WHERE l.released_at IS NULL AND l.expires_at<CURRENT_TIMESTAMP(6) ORDER BY l.expires_at LIMIT 100")
            .fetch_all(&self.pool).await?;
        let mut recovered = 0;
        for row in rows {
            let accepted = self
                .report_task(
                    row.try_get("tenant_id")?,
                    row.try_get("execution_id")?,
                    row.try_get("node_execution_id")?,
                    row.try_get("node_attempt_id")?,
                    row.try_get("lease_token")?,
                    TaskResult::Failed {
                        code: "LEASE_EXPIRED".into(),
                        message: "Worker lease expired".into(),
                        retryable: true,
                    },
                )
                .await
                .unwrap_or(false);
            recovered += u64::from(accepted);
        }
        Ok(recovered)
    }

    async fn load_task(&self, message: &DispatchMessage) -> Result<RuntimeTask> {
        let row=sqlx::query("SELECT n.node_id,n.node_type,n.node_version,n.input_json,n.run_index,n.iteration_index,n.capability,a.attempt_number,a.idempotency_key,a.deadline_at,e.workflow_version_id,e.trace_id,e.execution_type,s.compiled_ir_json,s.definition_json FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id JOIN execution_snapshots s ON s.execution_id=e.id WHERE a.tenant_id=? AND a.id=?")
            .bind(message.tenant_id).bind(message.attempt_id).fetch_one(&self.pool).await?;
        let compiled: CompiledWorkflow = serde_json::from_value(row.try_get("compiled_ir_json")?)?;
        let node_id: String = row.try_get("node_id")?;
        let node = compiled
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .context("Compiled node is missing")?;
        let definition: WorkflowDefinition =
            serde_json::from_value(row.try_get("definition_json")?)?;
        let resource_references = definition
            .nodes
            .iter()
            .find(|candidate| candidate.id == node_id)
            .map(|candidate| candidate.resource_references.clone())
            .context("Definition node is missing")?;
        let linked_rows=sqlx::query("SELECT node_name,run_index,output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND output_json IS NOT NULL ORDER BY run_index,id")
            .bind(message.tenant_id).bind(message.execution_id).fetch_all(&self.pool).await?;
        let mut linked_nodes = serde_json::Map::new();
        for linked in linked_rows {
            let name: String = linked.try_get("node_name")?;
            let run: u32 = linked.try_get("run_index")?;
            let outputs: Value = linked.try_get("output_json")?;
            let node = linked_nodes.entry(name).or_insert_with(|| json!({}));
            if let (Some(node), Some(outputs)) = (node.as_object_mut(), outputs.as_object()) {
                for (port, items) in outputs {
                    node.entry(port.clone())
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .expect("port map")
                        .insert(run.to_string(), items.clone());
                }
            }
        }
        Ok(RuntimeTask {
            tenant_id: message.tenant_id,
            workflow_version_id: row.try_get("workflow_version_id")?,
            execution_id: message.execution_id,
            node_execution_id: message.node_execution_id,
            attempt_id: message.attempt_id,
            attempt_number: row.try_get::<u32, _>("attempt_number")? as u16,
            node_type: row.try_get("node_type")?,
            node_version: row.try_get("node_version")?,
            node_parameters: node.parameters.clone(),
            inputs: serde_json::from_value(
                row.try_get::<Option<Value>, _>("input_json")?
                    .unwrap_or_else(|| json!({})),
            )?,
            run_index: row.try_get("run_index")?,
            iteration_index: row.try_get("iteration_index")?,
            capability: row.try_get("capability")?,
            idempotency_key: row.try_get("idempotency_key")?,
            deadline: row
                .try_get::<Option<OffsetDateTime>, _>("deadline_at")?
                .unwrap_or_else(|| OffsetDateTime::now_utc() + Duration::minutes(5)),
            mode: row.try_get("execution_type")?,
            trace_id: row.try_get("trace_id")?,
            linked_nodes: Value::Object(linked_nodes),
            resource_references,
        })
    }

    pub(crate) async fn load_machine(
        &self,
        transaction: &mut Transaction<'_, MySql>,
        tenant_id: Uuid,
        execution_id: Uuid,
    ) -> Result<ExecutionMachine> {
        let row = sqlx::query("SELECT payload_json,payload_artifact_id FROM checkpoints WHERE tenant_id=? AND execution_id=? ORDER BY sequence_number DESC LIMIT 1 FOR UPDATE")
            .bind(tenant_id).bind(execution_id).fetch_one(&mut **transaction).await?;
        self.decode_checkpoint(
            tenant_id,
            row.try_get("payload_json")?,
            row.try_get("payload_artifact_id")?,
        )
        .await
    }

    pub async fn load_checkpoint_machine(
        &self,
        tenant_id: Uuid,
        checkpoint_id: Uuid,
    ) -> Result<ExecutionMachine> {
        let row = sqlx::query(
            "SELECT payload_json,payload_artifact_id FROM checkpoints WHERE tenant_id=? AND id=?",
        )
        .bind(tenant_id)
        .bind(checkpoint_id)
        .fetch_one(&self.pool)
        .await?;
        self.decode_checkpoint(
            tenant_id,
            row.try_get("payload_json")?,
            row.try_get("payload_artifact_id")?,
        )
        .await
    }

    async fn decode_checkpoint(
        &self,
        tenant_id: Uuid,
        payload: Option<Value>,
        artifact_id: Option<Uuid>,
    ) -> Result<ExecutionMachine> {
        let payload = match (payload, artifact_id) {
            (Some(payload), None) => payload,
            (None, Some(artifact_id)) => {
                let store = self
                    .checkpoint_artifacts
                    .as_ref()
                    .context("Checkpoint artifact storage is not configured")?;
                let artifact = store
                    .get(
                        TenantId::from_uuid(tenant_id),
                        ArtifactId::from_uuid(artifact_id),
                    )
                    .await?
                    .context("Checkpoint artifact is missing")?;
                serde_json::from_slice(&artifact.content)
                    .context("Checkpoint artifact payload is invalid JSON")?
            }
            _ => anyhow::bail!("Checkpoint has no valid payload source"),
        };
        serde_json::from_value(payload).context("Checkpoint machine state is invalid")
    }

    pub async fn externalize_checkpoints(&self, limit: u32) -> Result<u64> {
        let Some(store) = &self.checkpoint_artifacts else {
            return Ok(0);
        };
        let rows = sqlx::query("SELECT id,tenant_id,state_hash,payload_json FROM checkpoints WHERE payload_json IS NOT NULL AND payload_artifact_id IS NULL ORDER BY created_at LIMIT ?")
            .bind(limit.clamp(1, 500)).fetch_all(&self.pool).await?;
        let mut externalized = 0;
        for row in rows {
            let checkpoint_id: Uuid = row.try_get("id")?;
            let tenant_id: Uuid = row.try_get("tenant_id")?;
            let state_hash: String = row.try_get("state_hash")?;
            let payload: Value = row.try_get("payload_json")?;
            let bytes = serde_json::to_vec(&payload)?;
            if bytes.len() <= self.checkpoint_artifact_threshold {
                continue;
            }
            let artifact = store
                .put(ArtifactWrite {
                    tenant_id: TenantId::from_uuid(tenant_id),
                    content_type: "application/vnd.agentx.checkpoint+json".into(),
                    content: bytes,
                })
                .await?;
            let switched = self
                .switch_checkpoint_to_artifact(
                    checkpoint_id,
                    tenant_id,
                    &state_hash,
                    artifact.id.as_uuid(),
                )
                .await;
            match switched {
                Ok(true) => externalized += 1,
                Ok(false) => {
                    store
                        .delete(TenantId::from_uuid(tenant_id), artifact.id)
                        .await?;
                }
                Err(error) => {
                    let _ = store
                        .delete(TenantId::from_uuid(tenant_id), artifact.id)
                        .await;
                    return Err(error);
                }
            }
        }
        Ok(externalized)
    }

    async fn switch_checkpoint_to_artifact(
        &self,
        checkpoint_id: Uuid,
        tenant_id: Uuid,
        state_hash: &str,
        artifact_id: Uuid,
    ) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query("UPDATE checkpoints SET payload_json=NULL,payload_artifact_id=? WHERE id=? AND tenant_id=? AND state_hash=? AND payload_json IS NOT NULL AND payload_artifact_id IS NULL")
            .bind(artifact_id).bind(checkpoint_id).bind(tenant_id).bind(state_hash)
            .execute(&mut *transaction).await?;
        if result.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query("INSERT INTO checkpoint_artifacts(tenant_id,checkpoint_id,artifact_id,role) VALUES(?,?,?,'state')")
            .bind(tenant_id).bind(checkpoint_id).bind(artifact_id)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(true)
    }
}

pub(crate) async fn queue_ready_attempts(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    machine: &mut ExecutionMachine,
) -> Result<()> {
    let parallel = machine.workflow().execution_order == ExecutionOrder::Parallel;
    let execution=sqlx::query("SELECT e.parent_execution_id,s.runtime_settings_json FROM workflow_executions e JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=?")
        .bind(tenant_id).bind(execution_id).fetch_one(&mut **transaction).await?;
    let parent_execution_id: Option<Uuid> = execution.try_get("parent_execution_id")?;
    let runtime_settings: Value = execution.try_get("runtime_settings_json")?;
    while let Some(node_execution_id) = machine.next_ready() {
        let node_index = machine
            .activation(node_execution_id)
            .expect("activation exists")
            .node_index;
        let node = machine.workflow().nodes[node_index].clone();
        if let Some(parent_execution_id) = parent_execution_id
            && node.side_effect_level == SideEffectLevel::Irreversible
        {
            let decision = side_effect_decision(
                transaction,
                tenant_id,
                execution_id,
                node_execution_id.as_uuid(),
                &runtime_settings,
                &node.id,
            )
            .await?;
            match decision.as_deref() {
                None => {
                    machine.defer_for_confirmation(node_execution_id)?;
                    break;
                }
                Some("reuse_output") => {
                    let output: Value = sqlx::query_scalar("SELECT output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND node_id=? AND status='succeeded' AND output_json IS NOT NULL ORDER BY run_index DESC,id DESC LIMIT 1")
                        .bind(tenant_id).bind(parent_execution_id).bind(&node.id)
                        .fetch_optional(&mut **transaction).await?.context("SIDE_EFFECT_REUSE_OUTPUT_UNAVAILABLE")?;
                    complete_without_worker(
                        transaction,
                        tenant_id,
                        execution_id,
                        machine,
                        node_execution_id,
                        &node,
                        serde_json::from_value(output)?,
                        "reuse_output",
                    )
                    .await?;
                    continue;
                }
                Some("dry_run") => {
                    let mut items = machine
                        .activation(node_execution_id)
                        .expect("activation exists")
                        .inputs
                        .values()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>();
                    for item in &mut items {
                        item.metadata.insert(
                            "agentx.sideEffectDecision".into(),
                            Value::String("dry_run".into()),
                        );
                    }
                    complete_without_worker(
                        transaction,
                        tenant_id,
                        execution_id,
                        machine,
                        node_execution_id,
                        &node,
                        BTreeMap::from([("main".into(), items)]),
                        "dry_run",
                    )
                    .await?;
                    continue;
                }
                Some("execute") => {}
                Some(_) => anyhow::bail!("INVALID_SIDE_EFFECT_DECISION"),
            }
        }
        let attempt_id = machine.start_attempt(node_execution_id)?;
        let activation = machine
            .activation(node_execution_id)
            .expect("activation exists");
        upsert_activation(transaction, tenant_id, execution_id, activation, &node).await?;
        let capability = capability_name(&node.capability);
        let attempt_number = activation
            .attempts
            .last()
            .expect("attempt exists")
            .attempt_number;
        let idempotency_key = format!("{execution_id}:{node_execution_id}:{attempt_number}");
        let deadline_ms = node
            .settings
            .timeout_ms
            .unwrap_or(300_000)
            .clamp(1_000, 86_400_000);
        let inserted=sqlx::query("INSERT IGNORE INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,status,idempotency_key,deadline_at,input_json) VALUES(?,?,?,?,?,'queued',?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? MICROSECOND),?)")
            .bind(attempt_id.as_uuid()).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid())
            .bind(attempt_number).bind(&idempotency_key).bind(deadline_ms * 1000).bind(serde_json::to_value(&activation.inputs)?)
            .execute(&mut **transaction).await?.rows_affected() == 1;
        if inserted {
            let message = DispatchMessage {
                tenant_id,
                execution_id,
                node_execution_id: node_execution_id.as_uuid(),
                attempt_id: attempt_id.as_uuid(),
                capability: capability.into(),
            };
            sqlx::query("INSERT INTO execution_outbox(id,tenant_id,execution_id,node_execution_id,attempt_id,message_type,capability,payload_json) VALUES(?,?,?,?,?,'dispatch_node',?,?)")
                .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(node_execution_id.as_uuid()).bind(attempt_id.as_uuid())
                .bind(capability).bind(serde_json::to_value(message)?).execute(&mut **transaction).await?;
        }
        if !parallel {
            break;
        }
    }
    Ok(())
}

async fn side_effect_decision(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    node_execution_id: Uuid,
    runtime_settings: &Value,
    node_id: &str,
) -> Result<Option<String>> {
    let confirmed: Option<String> = sqlx::query_scalar("SELECT decision FROM side_effect_confirmations WHERE tenant_id=? AND execution_id=? AND node_execution_id=?")
        .bind(tenant_id).bind(execution_id).bind(node_execution_id).fetch_optional(&mut **transaction).await?;
    Ok(confirmed.or_else(|| {
        runtime_settings
            .get("sideEffectDecisions")
            .and_then(|value| value.get(node_id))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }))
}

#[allow(clippy::too_many_arguments)]
async fn complete_without_worker(
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
    insert_execution_event(
        transaction,
        tenant_id,
        execution_id,
        "node.side_effect_resolved",
        "succeeded",
        json!({"nodeId":node.id,"nodeExecutionId":node_execution_id,"decision":decision}),
    )
    .await?;
    Ok(())
}

async fn upsert_activation(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    activation: &agentx_runtime::NodeActivation,
    node: &agentx_runtime::CompiledNode,
) -> Result<()> {
    let status = activation_status_name(activation.status);
    sqlx::query("INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_name,node_type,node_version,generation,activation_slot,run_index,iteration_index,status,capability,side_effect_level,input_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE status=VALUES(status),input_json=VALUES(input_json),updated_at=CURRENT_TIMESTAMP(6)")
        .bind(activation.id.as_uuid()).bind(tenant_id).bind(execution_id).bind(&node.id).bind(&node.name).bind(&node.node_type).bind(node.type_version)
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
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM checkpoints WHERE execution_id=?")
        .bind(execution_id).fetch_one(&mut **transaction).await?;
    let payload = serde_json::to_value(machine)?;
    let hash = hash_json(&payload)?;
    sqlx::query("INSERT INTO checkpoints(id,tenant_id,execution_id,node_execution_id,sequence_number,checkpoint_type,state_hash,payload_json) VALUES(?,?,?,NULL,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(sequence).bind(checkpoint_type).bind(hash).bind(payload)
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn create_wait(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    node_execution_id: Uuid,
    contract: &Value,
) -> Result<()> {
    let kind = contract
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("webhook");
    let resume_kind = if kind == "approval" {
        "approval"
    } else if kind == "time" {
        "time"
    } else if kind == "form" {
        "form"
    } else {
        "webhook"
    };
    let wait_kind = if kind == "time" {
        contract
            .get("waitKind")
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "duration" | "datetime"))
            .unwrap_or("duration")
    } else {
        resume_kind
    };
    let authentication = contract
        .get("authenticationMode")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "none" | "header" | "basic" | "signed"))
        .unwrap_or("signed");
    let authentication_hash = contract
        .get("authenticationConfigHash")
        .and_then(Value::as_str);
    let resume_token = Uuid::now_v7().to_string();
    let token_hash = format!("{:x}", Sha256::digest(resume_token.as_bytes()));
    let token_id = Uuid::now_v7();
    let timeout_at = contract
        .get("timeoutAt")
        .and_then(Value::as_str)
        .and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });
    let wake_at = contract
        .get("wakeAt")
        .and_then(Value::as_str)
        .and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });
    sqlx::query("INSERT INTO execution_resume_tokens(id,tenant_id,execution_id,node_execution_id,token_hash,resume_kind,expires_at) VALUES(?,?,?,?,?,?,?)")
        .bind(token_id).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(token_hash).bind(resume_kind).bind(timeout_at).execute(&mut **transaction).await?;
    let wait_id = Uuid::parse_str(&resume_token)?;
    sqlx::query("INSERT INTO wait_subscriptions(id,tenant_id,execution_id,node_execution_id,resume_token_id,wait_kind,wake_at,timeout_at,authentication_mode,response_mode,payload_schema_json) VALUES(?,?,?,?,?,?,?,?,?,'accepted',?)")
        .bind(wait_id).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(token_id).bind(wait_kind)
        .bind(wake_at).bind(timeout_at).bind(authentication).bind(contract.get("payloadSchema").cloned()).execute(&mut **transaction).await?;
    if matches!(resume_kind, "webhook" | "form") {
        sqlx::query("INSERT INTO resume_webhook_bindings(id,tenant_id,wait_subscription_id,path_token_hash,http_method,authentication_config_hash,status,expires_at) VALUES(?,?,?,?,'POST',?,'active',?)")
            .bind(wait_id).bind(tenant_id).bind(wait_id).bind(format!("{:x}",Sha256::digest(resume_token.as_bytes()))).bind(authentication_hash).bind(timeout_at).execute(&mut **transaction).await?;
    }
    if resume_kind == "approval" {
        let execution=sqlx::query("SELECT e.workflow_id,e.requested_by,n.node_id FROM workflow_executions e JOIN node_executions n ON n.execution_id=e.id AND n.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=? AND n.id=?")
            .bind(tenant_id).bind(execution_id).bind(node_execution_id).fetch_one(&mut **transaction).await?;
        let candidate = contract
            .get("candidateUserId")
            .and_then(Value::as_str)
            .map(Uuid::parse_str)
            .transpose()?
            .or(execution.try_get("requested_by")?)
            .context("Approval candidate is required")?;
        let title = contract
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Workflow approval required");
        let description = contract.get("description").and_then(Value::as_str);
        let workflow_id: Uuid = execution.try_get("workflow_id")?;
        let node_id: String = execution.try_get("node_id")?;
        sqlx::query("INSERT INTO approval_tasks(id,tenant_id,execution_id,workflow_id,node_id,node_execution_id,resume_token_id,title,description,request_payload_json,deadline_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(wait_id).bind(tenant_id).bind(execution_id).bind(workflow_id).bind(node_id).bind(node_execution_id).bind(token_id).bind(title).bind(description).bind(contract).bind(timeout_at).execute(&mut **transaction).await?;
        sqlx::query("INSERT INTO approval_candidates(tenant_id,approval_task_id,candidate_type,candidate_id) VALUES(?,?,'user',?)")
            .bind(tenant_id).bind(wait_id).bind(candidate).execute(&mut **transaction).await?;
        let notification_id = Uuid::now_v7();
        sqlx::query("INSERT INTO notifications(id,tenant_id,source_event_id,notification_type,title_key,body_key,arguments_json,target_type,target_id,target_path,tone) VALUES(?,?,?,'approval_created','notifications.approvalReassigned.title','notifications.approvalReassigned.body',JSON_OBJECT(),'approval',?,?,'warning')")
            .bind(notification_id).bind(tenant_id).bind(wait_id).bind(wait_id).bind(format!("/approvals/{wait_id}")).execute(&mut **transaction).await?;
        sqlx::query(
            "INSERT INTO notification_receipts(tenant_id,notification_id,user_id) VALUES(?,?,?)",
        )
        .bind(tenant_id)
        .bind(notification_id)
        .bind(candidate)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

pub(crate) async fn sync_execution_status(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    status: RuntimeExecutionStatus,
) -> Result<()> {
    let status = execution_status_name(status);
    let terminal = matches!(status, "succeeded" | "failed" | "cancelled" | "timed_out");
    sqlx::query("UPDATE workflow_executions SET status=?,ended_at=IF(?,COALESCE(ended_at,CURRENT_TIMESTAMP(6)),NULL),duration_ms=IF(?,TIMESTAMPDIFF(MICROSECOND,started_at,COALESCE(ended_at,CURRENT_TIMESTAMP(6)))/1000,NULL),state_version=state_version+1 WHERE tenant_id=? AND id=?")
        .bind(status).bind(terminal).bind(terminal).bind(tenant_id).bind(execution_id).execute(&mut **transaction).await?;
    Ok(())
}

pub(crate) async fn insert_execution_event(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    event_type: &str,
    status: &str,
    summary: Value,
) -> Result<()> {
    let sequence:u64=sqlx::query_scalar("SELECT CAST(COALESCE(MAX(sequence_number),0)+1 AS UNSIGNED) FROM execution_events WHERE tenant_id=? AND execution_id=?")
        .bind(tenant_id).bind(execution_id).fetch_one(&mut **transaction).await?;
    sqlx::query("INSERT INTO execution_events(tenant_id,execution_id,sequence_number,event_type,status,summary_json,occurred_at) VALUES(?,?,?,?,?,?,CURRENT_TIMESTAMP(6))")
        .bind(tenant_id).bind(execution_id).bind(sequence).bind(event_type).bind(status).bind(summary).execute(&mut **transaction).await?;
    Ok(())
}

struct TraceInsert<'a> {
    tenant_id: Uuid,
    execution_id: Uuid,
    workflow_id: Uuid,
    workflow_version_id: Uuid,
    trace_id: Uuid,
    node_execution_id: Option<Uuid>,
    node_id: Option<&'a str>,
    event_type: &'a str,
    status: &'a str,
    run_index: u32,
    attributes: Value,
    error_code: Option<String>,
    error_message: Option<String>,
}
async fn insert_trace(
    transaction: &mut Transaction<'_, MySql>,
    value: TraceInsert<'_>,
) -> Result<()> {
    let event_id = Uuid::now_v7();
    let payload = json!({"eventId":event_id,"tenantId":value.tenant_id,"traceId":value.trace_id,"spanId":Uuid::now_v7(),"parentSpanId":null,"executionId":value.execution_id,"workflowId":value.workflow_id,"workflowVersionId":value.workflow_version_id,"nodeExecutionId":value.node_execution_id,"nodeId":value.node_id,"eventType":value.event_type,"status":value.status,"eventTime":OffsetDateTime::now_utc(),"durationMs":null,"runIndex":value.run_index,"iterationIndex":0,"modelName":null,"providerName":null,"mcpToolName":null,"inputTokens":null,"outputTokens":null,"costMicros":0,"errorCode":value.error_code,"errorMessage":value.error_message,"attributes":value.attributes,"contentRef":null});
    sqlx::query("INSERT INTO trace_delivery_outbox(event_id,tenant_id,execution_id,payload_json) VALUES(?,?,?,?)")
        .bind(event_id).bind(value.tenant_id).bind(value.execution_id).bind(payload).execute(&mut **transaction).await?;
    Ok(())
}

pub(crate) fn invocation_items(input: &Value) -> Vec<Item> {
    match input {
        Value::Array(values) => values
            .iter()
            .cloned()
            .map(|json| Item {
                json,
                ..Item::default()
            })
            .collect(),
        value => vec![Item {
            json: value.clone(),
            ..Item::default()
        }],
    }
}
fn hash_json(value: &Value) -> Result<String> {
    Ok(format!(
        "sha256:v1:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}
fn parse_uuid(value: &Value, key: &str) -> Result<Uuid> {
    Uuid::parse_str(
        value
            .get(key)
            .and_then(Value::as_str)
            .context(format!("{key} is missing"))?,
    )
    .map_err(Into::into)
}
fn checkpoint_type(result: &TaskResult) -> &'static str {
    match result {
        TaskResult::Completed(_) | TaskResult::Failed { .. } => "node_completed",
        TaskResult::Suspended(_) => "node_suspended",
    }
}
fn capability_name(value: &NodeCapability) -> &'static str {
    match value {
        NodeCapability::Builtin => "builtin",
        NodeCapability::DeclarativeHttp => "declarative_http",
        NodeCapability::RemoteAction => "remote_action",
    }
}
fn side_effect_name(value: &agentx_node_protocol::SideEffectLevel) -> &'static str {
    match value {
        agentx_node_protocol::SideEffectLevel::None => "none",
        agentx_node_protocol::SideEffectLevel::Idempotent => "idempotent",
        agentx_node_protocol::SideEffectLevel::Reversible => "reversible",
        agentx_node_protocol::SideEffectLevel::Irreversible => "irreversible",
    }
}
fn activation_status_name(value: agentx_runtime::ActivationStatus) -> &'static str {
    match value {
        agentx_runtime::ActivationStatus::Ready => "ready",
        agentx_runtime::ActivationStatus::Running => "queued",
        agentx_runtime::ActivationStatus::Waiting => "waiting",
        agentx_runtime::ActivationStatus::Succeeded => "succeeded",
        agentx_runtime::ActivationStatus::Failed => "failed",
        agentx_runtime::ActivationStatus::Skipped => "skipped",
        agentx_runtime::ActivationStatus::Cancelled => "cancelled",
    }
}
fn attempt_status_name(value: AttemptStatus) -> &'static str {
    match value {
        AttemptStatus::Running => "queued",
        AttemptStatus::Succeeded => "succeeded",
        AttemptStatus::Failed => "failed",
        AttemptStatus::Suspended => "suspended",
        AttemptStatus::Cancelled => "cancelled",
    }
}
fn execution_status_name(value: RuntimeExecutionStatus) -> &'static str {
    match value {
        RuntimeExecutionStatus::Created => "created",
        RuntimeExecutionStatus::Running => "running",
        RuntimeExecutionStatus::Waiting => "waiting",
        RuntimeExecutionStatus::Succeeded => "succeeded",
        RuntimeExecutionStatus::Failed => "failed",
        RuntimeExecutionStatus::Cancelled => "cancelled",
        RuntimeExecutionStatus::TimedOut => "timed_out",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invocation_input_preserves_n8n_item_shape() {
        assert_eq!(invocation_items(&json!([{"a":1},{"a":2}])).len(), 2);
        assert_eq!(invocation_items(&json!({"a":1}))[0].json["a"], 1)
    }
    #[test]
    fn dispatch_message_round_trips() {
        let value = DispatchMessage {
            tenant_id: Uuid::nil(),
            execution_id: Uuid::nil(),
            node_execution_id: Uuid::nil(),
            attempt_id: Uuid::nil(),
            capability: "builtin".into(),
        };
        assert_eq!(
            serde_json::from_value::<DispatchMessage>(serde_json::to_value(value).unwrap())
                .unwrap()
                .capability,
            "builtin"
        )
    }
}
