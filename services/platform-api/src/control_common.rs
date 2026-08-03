use axum::http::HeaderMap;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{MySql, Transaction};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    security::AuthActor,
};

pub fn validate_name(value: &str, max: usize) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(AppError::bad_request(
            "INVALID_NAME",
            format!("Name is required and must not exceed {max} characters"),
        ));
    }
    Ok(value.to_owned())
}

pub fn idempotency_key(headers: &HeaderMap) -> AppResult<Option<String>> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| {
            AppError::bad_request(
                "INVALID_IDEMPOTENCY_KEY",
                "Idempotency-Key must be valid text",
            )
        })?
        .trim();
    if value.is_empty() || value.len() > 128 {
        return Err(AppError::bad_request(
            "INVALID_IDEMPOTENCY_KEY",
            "Idempotency-Key must contain 1 to 128 characters",
        ));
    }
    Ok(Some(value.to_owned()))
}

pub enum IdempotencyReservation {
    Disabled,
    Reserved,
    Replay { response: Option<Value> },
}

pub async fn reserve_idempotency<T: Serialize>(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    operation_key: &str,
    idempotency_key: Option<&str>,
    request: &T,
) -> AppResult<IdempotencyReservation> {
    let Some(idempotency_key) = idempotency_key else {
        return Ok(IdempotencyReservation::Disabled);
    };
    let request = serde_json::to_vec(request).map_err(AppError::internal)?;
    let request_hash = format!("{:x}", Sha256::digest(request));
    let inserted = sqlx::query("INSERT IGNORE INTO idempotency_records(tenant_id,operation_key,idempotency_key,request_hash,expires_at) VALUES(?,?,?,?,DATE_ADD(CURRENT_TIMESTAMP(6), INTERVAL 1 DAY))")
        .bind(tenant_id).bind(operation_key).bind(idempotency_key).bind(&request_hash)
        .execute(&mut **tx).await?;
    let row = sqlx::query("SELECT request_hash,response_json FROM idempotency_records WHERE tenant_id=? AND operation_key=? AND idempotency_key=? FOR UPDATE")
        .bind(tenant_id).bind(operation_key).bind(idempotency_key)
        .fetch_one(&mut **tx).await?;
    let stored_hash: String = sqlx::Row::try_get(&row, "request_hash")?;
    if stored_hash != request_hash {
        return Err(AppError::conflict(
            "IDEMPOTENCY_KEY_REUSED",
            "Idempotency-Key was already used with a different request",
        ));
    }
    if inserted.rows_affected() == 1 {
        Ok(IdempotencyReservation::Reserved)
    } else {
        Ok(IdempotencyReservation::Replay {
            response: sqlx::Row::try_get(&row, "response_json")?,
        })
    }
}

pub async fn complete_idempotency<T: Serialize>(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    operation_key: &str,
    idempotency_key: Option<&str>,
    resource_id: Uuid,
    response: &T,
) -> AppResult<()> {
    if let Some(idempotency_key) = idempotency_key {
        let response = serde_json::to_value(response).map_err(AppError::internal)?;
        sqlx::query("UPDATE idempotency_records SET resource_id=?,response_json=? WHERE tenant_id=? AND operation_key=? AND idempotency_key=?")
            .bind(resource_id).bind(response).bind(tenant_id).bind(operation_key).bind(idempotency_key)
            .execute(&mut **tx).await?;
    }
    Ok(())
}

pub async fn audit(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    action: &str,
    target_type: &str,
    target_id: Uuid,
    detail: Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO audit_events(id,tenant_id,actor_user_id,action,target_type,target_id,request_id,detail_json) VALUES(?,?,?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(actor.user_id).bind(action)
        .bind(target_type).bind(target_id.to_string()).bind(Uuid::now_v7()).bind(detail)
        .execute(&mut **tx).await?;
    Ok(())
}

pub async fn outbox(
    tx: &mut Transaction<'_, MySql>,
    actor: &AuthActor,
    event_type: &str,
    aggregate_type: &str,
    aggregate_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO outbox_events(id,tenant_id,event_type,aggregate_type,aggregate_id,payload_json) VALUES(?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(event_type).bind(aggregate_type)
        .bind(aggregate_id.to_string()).bind(payload).execute(&mut **tx).await?;
    Ok(())
}

pub async fn require_workflow_access(
    pool: &sqlx::MySqlPool,
    actor: &AuthActor,
    workflow_id: Uuid,
    write: bool,
) -> AppResult<()> {
    if actor.company_admin {
        return Ok(());
    }
    let allowed: bool = if write {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflows w LEFT JOIN workflow_members wm ON wm.workflow_id=w.id AND wm.user_id=? WHERE w.id=? AND w.tenant_id=? AND (w.owner_user_id=? OR wm.member_role IN ('editor','manager') OR EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id)))")
            .bind(actor.user_id).bind(workflow_id).bind(actor.tenant_id).bind(actor.user_id).bind(actor.user_id).fetch_one(pool).await?
    } else {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflows w LEFT JOIN workflow_members wm ON wm.workflow_id=w.id AND wm.user_id=? WHERE w.id=? AND w.tenant_id=? AND (w.visibility='company' OR w.owner_user_id=? OR wm.user_id IS NOT NULL OR (w.visibility='department' AND EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.user_id=? AND ur.tenant_id=w.tenant_id AND dc.descendant_id=w.owner_department_id))))")
            .bind(actor.user_id).bind(workflow_id).bind(actor.tenant_id).bind(actor.user_id).bind(actor.user_id).fetch_one(pool).await?
    };
    if allowed {
        Ok(())
    } else {
        Err(AppError::not_found("Workflow"))
    }
}

pub async fn require_department_scope(
    pool: &sqlx::MySqlPool,
    actor: &AuthActor,
    department_id: Uuid,
) -> AppResult<()> {
    if actor.company_admin {
        return Ok(());
    }
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN department_closure dc ON dc.ancestor_id=ur.scope_department_id AND dc.tenant_id=ur.tenant_id WHERE ur.tenant_id=? AND ur.user_id=? AND dc.descendant_id=?)")
        .bind(actor.tenant_id).bind(actor.user_id).bind(department_id).fetch_one(pool).await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::forbidden("Department is outside your data scope"))
    }
}
