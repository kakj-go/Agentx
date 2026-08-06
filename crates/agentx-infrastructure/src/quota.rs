use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{config::RedisSettings, runtime_repository::RuntimeTask};

pub const EXECUTION_CONCURRENCY: &str = "execution_concurrency";
pub const NODE_CONCURRENCY: &str = "node_concurrency";
pub const SANDBOX_CONCURRENCY: &str = "sandbox_concurrency";
pub const TOKENS: &str = "tokens";
pub const COST_MICROS: &str = "cost_micros";
pub const ARTIFACT_BYTES: &str = "artifact_bytes";
pub const AGENT_ITERATIONS: &str = "agent_iterations";
pub const SUPPORTED_DIMENSIONS: &[&str] = &[
    EXECUTION_CONCURRENCY,
    NODE_CONCURRENCY,
    SANDBOX_CONCURRENCY,
    AGENT_ITERATIONS,
    TOKENS,
    COST_MICROS,
    ARTIFACT_BYTES,
    "cpu_millis",
    "memory_bytes",
    "pids",
    "disk_bytes",
    "ttl_seconds",
];

const REDIS_QUOTA_PREFIX: &str = "agentx:quota:v1";

#[derive(Clone)]
pub struct QuotaAdmission {
    settings: RedisSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionDecision {
    Reserved,
    AlreadyReserved,
}

impl QuotaAdmission {
    #[must_use]
    pub fn new(settings: RedisSettings) -> Self {
        Self { settings }
    }

