use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use agentx_application::{ArtifactStore, ArtifactWrite, RuntimeResourceSnapshot};
use agentx_domain::{
    ArtifactId, ExecutionOrder, NodeExecutionId, ResourceReference, TenantId, WorkflowDefinition,
};
use agentx_node_protocol::{Item, NodeCapability, NodeManifestVersion, SideEffectLevel};
use agentx_runtime::{
    AttemptStatus, CompileContext, CompiledWorkflow, ExecutionMachine, MachineError, NodeRegistry,
    PartialExecutionMode, RuntimeExecutionStatus, WorkflowCompiler,
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
    pub source: RuntimeExecutionSource,
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
    pub debug_plan: Value,
    pub debug_overlay_snapshot: Value,
    pub draft_resource_snapshots: Vec<RuntimeResourceSnapshot>,
    pub initial_machine: Option<ExecutionMachine>,
}

#[derive(Clone, Debug)]
pub enum RuntimeExecutionSource {
    Version(Uuid),
    DraftRevision { workflow_id: Uuid, revision: u64 },
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
    pub workflow_id: Uuid,
    pub workflow_service_identity_id: Uuid,
    pub workflow_version_id: Option<Uuid>,
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
    pub resource_snapshots: Vec<RuntimeResourceSnapshot>,
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
        let source_value = match &command.source {
            RuntimeExecutionSource::Version(version_id) => {
                json!({"kind":"version","id":version_id})
            }
            RuntimeExecutionSource::DraftRevision {
                workflow_id,
                revision,
            } => json!({"kind":"draft_revision","id":workflow_id,"revision":revision}),
        };
        let request_hash = hash_json(&json!({
            "source": &source_value,
            "invocationId": command.invocation_id,
            "input": command.input,
            "triggerType": command.trigger_type,
            "executionType": command.execution_type,
            "parentExecutionId": command.parent_execution_id,
            "forkCheckpointId": command.fork_checkpoint_id,
            "forkMode": command.fork_mode,
            "runtimeSettings": command.runtime_settings,
            "debugPlan": command.debug_plan,
            "debugOverlay": command.debug_overlay_snapshot,
            "draftResourceSnapshots": command.draft_resource_snapshots,
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

        let (
            workflow_version_id,
            workflow_id,
            definition_value,
            stored_compiled,
            source_kind,
            source_id,
            source_revision,
        ) = match command.source {
            RuntimeExecutionSource::Version(version_id) => {
                let version = sqlx::query("SELECT workflow_id,definition_json,compiled_ir_json FROM workflow_versions WHERE tenant_id=? AND id=? FOR UPDATE")
                    .bind(command.tenant_id).bind(version_id).fetch_optional(&mut *transaction).await?
                    .context("Workflow Version was not found")?;
                (
                    Some(version_id),
                    version.try_get::<Uuid, _>("workflow_id")?,
                    version.try_get::<Value, _>("definition_json")?,
                    version.try_get::<Option<Value>, _>("compiled_ir_json")?,
                    "version",
                    version_id,
                    None::<u64>,
                )
            }
            RuntimeExecutionSource::DraftRevision {
                workflow_id,
                revision,
            } => {
                let draft = sqlx::query("SELECT definition_json FROM workflow_draft_revisions WHERE tenant_id=? AND workflow_id=? AND revision=? FOR UPDATE")
                    .bind(command.tenant_id).bind(workflow_id).bind(revision).fetch_optional(&mut *transaction).await?
                    .context("Workflow Draft Revision was not found")?;
                (
                    None,
                    workflow_id,
                    draft.try_get::<Value, _>("definition_json")?,
                    None,
                    "draft_revision",
                    workflow_id,
                    Some(revision),
                )
            }
        };
        let definition: WorkflowDefinition = serde_json::from_value(definition_value.clone())
            .context("Workflow Definition is invalid")?;
        let registry = load_node_registry(&mut transaction, command.tenant_id).await?;
        let compiled = match stored_compiled {
            Some(value) => {
                serde_json::from_value(value).context("Compiled Workflow IR is invalid")?
            }
            None => {
                let compiler = WorkflowCompiler::new(&registry);
                let compiled = compiler
                    .compile(
                        &definition,
                        &CompileContext {
                            current_workflow_version_id: workflow_version_id
                                .map(|value| value.to_string()),
                            ancestor_workflow_version_ids: Default::default(),
                        },
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            serde_json::to_string(&error.issues)
                                .unwrap_or_else(|_| error.to_string())
                        )
                    })?;
                if let Some(version_id) = workflow_version_id {
                    sqlx::query("UPDATE workflow_versions SET compiled_ir_json=?,compiled_ir_hash=?,compiler_version=?,compiled_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND compiled_ir_json IS NULL")
                        .bind(serde_json::to_value(&compiled)?).bind(&compiled.canonical_hash).bind(&compiled.compiler_version)
                        .bind(command.tenant_id).bind(version_id).execute(&mut *transaction).await?;
                }
                compiled
            }
        };

        let identity_id: Uuid = sqlx::query_scalar("SELECT id FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=? AND status='active'")
            .bind(command.tenant_id).bind(workflow_id).fetch_optional(&mut *transaction).await?
            .context("Workflow Service Identity is missing or disabled")?;
        let resource_rows = if let Some(version_id) = workflow_version_id {
            sqlx::query("SELECT node_id,binding_id,binding_role,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash FROM workflow_version_resources WHERE tenant_id=? AND workflow_version_id=? ORDER BY node_id,resource_type,resource_id")
                .bind(command.tenant_id).bind(version_id).fetch_all(&mut *transaction).await?
        } else {
            Vec::new()
        };
        let mut execution_resources = Vec::with_capacity(resource_rows.len());
        for resource in resource_rows {
            let resource_type: String = resource.try_get("resource_type")?;
            let resource_id: Uuid = resource.try_get("resource_id")?;
            let operation: String = resource.try_get("operation_key")?;
            let grant_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM resource_grants WHERE tenant_id=? AND subject_type='workflow_service_identity' AND subject_id=? AND resource_type=? AND resource_id=? AND operation_key IN (?, 'manage') ORDER BY created_at LIMIT 1")
                .bind(command.tenant_id).bind(identity_id).bind(&resource_type).bind(resource_id).bind(&operation)
                .fetch_optional(&mut *transaction).await?;
            let grant_id = grant_id.context(format!(
                "RESOURCE_GRANT_MISSING: {resource_type} {resource_id} {operation}"
            ))?;
            execution_resources.push(json!({
                "nodeId": resource.try_get::<String,_>("node_id")?,
                "reference": {
                    "bindingId": resource.try_get::<Option<String>,_>("binding_id")?,
                    "bindingRole": resource.try_get::<Option<String>,_>("binding_role")?,
                    "resourceType": resource_type,
                    "resourceId": resource_id,
                    "resourceVersionId": resource.try_get::<Option<Uuid>,_>("resource_version_id")?,
                    "operation": operation,
                },
                "snapshotHash": resource.try_get::<String,_>("snapshot_hash")?,
                "snapshot": resource.try_get::<Value,_>("snapshot_json")?,
                "grantId": grant_id,
                "authorizedAt": OffsetDateTime::now_utc(),
            }));
        }
        if workflow_version_id.is_none() {
            anyhow::ensure!(
                draft_resource_snapshots_cover_definition(
                    &definition,
                    &command.draft_resource_snapshots
                ),
                "DRAFT_RESOURCE_SNAPSHOT_MISMATCH"
            );
            for resource in &command.draft_resource_snapshots {
                let grant_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM resource_grants WHERE tenant_id=? AND subject_type='workflow_service_identity' AND subject_id=? AND resource_type=? AND resource_id=? AND operation_key IN (?, 'manage') ORDER BY created_at LIMIT 1")
                    .bind(command.tenant_id).bind(identity_id).bind(resource.reference.resource_type.as_str()).bind(resource.reference.resource_id).bind(resource.reference.operation.as_str()).fetch_optional(&mut *transaction).await?;
                let grant_id = grant_id.context(format!(
                    "RESOURCE_GRANT_MISSING: {} {} {}",
                    resource.reference.resource_type.as_str(),
                    resource.reference.resource_id,
                    resource.reference.operation.as_str()
                ))?;
                execution_resources.push(json!({
                    "nodeId": resource.node_id,
                    "reference": resource.reference,
                    "snapshotHash": resource.snapshot_hash,
                    "snapshot": resource.snapshot,
                    "grantId": grant_id,
                    "authorizedAt": OffsetDateTime::now_utc(),
                }));
            }
        }
        let resource_snapshot = json!({
            "schemaVersion": "1.0",
            "workflowServiceIdentityId": identity_id,
            "resources": execution_resources,
        });
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,source_kind,source_id,source_revision,invocation_id,session_id,parent_execution_id,caller_execution_id,fork_checkpoint_id,trace_id,trigger_type,execution_type,fork_mode,requested_by,input_json,status,started_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'queued',CURRENT_TIMESTAMP(6))")
            .bind(execution_id).bind(command.tenant_id).bind(workflow_id).bind(workflow_version_id).bind(source_kind).bind(source_id).bind(source_revision)
            .bind(command.invocation_id).bind(command.session_id).bind(command.parent_execution_id).bind(command.caller_execution_id).bind(command.fork_checkpoint_id)
            .bind(trace_id).bind(&command.trigger_type).bind(&command.execution_type).bind(&command.fork_mode)
            .bind(command.requested_by).bind(&command.input).execute(&mut *transaction).await?;
        let items = invocation_items(&command.input);
        let mut machine = if let Some(machine) = command.initial_machine.take() {
            machine
        } else {
            let base = ExecutionMachine::new(compiled.clone(), items.clone())?;
            let mode = command
                .debug_plan
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("full");
            match mode {
                "single_node" | "to_node" | "from_node" => {
                    let target = command
                        .debug_plan
                        .get("targetNodeId")
                        .and_then(Value::as_str)
                        .context("Partial debug target is missing")?;
                    let partial_mode = match mode {
                        "single_node" => PartialExecutionMode::Node,
                        "to_node" => PartialExecutionMode::ToNode,
                        "from_node" => PartialExecutionMode::FromNode,
                        _ => unreachable!(),
                    };
                    ExecutionMachine::new_partial(compiled.clone(), partial_mode, target, items)?
                }
                _ => base,
            }
        };
        let manifest_snapshot = serde_json::to_value(registry.manifests().collect::<Vec<_>>())?;
        let snapshot_hash = execution_snapshot_hash(
            &definition_value,
            &machine.workflow().canonical_hash,
            &source_value,
            &manifest_snapshot,
            &resource_snapshot,
            &command.runtime_settings,
            &command.debug_plan,
            &command.debug_overlay_snapshot,
        )?;
        sqlx::query("INSERT INTO execution_snapshots(execution_id,tenant_id,workflow_version_id,definition_json,compiled_ir_json,compiled_ir_hash,compiler_version,manifest_snapshot_json,debug_plan_json,debug_overlay_snapshot_json,resource_snapshot_json,runtime_settings_json,state_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(execution_id).bind(command.tenant_id).bind(workflow_version_id).bind(&definition_value)
            .bind(serde_json::to_value(machine.workflow())?).bind(&machine.workflow().canonical_hash).bind(&machine.workflow().compiler_version)
            .bind(manifest_snapshot).bind(&command.debug_plan).bind(&command.debug_overlay_snapshot).bind(&resource_snapshot).bind(&command.runtime_settings).bind(snapshot_hash).execute(&mut *transaction).await?;
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
                workflow_version_id,
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
        let workflow_version_id: Option<Uuid> = row.try_get("workflow_version_id")?;
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
        let row=sqlx::query("SELECT n.node_id,n.node_type,n.node_version,n.input_json,n.run_index,n.iteration_index,n.capability,a.attempt_number,a.idempotency_key,a.deadline_at,e.workflow_id,e.workflow_version_id,e.trace_id,e.execution_type,s.compiled_ir_json,s.definition_json,s.resource_snapshot_json FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id JOIN execution_snapshots s ON s.execution_id=e.id WHERE a.tenant_id=? AND a.id=?")
            .bind(message.tenant_id).bind(message.attempt_id).fetch_one(&self.pool).await?;
        let compiled: CompiledWorkflow = serde_json::from_value(row.try_get("compiled_ir_json")?)?;
        let node_id: String = row.try_get("node_id")?;
        let node = compiled
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .context("Compiled node is missing")?;
        let resource_snapshot: Value = row.try_get("resource_snapshot_json")?;
        let workflow_service_identity_id = resource_snapshot
            .get("workflowServiceIdentityId")
            .and_then(Value::as_str)
            .map(Uuid::parse_str)
            .transpose()?
            .context("Execution resource snapshot has no service identity")?;
        let resource_snapshots = resource_snapshot
            .get("resources")
            .and_then(Value::as_array)
            .context("Execution resource snapshot has no resources")?
            .iter()
            .filter(|value| value.get("nodeId").and_then(Value::as_str) == Some(node_id.as_str()))
            .map(|value| serde_json::from_value::<RuntimeResourceSnapshot>(value.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let resource_references = resource_snapshots
            .iter()
            .map(|value| value.reference.clone())
            .collect();
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
            workflow_id: row.try_get("workflow_id")?,
            workflow_service_identity_id,
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
            resource_snapshots,
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

async fn load_node_registry(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
) -> Result<NodeRegistry> {
    let rows = sqlx::query("SELECT v.manifest_json FROM node_definitions d JOIN node_definition_versions v ON v.node_definition_id=d.id WHERE d.status='active' AND (d.tenant_id IS NULL OR d.tenant_id=?) ORDER BY d.node_type,v.version_number,d.tenant_id IS NULL")
        .bind(tenant_id)
        .fetch_all(&mut **transaction)
        .await?;
    if rows.is_empty() {
        return Ok(NodeRegistry::m5_defaults());
    }
    let mut seen = HashSet::new();
    let mut registry = NodeRegistry::default();
    for row in rows {
        let manifest: NodeManifestVersion = serde_json::from_value(row.try_get("manifest_json")?)
            .context("Node Catalog Manifest is invalid")?;
        if seen.insert((manifest.node_type.clone(), manifest.version)) {
            registry.register(manifest)?;
        }
    }
    Ok(registry)
}

pub(crate) async fn queue_ready_attempts(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    machine: &mut ExecutionMachine,
) -> Result<()> {
    let parallel = machine.workflow().execution_order == ExecutionOrder::Parallel;
    let execution=sqlx::query("SELECT e.parent_execution_id,s.runtime_settings_json,s.debug_overlay_snapshot_json FROM workflow_executions e JOIN execution_snapshots s ON s.execution_id=e.id AND s.tenant_id=e.tenant_id WHERE e.tenant_id=? AND e.id=?")
        .bind(tenant_id).bind(execution_id).fetch_one(&mut **transaction).await?;
    let parent_execution_id: Option<Uuid> = execution.try_get("parent_execution_id")?;
    let runtime_settings: Value = execution.try_get("runtime_settings_json")?;
    let debug_overlay_snapshot: Value = execution.try_get("debug_overlay_snapshot_json")?;
    while let Some(node_execution_id) = machine.next_ready() {
        let node_index = machine
            .activation(node_execution_id)
            .expect("activation exists")
            .node_index;
        let node = machine.workflow().nodes[node_index].clone();
        if let Some((kind, payload)) = debug_overlay_for_node(&debug_overlay_snapshot, &node.id) {
            if kind == "temporary_input" {
                machine.replace_ready_inputs(node_execution_id, overlay_items(payload))?;
            } else if matches!(
                kind,
                "pin_data" | "mock_output" | "history_output" | "artifact"
            ) {
                complete_without_worker(
                    transaction,
                    tenant_id,
                    execution_id,
                    machine,
                    node_execution_id,
                    &node,
                    overlay_items(payload),
                    kind,
                )
                .await?;
                continue;
            }
        }
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

fn debug_overlay_for_node<'a>(snapshot: &'a Value, node_id: &str) -> Option<(&'a str, &'a Value)> {
    snapshot
        .get("items")?
        .as_array()?
        .iter()
        .find(|item| item.get("nodeId").and_then(Value::as_str) == Some(node_id))
        .and_then(|item| Some((item.get("kind")?.as_str()?, item.get("payload")?)))
}

fn overlay_items(payload: &Value) -> BTreeMap<String, Vec<Item>> {
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
    sqlx::query("SELECT id FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE")
        .bind(tenant_id)
        .bind(execution_id)
        .fetch_one(&mut **transaction)
        .await?;
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
    workflow_version_id: Option<Uuid>,
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
#[allow(clippy::too_many_arguments)]
fn execution_snapshot_hash(
    definition: &Value,
    compiled_ir_hash: &str,
    source: &Value,
    manifest_snapshot: &Value,
    resources: &Value,
    runtime_settings: &Value,
    debug_plan: &Value,
    debug_overlay: &Value,
) -> Result<String> {
    hash_json(&json!({
        "definition": definition,
        "compiledIrHash": compiled_ir_hash,
        "source": source,
        "manifestSnapshot": manifest_snapshot,
        "resources": resources,
        "runtimeSettings": runtime_settings,
        "debugPlan": debug_plan,
        "debugOverlay": debug_overlay,
    }))
}
fn draft_resource_snapshots_cover_definition(
    definition: &WorkflowDefinition,
    snapshots: &[RuntimeResourceSnapshot],
) -> bool {
    if snapshots.iter().any(|snapshot| {
        !definition
            .nodes
            .iter()
            .any(|node| node.id == snapshot.node_id)
    }) {
        return false;
    }
    definition.nodes.iter().all(|node| {
        node.resource_references.iter().all(|expected| {
            snapshots.iter().any(|snapshot| {
                snapshot.node_id == node.id
                    && resolved_resource_reference_matches(expected, &snapshot.reference)
            })
        })
    })
}
fn resolved_resource_reference_matches(
    expected: &ResourceReference,
    actual: &ResourceReference,
) -> bool {
    expected.binding_id == actual.binding_id
        && expected.binding_role == actual.binding_role
        && expected.resource_type == actual.resource_type
        && expected.resource_id == actual.resource_id
        && expected.operation == actual.operation
        && expected
            .resource_version_id
            .is_none_or(|version| actual.resource_version_id == Some(version))
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
    value.as_str()
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
    #[test]
    fn debug_overlay_accepts_port_maps_and_plain_json() {
        let ports = overlay_items(&json!({"main":[{"json":{"value":1}}]}));
        assert_eq!(ports["main"][0].json["value"], 1);
        let plain = overlay_items(&json!([{"value":2}]));
        assert_eq!(plain["main"][0].json[0]["value"], 2);
        let snapshot =
            json!({"items":[{"nodeId":"agent","kind":"mock_output","payload":{"ok":true}}]});
        let (kind, payload) = debug_overlay_for_node(&snapshot, "agent").unwrap();
        assert_eq!(kind, "mock_output");
        assert_eq!(payload["ok"], true);
    }

    #[test]
    fn draft_revision_snapshot_hash_is_stable_and_covers_debug_inputs() {
        let definition = json!({"schemaVersion":"3.0","nodes":[],"connections":[],"settings":{}});
        let source = json!({"kind":"draft_revision","id":Uuid::nil(),"revision":7});
        let manifest = json!([{"nodeType":"manual_trigger","version":1}]);
        let resources = json!({"schemaVersion":"1.0","resources":[]});
        let base = execution_snapshot_hash(
            &definition,
            "sha256:compiled",
            &source,
            &manifest,
            &resources,
            &json!({"sideEffectDecisions":{}}),
            &json!({"mode":"full"}),
            &json!({"items":[]}),
        )
        .unwrap();
        let repeated = execution_snapshot_hash(
            &definition,
            "sha256:compiled",
            &source,
            &manifest,
            &resources,
            &json!({"sideEffectDecisions":{}}),
            &json!({"mode":"full"}),
            &json!({"items":[]}),
        )
        .unwrap();
        assert_eq!(base, repeated);

        let changed_overlay = execution_snapshot_hash(
            &definition,
            "sha256:compiled",
            &source,
            &manifest,
            &resources,
            &json!({"sideEffectDecisions":{}}),
            &json!({"mode":"full"}),
            &json!({"items":[{"nodeId":"agent","kind":"mock_output"}]}),
        )
        .unwrap();
        assert_ne!(base, changed_overlay);
    }

    #[test]
    fn draft_resource_snapshots_allow_resolved_versions_and_transitive_dependencies() {
        let resource_id = Uuid::from_u128(1);
        let version_id = Uuid::from_u128(2);
        let expected = ResourceReference {
            binding_id: Some("model-binding".into()),
            binding_role: Some("ai_model".into()),
            resource_type: agentx_domain::ResourceType::Model,
            resource_id,
            resource_version_id: None,
            operation: agentx_domain::ResourceOperation::Use,
        };
        let mut definition = WorkflowDefinition::empty();
        definition.nodes[0].resource_references = vec![expected.clone()];
        let direct = RuntimeResourceSnapshot {
            node_id: "manual-trigger".into(),
            reference: ResourceReference {
                resource_version_id: Some(version_id),
                ..expected
            },
            snapshot_hash: "sha256:direct".into(),
            snapshot: json!({}),
        };
        let dependency = RuntimeResourceSnapshot {
            node_id: "manual-trigger".into(),
            reference: ResourceReference {
                binding_id: None,
                binding_role: None,
                resource_type: agentx_domain::ResourceType::Credential,
                resource_id: Uuid::from_u128(3),
                resource_version_id: None,
                operation: agentx_domain::ResourceOperation::Use,
            },
            snapshot_hash: "sha256:dependency".into(),
            snapshot: json!({}),
        };
        assert!(draft_resource_snapshots_cover_definition(
            &definition,
            &[direct.clone(), dependency.clone()]
        ));
        assert!(!draft_resource_snapshots_cover_definition(
            &definition,
            &[
                RuntimeResourceSnapshot {
                    node_id: "unknown".into(),
                    ..dependency
                },
                direct
            ]
        ));
    }
}
