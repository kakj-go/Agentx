use std::{collections::BTreeMap, future::Future};

use agentx_runtime_contracts::{RuntimeResourceBindingV1, RuntimeResourceConfigurationV1};
use axum::{Json, Router, extract::State, routing::post};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{StatusCode, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use crate::error::{RuntimeError, RuntimeResult};

const LEASE_SECONDS: u32 = 30;
const EXECD_PORT: u16 = 44_772;

#[derive(Clone)]
pub struct SandboxManagerState {
    pub pool: MySqlPool,
    pub client: reqwest::Client,
    pub provider_endpoint: String,
    pub provider_api_key: Option<String>,
    pub provider_secure_access: bool,
    pub owner: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxExecuteRequestV1 {
    pub api_version: u32,
    pub tenant_id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: Uuid,
    pub fencing_token: u64,
    pub idempotency_key: String,
    pub profile: RuntimeResourceBindingV1,
    pub input: Value,
    pub parameters: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxExecuteResponseV1 {
    pub api_version: u32,
    pub lease_id: Uuid,
    pub sandbox_id: Option<String>,
    pub replayed: bool,
    pub output: Value,
}

pub fn router(state: SandboxManagerState) -> Router {
    Router::new()
        .route("/internal/runtime/v1/sandboxes:execute", post(execute))
        .with_state(state)
}

async fn execute(
    State(state): State<SandboxManagerState>,
    Json(request): Json<SandboxExecuteRequestV1>,
) -> RuntimeResult<Json<SandboxExecuteResponseV1>> {
    if request.api_version != 1 || request.idempotency_key.trim().is_empty() {
        return Err(bad_request("invalid Sandbox Manager request"));
    }
    let RuntimeResourceConfigurationV1::SandboxProfile {
        maximum_ttl_seconds,
        ..
    } = &request.profile.configuration
    else {
        return Err(bad_request(
            "Sandbox request requires an immutable Sandbox Profile",
        ));
    };
    let attempt_valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM node_attempts WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND id=? AND status='running' AND worker_instance_id=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6))",
    )
    .bind(request.tenant_id)
    .bind(request.execution_id)
    .bind(request.node_execution_id)
    .bind(request.attempt_id)
    .bind(request.worker_id.to_string())
    .bind(request.fencing_token)
    .fetch_one(&state.pool)
    .await?;
    if !attempt_valid {
        return Err(conflict("Sandbox Attempt Lease was lost"));
    }
    let lease_id = stable_id(request.attempt_id, b"sandbox-lease");
    let ttl = (*maximum_ttl_seconds).clamp(1, 86_400);
    let labels = json!({
        "agentxLeaseId": lease_id,
        "agentxTenantId": request.tenant_id,
        "agentxExecutionId": request.execution_id,
        "agentxAttemptId": request.attempt_id,
        "agentxFencingToken": request.fencing_token,
    });
    let request_hash = sandbox_request_hash(&request)?;
    let operation = loop {
        let mut tx = state.pool.begin().await?;
        let existing = sqlx::query(
            "SELECT id,status,request_hash,sandbox_id,result_json,locked_until,fencing_token,locked_until>UTC_TIMESTAMP(6) lease_active FROM sandbox_leases WHERE tenant_id=? AND idempotency_key=? FOR UPDATE",
        )
        .bind(request.tenant_id)
        .bind(&request.idempotency_key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            if row.try_get::<String, _>("request_hash")? != request_hash.as_str() {
                return Err(conflict(
                    "Sandbox idempotency key was reused with a different request",
                ));
            }
            let status: String = row.try_get("status")?;
            if status == "terminated" {
                let response = sandbox_response(&row, true)?;
                tx.commit().await?;
                return Ok(Json(response));
            }
            if status == "failed" {
                return Err(RuntimeError::Unavailable);
            }
            if row.try_get::<bool, _>("lease_active")? {
                tx.commit().await?;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                continue;
            }
            ensure_attempt_lease(&mut tx, &request).await?;
            let fencing_token = row.try_get::<u64, _>("fencing_token")? + 1;
            let worker_lease_token =
                stable_id(request.attempt_id, &request.fencing_token.to_be_bytes());
            let changed = sqlx::query(
                "UPDATE sandbox_leases SET worker_lease_token=?,lease_token_hash=?,provider_labels_json=?,locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),heartbeat_at=UTC_TIMESTAMP(6),fencing_token=? WHERE id=? AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6))",
            )
            .bind(worker_lease_token)
            .bind(raw_hash(worker_lease_token.as_bytes()))
            .bind(&labels)
            .bind(state.owner)
            .bind(LEASE_SECONDS)
            .bind(fencing_token)
            .bind(lease_id)
            .execute(&mut *tx)
            .await?;
            if changed.rows_affected() != 1 {
                tx.rollback().await?;
                continue;
            }
            let operation = SandboxOperation {
                lease_id,
                fencing_token,
                sandbox_id: row.try_get("sandbox_id")?,
                result: row.try_get("result_json")?,
                replayed: true,
            };
            tx.commit().await?;
            break operation;
        }
        let worker_lease_token =
            stable_id(request.attempt_id, &request.fencing_token.to_be_bytes());
        sqlx::query(
            "INSERT INTO sandbox_leases(id,tenant_id,execution_id,node_execution_id,attempt_id,worker_lease_token,lease_token_hash,profile_version_id,idempotency_key,status,provider_labels_json,request_hash,expires_at,locked_by,locked_until,fencing_token) VALUES(?,?,?,?,?,?,?,?,?,'creating',?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),1)",
        )
        .bind(lease_id)
        .bind(request.tenant_id)
        .bind(request.execution_id)
        .bind(request.node_execution_id)
        .bind(request.attempt_id)
        .bind(worker_lease_token)
        .bind(raw_hash(worker_lease_token.as_bytes()))
        .bind(request.profile.resource_id)
        .bind(&request.idempotency_key)
        .bind(&labels)
        .bind(request_hash.as_str())
        .bind(ttl)
        .bind(state.owner)
        .bind(LEASE_SECONDS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        break SandboxOperation {
            lease_id,
            fencing_token: 1,
            sandbox_id: None,
            result: None,
            replayed: false,
        };
    };

    run_sandbox_operation(&state, &request, operation, ttl)
        .await
        .map(Json)
}

#[derive(Debug)]
struct SandboxOperation {
    lease_id: Uuid,
    fencing_token: u64,
    sandbox_id: Option<String>,
    result: Option<Value>,
    replayed: bool,
}

async fn ensure_attempt_lease(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    request: &SandboxExecuteRequestV1,
) -> RuntimeResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM node_attempts WHERE tenant_id=? AND execution_id=? AND node_execution_id=? AND id=? AND status='running' AND worker_instance_id=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6))",
    )
    .bind(request.tenant_id)
    .bind(request.execution_id)
    .bind(request.node_execution_id)
    .bind(request.attempt_id)
    .bind(request.worker_id.to_string())
    .bind(request.fencing_token)
    .fetch_one(&mut **tx)
    .await?;
    if !valid {
        return Err(conflict("Sandbox Attempt Lease was lost"));
    }
    Ok(())
}

