use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) async fn create_wait(
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