    /// Atomically admit a scope in Redis. MySQL still performs the authoritative
    /// reservation and this counter is periodically rebuilt from it.
    #[allow(clippy::too_many_arguments)]
    pub async fn try_admit(
        &self,
        tenant_id: Uuid,
        dimension: &str,
        scope_type: &str,
        scope_id: &str,
        amount: Decimal,
        hard_limit: Decimal,
        ttl_seconds: u64,
    ) -> Result<bool> {
        let reservation = QuotaReservation {
            tenant_id,
            dimension,
            scope_type,
            scope_id,
            amount,
            ttl_seconds,
            fail_closed: true,
        };
        match self.reserve(&reservation, hard_limit).await {
            Ok(AdmissionDecision::Reserved) => Ok(true),
            Ok(AdmissionDecision::AlreadyReserved) => Ok(false),
            Err(error) if error.to_string().contains("QUOTA_EXCEEDED") => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub async fn release_scope(
        &self,
        tenant_id: Uuid,
        dimension: &str,
        scope_type: &str,
        scope_id: &str,
    ) -> Result<()> {
        self.release(tenant_id, dimension, scope_type, scope_id)
            .await
    }

    async fn reserve(
        &self,
        reservation: &QuotaReservation<'_>,
        hard_limit: Decimal,
    ) -> Result<AdmissionDecision> {
        let mut connection = crate::clients::connect_redis(&self.settings).await?;
        let ttl_seconds = reservation.ttl_seconds.clamp(5, 604_800);
        let result: i64 = redis::Script::new(
            r#"
local existing = redis.call('GET', KEYS[2])
if existing then
  return 2
end
local current = tonumber(redis.call('GET', KEYS[1]) or '0')
local requested = tonumber(ARGV[1])
local hard_limit = tonumber(ARGV[2])
if not current or not requested or not hard_limit or current + requested > hard_limit then
  return 0
end
redis.call('INCRBYFLOAT', KEYS[1], ARGV[1])
redis.call('SET', KEYS[2], ARGV[1], 'EX', ARGV[3])
local counter_ttl = redis.call('TTL', KEYS[1])
if counter_ttl < tonumber(ARGV[3]) then
  redis.call('EXPIRE', KEYS[1], ARGV[3])
end
return 1
"#,
        )
        .key(counter_key(reservation.tenant_id, reservation.dimension))
        .key(scope_key(
            reservation.tenant_id,
            reservation.dimension,
            reservation.scope_type,
            reservation.scope_id,
        ))
        .arg(reservation.amount.to_string())
        .arg(hard_limit.to_string())
        .arg(ttl_seconds)
        .invoke_async(&mut connection)
        .await
        .context("Redis quota admission failed")?;
        match result {
            1 => Ok(AdmissionDecision::Reserved),
            2 => Ok(AdmissionDecision::AlreadyReserved),
            _ => anyhow::bail!("QUOTA_EXCEEDED: {}", reservation.dimension),
        }
    }

    async fn release(
        &self,
        tenant_id: Uuid,
        dimension: &str,
        scope_type: &str,
        scope_id: &str,
    ) -> Result<()> {
        let mut connection = crate::clients::connect_redis(&self.settings).await?;
        redis::Script::new(
            r#"
local amount = redis.call('GET', KEYS[2])
if not amount then
  return 0
end
redis.call('DEL', KEYS[2])
local remaining = tonumber(redis.call('INCRBYFLOAT', KEYS[1], '-' .. amount))
if not remaining or remaining <= 0 then
  redis.call('DEL', KEYS[1])
end
return 1
"#,
        )
        .key(counter_key(tenant_id, dimension))
        .key(scope_key(tenant_id, dimension, scope_type, scope_id))
        .invoke_async::<i64>(&mut connection)
        .await
        .context("Redis quota release failed")?;
        Ok(())
    }

    pub async fn rebuild_from_mysql(&self, pool: &sqlx::MySqlPool) -> Result<u64> {
        let rows = sqlx::query(
            "SELECT tenant_id,dimension_key,scope_type,scope_id,amount,expires_at
             FROM quota_reservations
             WHERE status='active' AND expires_at>CURRENT_TIMESTAMP(6)
             ORDER BY tenant_id,dimension_key,scope_type,scope_id",
        )
        .fetch_all(pool)
        .await
        .context("load active quota reservations for Redis calibration")?;
        let mut connection = crate::clients::connect_redis(&self.settings).await?;
        let mut cursor = 0_u64;
        loop {
            let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(format!("{REDIS_QUOTA_PREFIX}:*"))
                .arg("COUNT")
                .arg(500)
                .query_async(&mut connection)
                .await
                .context("scan Redis quota keys")?;
            if !keys.is_empty() {
                redis::cmd("DEL")
                    .arg(keys)
                    .query_async::<u64>(&mut connection)
                    .await
                    .context("clear Redis quota keys before calibration")?;
            }
            cursor = next;
            if cursor == 0 {
                break;
            }
        }

        let now = OffsetDateTime::now_utc();
        let mut counters = BTreeMap::<(Uuid, String), (Decimal, u64)>::new();
        let mut active = 0_u64;
        for row in rows {
            let tenant_id: Uuid = row.try_get("tenant_id")?;
            let dimension: String = row.try_get("dimension_key")?;
            let scope_type: String = row.try_get("scope_type")?;
            let scope_id: String = row.try_get("scope_id")?;
            let amount: Decimal = row.try_get("amount")?;
            let expires_at: OffsetDateTime = row.try_get("expires_at")?;
            let ttl = (expires_at - now).whole_seconds().max(1) as u64;
            let entry = counters
                .entry((tenant_id, dimension.clone()))
                .or_insert((Decimal::ZERO, 0));
            entry.0 += amount;
            entry.1 = entry.1.max(ttl);
            redis::cmd("SET")
                .arg(scope_key(tenant_id, &dimension, &scope_type, &scope_id))
                .arg(amount.to_string())
                .arg("EX")
                .arg(ttl)
                .query_async::<String>(&mut connection)
                .await
                .context("restore Redis quota scope")?;
            active += 1;
        }
        for ((tenant_id, dimension), (amount, ttl)) in counters {
            redis::cmd("SET")
                .arg(counter_key(tenant_id, &dimension))
                .arg(amount.to_string())
                .arg("EX")
                .arg(ttl.max(1))
                .query_async::<String>(&mut connection)
                .await
                .context("restore Redis quota counter")?;
        }
        Ok(active)
    }
}

fn counter_key(tenant_id: Uuid, dimension: &str) -> String {
    format!("{REDIS_QUOTA_PREFIX}:reserved:{tenant_id}:{dimension}")
}

fn scope_key(tenant_id: Uuid, dimension: &str, scope_type: &str, scope_id: &str) -> String {
    format!("{REDIS_QUOTA_PREFIX}:scope:{tenant_id}:{dimension}:{scope_type}:{scope_id}")
}

pub struct QuotaReservation<'a> {
    pub tenant_id: Uuid,
    pub dimension: &'a str,
    pub scope_type: &'a str,
    pub scope_id: &'a str,
    pub amount: Decimal,
    pub ttl_seconds: u64,
    pub fail_closed: bool,
}

pub async fn reserve(
    transaction: &mut Transaction<'_, MySql>,
    reservation: &QuotaReservation<'_>,
) -> Result<Option<Uuid>> {
    reserve_with_admission(transaction, reservation, None).await
}

pub async fn reserve_with_admission(
    transaction: &mut Transaction<'_, MySql>,
    reservation: &QuotaReservation<'_>,
    admission: Option<&QuotaAdmission>,
) -> Result<Option<Uuid>> {
    let QuotaReservation {
        tenant_id,
        dimension,
        scope_type,
        scope_id,
        amount,
        ttl_seconds,
        fail_closed,
    } = reservation;
    let policy = sqlx::query("SELECT hard_limit,period_seconds FROM quota_policies WHERE tenant_id=? AND dimension_key=? FOR UPDATE")
        .bind(tenant_id)
        .bind(dimension)
        .fetch_optional(&mut **transaction)
        .await?;
    let Some(policy) = policy else {
        anyhow::ensure!(!*fail_closed, "QUOTA_POLICY_MISSING: {dimension}");
        return Ok(None);
    };
    if let Some(existing) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM quota_reservations WHERE tenant_id=? AND dimension_key=? AND scope_type=? AND scope_id=? AND status='active'")
        .bind(tenant_id)
        .bind(dimension)
        .bind(scope_type)
        .bind(scope_id)
        .fetch_optional(&mut **transaction)
        .await?
    {
        return Ok(Some(existing));
    }
    let hard_limit: Decimal = policy.try_get("hard_limit")?;
    let active: Decimal = sqlx::query_scalar("SELECT COALESCE(SUM(amount),0) FROM quota_reservations WHERE tenant_id=? AND dimension_key=? AND status='active' AND expires_at>CURRENT_TIMESTAMP(6)")
        .bind(tenant_id)
        .bind(dimension)
        .fetch_one(&mut **transaction)
        .await?;
    let period_seconds: Option<u64> = policy.try_get("period_seconds")?;
    let used = if let Some(period_seconds) = period_seconds {
        sqlx::query_scalar("SELECT COALESCE(SUM(amount),0) FROM quota_usage_ledger WHERE tenant_id=? AND dimension_key=? AND occurred_at>=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND)")
            .bind(tenant_id)
            .bind(dimension)
            .bind(period_seconds)
            .fetch_one(&mut **transaction)
            .await?
    } else if *dimension == ARTIFACT_BYTES {
        sqlx::query_scalar("SELECT COALESCE(SUM(amount),0) FROM quota_usage_ledger WHERE tenant_id=? AND dimension_key=?")
            .bind(tenant_id)
            .bind(dimension)
            .fetch_one(&mut **transaction)
            .await?
    } else {
        Decimal::ZERO
    };
    anyhow::ensure!(
        active + used + *amount <= hard_limit,
        "QUOTA_EXCEEDED: {dimension}"
    );
    let admission_decision = match admission {
        Some(admission) => match admission.reserve(reservation, hard_limit).await {
            Ok(decision) => Some(decision),
            Err(error) if !*fail_closed && !error.to_string().contains("QUOTA_EXCEEDED") => {
                warn!(%error, %tenant_id, %dimension, "Redis quota admission unavailable; MySQL remains authoritative in non-production environment");
                None
            }
            Err(error) => return Err(error),
        },
        None => {
            anyhow::ensure!(!*fail_closed, "QUOTA_ADMISSION_UNAVAILABLE: {dimension}");
            None
        }
    };
    let reservation_id = Uuid::now_v7();
    let inserted = sqlx::query("INSERT INTO quota_reservations(id,tenant_id,dimension_key,scope_type,scope_id,amount,expires_at) VALUES(?,?,?,?,?,?,DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL ? SECOND))")
        .bind(reservation_id)
        .bind(tenant_id)
        .bind(dimension)
        .bind(scope_type)
        .bind(scope_id)
        .bind(amount)
        .bind((*ttl_seconds).clamp(5, 604_800))
        .execute(&mut **transaction)
        .await;
    if let Err(error) = inserted {
        if admission_decision == Some(AdmissionDecision::Reserved)
            && let Some(admission) = admission
        {
            let _ = admission
                .release(*tenant_id, dimension, scope_type, scope_id)
                .await;
        }
        return Err(error.into());
    }
    Ok(Some(reservation_id))
}

pub async fn release_scope(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    scope_type: &str,
    scope_id: &str,
) -> Result<u64> {
    release_scope_with_admission(transaction, tenant_id, scope_type, scope_id, None).await
}

pub async fn release_scope_with_admission(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    scope_type: &str,
    scope_id: &str,
    admission: Option<&QuotaAdmission>,
) -> Result<u64> {
    let reservations = sqlx::query(
        "SELECT dimension_key FROM quota_reservations
         WHERE tenant_id=? AND scope_type=? AND scope_id=? AND status='active' FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(scope_type)
    .bind(scope_id)
    .fetch_all(&mut **transaction)
    .await?;
    let changed = sqlx::query("UPDATE quota_reservations SET status='released',settled_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND scope_type=? AND scope_id=? AND status='active'")
        .bind(tenant_id)
        .bind(scope_type)
        .bind(scope_id)
        .execute(&mut **transaction)
        .await?;
    if let Some(admission) = admission {
        for reservation in reservations {
            let dimension: String = reservation.try_get("dimension_key")?;
            if let Err(error) = admission
                .release(tenant_id, &dimension, scope_type, scope_id)
                .await
            {
                warn!(%error, %tenant_id, %dimension, %scope_type, %scope_id, "Redis quota release will be repaired by calibration");
            }
        }
    }
    Ok(changed.rows_affected())
}

pub async fn reap_expired(pool: &sqlx::MySqlPool) -> Result<u64> {
    let changed = sqlx::query("UPDATE quota_reservations SET status='expired',settled_at=CURRENT_TIMESTAMP(6) WHERE status='active' AND expires_at<=CURRENT_TIMESTAMP(6)")
        .execute(pool)
        .await
        .context("reap expired quota reservations")?;
    Ok(changed.rows_affected())
}

pub async fn reserve_attempt(pool: &sqlx::MySqlPool, task: &RuntimeTask) -> Result<()> {
    reserve_attempt_with_admission(pool, task, None).await
}

pub async fn reserve_attempt_with_admission(
    pool: &sqlx::MySqlPool,
    task: &RuntimeTask,
    admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let now = time::OffsetDateTime::now_utc();
    let ttl = (task.deadline - now).whole_seconds().max(5) as u64;
    let fail_closed = production_fail_closed();
    let scope_id = task.attempt_id.to_string();
    reserve_with_admission(
        &mut tx,
        &QuotaReservation {
            tenant_id: task.tenant_id,
            dimension: NODE_CONCURRENCY,
            scope_type: "node_attempt",
            scope_id: &scope_id,
            amount: Decimal::ONE,
            ttl_seconds: ttl,
            fail_closed,
        },
        admission,
    )
    .await?;
    if task.capability == "sandbox" {
        reserve_with_admission(
            &mut tx,
            &QuotaReservation {
                tenant_id: task.tenant_id,
                dimension: SANDBOX_CONCURRENCY,
                scope_type: "node_attempt",
                scope_id: &scope_id,
                amount: Decimal::ONE,
                ttl_seconds: ttl,
                fail_closed,
            },
            admission,
        )
        .await?;
        let profile = task
            .resource_snapshots
            .iter()
            .find(|snapshot| snapshot.reference.resource_type.as_str() == "sandbox_profile")
            .map(|snapshot| &snapshot.snapshot)
            .context("Sandbox Profile snapshot is missing")?;
        for (dimension, field) in [
            ("cpu_millis", "cpuMillis"),
            ("memory_bytes", "memoryBytes"),
            ("pids", "pidsLimit"),
            ("disk_bytes", "diskBytes"),
            ("ttl_seconds", "timeoutSeconds"),
        ] {
            let amount = profile
                .get(field)
                .and_then(serde_json::Value::as_u64)
                .with_context(|| format!("Sandbox Profile {field} is missing"))?;
            reserve_with_admission(
                &mut tx,
                &QuotaReservation {
                    tenant_id: task.tenant_id,
                    dimension,
                    scope_type: "node_attempt",
                    scope_id: &scope_id,
                    amount: Decimal::from(amount),
                    ttl_seconds: ttl,
                    fail_closed,
                },
                admission,
            )
            .await?;
        }
    }
    if task.capability == "agent" {
        reserve_with_admission(
            &mut tx,
            &QuotaReservation {
                tenant_id: task.tenant_id,
                dimension: AGENT_ITERATIONS,
                scope_type: "node_attempt",
                scope_id: &scope_id,
                amount: Decimal::from(
                    task.node_parameters
                        .get("maxIterations")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(12),
                ),
                ttl_seconds: ttl,
                fail_closed,
            },
            admission,
        )
        .await?;
        reserve_with_admission(
            &mut tx,
            &QuotaReservation {
                tenant_id: task.tenant_id,
                dimension: TOKENS,
                scope_type: "node_attempt",
                scope_id: &scope_id,
                amount: Decimal::from(
                    task.node_parameters
                        .get("maxTotalTokens")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(64_000),
                ),
                ttl_seconds: ttl,
                fail_closed,
            },
            admission,
        )
        .await?;
        reserve_with_admission(
            &mut tx,
            &QuotaReservation {
                tenant_id: task.tenant_id,
                dimension: COST_MICROS,
                scope_type: "node_attempt",
                scope_id: &scope_id,
                amount: Decimal::from(
                    task.node_parameters
                        .get("maxCostMicros")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1_000_000),
                ),
                ttl_seconds: ttl,
                fail_closed,
            },
            admission,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn reserve_artifact(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    artifact_id: Uuid,
    size_bytes: u64,
) -> Result<()> {
    reserve_artifact_with_admission(pool, tenant_id, artifact_id, size_bytes, None).await
}

pub async fn reserve_artifact_with_admission(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    artifact_id: Uuid,
    size_bytes: u64,
    admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let scope_id = artifact_id.to_string();
    reserve_with_admission(
        &mut tx,
        &QuotaReservation {
            tenant_id,
            dimension: ARTIFACT_BYTES,
            scope_type: "artifact",
            scope_id: &scope_id,
            amount: Decimal::from(size_bytes),
            ttl_seconds: 600,
            fail_closed: production_fail_closed(),
        },
        admission,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn settle_artifact(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    artifact_id: Uuid,
    size_bytes: u64,
) -> Result<()> {
    settle_artifact_with_admission(transaction, tenant_id, artifact_id, size_bytes, None).await
}

pub async fn settle_artifact_with_admission(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    artifact_id: Uuid,
    size_bytes: u64,
    admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let scope_id = artifact_id.to_string();
    sqlx::query(
        "INSERT IGNORE INTO quota_usage_ledger
         (id,tenant_id,dimension_key,scope_type,scope_id,amount,idempotency_key)
         VALUES(?,?,'artifact_bytes','artifact',?,?,?)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(&scope_id)
    .bind(Decimal::from(size_bytes))
    .bind(format!("artifact:{artifact_id}:created"))
    .execute(&mut **transaction)
    .await?;
    sqlx::query("UPDATE quota_reservations SET status='settled',settled_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND dimension_key='artifact_bytes' AND scope_type='artifact' AND scope_id=? AND status='active'")
        .bind(tenant_id)
        .bind(&scope_id)
        .execute(&mut **transaction)
        .await?;
    if let Some(admission) = admission
        && let Err(error) = admission
            .release(tenant_id, ARTIFACT_BYTES, "artifact", &scope_id)
            .await
    {
        warn!(%error, %tenant_id, artifact_id=%scope_id, "Redis artifact quota release will be repaired by calibration");
    }
    Ok(())
}

pub async fn release_artifact_reservation(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    artifact_id: Uuid,
) -> Result<()> {
    release_artifact_reservation_with_admission(pool, tenant_id, artifact_id, None).await
}

pub async fn release_artifact_reservation_with_admission(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    artifact_id: Uuid,
    admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    release_scope_with_admission(
        &mut tx,
        tenant_id,
        "artifact",
        &artifact_id.to_string(),
        admission,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn record_artifact_deletion(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    artifact_id: Uuid,
    size_bytes: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT IGNORE INTO quota_usage_ledger
         (id,tenant_id,dimension_key,scope_type,scope_id,amount,idempotency_key)
         VALUES(?,?,'artifact_bytes','artifact',?,?,?)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(artifact_id.to_string())
    .bind(-Decimal::from(size_bytes))
    .bind(format!("artifact:{artifact_id}:deleted"))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn settle_attempt(pool: &sqlx::MySqlPool, task: &RuntimeTask) -> Result<()> {
    settle_attempt_with_admission(pool, task, None).await
}

pub async fn settle_attempt_with_admission(
    pool: &sqlx::MySqlPool,
    task: &RuntimeTask,
    admission: Option<&QuotaAdmission>,
) -> Result<()> {
    let usage = sqlx::query(
        "SELECT CAST(COALESCE(SUM(input_tokens+output_tokens),0) AS UNSIGNED) tokens,
         CAST(COALESCE(SUM(cost_micros),0) AS UNSIGNED) cost_micros,
         CAST(COALESCE((SELECT MAX(iteration_count) FROM agent_runs WHERE tenant_id=? AND node_execution_id=?),0) AS UNSIGNED) agent_iterations
         FROM runtime_calls
         WHERE tenant_id=? AND attempt_id=? AND status IN ('succeeded','failed')",
    )
    .bind(task.tenant_id)
    .bind(task.node_execution_id)
    .bind(task.tenant_id)
    .bind(task.attempt_id)
    .fetch_one(pool)
    .await?;
    let tokens = Decimal::from(usage.try_get::<u64, _>("tokens")?);
    let cost = Decimal::from(usage.try_get::<u64, _>("cost_micros")?);
    let iterations = Decimal::from(usage.try_get::<u64, _>("agent_iterations")?);
    let scope_id = task.attempt_id.to_string();
    let mut tx = pool.begin().await?;
    settle_dimension(&mut tx, task.tenant_id, TOKENS, &scope_id, tokens).await?;
    settle_dimension(&mut tx, task.tenant_id, COST_MICROS, &scope_id, cost).await?;
    settle_dimension(
        &mut tx,
        task.tenant_id,
        AGENT_ITERATIONS,
        &scope_id,
        iterations,
    )
    .await?;
    release_scope_with_admission(
        &mut tx,
        task.tenant_id,
        "node_attempt",
        &scope_id,
        admission,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn settle_dimension(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    dimension: &str,
    scope_id: &str,
    amount: Decimal,
) -> Result<()> {
    let idempotency_key = format!("node_attempt:{scope_id}:{dimension}");
    sqlx::query(
        "INSERT IGNORE INTO quota_usage_ledger
         (id,tenant_id,dimension_key,scope_type,scope_id,amount,idempotency_key)
         VALUES(?,?,?,'node_attempt',?,?,?)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(dimension)
    .bind(scope_id)
    .bind(amount)
    .bind(idempotency_key)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE quota_reservations SET status='settled',settled_at=CURRENT_TIMESTAMP(6)
         WHERE tenant_id=? AND dimension_key=? AND scope_type='node_attempt' AND scope_id=? AND status='active'",
    )
    .bind(tenant_id)
    .bind(dimension)
    .bind(scope_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn production_fail_closed() -> bool {
    crate::config::is_production_environment()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rust_decimal::Decimal;
    use secrecy::SecretString;
    use testcontainers::{
        GenericImage,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use tokio::task::JoinSet;
    use uuid::Uuid;

    use crate::config::RedisSettings;

    use super::{EXECUTION_CONCURRENCY, QuotaAdmission};

    #[test]
    fn decimal_limits_do_not_use_floating_point() {
        assert_eq!(Decimal::new(1, 1) + Decimal::new(2, 1), Decimal::new(3, 1));
    }

    #[tokio::test]
    #[ignore = "requires Docker"]
    async fn redis_admission_is_atomic_idempotent_and_releasable() {
        let container = GenericImage::new("redis", "7.4-alpine")
            .with_exposed_port(6379.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .await
            .expect("Redis container should start");
        let port = container
            .get_host_port_ipv4(6379.tcp())
            .await
            .expect("mapped Redis port");
        let admission = Arc::new(QuotaAdmission::new(RedisSettings {
            url: SecretString::from(format!("redis://127.0.0.1:{port}/")),
            password: None,
            tls_ca_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
        }));
        let tenant = Uuid::now_v7();
        let mut tasks = JoinSet::new();
        for index in 0..32 {
            let admission = admission.clone();
            tasks.spawn(async move {
                admission
                    .try_admit(
                        tenant,
                        EXECUTION_CONCURRENCY,
                        "execution",
                        &format!("scope-{index}"),
                        Decimal::ONE,
                        Decimal::from(10_u32),
                        60,
                    )
                    .await
                    .expect("Redis admission request")
            });
        }
        let mut admitted = 0;
        while let Some(result) = tasks.join_next().await {
            if result.expect("admission task") {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 10, "hard limit must be enforced atomically");
        assert!(
            !admission
                .try_admit(
                    tenant,
                    EXECUTION_CONCURRENCY,
                    "execution",
                    "scope-0",
                    Decimal::ONE,
                    Decimal::from(10_u32),
                    60,
                )
                .await
                .expect("idempotent admission request")
        );
        admission
            .release_scope(tenant, EXECUTION_CONCURRENCY, "execution", "scope-0")
            .await
            .expect("release admission");
        assert!(
            admission
                .try_admit(
                    tenant,
                    EXECUTION_CONCURRENCY,
                    "execution",
                    "scope-new",
                    Decimal::ONE,
                    Decimal::from(10_u32),
                    60,
                )
                .await
                .expect("admission after release")
        );
    }
}