async fn run_sandbox_operation(
    state: &SandboxManagerState,
    request: &SandboxExecuteRequestV1,
    operation: SandboxOperation,
    ttl: u32,
) -> RuntimeResult<SandboxExecuteResponseV1> {
    if operation.result.is_some() {
        if let Some(sandbox_id) = operation.sandbox_id.as_deref() {
            let termination = with_lease_heartbeat(
                state,
                operation.lease_id,
                operation.fencing_token,
                terminate_provider(state, sandbox_id, &request.idempotency_key),
            )
            .await?;
            if let Err(error) = termination {
                mark_orphaned(
                    state,
                    operation.lease_id,
                    operation.fencing_token,
                    &error.message,
                )
                .await?;
                return Err(RuntimeError::Unavailable);
            }
        }
        complete_lease(
            state,
            operation.lease_id,
            operation.fencing_token,
            operation.sandbox_id.as_deref(),
        )
        .await?;
        return Ok(SandboxExecuteResponseV1 {
            api_version: 1,
            lease_id: operation.lease_id,
            sandbox_id: operation.sandbox_id,
            replayed: true,
            output: operation.result.unwrap_or(Value::Null),
        });
    }

    let (image, cpu_millis, memory_bytes, disk_bytes, pid_limit, network_policy) =
        match &request.profile.configuration {
            RuntimeResourceConfigurationV1::SandboxProfile {
                image,
                cpu_millis,
                memory_bytes,
                disk_bytes,
                pid_limit,
                network_policy,
                ..
            } => (
                image,
                cpu_millis,
                memory_bytes,
                disk_bytes,
                pid_limit,
                network_policy,
            ),
            _ => unreachable!(),
        };
    let labels = json!({
        "agentxLeaseId": operation.lease_id,
        "agentxTenantId": request.tenant_id,
        "agentxExecutionId": request.execution_id,
        "agentxAttemptId": request.attempt_id,
        "agentxFencingToken": request.fencing_token,
    });
    let metadata = labels
        .as_object()
        .expect("Sandbox labels are constructed as an object")
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut sandbox_id = operation.sandbox_id.clone();
    let mut create_operation_id = None;
    if sandbox_id.is_none() {
        sandbox_id = find_provider_sandbox(state, operation.lease_id).await;
    }
    if sandbox_id.is_none() {
        let create = with_lease_heartbeat(
            state,
            operation.lease_id,
            operation.fencing_token,
            create_provider_sandbox(
                state,
                image,
                ttl,
                *cpu_millis,
                *memory_bytes,
                *disk_bytes,
                *pid_limit,
                network_policy,
                metadata,
                &request.idempotency_key,
            ),
        )
        .await?;
        let (create_payload, provider_operation_id) = match create {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(
                    lease_id = %operation.lease_id,
                    outcome_unknown = error.outcome_unknown,
                    provider_error = %error.message,
                    "OpenSandbox create failed"
                );
                fail_lease(
                    state,
                    operation.lease_id,
                    operation.fencing_token,
                    &error.message,
                    error.outcome_unknown,
                )
                .await?;
                return Err(if error.outcome_unknown {
                    RuntimeError::Unavailable
                } else {
                    RuntimeError::ProviderRejected
                });
            }
        };
        sandbox_id = create_payload
            .get("sandboxId")
            .or_else(|| create_payload.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        create_operation_id = provider_operation_id;
    }
    let sandbox_id = sandbox_id.ok_or_else(|| {
        RuntimeError::Internal(anyhow::anyhow!(
            "OpenSandbox create/reconciliation has no sandbox id"
        ))
    })?;
    advance_lease(
        state,
        operation.lease_id,
        operation.fencing_token,
        "running",
        Some(&sandbox_id),
        create_operation_id.as_deref(),
    )
    .await?;
    advance_lease(
        state,
        operation.lease_id,
        operation.fencing_token,
        "interrupting",
        Some(&sandbox_id),
        None,
    )
    .await?;
    let execution = with_lease_heartbeat(
        state,
        operation.lease_id,
        operation.fencing_token,
        execute_provider_command(
            state,
            &sandbox_id,
            &request.parameters,
            &request.idempotency_key,
        ),
    )
    .await?;
    let (result, execute_operation_id) = match execution {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                lease_id = %operation.lease_id,
                sandbox_id = %sandbox_id,
                outcome_unknown = error.outcome_unknown,
                provider_error = %error.message,
                "OpenSandbox command failed"
            );
            if error.outcome_unknown {
                mark_orphaned(
                    state,
                    operation.lease_id,
                    operation.fencing_token,
                    &error.message,
                )
                .await?;
                return Err(RuntimeError::Unavailable);
            }
            let _ = with_lease_heartbeat(
                state,
                operation.lease_id,
                operation.fencing_token,
                terminate_provider(state, &sandbox_id, &request.idempotency_key),
            )
            .await?;
            fail_lease(
                state,
                operation.lease_id,
                operation.fencing_token,
                &error.message,
                false,
            )
            .await?;
            return Err(RuntimeError::ProviderRejected);
        }
    };
    record_result(
        state,
        operation.lease_id,
        operation.fencing_token,
        &sandbox_id,
        execute_operation_id.or(create_operation_id).as_deref(),
        &result,
    )
    .await?;
    let termination = with_lease_heartbeat(
        state,
        operation.lease_id,
        operation.fencing_token,
        terminate_provider(state, &sandbox_id, &request.idempotency_key),
    )
    .await?;
    if let Err(error) = termination {
        mark_orphaned(
            state,
            operation.lease_id,
            operation.fencing_token,
            &error.message,
        )
        .await?;
        return Err(RuntimeError::Unavailable);
    }
    complete_lease(
        state,
        operation.lease_id,
        operation.fencing_token,
        Some(&sandbox_id),
    )
    .await?;
    Ok(SandboxExecuteResponseV1 {
        api_version: 1,
        lease_id: operation.lease_id,
        sandbox_id: Some(sandbox_id),
        replayed: operation.replayed,
        output: result,
    })
}

