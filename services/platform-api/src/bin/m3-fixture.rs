use agentx_application::{
    ApprovalTaskPort, ArtifactStore, ArtifactWrite, ExecutionProjectionPort, NotificationPublisher,
    TraceSink,
};
use agentx_domain::{
    ApprovalTaskId, ExecutionId, ExecutionStatus, ExecutionSummary, NotificationId, TenantId,
    TraceEvent, TraceId, WorkflowId, WorkflowVersionId,
};
use agentx_infrastructure::{
    artifact::MySqlObjectArtifactStore, clients, config::InfrastructureSettings, mysql,
    operations_projection::MySqlOperationsProjection,
};
use anyhow::{Context, Result};
use serde_json::json;
use sqlx::Row;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let settings = InfrastructureSettings::from_env()?;
    let pool = mysql::connect(&settings.mysql).await?;
    let row = sqlx::query("SELECT t.id tenant_id,u.id user_id,w.id workflow_id,wv.id workflow_version_id FROM tenants t JOIN users u ON u.tenant_id=t.id AND u.status='active' JOIN workflows w ON w.tenant_id=t.id AND w.status='active' JOIN workflow_versions wv ON wv.workflow_id=w.id AND wv.tenant_id=t.id ORDER BY u.created_at,wv.version_number DESC LIMIT 1")
        .fetch_optional(&pool).await?.context("M3 E2E control-plane data is missing")?;
    let tenant = TenantId::from_uuid(row.try_get("tenant_id")?);
    let user: Uuid = row.try_get("user_id")?;
    let workflow = WorkflowId::from_uuid(row.try_get("workflow_id")?);
    let version = WorkflowVersionId::from_uuid(row.try_get("workflow_version_id")?);
    let artifact_store = MySqlObjectArtifactStore::new(
        pool.clone(),
        clients::object_store(&settings.object_storage)?,
    );
    let artifact = artifact_store
        .put(ArtifactWrite {
            tenant_id: tenant,
            content_type: "text/plain".into(),
            content: b"Agentx M3 trace artifact".to_vec(),
        })
        .await?;
    let projection = MySqlOperationsProjection::new(pool);
    let now = OffsetDateTime::now_utc();
    let execution = ExecutionId::new();
    let trace = TraceId::new();
    let trace_context = TraceContext {
        tenant_id: tenant,
        execution_id: execution,
        workflow_id: workflow,
        workflow_version_id: version,
        trace_id: trace,
    };
    projection
        .create(&ExecutionSummary {
            id: execution,
            tenant_id: tenant,
            workflow_id: workflow,
            workflow_version_id: Some(version),
            invocation_id: None,
            session_id: None,
            trace_id: trace,
            trigger_type: "e2e_fixture".into(),
            status: ExecutionStatus::WaitingApproval,
            started_at: now - Duration::seconds(3),
            ended_at: None,
            duration_ms: None,
            cost_micros: 182_000,
            error_code: None,
            error_message: None,
        })
        .await?;
    let root_span = Uuid::now_v7();
    projection
        .append(event(
            &trace_context,
            root_span,
            None,
            "workflow.started",
            "running",
            now - Duration::seconds(3),
            json!({"source":"m3-e2e"}),
        ))
        .await?;
    let mut model = event(
        &trace_context,
        Uuid::now_v7(),
        Some(root_span),
        "model.completed",
        "succeeded",
        now - Duration::seconds(2),
        json!({"nodeId":"model-1","authorization":"must-not-leak"}),
    );
    model.model_name = Some("echo-model-v2".into());
    model.provider_name = Some("E2E Provider".into());
    model.input_tokens = Some(24);
    model.output_tokens = Some(11);
    model.cost_micros = 182_000;
    model.duration_ms = Some(420);
    model.content_ref = Some(artifact.id);
    projection.append(model).await?;
    projection
        .publish(
            tenant,
            NotificationId::new(),
            json!({
                "recipientUserId":user,
                "notificationType":"execution_fixture",
                "titleKey":"notifications.execution.title",
                "bodyKey":"notifications.execution.description",
                "arguments":{},
                "targetType":"execution",
                "targetId":execution.as_uuid(),
                "targetPath":format!("/executions/{}", execution.as_uuid()),
                "tone":"danger"
            }),
        )
        .await?;
    projection.create_task(tenant, ApprovalTaskId::new(), json!({"executionId":execution.as_uuid(),"workflowId":workflow.as_uuid(),"candidateUserId":user,"nodeId":"approval-1","title":"M3 E2E 发布审批","description":"验证领取、释放和终态审批动作。","requestPayload":{"risk":"medium"},"deadlineAt":(now+Duration::hours(24)).format(&time::format_description::well_known::Rfc3339)?})).await?;
    Ok(())
}

struct TraceContext {
    tenant_id: TenantId,
    execution_id: ExecutionId,
    workflow_id: WorkflowId,
    workflow_version_id: WorkflowVersionId,
    trace_id: TraceId,
}

fn event(
    context: &TraceContext,
    span_id: Uuid,
    parent_span_id: Option<Uuid>,
    event_type: &str,
    status: &str,
    event_time: OffsetDateTime,
    attributes: serde_json::Value,
) -> TraceEvent {
    TraceEvent {
        event_id: Uuid::now_v7(),
        tenant_id: context.tenant_id,
        trace_id: context.trace_id,
        span_id,
        parent_span_id,
        execution_id: context.execution_id,
        workflow_id: context.workflow_id,
        workflow_version_id: Some(context.workflow_version_id),
        node_execution_id: None,
        attempt_id: None,
        agent_run_id: None,
        runtime_call_id: None,
        sandbox_id: None,
        resource_type: None,
        resource_id: None,
        resource_version_id: None,
        event_type: event_type.into(),
        status: status.into(),
        event_time,
        run_index: 0,
        iteration_index: 0,
        duration_ms: None,
        model_name: None,
        provider_name: None,
        mcp_tool_name: None,
        input_tokens: None,
        output_tokens: None,
        cost_micros: 0,
        error_code: None,
        error_message: None,
        stop_reason: None,
        partial: false,
        content_ref: None,
        attributes,
    }
}
