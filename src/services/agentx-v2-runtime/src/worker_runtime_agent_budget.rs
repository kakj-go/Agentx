use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use agentx_agent_core::{BudgetDecision, BudgetPort, ClockPort};
use serde_json::Value;

use super::{ClaimedWorkerAttempt, RuntimeWorker};

pub(super) struct WorkerClock;
impl ClockPort for WorkerClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|v| v.as_millis() as u64)
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub(super) struct BudgetCounters {
    pub(super) input_tokens: AtomicU64,
    pub(super) output_tokens: AtomicU64,
    pub(super) cost_micros: AtomicU64,
}

pub(super) struct WorkerBudget<'a> {
    worker: &'a RuntimeWorker,
    claim: &'a ClaimedWorkerAttempt,
    budget: Value,
    started: std::time::Instant,
    deadline: time::OffsetDateTime,
    counters: Arc<BudgetCounters>,
}
impl<'a> WorkerBudget<'a> {
    pub(super) fn new(
        worker: &'a RuntimeWorker,
        claim: &'a ClaimedWorkerAttempt,
        budget: Value,
        counters: Arc<BudgetCounters>,
        deadline: time::OffsetDateTime,
    ) -> Self {
        Self {
            worker,
            claim,
            budget,
            started: std::time::Instant::now(),
            deadline,
            counters,
        }
    }
}
impl BudgetPort for WorkerBudget<'_> {
    fn admit_turn(&mut self, turn: u32, projected_tokens: u64) -> BudgetDecision {
        let cancelled = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                sqlx::query_scalar::<_, String>(
                    "SELECT status FROM node_attempts WHERE tenant_id=? AND id=?",
                )
                .bind(self.claim.task.tenant_id)
                .bind(self.claim.task.attempt_id)
                .fetch_optional(&self.worker.pool)
                .await
                .ok()
                .flatten()
            })
        });
        // A completed attempt may re-enter this adapter only to reconcile
        // already-settled effects after a crash at the settlement/state
        // boundary. The ledger prevents the provider effect from running
        // again. Every other non-running state has no valid lease to advance.
        if cancelled
            .as_deref()
            .is_some_and(|status| status != "running" && status != "succeeded")
        {
            return BudgetDecision::Cancel;
        }
        if WorkerClock.now_millis()
            >= self.deadline.unix_timestamp_nanos().max(0) as u64 / 1_000_000
        {
            return BudgetDecision::Cancel;
        }
        if self.started.elapsed().as_millis() as u64
            > self
                .budget
                .get("maxDurationMs")
                .and_then(Value::as_u64)
                .unwrap_or(300_000)
        {
            return BudgetDecision::Exhausted;
        }
        let input_tokens = self.counters.input_tokens.load(Ordering::Relaxed);
        let output_tokens = self.counters.output_tokens.load(Ordering::Relaxed);
        let cost_micros = self.counters.cost_micros.load(Ordering::Relaxed);
        if turn
            > self
                .budget
                .get("maxIterations")
                .and_then(Value::as_u64)
                .unwrap_or(12) as u32
            || turn
                > self
                    .budget
                    .get("maxModelCalls")
                    .and_then(Value::as_u64)
                    .unwrap_or(12) as u32
            || projected_tokens
                .saturating_add(input_tokens)
                .saturating_add(output_tokens)
                > self
                    .budget
                    .get("maxTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(64_000)
            || output_tokens
                > self
                    .budget
                    .get("maxOutputTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(4_096)
            || cost_micros
                > self
                    .budget
                    .get("maxCostMicros")
                    .and_then(Value::as_u64)
                    .unwrap_or(1_000_000)
        {
            return BudgetDecision::Exhausted;
        }
        BudgetDecision::Continue
    }
}