fn sandbox_response(
    row: &sqlx::mysql::MySqlRow,
    replayed: bool,
) -> RuntimeResult<SandboxExecuteResponseV1> {
    Ok(SandboxExecuteResponseV1 {
        api_version: 1,
        lease_id: row.try_get("id")?,
        sandbox_id: row.try_get("sandbox_id")?,
        replayed,
        output: row
            .try_get::<Option<Value>, _>("result_json")?
            .unwrap_or(Value::Null),
    })
}

async fn find_provider_sandbox(state: &SandboxManagerState, lease_id: Uuid) -> Option<String> {
    let list_url = format!(
        "{}/v1/sandboxes?metadata=agentxLeaseId%3D{}&pageSize=100",
        state.provider_endpoint.trim_end_matches('/'),
        lease_id
    );
    provider_json(authenticated(state, state.client.get(list_url)))
        .await
        .ok()
        .and_then(|(payload, _)| {
            payload
                .get("items")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("sandboxId").or_else(|| item.get("id")))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

async fn with_lease_heartbeat<F, T>(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    operation: F,
) -> RuntimeResult<Result<T, ProviderError>>
where
    F: Future<Output = Result<T, ProviderError>>,
{
    tokio::pin!(operation);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(10));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            result = &mut operation => return Ok(result),
            _ = heartbeat.tick() => heartbeat_lease(state, lease_id, fencing_token).await?,
        }
    }
}

async fn heartbeat_lease(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET heartbeat_at=UTC_TIMESTAMP(6),locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND) WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(LEASE_SECONDS)
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict("Sandbox operation Lease was lost"));
    }
    Ok(())
}

