use std::{env, time::Duration};

use agentx_application::{RuntimeCommand, RuntimeCommandType, StartExecutionCommandPayload};
use agentx_domain::{TenantId, WorkflowVersionId};
use agentx_infrastructure::runtime_commands::RuntimeCommandRepository;
use agentx_node_protocol::{LifecycleOperation, LifecycleRequest, NODE_PROTOCOL_VERSION};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use crate::stable_invocation_id;

pub async fn run(pool: MySqlPool) {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("trigger HTTP client is valid");
    let token = env::var("AGENTX_REMOTE_NODE_AUTH_TOKEN").ok();
    let owner = Uuid::now_v7();
    loop {
        if let Err(error) = scan_lifecycle(&pool, &client, token.as_deref(), owner).await {
            tracing::error!(%error, "trigger lifecycle scan failed");
        }
        match scan_once(&pool, &client, token.as_deref(), owner).await {
            Ok(()) => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(error) => {
                tracing::error!(%error, "trigger binding poll failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn scan_once(
    pool: &MySqlPool,
    client: &reqwest::Client,
    token: Option<&str>,
    owner: Uuid,
) -> Result<()> {
    let rows = sqlx::query("SELECT b.id,b.tenant_id,b.application_id,b.application_deployment_id,b.workflow_version_id,b.node_id,b.configuration_json FROM trigger_bindings b JOIN applications a ON a.id=b.application_id AND a.tenant_id=b.tenant_id AND a.status='active' WHERE b.status='active' AND b.trigger_kind='poll' AND (b.next_poll_at IS NULL OR b.next_poll_at<=CURRENT_TIMESTAMP(6)) AND (b.locked_until IS NULL OR b.locked_until<CURRENT_TIMESTAMP(6)) ORDER BY b.next_poll_at,b.id LIMIT 20")
        .fetch_all(pool)
        .await?;
    for row in rows {
        poll_binding(pool, client, token, owner, row).await?;
    }
    Ok(())
}

async fn poll_binding(
    pool: &MySqlPool,
    client: &reqwest::Client,
    token: Option<&str>,
    owner: Uuid,
    row: sqlx::mysql::MySqlRow,
) -> Result<()> {
    let binding_id: Uuid = row.try_get("id")?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let application_id: Uuid = row.try_get("application_id")?;
    let application_deployment_id: Uuid = row.try_get("application_deployment_id")?;
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let claimed = sqlx::query("UPDATE trigger_bindings SET locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 120 SECOND) WHERE id=? AND tenant_id=? AND status='active' AND trigger_kind='poll' AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6))")
        .bind(owner).bind(binding_id).bind(tenant_id).execute(pool).await?;
    if claimed.rows_affected() == 0 {
        return Ok(());
    }
    let configuration: Value = row.try_get("configuration_json")?;
    let parameters = configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(endpoint) = parameters.get("endpoint").and_then(Value::as_str) else {
        advance(
            pool,
            binding_id,
            tenant_id,
            owner,
            60,
            Some("POLL_ENDPOINT_MISSING"),
        )
        .await?;
        return Ok(());
    };
    let endpoint = format!(
        "{}/agentx/node/v1/lifecycle/poll",
        endpoint.trim_end_matches('/')
    );
    let body = json!({
        "protocolVersion":"1.0",
        "operation":"poll",
        "nodeType":configuration.get("nodeType"),
        "nodeVersion":configuration.get("nodeVersion"),
        "tenantId":tenant_id,
        "workflowVersionId":workflow_version_id,
        "configuration":parameters,
    });
    let mut request = client.post(endpoint).json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await;
    let value = match response {
        Ok(response) if response.status().is_success() => response.json::<Value>().await?,
        Ok(response) => {
            advance(
                pool,
                binding_id,
                tenant_id,
                owner,
                60,
                Some(&format!("POLL_HTTP_{}", response.status())),
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            advance(
                pool,
                binding_id,
                tenant_id,
                owner,
                60,
                Some(&error.to_string()),
            )
            .await?;
            return Ok(());
        }
    };
    let interval = parameters
        .get("pollIntervalSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(60)
        .clamp(1, 86_400);
    let Some(input) = value.get("input").cloned().or_else(|| {
        value
            .get("state")
            .and_then(|state| state.get("input"))
            .cloned()
    }) else {
        advance(pool, binding_id, tenant_id, owner, interval, None).await?;
        return Ok(());
    };
    let idempotency_key = poll_idempotency_key(binding_id, &value)?;
    let invocation_id = stable_invocation_id(
        tenant_id,
        application_id,
        "poll",
        Some(binding_id),
        &idempotency_key,
    );
    let command = RuntimeCommand::new(
        TenantId::from_uuid(tenant_id),
        RuntimeCommandType::StartExecution,
        "application_invocation",
        invocation_id.to_string(),
        idempotency_key.clone(),
        serde_json::to_value(StartExecutionCommandPayload {
            workflow_version_id,
            invocation_id: Some(invocation_id.as_uuid()),
            session_id: None,
            requested_by: None,
            trigger_type: "poll".into(),
            input,
            runtime_settings: json!({"triggerBindingId":binding_id}),
        })?,
    );
    let mut tx = pool.begin().await?;
    RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &command).await?;
    sqlx::query("INSERT INTO application_invocations(id,tenant_id,application_id,application_deployment_id,workflow_version_id,execution_id,runtime_command_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(?,?,?,?,?,NULL,?,'poll',?,?,?,'queued') ON DUPLICATE KEY UPDATE id=id")
        .bind(invocation_id.as_uuid())
        .bind(tenant_id)
        .bind(application_id)
        .bind(application_deployment_id)
        .bind(workflow_version_id)
        .bind(command.id)
        .bind(binding_id)
        .bind(format!("{:x}", Sha256::digest(idempotency_key.as_bytes())))
        .bind(&idempotency_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,1,'invocation.queued',?) ON DUPLICATE KEY UPDATE invocation_id=VALUES(invocation_id)")
        .bind(tenant_id)
        .bind(invocation_id.as_uuid())
        .bind(json!({"commandId":command.id,"triggerBindingId":binding_id}))
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE trigger_bindings SET last_poll_at=CURRENT_TIMESTAMP(6),next_poll_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),last_error=NULL,locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?")
        .bind(interval)
        .bind(tenant_id)
        .bind(binding_id)
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn poll_idempotency_key(binding_id: Uuid, response: &Value) -> Result<String> {
    let event_key = response
        .get("eventId")
        .or_else(|| response.get("eventID"))
        .or_else(|| response.get("idempotencyKey"))
        .or_else(|| response.get("cursor"))
        .or_else(|| response.get("id"));
    if let Some(event_key) = event_key.filter(|value| !value.is_null()) {
        return Ok(format!(
            "poll:{binding_id}:event:{:x}",
            Sha256::digest(serde_json::to_vec(event_key)?)
        ));
    }
    let canonical = serde_json::to_vec(response)?;
    Ok(format!(
        "poll:{binding_id}:payload:{:x}",
        Sha256::digest(canonical)
    ))
}

async fn advance(
    pool: &MySqlPool,
    binding_id: Uuid,
    tenant_id: Uuid,
    owner: Uuid,
    seconds: u64,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE trigger_bindings SET next_poll_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND),last_error=?,locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?")
        .bind(seconds.clamp(1, 86_400))
        .bind(error.map(|value| value.chars().take(1000).collect::<String>()))
        .bind(tenant_id).bind(binding_id).bind(owner)
        .execute(pool)
        .await?;
    Ok(())
}

async fn scan_lifecycle(
    pool: &MySqlPool,
    client: &reqwest::Client,
    token: Option<&str>,
    owner: Uuid,
) -> Result<()> {
    let rows = sqlx::query("SELECT id,tenant_id,application_deployment_id,workflow_version_id,node_id,configuration_json,status FROM trigger_bindings WHERE trigger_kind='lifecycle' AND status IN ('activating','deactivating') AND (next_poll_at IS NULL OR next_poll_at<=CURRENT_TIMESTAMP(6)) AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6)) ORDER BY updated_at,id LIMIT 20")
        .fetch_all(pool).await?;
    for row in rows {
        lifecycle_binding(pool, client, token, owner, row).await?;
    }
    Ok(())
}

async fn lifecycle_binding(
    pool: &MySqlPool,
    client: &reqwest::Client,
    token: Option<&str>,
    owner: Uuid,
    row: sqlx::mysql::MySqlRow,
) -> Result<()> {
    let binding_id: Uuid = row.try_get("id")?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let status: String = row.try_get("status")?;
    let claimed = sqlx::query("UPDATE trigger_bindings SET locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 120 SECOND) WHERE id=? AND tenant_id=? AND status=? AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6))")
        .bind(owner).bind(binding_id).bind(tenant_id).bind(&status).execute(pool).await?;
    if claimed.rows_affected() == 0 {
        return Ok(());
    }
    let configuration: Value = row.try_get("configuration_json")?;
    let parameters = configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(endpoint) = parameters.get("endpoint").and_then(Value::as_str) else {
        return lifecycle_retry(
            pool,
            tenant_id,
            binding_id,
            owner,
            "LIFECYCLE_ENDPOINT_MISSING",
        )
        .await;
    };
    let (operation, operation_name) = if status == "activating" {
        (LifecycleOperation::Activate, "activate")
    } else {
        (LifecycleOperation::Deactivate, "deactivate")
    };
    let url = format!(
        "{}/agentx/node/v1/lifecycle/{operation_name}",
        endpoint.trim_end_matches('/')
    );
    let body = lifecycle_request(
        operation,
        tenant_id,
        row.try_get("workflow_version_id")?,
        &configuration,
        parameters,
    )?;
    let mut request = client.post(url).json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = match request.send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return lifecycle_retry(
                pool,
                tenant_id,
                binding_id,
                owner,
                &format!("LIFECYCLE_HTTP_{}", response.status()),
            )
            .await;
        }
        Err(error) => {
            return lifecycle_retry(pool, tenant_id, binding_id, owner, &error.to_string()).await;
        }
    };
    let response: Value = response.json().await?;
    if !response
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return lifecycle_retry(pool, tenant_id, binding_id, owner, "LIFECYCLE_REJECTED").await;
    }
    let next_status = if operation_name == "activate" {
        "active"
    } else {
        "disabled"
    };
    sqlx::query("UPDATE trigger_bindings SET status=?,configuration_json=JSON_SET(configuration_json,'$.lifecycleState',CAST(? AS JSON)),next_poll_at=NULL,last_error=NULL,locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?")
        .bind(next_status).bind(response.get("state").cloned().unwrap_or_else(|| json!({})).to_string())
        .bind(tenant_id).bind(binding_id).bind(owner).execute(pool).await?;
    Ok(())
}

fn lifecycle_request(
    operation: LifecycleOperation,
    tenant_id: Uuid,
    workflow_version_id: Uuid,
    binding_configuration: &Value,
    parameters: Value,
) -> Result<LifecycleRequest> {
    let node_type = binding_configuration
        .get("nodeType")
        .and_then(Value::as_str)
        .context("trigger binding nodeType is missing")?;
    let node_version = binding_configuration
        .get("nodeVersion")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .context("trigger binding nodeVersion is invalid")?;
    Ok(LifecycleRequest {
        protocol_version: NODE_PROTOCOL_VERSION.into(),
        operation,
        node_type: node_type.into(),
        node_version,
        tenant_id: TenantId::from_uuid(tenant_id),
        workflow_version_id: WorkflowVersionId::from_uuid(workflow_version_id),
        configuration: parameters,
    })
}

async fn lifecycle_retry(
    pool: &MySqlPool,
    tenant_id: Uuid,
    binding_id: Uuid,
    owner: Uuid,
    error: &str,
) -> Result<()> {
    sqlx::query("UPDATE trigger_bindings SET next_poll_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 30 SECOND),last_error=?,locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?")
        .bind(error.chars().take(1000).collect::<String>()).bind(tenant_id).bind(binding_id).bind(owner).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{lifecycle_request, poll_idempotency_key};
    use agentx_node_protocol::LifecycleOperation;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn poll_key_prefers_provider_event_identity() {
        let binding = Uuid::from_u128(1);
        let first =
            poll_idempotency_key(binding, &json!({"eventId":"evt-7","input":{"value":1}})).unwrap();
        let second =
            poll_idempotency_key(binding, &json!({"eventId":"evt-7","input":{"value":2}})).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn poll_key_hashes_response_when_provider_has_no_identity() {
        let binding = Uuid::from_u128(1);
        let first = poll_idempotency_key(binding, &json!({"input":{"value":1}})).unwrap();
        let second = poll_idempotency_key(binding, &json!({"input":{"value":1}})).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("poll:00000000-0000-0000-0000-000000000001:payload:"));
    }

    #[test]
    fn lifecycle_body_is_exactly_the_node_protocol_contract() {
        let value = serde_json::to_value(
            lifecycle_request(
                LifecycleOperation::Activate,
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                &json!({"nodeType":"remote_trigger","nodeVersion":1}),
                json!({"endpoint":"http://echo-node:8080"}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            value,
            json!({
                "protocolVersion":"1.0",
                "operation":"activate",
                "nodeType":"remote_trigger",
                "nodeVersion":1,
                "tenantId":"00000000-0000-0000-0000-000000000001",
                "workflowVersionId":"00000000-0000-0000-0000-000000000002",
                "configuration":{"endpoint":"http://echo-node:8080"}
            })
        );
    }
}
