use std::{sync::Arc, time::Duration};

use anyhow::Result;
use object_store::{ObjectStore, path::Path};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

const MAX_ATTEMPTS: u32 = 5;

struct RetentionRun {
    id: Uuid,
    tenant_id: Uuid,
    dry_run: bool,
}

/// Builds retention manifests and executes them in small, recoverable batches.
/// Object deletion is intentionally outside a MySQL transaction; the metadata
/// update is idempotent and a retry can safely repeat an object-store delete.
pub struct RetentionProcessor {
    pool: MySqlPool,
    objects: Option<Arc<dyn ObjectStore>>,
    clickhouse: Option<clickhouse::Client>,
    owner: Uuid,
}

impl RetentionProcessor {
    #[must_use]
    pub fn new(
        pool: MySqlPool,
        objects: Option<Arc<dyn ObjectStore>>,
        clickhouse: Option<clickhouse::Client>,
    ) -> Self {
        Self {
            pool,
            objects,
            clickhouse,
            owner: Uuid::now_v7(),
        }
    }

    /// Processes at most one retention run. Returns zero when no runnable run
    /// is available, which lets the caller use a cheap polling loop.
    pub async fn process_batch(&self) -> Result<u64> {
        let Some(run) = self.claim_run().await? else {
            return Ok(0);
        };
        if let Err(error) = self.prepare_items(&run).await {
            self.fail_run(&run, &error.to_string()).await?;
            return Err(error);
        }
        if run.dry_run {
            self.complete_run(&run).await?;
            return Ok(1);
        }
        let processed = self.delete_items(&run).await?;
        self.finish_or_retry(&run, processed > 0).await?;
        Ok(processed)
    }