async fn record_result(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    sandbox_id: &str,
    provider_operation_id: Option<&str>,
    result: &Value,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET status='terminating',sandbox_id=?,provider_operation_id=COALESCE(?,provider_operation_id),result_json=?,heartbeat_at=UTC_TIMESTAMP(6),locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND) WHERE id=? AND status='interrupting' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(sandbox_id)
    .bind(provider_operation_id)
    .bind(result)
    .bind(LEASE_SECONDS)
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict(
            "Sandbox Lease was lost before persisting the Provider result",
        ));
    }
    Ok(())
}

async fn complete_lease(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    sandbox_id: Option<&str>,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET status='terminated',sandbox_id=COALESCE(?,sandbox_id),terminated_at=UTC_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,outcome_unknown=FALSE,last_error=NULL WHERE id=? AND status IN ('terminating','orphaned') AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(sandbox_id)
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict(
            "Sandbox Lease was lost before recording termination",
        ));
    }
    Ok(())
}

pub async fn reconcile_one(state: &SandboxManagerState) -> RuntimeResult<bool> {
    let mut tx = state.pool.begin().await?;
    let Some(row) = sqlx::query(
        "SELECT id,sandbox_id,fencing_token,idempotency_key FROM sandbox_leases WHERE (status='orphaned' OR (status IN ('ready','running','interrupting','terminating') AND expires_at<=UTC_TIMESTAMP(6))) AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) ORDER BY expires_at,id LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.commit().await?;
        return Ok(false);
    };
    let lease_id: Uuid = row.try_get("id")?;
    let fencing_token = row.try_get::<u64, _>("fencing_token")? + 1;
    sqlx::query(
        "UPDATE sandbox_leases SET status='terminating',locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND),fencing_token=?,termination_attempts=termination_attempts+1 WHERE id=?",
    )
    .bind(state.owner)
    .bind(LEASE_SECONDS)
    .bind(fencing_token)
    .bind(lease_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut sandbox_id = row.try_get::<Option<String>, _>("sandbox_id")?;
    if sandbox_id.is_none() {
        let list_url = format!(
            "{}/v1/sandboxes?metadata=agentxLeaseId%3D{}&pageSize=100",
            state.provider_endpoint.trim_end_matches('/'),
            lease_id
        );
        if let Ok((payload, _)) = with_lease_heartbeat(
            state,
            lease_id,
            fencing_token,
            provider_json(authenticated(state, state.client.get(list_url))),
        )
        .await?
        {
            sandbox_id = payload
                .get("items")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("sandboxId").or_else(|| item.get("id")))
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    let termination = if let Some(sandbox_id) = sandbox_id.as_deref() {
        with_lease_heartbeat(
            state,
            lease_id,
            fencing_token,
            terminate_provider(
                state,
                sandbox_id,
                &row.try_get::<String, _>("idempotency_key")?,
            ),
        )
        .await?
    } else {
        Ok(())
    };
    let changed = match termination {
        Ok(()) => sqlx::query(
            "UPDATE sandbox_leases SET status='terminated',sandbox_id=COALESCE(sandbox_id,?),terminated_at=UTC_TIMESTAMP(6),locked_by=NULL,locked_until=NULL,outcome_unknown=FALSE WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
        )
        .bind(sandbox_id)
        .bind(lease_id)
        .bind(state.owner)
        .bind(fencing_token)
        .execute(&state.pool)
        .await?,
        Err(error) => sqlx::query(
            "UPDATE sandbox_leases SET status='orphaned',outcome_unknown=TRUE,last_error=?,locked_by=NULL,locked_until=NULL WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
        )
        .bind(error.message.chars().take(1000).collect::<String>())
        .bind(lease_id)
        .bind(state.owner)
        .bind(fencing_token)
        .execute(&state.pool)
        .await?,
    };
    if changed.rows_affected() != 1 {
        return Err(conflict("Sandbox Reaper Lease was lost"));
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn create_provider_sandbox(
    state: &SandboxManagerState,
    image: &str,
    ttl_seconds: u32,
    cpu_millis: u32,
    memory_bytes: u64,
    disk_bytes: u64,
    pid_limit: u32,
    network_policy: &str,
    metadata: BTreeMap<String, String>,
    idempotency_key: &str,
) -> Result<(Value, Option<String>), ProviderError> {
    if network_policy != "deny" {
        return Err(ProviderError {
            message: "Sandbox network policy must default to deny".into(),
            outcome_unknown: false,
        });
    }
    let create_url = format!(
        "{}/v1/sandboxes",
        state.provider_endpoint.trim_end_matches('/')
    );
    let request = json!({
        "image":{"uri":image},
        "timeout":ttl_seconds.max(60).saturating_add(30).min(86_400),
        "resourceLimits":{
            "cpu":format!("{cpu_millis}m"),
            "memory":memory_bytes.to_string(),
            "ephemeral-storage":disk_bytes.to_string(),
            "pids":pid_limit.to_string()
        },
        "entrypoint":["tail","-f","/dev/null"],
        "metadata":metadata,
        "networkPolicy":{"defaultAction":"deny","egress":[]},
        "secureAccess":state.provider_secure_access
    });
    let mut response = provider_json(
        authenticated(state, state.client.post(create_url))
            .header("Idempotency-Key", idempotency_key)
            .json(&request),
    )
    .await?;
    let sandbox_id = response
        .0
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError {
            message: "OpenSandbox create response has no id".into(),
            outcome_unknown: false,
        })?
        .to_owned();
    for _ in 0..120 {
        let state_name = response
            .0
            .pointer("/status/state")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if state_name == "Running" {
            return Ok(response);
        }
        if state_name != "Pending" {
            return Err(ProviderError {
                message: format!("OpenSandbox entered state {state_name}: {}", response.0),
                outcome_unknown: false,
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        response = provider_json(authenticated(
            state,
            state.client.get(format!(
                "{}/v1/sandboxes/{sandbox_id}",
                state.provider_endpoint.trim_end_matches('/')
            )),
        ))
        .await?;
    }
    Err(ProviderError {
        message: "OpenSandbox did not become ready within 30 seconds".into(),
        outcome_unknown: true,
    })
}

async fn execute_provider_command(
    state: &SandboxManagerState,
    sandbox_id: &str,
    parameters: &Value,
    idempotency_key: &str,
) -> Result<(Value, Option<String>), ProviderError> {
    let endpoint_url = format!(
        "{}/v1/sandboxes/{sandbox_id}/endpoints/{EXECD_PORT}?use_server_proxy=true",
        state.provider_endpoint.trim_end_matches('/')
    );
    let (endpoint_payload, _) =
        provider_json(authenticated(state, state.client.get(endpoint_url))).await?;
    let endpoint = endpoint_payload
        .get("endpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError {
            message: "OpenSandbox execd endpoint response is missing endpoint".into(),
            outcome_unknown: false,
        })?;
    let endpoint = server_proxy_endpoint(&state.provider_endpoint, endpoint, sandbox_id)?;
    let mut headers = HeaderMap::new();
    if let Some(values) = endpoint_payload.get("headers").and_then(Value::as_object) {
        for (name, value) in values {
            let Some(value) = value.as_str() else {
                return Err(ProviderError {
                    message: "OpenSandbox execd header is not a string".into(),
                    outcome_unknown: false,
                });
            };
            let name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    ProviderError {
                        message: format!("invalid OpenSandbox execd header name: {error}"),
                        outcome_unknown: false,
                    }
                })?;
            let value =
                reqwest::header::HeaderValue::from_str(value).map_err(|error| ProviderError {
                    message: format!("invalid OpenSandbox execd header value: {error}"),
                    outcome_unknown: false,
                })?;
            headers.insert(name, value);
        }
    }
    if endpoint.host_str()
        == reqwest::Url::parse(&state.provider_endpoint)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .as_deref()
        && let Some(key) = &state.provider_api_key
    {
        headers.insert(
            reqwest::header::HeaderName::from_static("open-sandbox-api-key"),
            reqwest::header::HeaderValue::from_str(key).map_err(|error| ProviderError {
                message: format!("invalid OpenSandbox API key header: {error}"),
                outcome_unknown: false,
            })?,
        );
    }
    let command = sandbox_command(parameters, idempotency_key)?;
    let command_url = endpoint.join("command").map_err(|error| ProviderError {
        message: format!("invalid OpenSandbox command endpoint: {error}"),
        outcome_unknown: false,
    })?;
    let response = state
        .client
        .post(command_url)
        .headers(headers)
        .json(&json!({
            "command":command,
            "cwd":"/workspace",
            "background":false,
            "timeout":120_000,
            "envs":{}
        }))
        .send()
        .await
        .map_err(|error| ProviderError {
            message: error.to_string(),
            outcome_unknown: !error.is_connect(),
        })?;
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.text().await.map_err(|error| ProviderError {
        message: error.to_string(),
        outcome_unknown: true,
    })?;
    if !status.is_success() {
        return Err(ProviderError {
            message: format!("OpenSandbox execd HTTP {status}: {body}"),
            outcome_unknown: false,
        });
    }
    let (stdout, stderr, exit_code) = parse_command_stream(&body)?;
    if exit_code != 0 {
        return Err(ProviderError {
            message: format!("OpenSandbox command exited with {exit_code}: {stderr}"),
            outcome_unknown: false,
        });
    }
    Ok((
        json!({
            "stdout":stdout,
            "stderr":stderr,
            "exitCode":exit_code,
            "partial":false,
            "downloadedArtifacts":[],
            "sandboxId":sandbox_id
        }),
        request_id,
    ))
}

fn parse_command_stream(body: &str) -> Result<(String, String, i64), ProviderError> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0_i64;
    let mut recognized_event = false;
    let mut terminal_event = false;

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
            continue;
        }
        let data = line.strip_prefix("data:").unwrap_or(line).trim();
        let event = serde_json::from_str::<Value>(data).map_err(|error| ProviderError {
            message: format!("invalid OpenSandbox command stream event: {error}"),
            outcome_unknown: false,
        })?;
        let event_type =
            event
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderError {
                    message: "OpenSandbox command stream event has no type".into(),
                    outcome_unknown: false,
                })?;
        recognized_event = true;
        let text = event
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "stdout" => stdout.push_str(text),
            "stderr" => stderr.push_str(text),
            "result" => {
                if let Some(code) = event
                    .get("exit_code")
                    .or_else(|| event.get("exitCode"))
                    .and_then(Value::as_i64)
                {
                    exit_code = code;
                    terminal_event = true;
                }
            }
            "execution_complete" => terminal_event = true,
            "error" => {
                let message = event
                    .pointer("/error/evalue")
                    .or_else(|| event.get("evalue"))
                    .or_else(|| event.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("OpenSandbox command failed");
                return Err(ProviderError {
                    message: message.to_owned(),
                    outcome_unknown: false,
                });
            }
            "init" | "status" | "execution_count" | "ping" => {}
            other => {
                return Err(ProviderError {
                    message: format!("unsupported OpenSandbox command stream event type {other}"),
                    outcome_unknown: false,
                });
            }
        }
    }

    if !recognized_event || !terminal_event {
        return Err(ProviderError {
            message: "OpenSandbox command stream ended without a terminal event".into(),
            outcome_unknown: true,
        });
    }
    Ok((stdout, stderr, exit_code))
}

