use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use agentx_application::{ArtifactStore, ArtifactWrite, RuntimeResourceSnapshot};
use agentx_domain::{
    ArtifactId, ContextMergePolicy, ContextScope, ExecutionOrder, NodeExecutionId,
    ResourceReference, TenantId, WorkflowDefinition,
};
use agentx_node_protocol::{Item, NodeManifestVersion, SideEffectLevel};
use agentx_runtime::{
    CompileContext, CompiledWorkflow, ExecutionMachine, ExpressionContext, MachineError,
    NodeRegistry, PartialExecutionMode, RuntimeExecutionStatus, WorkflowCompiler,
    materialize_and_validate_start_input,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub(crate) use self::runtime_repository_completion::persist_machine;
use self::runtime_repository_completion::{
    complete_without_worker, debug_overlay_for_node, overlay_items, upsert_activation,
};
use crate::quota::QuotaAdmission;
use crate::runtime_context::{
    apply_context_writes, apply_output_projection, initial_context, load_output_namespace,
    merge_context_overlay, merge_output_namespace, scoped_context,
};
use crate::runtime_events_repository::{
    TraceInsert, insert_execution_event, insert_trace, sync_execution_status,
};
use crate::runtime_repository_support::*;
use crate::runtime_wait::create_wait;

#[path = "runtime_repository_completion.rs"]
mod runtime_repository_completion;

#[derive(Clone)]
pub struct RuntimeRepository {
    pub(crate) pool: MySqlPool,
    checkpoint_artifacts: Option<Arc<dyn ArtifactStore>>,
    checkpoint_artifact_threshold: usize,
    quota_admission: Option<QuotaAdmission>,
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
    pub context_overlay: Value,
    pub idempotency_key: Option<String>,
    pub caller_execution_id: Option<Uuid>,
    pub caller_node_execution_id: Option<Uuid>,
    pub execution_type: String,
    pub parent_execution_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
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
    #[serde(default = "default_node_protocol_version")]
    pub node_protocol_version: String,
    #[serde(default)]
    pub compiler_version: String,
    #[serde(default)]
    pub ir_schema_version: String,
}

fn default_node_protocol_version() -> String {
    agentx_node_protocol::NODE_PROTOCOL_VERSION.into()
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
    pub workflow_inputs: Value,
    pub contexts: Value,
    pub context_version: u64,
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
            quota_admission: None,
        }
    }

    #[must_use]
    pub fn with_quota_admission(mut self, admission: QuotaAdmission) -> Self {
        self.quota_admission = Some(admission);
        self
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
        let execution_id = Uuid::now_v7();
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
        materialize_and_validate_start_input(&mut command.input, &compiled.start.inputs)
            .map_err(|error| anyhow::anyhow!("START_INPUT_INVALID: {error}"))?;

        let identity_id: Uuid = sqlx::query_scalar("SELECT id FROM workflow_service_identities WHERE tenant_id=? AND workflow_id=? AND status='active'")
            .bind(command.tenant_id).bind(workflow_id).fetch_optional(&mut *transaction).await?
            .context("Workflow Service Identity is missing or disabled")?;
        let resource_rows = if let Some(version_id) = workflow_version_id {
            sqlx::query("SELECT node_id,binding_id,binding_role,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash FROM workflow_version_resources WHERE tenant_id=? AND workflow_version_id=? AND resource_type<>'workflow' ORDER BY node_id,resource_type,resource_id")
                .bind(command.tenant_id).bind(version_id).fetch_all(&mut *transaction).await?
        } else {
            Vec::new()
        };
        let mut execution_resources = Vec::with_capacity(resource_rows.len());
        for resource in resource_rows {
            let resource_type: String = resource.try_get("resource_type")?;
            let resource_id: Uuid = resource.try_get("resource_id")?;
            let resource_version_id: Option<Uuid> = resource.try_get("resource_version_id")?;
            let operation: String = resource.try_get("operation_key")?;
            anyhow::ensure!(
                crate::runtime_resources::resource_is_active(
                    &mut transaction,
                    command.tenant_id,
                    &resource_type,
                    resource_id,
                    resource_version_id
                )
                .await?,
                "RESOURCE_UNAVAILABLE: {resource_type} {resource_id}"
            );
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
                    "resourceVersionId": resource_version_id,
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
                anyhow::ensure!(
                    crate::runtime_resources::resource_is_active(
                        &mut transaction,
                        command.tenant_id,
                        resource.reference.resource_type.as_str(),
                        resource.reference.resource_id,
                        resource.reference.resource_version_id
                    )
                    .await?,
                    "RESOURCE_UNAVAILABLE: {} {}",
                    resource.reference.resource_type.as_str(),
                    resource.reference.resource_id
                );
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
        let execution_scope_id = execution_id.to_string();
        crate::quota::reserve_with_admission(
            &mut transaction,
            &crate::quota::QuotaReservation {
                tenant_id: command.tenant_id,
                dimension: crate::quota::EXECUTION_CONCURRENCY,
                scope_type: "execution",
                scope_id: &execution_scope_id,
                amount: rust_decimal::Decimal::ONE,
                ttl_seconds: command
                    .runtime_settings
                    .get("timeoutSeconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(86_400),
                fail_closed: crate::config::is_production_environment(),
            },
            self.quota_admission.as_ref(),
        )
        .await?;
        let trace_id = command.trace_id.unwrap_or_else(Uuid::now_v7);
        let mut initial_context = initial_context(&compiled.start.contexts);
        if let (Some(target), Some(overlay)) = (
            initial_context.as_object_mut(),
            command.context_overlay.as_object(),
        ) {
            for (name, value) in overlay {
                if let Some(definition) = compiled.start.contexts.get(name) {
                    let validator = jsonschema::validator_for(&definition.schema)
                        .with_context(|| format!("Context schema '{name}' is invalid"))?;
                    validator.validate(value).map_err(|error| {
                        anyhow::anyhow!("CONTEXT_VALUE_INVALID: {name}: {error}")
                    })?;
                    if let Some(max_size) = definition.max_size {
                        anyhow::ensure!(
                            serde_json::to_vec(value)?.len() as u64 <= max_size,
                            "CONTEXT_VALUE_TOO_LARGE: {name}"
                        );
                    }
                    target.insert(name.clone(), value.clone());
                }
            }
        }
        let mut application_deployment_id = None;
        let mut session_context_version = 0_u64;
        if let Some(session_id) = command.session_id {
            let deployment_id: Uuid = sqlx::query_scalar("SELECT application_deployment_id FROM application_sessions WHERE tenant_id=? AND id=? AND status='active' FOR UPDATE")
                .bind(command.tenant_id).bind(session_id).fetch_one(&mut *transaction).await?;
            application_deployment_id = Some(deployment_id);
            let stored = sqlx::query("SELECT context_json,context_version FROM application_session_contexts WHERE tenant_id=? AND application_deployment_id=? AND session_id=? FOR UPDATE")
                .bind(command.tenant_id).bind(deployment_id).bind(session_id).fetch_optional(&mut *transaction).await?;
            let session_context = if let Some(row) = stored {
                session_context_version = row.try_get("context_version")?;
                row.try_get::<Value, _>("context_json")?
            } else {
                let value = scoped_context(&compiled.start.contexts, ContextScope::Session);
                sqlx::query("INSERT INTO application_session_contexts(tenant_id,application_deployment_id,session_id,context_json,context_version) VALUES(?,?,?,?,0)")
                    .bind(command.tenant_id).bind(deployment_id).bind(session_id).bind(&value).execute(&mut *transaction).await?;
                value
            };
            if let (Some(target), Some(session_values)) =
                (initial_context.as_object_mut(), session_context.as_object())
            {
                for (name, definition) in &compiled.start.contexts {
                    if definition.scope == ContextScope::Session
                        && let Some(value) = session_values.get(name)
                    {
                        target.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,source_kind,source_id,source_revision,invocation_id,session_id,application_deployment_id,parent_execution_id,caller_execution_id,caller_node_execution_id,fork_checkpoint_id,trace_id,trigger_type,execution_type,fork_mode,requested_by,input_json,context_json,context_base_json,context_version,session_context_version,status,started_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,0,?,'queued',CURRENT_TIMESTAMP(6))")
            .bind(execution_id).bind(command.tenant_id).bind(workflow_id).bind(workflow_version_id).bind(source_kind).bind(source_id).bind(source_revision)
            .bind(command.invocation_id).bind(command.session_id).bind(application_deployment_id).bind(command.parent_execution_id).bind(command.caller_execution_id).bind(command.caller_node_execution_id).bind(command.fork_checkpoint_id)
            .bind(trace_id).bind(&command.trigger_type).bind(&command.execution_type).bind(&command.fork_mode)
            .bind(command.requested_by).bind(&command.input).bind(&initial_context).bind(&initial_context).bind(session_context_version).execute(&mut *transaction).await?;
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
            self.quota_admission.as_ref(),
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
        let cancelled = sqlx::query_scalar::<_, bool>("SELECT e.cancellation_requested_at IS NOT NULL OR e.status IN ('cancelled','failed','timed_out') FROM node_attempts a JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.id=?")
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
        let row=sqlx::query("SELECT a.status,a.lease_token,n.node_id,n.run_index,e.workflow_id,e.workflow_version_id,e.trace_id,e.status execution_status,e.execution_type,e.input_json workflow_input_json,e.context_json,e.context_version,e.session_id,e.application_deployment_id,e.session_context_version FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id WHERE a.tenant_id=? AND a.execution_id=? AND a.node_execution_id=? AND a.id=? FOR UPDATE")
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
        let node_id: String = row.try_get("node_id")?;
        let compiled_node = machine
            .workflow()
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .cloned()
            .context("Compiled node is missing while committing task result")?;
        let mut result = match result {
            TaskResult::Completed(mut outputs) => {
                if compiled_node
                    .output_projection
                    .as_object()
                    .is_some_and(|value| !value.is_empty())
                {
                    let upstream =
                        load_output_namespace(&mut transaction, tenant_id, execution_id).await?;
                    apply_output_projection(
                        &mut outputs,
                        &compiled_node.output_projection,
                        &ExpressionContext {
                            inputs: row
                                .try_get::<Option<Value>, _>("workflow_input_json")?
                                .unwrap_or(Value::Null),
                            outputs: upstream,
                            contexts: row.try_get("context_json")?,
                            execution: json!({"executionId": execution_id, "nodeExecutionId": node_execution_id}),
                            ..ExpressionContext::default()
                        },
                    )?;
                }
                TaskResult::Completed(outputs)
            }
            other => other,
        };
        if matches!(result, TaskResult::Completed(_))
            && row.try_get::<String, _>("execution_type")? != "sub_workflow"
            && (is_subworkflow_type(&compiled_node.node_type)
                || compiled_node.context_writes.iter().any(|write| {
                    write
                        .path
                        .split('.')
                        .next()
                        .and_then(|name| machine.workflow().contexts.get(name))
                        .is_some_and(|definition| definition.scope == ContextScope::Session)
                }))
            && machine.workflow().contexts.values().any(|definition| {
                definition.scope == ContextScope::Session
                    && definition.merge_policy == ContextMergePolicy::RejectConflict
            })
            && let (Some(session_id), Some(deployment_id)) = (
                row.try_get::<Option<Uuid>, _>("session_id")?,
                row.try_get::<Option<Uuid>, _>("application_deployment_id")?,
            )
        {
            let stored_version: u64 = sqlx::query_scalar("SELECT context_version FROM application_session_contexts WHERE tenant_id=? AND application_deployment_id=? AND session_id=? FOR UPDATE")
                .bind(tenant_id)
                .bind(deployment_id)
                .bind(session_id)
                .fetch_one(&mut *transaction)
                .await?;
            if stored_version != row.try_get::<u64, _>("session_context_version")? {
                result = TaskResult::Failed {
                    code: "SESSION_CONTEXT_VERSION_CONFLICT".into(),
                    message: "Session Context changed after this execution started".into(),
                    retryable: false,
                };
            }
        }
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
        if let TaskResult::Completed(_) = &result
            && !machine.is_error_collecting()
            && (is_subworkflow_type(&compiled_node.node_type)
                || !compiled_node.context_writes.is_empty())
        {
            let outputs = load_output_namespace(&mut transaction, tenant_id, execution_id).await?;
            let mut contexts: Value = row.try_get("context_json")?;
            let mut child_context_changes = Vec::new();
            let mut child_overlay = None;
            let workflow_inputs = row
                .try_get::<Option<Value>, _>("workflow_input_json")?
                .unwrap_or(Value::Null);
            let version: u64 = row.try_get("context_version")?;
            if is_subworkflow_type(&compiled_node.node_type) {
                let child = sqlx::query("SELECT context_base_json,context_json FROM workflow_executions WHERE tenant_id=? AND caller_execution_id=? AND caller_node_execution_id=? AND status='succeeded' ORDER BY started_at DESC,id DESC LIMIT 1 FOR UPDATE")
                        .bind(tenant_id)
                        .bind(execution_id)
                        .bind(node_execution_id)
                        .fetch_optional(&mut *transaction)
                        .await?;
                if let Some(child) = child {
                    let base: Value = child.try_get("context_base_json")?;
                    let next: Value = child.try_get("context_json")?;
                    let parent = contexts
                        .as_object_mut()
                        .context("EXECUTION_CONTEXT_NOT_OBJECT")?;
                    for (name, definition) in &machine.workflow().contexts {
                        if !definition.mutable {
                            continue;
                        }
                        let (Some(base_value), Some(next_value)) = (base.get(name), next.get(name))
                        else {
                            continue;
                        };
                        if base_value == next_value {
                            continue;
                        }
                        let current = parent
                            .entry(name.clone())
                            .or_insert_with(|| definition.default.clone());
                        merge_context_overlay(
                            current,
                            base_value,
                            next_value,
                            definition.merge_policy,
                        )?;
                        child_context_changes.push((name.clone(), current.clone()));
                    }
                    child_overlay = Some((base, next));
                }
            }
            let expression_base = ExpressionContext {
                inputs: workflow_inputs.clone(),
                outputs: outputs.clone(),
                contexts: contexts.clone(),
                execution: json!({
                    "executionId": execution_id,
                    "nodeExecutionId": node_execution_id,
                    "runIndex": row.try_get::<u32, _>("run_index")?,
                    "contextVersion": version,
                }),
                ..ExpressionContext::default()
            };
            if !compiled_node.context_writes.is_empty() {
                apply_context_writes(
                    &mut contexts,
                    &compiled_node.context_writes,
                    &machine.workflow().contexts,
                    &expression_base,
                )?;
            }
            let session_writes = compiled_node
                .context_writes
                .iter()
                .filter(|write| {
                    write
                        .path
                        .split('.')
                        .next()
                        .and_then(|name| machine.workflow().contexts.get(name))
                        .is_some_and(|definition| definition.scope == ContextScope::Session)
                })
                .cloned()
                .collect::<Vec<_>>();
            let mut committed_session_version = None;
            let is_child_execution = row.try_get::<String, _>("execution_type")? == "sub_workflow";
            if let (Some(session_id), Some(deployment_id)) = (
                row.try_get::<Option<Uuid>, _>("session_id")?,
                row.try_get::<Option<Uuid>, _>("application_deployment_id")?,
            ) && !is_child_execution
                && (is_subworkflow_type(&compiled_node.node_type) || !session_writes.is_empty())
            {
                let stored = sqlx::query("SELECT context_json,context_version FROM application_session_contexts WHERE tenant_id=? AND application_deployment_id=? AND session_id=? FOR UPDATE")
                        .bind(tenant_id).bind(deployment_id).bind(session_id).fetch_one(&mut *transaction).await?;
                let mut session_context: Value = stored.try_get("context_json")?;
                let stored_version: u64 = stored.try_get("context_version")?;
                let execution_session_version: u64 = row.try_get("session_context_version")?;
                let rejects_conflict = machine.workflow().contexts.values().any(|definition| {
                    definition.scope == ContextScope::Session
                        && definition.merge_policy == ContextMergePolicy::RejectConflict
                });
                anyhow::ensure!(
                    !rejects_conflict || stored_version == execution_session_version,
                    "SESSION_CONTEXT_VERSION_CONFLICT"
                );
                if is_subworkflow_type(&compiled_node.node_type) {
                    if let Some((child_base, child_next)) = &child_overlay {
                        for (name, definition) in &machine.workflow().contexts {
                            if definition.scope != ContextScope::Session || !definition.mutable {
                                continue;
                            }
                            let Some(base) = child_base.get(name) else {
                                continue;
                            };
                            let Some(next) = child_next.get(name) else {
                                continue;
                            };
                            if base == next {
                                continue;
                            }
                            let current = session_context
                                .as_object_mut()
                                .context("SESSION_CONTEXT_NOT_OBJECT")?
                                .entry(name.clone())
                                .or_insert_with(|| definition.default.clone());
                            merge_context_overlay(current, base, next, definition.merge_policy)?;
                        }
                    }
                }
                if !session_writes.is_empty() {
                    let mut session_expression_base = expression_base.clone();
                    session_expression_base.contexts = session_context.clone();
                    apply_context_writes(
                        &mut session_context,
                        &session_writes,
                        &machine.workflow().contexts,
                        &session_expression_base,
                    )?;
                }
                let changed = sqlx::query("UPDATE application_session_contexts SET context_json=?,context_version=context_version+1 WHERE tenant_id=? AND application_deployment_id=? AND session_id=? AND context_version=?")
                        .bind(&session_context).bind(tenant_id).bind(deployment_id).bind(session_id).bind(stored_version).execute(&mut *transaction).await?;
                anyhow::ensure!(
                    changed.rows_affected() == 1,
                    "SESSION_CONTEXT_VERSION_CONFLICT"
                );
                committed_session_version = Some(stored_version + 1);
                if let (Some(target), Some(stored_values)) =
                    (contexts.as_object_mut(), session_context.as_object())
                {
                    for (name, definition) in &machine.workflow().contexts {
                        if definition.scope == ContextScope::Session
                            && let Some(value) = stored_values.get(name)
                        {
                            target.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
            let changed = sqlx::query("UPDATE workflow_executions SET context_json=?,context_version=context_version+1,session_context_version=COALESCE(?,session_context_version) WHERE tenant_id=? AND id=? AND context_version=?")
                    .bind(contexts)
                    .bind(committed_session_version)
                    .bind(tenant_id)
                    .bind(execution_id)
                    .bind(version)
                    .execute(&mut *transaction)
                    .await?;
            anyhow::ensure!(changed.rows_affected() == 1, "CONTEXT_VERSION_CONFLICT");
            for (patch_index, write) in compiled_node.context_writes.iter().enumerate() {
                let operation = serde_json::to_value(write.operation)?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned();
                sqlx::query("INSERT INTO workflow_context_patches(id,tenant_id,execution_id,node_execution_id,attempt_id,patch_index,operation_key,context_path,value_json,context_version_before,context_version_after) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                        .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(attempt_id)
                        .bind(patch_index as u32).bind(operation).bind(&write.path).bind(&write.value).bind(version).bind(version + 1)
                        .execute(&mut *transaction).await?;
            }
            for (offset, (path, value)) in child_context_changes.iter().enumerate() {
                sqlx::query("INSERT INTO workflow_context_patches(id,tenant_id,execution_id,node_execution_id,attempt_id,patch_index,operation_key,context_path,value_json,context_version_before,context_version_after) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                        .bind(Uuid::now_v7()).bind(tenant_id).bind(execution_id).bind(node_execution_id).bind(attempt_id)
                        .bind((compiled_node.context_writes.len() + offset) as u32).bind("merge_overlay").bind(path).bind(value).bind(version).bind(version + 1)
                        .execute(&mut *transaction).await?;
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
        let execution_error = transition_error
            .as_ref()
            .map(|(code, message)| ((*code).to_owned(), message.clone()))
            .or_else(|| {
                if machine.status() == RuntimeExecutionStatus::Failed
                    && let TaskResult::Failed { code, message, .. } = &result
                {
                    Some((code.clone(), message.clone()))
                } else {
                    None
                }
            });
        if let Some((code, message)) = execution_error {
            sqlx::query("UPDATE workflow_executions SET error_code=?,error_message=? WHERE tenant_id=? AND id=?")
                .bind(code).bind(message).bind(tenant_id).bind(execution_id)
                .execute(&mut *transaction).await?;
        }
        sync_execution_status(
            &mut transaction,
            tenant_id,
            execution_id,
            machine.status(),
            self.quota_admission.as_ref(),
        )
        .await?;
        if let Some((code, message)) = transition_error {
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
        sqlx::query("UPDATE workflow_executions SET status='cancelled',cancellation_requested_at=CURRENT_TIMESTAMP(6),ended_at=CURRENT_TIMESTAMP(6),duration_ms=TIMESTAMPDIFF(MICROSECOND,started_at,CURRENT_TIMESTAMP(6))/1000,terminal_event_emitted=TRUE WHERE tenant_id=? AND id=?")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE execution_resume_tokens SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='active'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE wait_subscriptions SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='waiting'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE resume_webhook_bindings b JOIN wait_subscriptions w ON w.id=b.wait_subscription_id AND w.tenant_id=b.tenant_id SET b.status='cancelled' WHERE w.tenant_id=? AND w.execution_id=? AND b.status='active'")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE approval_tasks SET status='cancelled',resume_status='succeeded',version=version+1 WHERE tenant_id=? AND execution_id=? AND status IN ('pending','claimed')")
            .bind(tenant_id).bind(execution_id).execute(&mut *transaction).await?;
        let mut pending_parents = vec![execution_id];
        while let Some(parent_id) = pending_parents.pop() {
            let children = sqlx::query_scalar::<_, Uuid>("SELECT id FROM workflow_executions WHERE tenant_id=? AND (parent_execution_id=? OR caller_execution_id=?) AND status NOT IN ('succeeded','failed','cancelled','timed_out') FOR UPDATE")
                .bind(tenant_id).bind(parent_id).bind(parent_id).fetch_all(&mut *transaction).await?;
            for child_id in children {
                sqlx::query("UPDATE workflow_executions SET status='cancelled',cancellation_requested_at=CURRENT_TIMESTAMP(6),ended_at=CURRENT_TIMESTAMP(6),duration_ms=TIMESTAMPDIFF(MICROSECOND,started_at,CURRENT_TIMESTAMP(6))/1000,terminal_event_emitted=TRUE WHERE tenant_id=? AND id=?")
                    .bind(tenant_id).bind(child_id).execute(&mut *transaction).await?;
                sqlx::query("UPDATE execution_resume_tokens SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='active'")
                    .bind(tenant_id).bind(child_id).execute(&mut *transaction).await?;
                sqlx::query("UPDATE wait_subscriptions SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='waiting'")
                    .bind(tenant_id).bind(child_id).execute(&mut *transaction).await?;
                sqlx::query("UPDATE approval_tasks SET status='cancelled',resume_status='succeeded',version=version+1 WHERE tenant_id=? AND execution_id=? AND status IN ('pending','claimed')")
                    .bind(tenant_id).bind(child_id).execute(&mut *transaction).await?;
                insert_execution_event(
                    &mut transaction,
                    tenant_id,
                    child_id,
                    "execution.cancelled",
                    "cancelled",
                    json!({"reason":"parent_cancelled","parentExecutionId":parent_id}),
                )
                .await?;
                pending_parents.push(child_id);
            }
        }
        crate::quota::release_scope_with_admission(
            &mut transaction,
            tenant_id,
            "execution",
            &execution_id.to_string(),
            self.quota_admission.as_ref(),
        )
        .await?;
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
        let resumed_outputs = resumed_output_map(&command.output_port, &command.payload);
        let resumed_items = resumed_outputs
            .get(&command.output_port)
            .cloned()
            .unwrap_or_default();
        let node_execution_id = NodeExecutionId::from_uuid(command.node_execution_id);
        machine.resume(node_execution_id, &command.output_port, resumed_items)?;
        let resumed_output_json = serde_json::to_value(&resumed_outputs)?;
        let attempt_id = machine
            .activation(node_execution_id)
            .and_then(|activation| activation.attempts.last())
            .map(|attempt| attempt.id.as_uuid())
            .context("Resumed activation has no attempt")?;
        sqlx::query("UPDATE node_attempts SET output_json=? WHERE id=?")
            .bind(&resumed_output_json)
            .bind(attempt_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE node_executions SET output_json=? WHERE id=?")
            .bind(resumed_output_json)
            .bind(command.node_execution_id)
            .execute(&mut *transaction)
            .await?;
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
        insert_execution_event(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            "execution.resumed",
            "running",
            json!({"nodeExecutionId":command.node_execution_id,"outputPort":command.output_port}),
        )
        .await?;
        sync_execution_status(
            &mut transaction,
            command.tenant_id,
            command.execution_id,
            machine.status(),
            self.quota_admission.as_ref(),
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

    pub async fn finalize_error_collections(&self) -> Result<u64> {
        let rows = sqlx::query("SELECT d.tenant_id,d.execution_id,MIN(d.created_at) started_at FROM execution_end_deliveries d JOIN workflow_executions e ON e.tenant_id=d.tenant_id AND e.id=d.execution_id WHERE d.target_port='error' AND e.status='running' GROUP BY d.tenant_id,d.execution_id ORDER BY started_at LIMIT 100")
            .fetch_all(&self.pool)
            .await?;
        let mut finalized = 0;
        for row in rows {
            let tenant_id: Uuid = row.try_get("tenant_id")?;
            let execution_id: Uuid = row.try_get("execution_id")?;
            let started_at: OffsetDateTime = row.try_get("started_at")?;
            let mut transaction = self.pool.begin().await?;
            let status = sqlx::query_scalar::<_, String>(
                "SELECT status FROM workflow_executions WHERE tenant_id=? AND id=? FOR UPDATE",
            )
            .bind(tenant_id)
            .bind(execution_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if status.as_deref() != Some("running") {
                transaction.rollback().await?;
                continue;
            }
            let mut machine = self
                .load_machine(&mut transaction, tenant_id, execution_id)
                .await?;
            let collect_window =
                Duration::milliseconds(machine.workflow().end.error.collect_window_ms as i64);
            if !machine.is_error_collecting()
                || OffsetDateTime::now_utc() < started_at + collect_window
            {
                transaction.rollback().await?;
                continue;
            }
            machine.finish_error_collection();
            persist_machine(
                &mut transaction,
                tenant_id,
                execution_id,
                &machine,
                "manual",
            )
            .await?;
            sqlx::query("UPDATE execution_outbox SET status='failed',last_error='END_ERROR_COLLECT_WINDOW_CLOSED',locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND execution_id=? AND status='pending'")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("UPDATE worker_leases l JOIN node_attempts a ON a.id=l.node_attempt_id SET l.released_at=CURRENT_TIMESTAMP(6) WHERE a.tenant_id=? AND a.execution_id=? AND l.released_at IS NULL")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("UPDATE execution_resume_tokens SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='active'")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("UPDATE wait_subscriptions SET status='cancelled' WHERE tenant_id=? AND execution_id=? AND status='waiting'")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("UPDATE approval_tasks SET status='cancelled',resume_status='succeeded',version=version+1 WHERE tenant_id=? AND execution_id=? AND status IN ('pending','claimed')")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("UPDATE workflow_executions SET error_code='WORKFLOW_FAILED',error_message='Workflow reached End.error' WHERE tenant_id=? AND id=?")
                .bind(tenant_id)
                .bind(execution_id)
                .execute(&mut *transaction)
                .await?;
            sync_execution_status(
                &mut transaction,
                tenant_id,
                execution_id,
                machine.status(),
                self.quota_admission.as_ref(),
            )
            .await?;
            transaction.commit().await?;
            finalized += 1;
        }
        Ok(finalized)
    }

    async fn load_task(&self, message: &DispatchMessage) -> Result<RuntimeTask> {
        let row=sqlx::query("SELECT n.node_id,n.node_type,n.node_version,n.input_json,n.run_index,n.iteration_index,n.capability,a.attempt_number,a.idempotency_key,a.deadline_at,e.workflow_id,e.workflow_version_id,e.trace_id,e.execution_type,e.input_json workflow_input_json,e.context_json,e.context_version,s.compiled_ir_json,s.definition_json,s.resource_snapshot_json FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id JOIN workflow_executions e ON e.id=a.execution_id JOIN execution_snapshots s ON s.execution_id=e.id WHERE a.tenant_id=? AND a.id=?")
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
        let linked_rows=sqlx::query("SELECT node_key,run_index,output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND output_json IS NOT NULL ORDER BY run_index,id")
            .bind(message.tenant_id).bind(message.execution_id).fetch_all(&self.pool).await?;
        let mut linked_nodes = serde_json::Map::new();
        for linked in linked_rows {
            let name: String = linked.try_get("node_key")?;
            let run: u32 = linked.try_get("run_index")?;
            let outputs: Value = linked.try_get("output_json")?;
            merge_output_namespace(&mut linked_nodes, &name, run, &outputs);
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
            workflow_inputs: row
                .try_get::<Option<Value>, _>("workflow_input_json")?
                .unwrap_or(Value::Null),
            contexts: row.try_get("context_json")?,
            context_version: row.try_get("context_version")?,
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

    pub async fn externalize_execution_results(&self, limit: u32) -> Result<u64> {
        let Some(store) = &self.checkpoint_artifacts else {
            return Ok(0);
        };
        crate::runtime_results::externalize_execution_results(
            &self.pool,
            store.as_ref(),
            self.checkpoint_artifact_threshold,
            limit,
        )
        .await
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
        sqlx::query("INSERT IGNORE INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'checkpoint',?,'state')")
            .bind(tenant_id)
            .bind(artifact_id)
            .bind(checkpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
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
                node_protocol_version: agentx_node_protocol::NODE_PROTOCOL_VERSION.into(),
                compiler_version: machine.workflow().compiler_version.clone(),
                ir_schema_version: machine.workflow().schema_version.clone(),
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