    async fn claim_run(&self) -> Result<Option<RetentionRun>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id,tenant_id,dry_run FROM retention_runs
             WHERE ((status='queued' AND available_at<=CURRENT_TIMESTAMP(6))
                 OR (status='running' AND (locked_until IS NULL OR locked_until<CURRENT_TIMESTAMP(6))))
             ORDER BY created_at,id LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let run = RetentionRun {
            id: row.try_get("id")?,
            tenant_id: row.try_get("tenant_id")?,
            dry_run: row.try_get("dry_run")?,
        };
        sqlx::query(
            "UPDATE retention_runs SET status='running',attempt_count=attempt_count+1,
             locked_by=?,locked_until=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 60 SECOND),
             started_at=COALESCE(started_at,CURRENT_TIMESTAMP(6)),error_message=NULL
             WHERE id=?",
        )
        .bind(self.owner)
        .bind(run.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    async fn prepare_items(&self, run: &RetentionRun) -> Result<()> {
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM retention_items WHERE tenant_id=? AND retention_run_id=?",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .fetch_one(&self.pool)
        .await?;
        if existing > 0 {
            return Ok(());
        }
        let policies = sqlx::query("SELECT data_type,retention_days FROM retention_policies WHERE tenant_id=? AND enabled=TRUE")
            .bind(run.tenant_id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| Ok((row.try_get::<String, _>("data_type")?, row.try_get::<u32, _>("retention_days")?)))
            .collect::<Result<std::collections::HashMap<_, _>, sqlx::Error>>()?;
        let mut tx = self.pool.begin().await?;
        if let Some(days) = policies.get("artifact") {
            let rows = sqlx::query("SELECT id FROM artifacts WHERE tenant_id=? AND deleted_at IS NULL AND created_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL ? DAY) ORDER BY created_at,id LIMIT 10000")
                .bind(run.tenant_id).bind(i64::from(*days)).fetch_all(&mut *tx).await?;
            for row in rows {
                let artifact_id: Uuid = row.try_get("id")?;
                let reason = reference_reason(&mut tx, run.tenant_id, artifact_id).await?;
                insert_item(&mut tx, run, "artifact", artifact_id, reason).await?;
            }
        }
        if let Some(days) = policies.get("trace") {
            let rows = sqlx::query("SELECT id FROM workflow_executions WHERE tenant_id=? AND status IN ('succeeded','failed','cancelled','timed_out') AND ended_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL ? DAY) ORDER BY ended_at,id LIMIT 10000")
                .bind(run.tenant_id).bind(i64::from(*days)).fetch_all(&mut *tx).await?;
            for row in rows {
                let execution_id: Uuid = row.try_get("id")?;
                let reason = trace_reference_reason(&mut tx, run.tenant_id, execution_id).await?;
                insert_item(&mut tx, run, "trace", execution_id, reason).await?;
            }
        }
        if let Some(days) = policies.get("application_message") {
            let rows = sqlx::query("SELECT m.id FROM application_messages m JOIN application_sessions s ON s.id=m.session_id AND s.tenant_id=m.tenant_id WHERE m.tenant_id=? AND s.status='closed' AND m.created_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL ? DAY) ORDER BY m.created_at,m.id LIMIT 10000")
                .bind(run.tenant_id).bind(i64::from(*days)).fetch_all(&mut *tx).await?;
            for row in rows {
                insert_item(
                    &mut tx,
                    run,
                    "application_message",
                    row.try_get("id")?,
                    None,
                )
                .await?;
            }
        }
        if let Some(days) = policies.get("evaluation_report") {
            let rows = sqlx::query("SELECT id FROM evaluation_runs WHERE tenant_id=? AND status IN ('completed','failed','cancelled') AND completed_at<DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL ? DAY) ORDER BY completed_at,id LIMIT 10000")
                .bind(run.tenant_id).bind(i64::from(*days)).fetch_all(&mut *tx).await?;
            for row in rows {
                let evaluation_id: Uuid = row.try_get("id")?;
                let referenced: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM evaluation_comparisons WHERE tenant_id=? AND (baseline_run_id=? OR candidate_run_id=?))")
                    .bind(run.tenant_id).bind(evaluation_id).bind(evaluation_id).fetch_one(&mut *tx).await?;
                insert_item(
                    &mut tx,
                    run,
                    "evaluation_report",
                    evaluation_id,
                    referenced.then(|| "evaluation_comparison".into()),
                )
                .await?;
            }
        }
        tx.commit().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM retention_items WHERE tenant_id=? AND retention_run_id=?",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .fetch_one(&self.pool)
        .await?;
        self.set_counts(run, count).await
    }

    async fn delete_items(&self, run: &RetentionRun) -> Result<u64> {
        let rows = sqlx::query(
            "SELECT id,data_type,target_id,attempt_count FROM retention_items
             WHERE tenant_id=? AND retention_run_id=? AND status IN ('candidate','failed')
               AND attempt_count<?
             ORDER BY created_at,id LIMIT 100",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(MAX_ATTEMPTS)
        .fetch_all(&self.pool)
        .await?;
        let mut deleted = 0;
        for row in rows {
            let item_id: Uuid = row.try_get("id")?;
            let data_type: String = row.try_get("data_type")?;
            let target_id: String = row.try_get("target_id")?;
            let attempt_count: u32 = row.try_get("attempt_count")?;
            let Ok(target_id) = Uuid::parse_str(&target_id) else {
                self.mark_item_failed(item_id, "invalid retention target id")
                    .await?;
                continue;
            };
            self.bump_item_attempt(item_id).await?;
            let result = match data_type.as_str() {
                "artifact" => {
                    self.delete_artifact(item_id, run.tenant_id, target_id)
                        .await
                }
                "trace" => self.delete_trace(item_id, run.tenant_id, target_id).await,
                "application_message" => {
                    self.delete_application_message(item_id, run.tenant_id, target_id)
                        .await
                }
                "evaluation_report" => {
                    self.delete_evaluation_report(item_id, run.tenant_id, target_id)
                        .await
                }
                _ => Err(anyhow::anyhow!(
                    "unsupported retention data type {data_type}"
                )),
            };
            match result {
                Ok(true) => {
                    deleted += 1;
                }
                Ok(false) => {}
                Err(error) => {
                    let message = format!("{data_type} deletion failed: {error}");
                    self.mark_item_failed(item_id, &message).await?;
                    if attempt_count + 1 >= MAX_ATTEMPTS {
                        tracing::error!(%error, %target_id, %data_type, "retention item exhausted retries");
                    }
                }
            }
        }
        Ok(deleted)
    }

    async fn delete_artifact(
        &self,
        item_id: Uuid,
        tenant_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<bool> {
        if let Some(reason) = reference_reason_pool(&self.pool, tenant_id, artifact_id).await? {
            self.mark_item_blocked(item_id, &reason).await?;
            return Ok(false);
        }
        let metadata = sqlx::query("SELECT storage_key,size_bytes FROM artifacts WHERE tenant_id=? AND id=? AND deleted_at IS NULL")
            .bind(tenant_id).bind(artifact_id).fetch_optional(&self.pool).await?;
        let Some(metadata) = metadata else {
            self.mark_item_deleted_without_metadata(item_id).await?;
            return Ok(true);
        };
        let objects = self
            .objects
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("object storage is unavailable"))?;
        objects
            .delete(&Path::from(metadata.try_get::<String, _>("storage_key")?))
            .await?;
        self.mark_item_deleted(
            item_id,
            tenant_id,
            artifact_id,
            metadata.try_get("size_bytes")?,
        )
        .await?;
        Ok(true)
    }

    async fn delete_trace(
        &self,
        item_id: Uuid,
        tenant_id: Uuid,
        execution_id: Uuid,
    ) -> Result<bool> {
        if let Some(reason) =
            trace_reference_reason_pool(&self.pool, tenant_id, execution_id).await?
        {
            self.mark_item_blocked(item_id, &reason).await?;
            return Ok(false);
        }
        let clickhouse = self
            .clickhouse
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ClickHouse is unavailable"))?;
        clickhouse.query("ALTER TABLE workflow_trace_events DELETE WHERE tenant_id=toUUID(?) AND execution_id=toUUID(?) SETTINGS mutations_sync=1")
            .bind(tenant_id.to_string()).bind(execution_id.to_string()).execute().await?;
        self.mark_item_deleted_without_metadata(item_id).await?;
        Ok(true)
    }

    async fn delete_application_message(
        &self,
        item_id: Uuid,
        tenant_id: Uuid,
        message_id: Uuid,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let status: Option<String> = sqlx::query_scalar("SELECT s.status FROM application_messages m JOIN application_sessions s ON s.id=m.session_id AND s.tenant_id=m.tenant_id WHERE m.tenant_id=? AND m.id=? FOR UPDATE")
            .bind(tenant_id).bind(message_id).fetch_optional(&mut *tx).await?;
        let Some(status) = status else {
            sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
                .bind(item_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(true);
        };
        if status != "closed" {
            tx.rollback().await?;
            self.mark_item_blocked(item_id, "active_session").await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM artifact_references WHERE tenant_id=? AND owner_type='application_message' AND owner_id=?")
            .bind(tenant_id).bind(message_id.to_string()).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM application_message_parts WHERE tenant_id=? AND message_id=?")
            .bind(tenant_id)
            .bind(message_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM application_messages WHERE tenant_id=? AND id=?")
            .bind(tenant_id)
            .bind(message_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn delete_evaluation_report(
        &self,
        item_id: Uuid,
        tenant_id: Uuid,
        evaluation_id: Uuid,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM evaluation_runs WHERE tenant_id=? AND id=? FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(evaluation_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(status) = status else {
            sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
                .bind(item_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(true);
        };
        let referenced: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM evaluation_comparisons WHERE tenant_id=? AND (baseline_run_id=? OR candidate_run_id=?))")
            .bind(tenant_id).bind(evaluation_id).bind(evaluation_id).fetch_one(&mut *tx).await?;
        if referenced || !matches!(status.as_str(), "completed" | "failed" | "cancelled") {
            tx.rollback().await?;
            self.mark_item_blocked(
                item_id,
                if referenced {
                    "evaluation_comparison"
                } else {
                    "active_evaluation"
                },
            )
            .await?;
            return Ok(false);
        }
        sqlx::query("DELETE rr FROM evaluation_rule_results rr JOIN evaluation_run_cases c ON c.id=rr.evaluation_run_case_id AND c.tenant_id=rr.tenant_id WHERE rr.tenant_id=? AND c.evaluation_run_id=?").bind(tenant_id).bind(evaluation_id).execute(&mut *tx).await?;
        sqlx::query(
            "DELETE FROM evaluation_case_results WHERE tenant_id=? AND evaluation_run_id=?",
        )
        .bind(tenant_id)
        .bind(evaluation_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM evaluation_metrics WHERE tenant_id=? AND evaluation_run_id=?")
            .bind(tenant_id)
            .bind(evaluation_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM evaluation_run_cases WHERE tenant_id=? AND evaluation_run_id=?")
            .bind(tenant_id)
            .bind(evaluation_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM evaluation_runs WHERE tenant_id=? AND id=?")
            .bind(tenant_id)
            .bind(evaluation_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn finish_or_retry(&self, run: &RetentionRun, made_progress: bool) -> Result<()> {
        let exhausted: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM retention_items WHERE tenant_id=? AND retention_run_id=? AND status='failed' AND attempt_count>=?",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(MAX_ATTEMPTS)
        .fetch_one(&self.pool)
        .await?;
        if exhausted > 0 {
            return self
                .fail_run(run, "one or more retention items exhausted retries")
                .await;
        }
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM retention_items WHERE tenant_id=? AND retention_run_id=? AND status IN ('candidate','failed')",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .fetch_one(&self.pool)
        .await?;
        if remaining == 0 {
            self.complete_run(run).await
        } else if made_progress {
            self.continue_run(run).await
        } else {
            self.retry_or_fail_run(run, "some retention items remain pending")
                .await
        }
    }

    async fn continue_run(&self, run: &RetentionRun) -> Result<()> {
        sqlx::query(
            "UPDATE retention_runs SET status='queued',attempt_count=0,
             available_at=CURRENT_TIMESTAMP(6),error_message=NULL,
             locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?",
        )
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(self.owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_counts(&self, run: &RetentionRun, count: i64) -> Result<()> {
        sqlx::query("UPDATE retention_runs SET candidate_count=? WHERE tenant_id=? AND id=?")
            .bind(count)
            .bind(run.tenant_id)
            .bind(run.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn complete_run(&self, run: &RetentionRun) -> Result<()> {
        sqlx::query(
            "UPDATE retention_runs SET status='completed',completed_at=CURRENT_TIMESTAMP(6),
             locked_by=NULL,locked_until=NULL,
             deleted_count=(SELECT COUNT(*) FROM retention_items WHERE retention_run_id=? AND status='deleted')
             WHERE tenant_id=? AND id=? AND locked_by=?",
        )
        .bind(run.id)
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(self.owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn retry_or_fail_run(&self, run: &RetentionRun, message: &str) -> Result<()> {
        sqlx::query(
            "UPDATE retention_runs SET status=IF(attempt_count>=5,'failed','queued'),
             available_at=DATE_ADD(CURRENT_TIMESTAMP(6),INTERVAL 5 SECOND),error_message=?,
             locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?",
        )
        .bind(message)
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(self.owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fail_run(&self, run: &RetentionRun, message: &str) -> Result<()> {
        sqlx::query(
            "UPDATE retention_runs SET status='failed',error_message=?,completed_at=CURRENT_TIMESTAMP(6),
             locked_by=NULL,locked_until=NULL WHERE tenant_id=? AND id=? AND locked_by=?",
        )
        .bind(message)
        .bind(run.tenant_id)
        .bind(run.id)
        .bind(self.owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn bump_item_attempt(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE retention_items SET attempt_count=attempt_count+1 WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_item_blocked(&self, id: Uuid, reason: &str) -> Result<()> {
        sqlx::query("UPDATE retention_items SET status='blocked',reason=? WHERE id=?")
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_item_failed(&self, id: Uuid, reason: &str) -> Result<()> {
        sqlx::query("UPDATE retention_items SET status='failed',reason=? WHERE id=?")
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_item_deleted(
        &self,
        item_id: Uuid,
        tenant_id: Uuid,
        artifact_id: Uuid,
        size_bytes: u64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE artifacts SET deleted_at=COALESCE(deleted_at,CURRENT_TIMESTAMP(6)) WHERE tenant_id=? AND id=?")
            .bind(tenant_id)
            .bind(artifact_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        crate::quota::record_artifact_deletion(&mut tx, tenant_id, artifact_id, size_bytes).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn mark_item_deleted_without_metadata(&self, item_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE retention_items SET status='deleted',reason=NULL WHERE id=?")
            .bind(item_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

async fn insert_item(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    run: &RetentionRun,
    data_type: &str,
    target_id: Uuid,
    reason: Option<String>,
) -> Result<()> {
    let status = if reason.is_some() {
        "blocked"
    } else {
        "candidate"
    };
    sqlx::query("INSERT IGNORE INTO retention_items(id,tenant_id,retention_run_id,data_type,target_id,status,reason) VALUES(?,?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(run.tenant_id).bind(run.id).bind(data_type)
        .bind(target_id.to_string()).bind(status).bind(reason).execute(&mut **tx).await?;
    Ok(())
}

async fn reference_reason_pool(
    pool: &MySqlPool,
    tenant_id: Uuid,
    artifact_id: Uuid,
) -> Result<Option<String>> {
    let mut tx = pool.begin().await?;
    let reason = reference_reason(&mut tx, tenant_id, artifact_id).await?;
    tx.rollback().await?;
    Ok(reason)
}

async fn trace_reference_reason_pool(
    pool: &MySqlPool,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> Result<Option<String>> {
    let mut tx = pool.begin().await?;
    let reason = trace_reference_reason(&mut tx, tenant_id, execution_id).await?;
    tx.rollback().await?;
    Ok(reason)
}

async fn trace_reference_reason(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
) -> Result<Option<String>> {
    let row = sqlx::query("SELECT CASE
        WHEN EXISTS(SELECT 1 FROM checkpoints WHERE tenant_id=? AND execution_id=?) THEN 'checkpoint'
        WHEN EXISTS(SELECT 1 FROM evaluation_case_results WHERE tenant_id=? AND execution_id=?) THEN 'evaluation'
        WHEN EXISTS(SELECT 1 FROM evaluation_run_cases WHERE tenant_id=? AND target_execution_id=?) THEN 'evaluation'
        WHEN EXISTS(SELECT 1 FROM evaluation_rule_results WHERE tenant_id=? AND evaluator_execution_id=?) THEN 'evaluation'
        WHEN EXISTS(SELECT 1 FROM application_invocations WHERE tenant_id=? AND execution_id=?) THEN 'application_session'
        WHEN EXISTS(SELECT 1 FROM workflow_executions WHERE tenant_id=? AND (parent_execution_id=? OR caller_execution_id=?)) THEN 'child_execution'
        ELSE NULL END reason")
        .bind(tenant_id).bind(execution_id)
        .bind(tenant_id).bind(execution_id)
        .bind(tenant_id).bind(execution_id)
        .bind(tenant_id).bind(execution_id)
        .bind(tenant_id).bind(execution_id)
        .bind(tenant_id).bind(execution_id).bind(execution_id)
        .fetch_one(&mut **tx).await?;
    Ok(row.try_get("reason")?)
}

pub(crate) async fn reference_reason<'a>(
    tx: &mut sqlx::Transaction<'a, sqlx::MySql>,
    tenant_id: Uuid,
    artifact_id: Uuid,
) -> Result<Option<String>> {
    let row = sqlx::query(
        "SELECT CASE
          WHEN EXISTS (SELECT 1 FROM artifact_references r WHERE r.tenant_id=? AND r.artifact_id=? AND (r.retention_until IS NULL OR r.retention_until>CURRENT_TIMESTAMP(6))) THEN 'registered_reference'
          WHEN EXISTS (SELECT 1 FROM checkpoint_artifacts r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'checkpoint'
          WHEN EXISTS (SELECT 1 FROM workflow_debug_overlays r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'debug_overlay'
          WHEN EXISTS (SELECT 1 FROM application_message_parts r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'application_message'
          WHEN EXISTS (SELECT 1 FROM skill_workspace_entries r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_workspace'
          WHEN EXISTS (SELECT 1 FROM skill_file_revisions r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_revision'
          WHEN EXISTS (SELECT 1 FROM skill_version_files r WHERE r.tenant_id=? AND r.artifact_id=?) THEN 'skill_version'
          WHEN EXISTS (SELECT 1 FROM node_attempts r WHERE r.tenant_id=? AND r.log_artifact_id=?) THEN 'node_log'
          WHEN EXISTS (SELECT 1 FROM execution_edge_deliveries r WHERE r.tenant_id=? AND r.payload_artifact_id=?) THEN 'edge_payload'
          WHEN EXISTS (SELECT 1 FROM checkpoints r WHERE r.tenant_id=? AND r.payload_artifact_id=?) THEN 'checkpoint_payload'
          WHEN EXISTS (SELECT 1 FROM workflow_executions r WHERE r.tenant_id=? AND r.result_artifact_id=?) THEN 'execution_result'
          WHEN EXISTS (SELECT 1 FROM agent_runs r WHERE r.tenant_id=? AND r.state_artifact_id=?) THEN 'agent_state'
          WHEN EXISTS (SELECT 1 FROM agent_iterations r WHERE r.tenant_id=? AND r.state_artifact_id=?) THEN 'agent_iteration'
          WHEN EXISTS (SELECT 1 FROM runtime_calls r WHERE r.tenant_id=? AND r.response_artifact_id=?) THEN 'runtime_call'
          ELSE NULL END AS reason",
    )
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .bind(tenant_id).bind(artifact_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.try_get("reason")?)
}

pub async fn run_retention_loop(
    pool: MySqlPool,
    objects: Option<Arc<dyn ObjectStore>>,
    clickhouse: Option<clickhouse::Client>,
) {
    let processor = RetentionProcessor::new(pool, objects, clickhouse);
    loop {
        match processor.process_batch().await {
            Ok(0) => tokio::time::sleep(Duration::from_secs(2)).await,
            Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(error) => {
                tracing::error!(%error, "retention processor failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