fn server_proxy_endpoint(
    lifecycle_endpoint: &str,
    provider_endpoint: &str,
    sandbox_id: &str,
) -> Result<reqwest::Url, ProviderError> {
    let lifecycle = reqwest::Url::parse(lifecycle_endpoint).map_err(|error| ProviderError {
        message: format!("invalid OpenSandbox lifecycle endpoint: {error}"),
        outcome_unknown: false,
    })?;
    let mut provider = if provider_endpoint.contains("://") {
        reqwest::Url::parse(provider_endpoint)
    } else {
        reqwest::Url::parse(&format!("{}://{provider_endpoint}", lifecycle.scheme()))
    }
    .map_err(|error| ProviderError {
        message: format!("invalid OpenSandbox execd endpoint: {error}"),
        outcome_unknown: false,
    })?;
    if matches!(provider.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
        provider
            .set_host(lifecycle.host_str())
            .map_err(|_| ProviderError {
                message: "OpenSandbox execd endpoint host cannot be rewritten".into(),
                outcome_unknown: false,
            })?;
    }
    if provider.scheme() != lifecycle.scheme()
        || provider.host_str() != lifecycle.host_str()
        || provider.port_or_known_default() != lifecycle.port_or_known_default()
    {
        return Err(ProviderError {
            message: "OpenSandbox execd endpoint is outside the lifecycle provider origin".into(),
            outcome_unknown: false,
        });
    }
    if !provider.username().is_empty()
        || provider.password().is_some()
        || provider.query().is_some()
        || provider.fragment().is_some()
    {
        return Err(ProviderError {
            message: "OpenSandbox server-proxy endpoint contains forbidden URL components".into(),
            outcome_unknown: false,
        });
    }
    let expected_path = format!("/v1/sandboxes/{sandbox_id}/proxy/{EXECD_PORT}");
    let expected_directory = format!("{expected_path}/");
    match provider.path() {
        path if path == expected_path => provider.set_path(&expected_directory),
        path if path == expected_directory => {}
        _ => {
            return Err(ProviderError {
                message: "OpenSandbox returned an invalid server-proxy endpoint path".into(),
                outcome_unknown: false,
            });
        }
    }
    Ok(provider)
}

