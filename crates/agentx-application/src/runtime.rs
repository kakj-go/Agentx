use std::{fmt, pin::Pin};

use agentx_domain::{
    ArtifactId, AttemptId, ExecutionId, NodeExecutionId, ResourceReference, TenantId, TraceId,
    WorkflowId, WorkflowServiceIdentityId, WorkflowVersionId,
};
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type RuntimeResult<T> = Result<T, RuntimeError>;
pub type RuntimeStream<T> = Pin<Box<dyn Stream<Item = RuntimeResult<T>> + Send + 'static>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeResourceSnapshot {
    pub node_id: String,
    pub reference: ResourceReference,
    pub snapshot_hash: String,
    pub snapshot: Value,
}

#[derive(Clone, Debug)]
pub struct RuntimeContext {
    pub tenant_id: TenantId,
    pub workflow_service_identity_id: WorkflowServiceIdentityId,
    pub workflow_id: WorkflowId,
    pub workflow_version_id: Option<WorkflowVersionId>,
    pub execution_id: ExecutionId,
    pub node_execution_id: NodeExecutionId,
    pub attempt_id: AttemptId,
    pub lease_token: Uuid,
    pub trace_id: TraceId,
    pub span_id: Uuid,
    pub deadline: OffsetDateTime,
    pub cancellation: CancellationToken,
    pub idempotency_key: String,
    pub resources: Vec<RuntimeResourceSnapshot>,
}

impl RuntimeContext {
    #[must_use]
    pub fn resource(&self, reference: &ResourceReference) -> Option<&RuntimeResourceSnapshot> {
        self.resources.iter().find(|candidate| {
            candidate.reference.resource_type == reference.resource_type
                && candidate.reference.resource_id == reference.resource_id
                && candidate.reference.operation == reference.operation
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub outcome_unknown: bool,
    pub partial: bool,
    #[serde(default)]
    pub attributes: Value,
}

impl RuntimeError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            outcome_unknown: false,
            partial: false,
            attributes: Value::Object(Default::default()),
        }
    }

    #[must_use]
    pub const fn retryable(mut self, value: bool) -> Self {
        self.retryable = value;
        self
    }

    #[must_use]
    pub const fn outcome_unknown(mut self, value: bool) -> Self {
        self.outcome_unknown = value;
        self
    }

    #[must_use]
    pub const fn partial(mut self, value: bool) -> Self {
        self.partial = value;
        self
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub resource: ResourceReference,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelEvent {
    TextDelta { text: String },
    ToolCallDelta { index: u32, delta: Value },
    Completed { response: ModelResponse },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    pub message: Value,
    #[serde(default)]
    pub tool_calls: Vec<Value>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micros: u64,
    pub usage_estimated: bool,
    pub stop_reason: String,
    pub partial: bool,
}

#[async_trait]
pub trait ModelRuntime: Send + Sync {
    async fn complete(
        &self,
        context: &RuntimeContext,
        request: ModelRequest,
    ) -> RuntimeResult<ModelResponse>;
    async fn stream(
        &self,
        context: &RuntimeContext,
        request: ModelRequest,
    ) -> RuntimeResult<RuntimeStream<ModelEvent>>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolRequest {
    pub resource: ResourceReference,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolResponse {
    pub content: Value,
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

#[async_trait]
pub trait McpToolRuntime: Send + Sync {
    async fn call(
        &self,
        context: &RuntimeContext,
        request: McpToolRequest,
    ) -> RuntimeResult<McpToolResponse>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBundle {
    pub instructions: String,
    pub files: Vec<SkillFile>,
    pub dependencies: Vec<RuntimeResourceSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    pub path: String,
    pub artifact_id: ArtifactId,
    pub content_hash: String,
    pub mime_type: String,
}

#[async_trait]
pub trait SkillRuntime: Send + Sync {
    async fn load(
        &self,
        context: &RuntimeContext,
        resource: ResourceReference,
    ) -> RuntimeResult<SkillBundle>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RagOperation {
    Query,
    Retrieve,
    Insert,
    Delete,
    HealthCheck,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RagRequest {
    pub resource: ResourceReference,
    pub operation: RagOperation,
    pub input: Value,
}

#[async_trait]
pub trait RagRuntime: Send + Sync {
    async fn execute(&self, context: &RuntimeContext, request: RagRequest) -> RuntimeResult<Value>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperation {
    Get,
    Search,
    Add,
    Update,
    Delete,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRequest {
    pub resource: ResourceReference,
    pub operation: MemoryOperation,
    pub input: Value,
}

#[async_trait]
pub trait MemoryRuntime: Send + Sync {
    async fn execute(
        &self,
        context: &RuntimeContext,
        request: MemoryRequest,
    ) -> RuntimeResult<Value>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCreateRequest {
    pub profile: RuntimeResourceSnapshot,
    pub labels: Value,
    pub network_policy: Value,
    #[serde(default)]
    pub credentials: Vec<SandboxCredentialHandle>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCredentialHandle {
    pub resource_id: Uuid,
    pub handle: String,
    pub environment_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxLease {
    pub lease_id: Uuid,
    pub sandbox_id: String,
    pub lease_token: String,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCommand {
    pub lease: SandboxLease,
    pub argv: Vec<String>,
    #[serde(default)]
    pub environment: Value,
    pub working_directory: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxEvent {
    Stdout { sequence: u64, data: Vec<u8> },
    Stderr { sequence: u64, data: Vec<u8> },
    Completed { exit_code: i32, partial: bool },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxMetrics {
    pub cpu_nanos: u64,
    pub memory_bytes: u64,
    pub pids: u64,
    pub disk_bytes: u64,
    pub raw: Value,
}

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn create(
        &self,
        context: &RuntimeContext,
        request: SandboxCreateRequest,
    ) -> RuntimeResult<SandboxLease>;
    async fn execute(
        &self,
        context: &RuntimeContext,
        command: SandboxCommand,
    ) -> RuntimeResult<RuntimeStream<SandboxEvent>>;
    async fn interrupt(&self, context: &RuntimeContext, lease: &SandboxLease) -> RuntimeResult<()>;
    async fn upload(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
        content: Vec<u8>,
    ) -> RuntimeResult<()>;
    async fn download(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
        path: &str,
    ) -> RuntimeResult<Vec<u8>>;
    async fn metrics(
        &self,
        context: &RuntimeContext,
        lease: &SandboxLease,
    ) -> RuntimeResult<SandboxMetrics>;
    async fn terminate(&self, context: &RuntimeContext, lease: &SandboxLease) -> RuntimeResult<()>;
}
