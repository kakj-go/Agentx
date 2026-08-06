use std::{str::FromStr, time::Duration};

use agentx_application::{RuntimeCommand, RuntimeCommandType, StartExecutionCommandPayload};
use agentx_domain::TenantId;
use agentx_infrastructure::runtime_commands::RuntimeCommandRepository;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::stable_invocation_id;

const MISFIRE_GRACE: chrono::Duration = chrono::Duration::seconds(5);

pub async fn run(pool: MySqlPool) {
    loop {
        if let Err(error) = scan_once(&pool).await {
            tracing::error!(%error, "schedule trigger scan failed");
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn scan_once(pool: &MySqlPool) -> Result<()> {
    let rows = sqlx::query("SELECT s.id,s.tenant_id,s.application_id,s.cron_expression,s.timezone,s.input_json,s.next_fire_at,s.last_fire_at,h.deployment_id,ad.workflow_version_id FROM application_schedules s JOIN applications a ON a.id=s.application_id AND a.tenant_id=s.tenant_id AND a.status='active' JOIN application_deployment_heads h ON h.application_id=s.application_id AND h.tenant_id=s.tenant_id JOIN application_deployments ad ON ad.id=h.deployment_id AND ad.tenant_id=s.tenant_id AND ad.status='active' WHERE s.status='active' AND (s.next_fire_at IS NULL OR s.next_fire_at<=CURRENT_TIMESTAMP(6)) AND (s.locked_until IS NULL OR s.locked_until<CURRENT_TIMESTAMP(6)) ORDER BY s.next_fire_at,s.id LIMIT 50")
        .fetch_all(pool)
        .await?;
    for row in rows {
        process_schedule(pool, row).await?;
    }
    Ok(())
}

async fn process_schedule(pool: &MySqlPool, row: sqlx::mysql::MySqlRow) -> Result<()> {
    let schedule_id: Uuid = row.try_get("id")?;
    let tenant_id: Uuid = row.try_get("tenant_id")?;
    let mut tx = pool.begin().await?;
    let current = sqlx::query("SELECT id,application_id,cron_expression,timezone,input_json,misfire_policy,next_fire_at,last_fire_at FROM application_schedules WHERE id=? AND tenant_id=? AND status='active' FOR UPDATE")
        .bind(schedule_id)
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(current) = current else {
        tx.rollback().await?;
        return Ok(());
    };
    let expression: String = current.try_get("cron_expression")?;
    let timezone: String = current.try_get("timezone")?;
    let now = Utc::now();
    let due = current
        .try_get::<Option<OffsetDateTime>, _>("next_fire_at")?
        .map(to_chrono)
        .unwrap_or(now);
    if due > now {
        tx.rollback().await?;
        return Ok(());
    }
    let application_id: Uuid = current.try_get("application_id")?;
    let workflow_version_id: Uuid = row.try_get("workflow_version_id")?;
    let deployment_id: Uuid = row.try_get("deployment_id")?;
    let input: Value = current.try_get("input_json")?;
    let misfire_policy: String = current.try_get("misfire_policy")?;
    let fire = should_fire(&misfire_policy, due, now);
    if fire {
        let caller_id = schedule_id;
        let invocation_id = stable_invocation_id(
            tenant_id,
            application_id,
            "schedule",
            Some(caller_id),
            &due.to_rfc3339(),
        );
        let idempotency_key = format!("schedule:{}:{}", schedule_id, due.to_rfc3339());
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
                trigger_type: "schedule".into(),
                input,
                runtime_settings: json!({"scheduleId":schedule_id,"scheduledAt":due.to_rfc3339()}),
            })?,
        );
        RuntimeCommandRepository::enqueue_in_transaction(&mut tx, &command).await?;
        sqlx::query("INSERT INTO application_invocations(id,tenant_id,application_id,application_deployment_id,session_id,workflow_version_id,execution_id,runtime_command_id,caller_type,caller_id,request_hash,idempotency_key,status) VALUES(?,?,?,?,?,?,NULL,?,'schedule',?,?,?, 'queued') ON DUPLICATE KEY UPDATE id=id")
            .bind(invocation_id.as_uuid())
            .bind(tenant_id)
            .bind(application_id)
            .bind(deployment_id)
            .bind(None::<Uuid>)
            .bind(workflow_version_id)
            .bind(command.id)
            .bind(caller_id)
            .bind(format!("{:x}", sha2::Sha256::digest(idempotency_key.as_bytes())))
            .bind(&idempotency_key)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO invocation_events(tenant_id,invocation_id,sequence_number,event_type,payload_json) VALUES(?,?,1,'invocation.queued',?) ON DUPLICATE KEY UPDATE invocation_id=VALUES(invocation_id)")
            .bind(tenant_id)
            .bind(invocation_id.as_uuid())
            .bind(json!({"commandId":command.id,"scheduledAt":due.to_rfc3339()}))
            .execute(&mut *tx)
            .await?;
    }
    let next = next_fire(&expression, &timezone, now)?;
    sqlx::query("UPDATE application_schedules SET last_fire_at=IF(?, ?, last_fire_at),next_fire_at=?,locked_by=NULL,locked_until=NULL WHERE id=? AND tenant_id=?")
        .bind(fire)
        .bind(from_chrono(due))
        .bind(from_chrono(next))
        .bind(schedule_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn should_fire(policy: &str, due: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    policy != "skip" || now.signed_duration_since(due) <= MISFIRE_GRACE
}

fn next_fire(expression: &str, timezone: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let normalized = if expression.split_whitespace().count() == 5 {
        format!("0 {expression}")
    } else {
        expression.into()
    };
    let schedule = Schedule::from_str(&normalized).context("cron expression is invalid")?;
    let tz: Tz = timezone
        .parse()
        .context("timezone must be an IANA timezone")?;
    let local = after.with_timezone(&tz);
    schedule
        .after(&local)
        .next()
        .map(|value| value.with_timezone(&Utc))
        .context("cron expression has no next fire time")
}

fn to_chrono(value: OffsetDateTime) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(value.unix_timestamp(), value.nanosecond())
        .unwrap_or_else(Utc::now)
}

fn from_chrono(value: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(value.timestamp())
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::{next_fire, should_fire};
    use chrono::{TimeZone, Utc};

    #[test]
    fn schedule_uses_iana_timezone() {
        let after = Utc.with_ymd_and_hms(2026, 8, 5, 0, 30, 0).unwrap();
        let next = next_fire("0 9 * * *", "Asia/Shanghai", after).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 8, 5, 1, 0, 0).unwrap());
    }

    #[test]
    fn nonexistent_dst_time_is_skipped() {
        let after = Utc.with_ymd_and_hms(2026, 3, 8, 6, 0, 0).unwrap();
        let next = next_fire("30 2 * * *", "America/New_York", after).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 9, 6, 30, 0).unwrap());
    }

    #[test]
    fn ambiguous_dst_time_fires_at_most_once() {
        let before_first = Utc.with_ymd_and_hms(2026, 11, 1, 4, 45, 0).unwrap();
        let first = next_fire("30 1 * * *", "America/New_York", before_first).unwrap();
        assert_eq!(first, Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap());
        let after = Utc.with_ymd_and_hms(2026, 11, 1, 5, 45, 0).unwrap();
        let next = next_fire("30 1 * * *", "America/New_York", after).unwrap();
        assert!(next > after);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 11, 2, 6, 30, 0).unwrap());
    }

    #[test]
    fn misfire_policy_skips_stale_runs_or_fires_once() {
        let now = Utc.with_ymd_and_hms(2026, 8, 5, 1, 0, 0).unwrap();
        let stale = Utc.with_ymd_and_hms(2026, 8, 5, 0, 55, 0).unwrap();
        assert!(!should_fire("skip", stale, now));
        assert!(should_fire("fire_once", stale, now));
        assert!(should_fire("skip", now, now));
    }
}