fn sandbox_command(parameters: &Value, idempotency_key: &str) -> Result<String, ProviderError> {
    let runner = parameters
        .get("runner")
        .and_then(Value::as_str)
        .unwrap_or("shell");
    let source = parameters
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError {
            message: "Sandbox code node requires parameters.source".into(),
            outcome_unknown: false,
        })?;
    let encoded = STANDARD.encode(source.as_bytes());
    let (path, executable) = match runner {
        "python" => ("/tmp/agentx-v2.py", "python3"),
        "javascript" => ("/tmp/agentx-v2.js", "node"),
        "shell" => ("/tmp/agentx-v2.sh", "sh"),
        other => {
            return Err(ProviderError {
                message: format!("unsupported Sandbox runner {other}"),
                outcome_unknown: false,
            });
        }
    };
    let operation_hash = raw_hash(idempotency_key.as_bytes());
    Ok(format!(
        "AGENTX_STATE='/tmp/agentx-{operation_hash}'; AGENTX_ACQUIRED=0; \
         while [ \"$AGENTX_ACQUIRED\" -eq 0 ] && [ ! -f \"${{AGENTX_STATE}}.exit\" ]; do \
           if mkdir \"${{AGENTX_STATE}}.lock\" 2>/dev/null; then \
             printf '%s' \"$$\" > \"${{AGENTX_STATE}}.lock/pid\"; AGENTX_ACQUIRED=1; \
           else \
             AGENTX_PID=$(cat \"${{AGENTX_STATE}}.lock/pid\" 2>/dev/null || true); \
             if [ -n \"$AGENTX_PID\" ] && ! kill -0 \"$AGENTX_PID\" 2>/dev/null; then rm -rf \"${{AGENTX_STATE}}.lock\"; else sleep 0.1; fi; \
           fi; \
         done; \
         if [ \"$AGENTX_ACQUIRED\" -eq 1 ]; then \
           set +e; (printf '%s' '{encoded}' | base64 -d > '{path}' && {executable} '{path}') > \"${{AGENTX_STATE}}.stdout.tmp\" 2> \"${{AGENTX_STATE}}.stderr.tmp\"; AGENTX_EXIT=$?; \
           mv \"${{AGENTX_STATE}}.stdout.tmp\" \"${{AGENTX_STATE}}.stdout\"; mv \"${{AGENTX_STATE}}.stderr.tmp\" \"${{AGENTX_STATE}}.stderr\"; \
           printf '%s' \"$AGENTX_EXIT\" > \"${{AGENTX_STATE}}.exit.tmp\"; mv \"${{AGENTX_STATE}}.exit.tmp\" \"${{AGENTX_STATE}}.exit\"; rm -rf \"${{AGENTX_STATE}}.lock\"; \
         fi; \
         cat \"${{AGENTX_STATE}}.stdout\"; cat \"${{AGENTX_STATE}}.stderr\" >&2; exit $(cat \"${{AGENTX_STATE}}.exit\")"
    ))
}

