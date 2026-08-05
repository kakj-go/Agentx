use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateSandboxRequest {
    pub image: ImageSpec,
    pub timeout: u64,
    pub resource_limits: BTreeMap<String, String>,
    pub entrypoint: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    pub network_policy: Value,
    pub secure_access: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImageSpec {
    pub uri: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxResponse {
    pub id: String,
    pub status: SandboxStatus,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(rename = "expiresAt")]
    pub _expires_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SandboxStatus {
    pub state: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ListSandboxesResponse {
    pub items: Vec<SandboxResponse>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct EndpointResponse {
    pub endpoint: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RunCommandRequest {
    pub command: String,
    pub cwd: String,
    pub background: bool,
    pub timeout: u64,
    #[serde(rename = "envs")]
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CommandStreamEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    pub exit_code: Option<i32>,
    pub error: Option<CommandError>,
    #[serde(default)]
    pub evalue: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CommandError {
    #[serde(default)]
    pub evalue: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ExecdMetrics {
    pub cpu_count: f64,
    pub cpu_used_pct: f64,
    pub mem_total_mib: f64,
    pub mem_used_mib: f64,
    pub timestamp: i64,
}
