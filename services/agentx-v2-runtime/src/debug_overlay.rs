use std::collections::BTreeMap;

use agentx_node_protocol::Item;
use agentx_runtime_contracts::{RuntimeDebugInputSourceV1, RuntimeDebugPlanV1};
use serde_json::Value;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Clone, Debug)]
pub(crate) struct DebugOverlay {
    pub kind: String,
    pub payload: Value,
}

pub(crate) fn for_node(runtime_settings: &Value, node_id: &str) -> Option<DebugOverlay> {
    let overlay = runtime_settings
        .get("workPackageOverlay")?
        .get("nodeParameters")?
        .get(node_id)?
        .get("debugOverlay")?;
    Some(DebugOverlay {
        kind: overlay.get("kind")?.as_str()?.to_owned(),
        payload: overlay.get("payload")?.clone(),
    })
}

pub(crate) fn items(payload: &Value) -> BTreeMap<String, Vec<Item>> {
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

pub(crate) fn completes_node(kind: &str) -> bool {
    matches!(
        kind,
        "pin_data" | "mock_output" | "history_output" | "artifact"
    )
}

pub(crate) fn plan(runtime_settings: &Value) -> RuntimeResult<Option<RuntimeDebugPlanV1>> {
    let Some(value) = runtime_settings
        .get("workPackageSpec")
        .and_then(|spec| {
            (spec.get("kind").and_then(Value::as_str) == Some("debug")).then_some(spec)
        })
        .and_then(|spec| spec.get("debugPlan"))
    else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| invalid("DEBUG_PLAN_INVALID", &error.to_string()))
}

pub(crate) async fn resolve_input_source(
    state: Option<&crate::RuntimeState>,
    pool: &MySqlPool,
    tenant_id: Uuid,
    plan: Option<&RuntimeDebugPlanV1>,
) -> RuntimeResult<Option<Vec<Item>>> {
    let Some(source) = plan.and_then(|plan| plan.input_source.as_ref()) else {
        return Ok(None);
    };
    match source {
        RuntimeDebugInputSourceV1::Manual { value } => Ok(Some(vec![Item {
            json: value.clone(),
            ..Item::default()
        }])),
        RuntimeDebugInputSourceV1::HistoryOutput {
            execution_id,
            node_execution_id,
            output_port,
        } => {
            let output: Value = sqlx::query_scalar(
                "SELECT output_json FROM node_executions WHERE tenant_id=? AND execution_id=? AND id=? AND status='succeeded' AND output_json IS NOT NULL",
            )
            .bind(tenant_id)
            .bind(execution_id)
            .bind(node_execution_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| {
                invalid(
                    "DEBUG_HISTORY_OUTPUT_NOT_FOUND",
                    "The selected successful node output is unavailable",
                )
            })?;
            let outputs: BTreeMap<String, Vec<Item>> = serde_json::from_value(output)
                .map_err(|error| invalid("DEBUG_HISTORY_OUTPUT_INVALID", &error.to_string()))?;
            outputs.get(output_port).cloned().map(Some).ok_or_else(|| {
                invalid(
                    "DEBUG_HISTORY_PORT_NOT_FOUND",
                    "The selected node output port is unavailable",
                )
            })
        }
        RuntimeDebugInputSourceV1::Artifact {
            execution_id,
            artifact_id,
        } => {
            let state = state.ok_or_else(|| {
                invalid(
                    "DEBUG_ARTIFACT_SOURCE_UNAVAILABLE",
                    "Artifact Debug input requires the Runtime object store",
                )
            })?;
            let row = sqlx::query(
                "SELECT a.sha256 FROM artifacts a WHERE a.tenant_id=? AND a.id=? AND a.deleted_at IS NULL AND EXISTS(SELECT 1 FROM artifact_references r WHERE r.tenant_id=a.tenant_id AND r.artifact_id=a.id AND (r.owner_type='execution' AND r.owner_id=? OR EXISTS(SELECT 1 FROM node_executions n WHERE n.tenant_id=a.tenant_id AND n.execution_id=? AND r.owner_id=BIN_TO_UUID(n.id))))",
            )
            .bind(tenant_id)
            .bind(artifact_id)
            .bind(execution_id.to_string())
            .bind(execution_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| {
                invalid(
                    "DEBUG_ARTIFACT_NOT_FOUND",
                    "The selected Execution artifact is unavailable",
                )
            })?;
            let expected_hash = format!("sha256:{}", row.try_get::<String, _>("sha256")?);
            let bytes =
                crate::artifact::load_artifact(state, tenant_id, *artifact_id, &expected_hash)
                    .await?;
            let payload: Value = serde_json::from_slice(&bytes)
                .map_err(|error| invalid("DEBUG_ARTIFACT_INVALID", &error.to_string()))?;
            let mut outputs = items(&payload);
            outputs
                .remove("main")
                .or_else(|| outputs.into_values().next())
                .map(Some)
                .ok_or_else(|| invalid("DEBUG_ARTIFACT_EMPTY", "Debug artifact contains no items"))
        }
    }
}

fn invalid(code: &'static str, message: &str) -> RuntimeError {
    RuntimeError::InvalidRequest(code, message.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{completes_node, for_node, items};

    #[test]
    fn reads_the_signed_work_package_overlay_snapshot() {
        let settings = json!({
            "workPackageOverlay": {
                "nodeParameters": {
                    "agent-1": {
                        "debugOverlay": {
                            "id":"00000000-0000-0000-0000-000000000001",
                            "kind":"mock_output",
                            "payload":{"main":[{"json":{"answer":"mocked"}}]},
                            "artifactId":null
                        }
                    }
                }
            }
        });
        let overlay = for_node(&settings, "agent-1").unwrap();
        assert_eq!(overlay.kind, "mock_output");
        assert_eq!(
            items(&overlay.payload)["main"][0].json,
            json!({"answer":"mocked"})
        );
        assert!(completes_node(&overlay.kind));
    }
}