#[derive(Debug)]
struct ProviderError {
    message: String,
    outcome_unknown: bool,
}

fn authenticated(
    state: &SandboxManagerState,
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    if let Some(key) = &state.provider_api_key {
        request.header("OPEN-SANDBOX-API-KEY", key)
    } else {
        request
    }
}

async fn provider_json(
    request: reqwest::RequestBuilder,
) -> Result<(Value, Option<String>), ProviderError> {
    let response = request.send().await.map_err(|error| ProviderError {
        outcome_unknown: !error.is_connect(),
        message: error.to_string(),
    })?;
    let status = response.status();
    let operation_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response.bytes().await.map_err(|error| ProviderError {
        outcome_unknown: true,
        message: error.to_string(),
    })?;
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(|error| ProviderError {
            outcome_unknown: false,
            message: format!("invalid OpenSandbox JSON response: {error}"),
        })?
    };
    if !status.is_success() {
        return Err(ProviderError {
            outcome_unknown: false,
            message: format!("OpenSandbox HTTP {status}: {payload}"),
        });
    }
    Ok((payload, operation_id))
}

async fn terminate_provider(
    state: &SandboxManagerState,
    sandbox_id: &str,
    idempotency_key: &str,
) -> Result<(), ProviderError> {
    let response = authenticated(
        state,
        state.client.delete(format!(
            "{}/v1/sandboxes/{sandbox_id}",
            state.provider_endpoint.trim_end_matches('/')
        )),
    )
    .header("Idempotency-Key", format!("{idempotency_key}:terminate"))
    .send()
    .await
    .map_err(|error| ProviderError {
        outcome_unknown: !error.is_connect(),
        message: error.to_string(),
    })?;
    if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(ProviderError {
        outcome_unknown: false,
        message: format!("OpenSandbox terminate HTTP {status}: {body}"),
    })
}

async fn advance_lease(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    status: &str,
    sandbox_id: Option<&str>,
    provider_operation_id: Option<&str>,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET status=?,sandbox_id=COALESCE(?,sandbox_id),provider_operation_id=COALESCE(?,provider_operation_id),heartbeat_at=UTC_TIMESTAMP(6),locked_until=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL ? SECOND) WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(status)
    .bind(sandbox_id)
    .bind(provider_operation_id)
    .bind(LEASE_SECONDS)
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict(
            "Sandbox Lease was lost between Provider operations",
        ));
    }
    Ok(())
}

async fn mark_orphaned(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    error: &str,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET status='orphaned',outcome_unknown=TRUE,last_error=?,locked_by=NULL,locked_until=NULL WHERE id=? AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(error.chars().take(1000).collect::<String>())
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict("Sandbox Lease was lost while recording orphan"));
    }
    Ok(())
}

async fn fail_lease(
    state: &SandboxManagerState,
    lease_id: Uuid,
    fencing_token: u64,
    error: &str,
    outcome_unknown: bool,
) -> RuntimeResult<()> {
    let changed = sqlx::query(
        "UPDATE sandbox_leases SET status=?,outcome_unknown=?,last_error=?,locked_by=NULL,locked_until=NULL WHERE id=? AND status IN ('creating','running','interrupting','terminating') AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)",
    )
    .bind(if outcome_unknown { "orphaned" } else { "failed" })
    .bind(outcome_unknown)
    .bind(error.chars().take(1000).collect::<String>())
    .bind(lease_id)
    .bind(state.owner)
    .bind(fencing_token)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(conflict("Sandbox Lease was lost while recording failure"));
    }
    Ok(())
}

fn stable_id(namespace: Uuid, label: &[u8]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(label);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn raw_hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn sandbox_request_hash(request: &SandboxExecuteRequestV1) -> RuntimeResult<String> {
    let mut stable_request =
        serde_json::to_value(request).map_err(|error| RuntimeError::Internal(error.into()))?;
    if let Some(object) = stable_request.as_object_mut() {
        object.remove("workerId");
        object.remove("fencingToken");
    }
    agentx_runtime_contracts::content_hash(&stable_request)
        .map(|hash| hash.to_string())
        .map_err(|error| RuntimeError::Internal(error.into()))
}

fn bad_request(message: &str) -> RuntimeError {
    RuntimeError::BadRequest(
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::AdmissionPrerequisiteMissing,
        message.into(),
    )
}

fn conflict(message: &str) -> RuntimeError {
    RuntimeError::Conflict(
        agentx_runtime_contracts::RuntimePublishErrorCodeV1::BundleReferenceConflict,
        message.into(),
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_command_stream, sandbox_command, server_proxy_endpoint};
    use serde_json::json;

    #[test]
    fn python_sandbox_command_uses_the_pinned_image_interpreter() {
        let command = sandbox_command(
            &json!({"runner":"python","source":"print('ok')"}),
            "sandbox:test",
        )
        .unwrap();
        assert!(command.contains("python3 '/tmp/agentx-v2.py'"));
        assert!(!command.contains(" python '/tmp/agentx-v2.py'"));
    }

    #[test]
    fn command_stream_accepts_the_pinned_execd_json_frames() {
        let body = concat!(
            "{\"type\":\"init\",\"text\":\"command-id\"}\n\n",
            "{\"type\":\"stdout\",\"text\":\"m6-studio-ok\\n\"}\n\n",
            "{\"type\":\"execution_complete\",\"execution_time\":2}\n\n"
        );
        let (stdout, stderr, exit_code) = parse_command_stream(body).unwrap();
        assert_eq!(stdout, "m6-studio-ok\n");
        assert!(stderr.is_empty());
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn command_stream_accepts_spec_sse_and_rejects_incomplete_success() {
        let (stdout, stderr, exit_code) = parse_command_stream(concat!(
            "data: {\"type\":\"stdout\",\"text\":\"ok\"}\n\n",
            "data: {\"type\":\"result\",\"exit_code\":0}\n\n"
        ))
        .unwrap();
        assert_eq!((stdout.as_str(), stderr.as_str(), exit_code), ("ok", "", 0));

        let error =
            parse_command_stream("{\"type\":\"stdout\",\"text\":\"partial\"}\n").unwrap_err();
        assert!(error.message.contains("without a terminal event"));
        assert!(error.outcome_unknown);
    }

    #[test]
    fn command_stream_reads_nested_execd_errors() {
        let error = parse_command_stream(
            "{\"type\":\"error\",\"error\":{\"ename\":\"ExitError\",\"evalue\":\"command failed\"}}\n",
        )
        .unwrap_err();
        assert_eq!(error.message, "command failed");
        assert!(!error.outcome_unknown);
    }

    #[test]
    fn server_proxy_endpoint_adds_trailing_slash_and_rewrites_loopback_host() {
        let endpoint = server_proxy_endpoint(
            "http://host.docker.internal:18080",
            "127.0.0.1:18080/v1/sandboxes/sbx-1/proxy/44772",
            "sbx-1",
        )
        .unwrap();
        assert_eq!(
            endpoint.as_str(),
            "http://host.docker.internal:18080/v1/sandboxes/sbx-1/proxy/44772/"
        );
        assert_eq!(
            endpoint.join("command").unwrap().as_str(),
            "http://host.docker.internal:18080/v1/sandboxes/sbx-1/proxy/44772/command"
        );
    }

    #[test]
    fn server_proxy_endpoint_accepts_the_exact_execd_path() {
        let endpoint = server_proxy_endpoint(
            "https://opensandbox.example.test",
            "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/44772/",
            "sbx-1",
        )
        .unwrap();
        assert_eq!(endpoint.path(), "/v1/sandboxes/sbx-1/proxy/44772/");
    }

    #[test]
    fn server_proxy_endpoint_cannot_escape_the_lifecycle_origin() {
        let error = server_proxy_endpoint(
            "https://opensandbox.example.test",
            "https://attacker.example.test/v1/sandboxes/sbx-1/proxy/",
            "sbx-1",
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("outside the lifecycle provider origin")
        );

        let error = server_proxy_endpoint(
            "https://opensandbox.example.test:8443",
            "https://opensandbox.example.test:9443/v1/sandboxes/sbx-1/proxy/",
            "sbx-1",
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("outside the lifecycle provider origin")
        );
    }

    #[test]
    fn server_proxy_endpoint_rejects_path_and_query_injection() {
        for endpoint in [
            "https://opensandbox.example.test/v1/sandboxes/other/proxy/",
            "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/",
            "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/22/",
            "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/command/",
            "https://opensandbox.example.test/v1/sandboxes/sbx-1/proxy/?target=metadata",
        ] {
            assert!(
                server_proxy_endpoint("https://opensandbox.example.test", endpoint, "sbx-1")
                    .is_err(),
                "endpoint should be rejected: {endpoint}"
            );
        }
    }
}
