use std::{env, time::Duration};

use agentx_infrastructure::{config::InfrastructureSettings, mysql};
use anyhow::{Context, Result};
use redis::{
    AsyncCommands, FromRedisValue,
    streams::{
        StreamAutoClaimOptions, StreamAutoClaimReply, StreamId, StreamReadOptions, StreamReadReply,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use tracing::{error, warn};
use uuid::Uuid;

const TRACE_STREAM: &str = "agentx:trace:v1";
const TRACE_GROUP: &str = "trace-writers";

#[derive(Clone)]
struct WriterState {
    pool: MySqlPool,
    redis: agentx_infrastructure::config::RedisSettings,
    clickhouse: clickhouse::Client,
    consumer: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TracePayload {
    event_id: Uuid,
    tenant_id: Uuid,
    trace_id: Uuid,
    span_id: Uuid,
    parent_span_id: Option<Uuid>,
    execution_id: Uuid,
    workflow_id: Uuid,
    workflow_version_id: Uuid,
    node_execution_id: Option<Uuid>,
    node_id: Option<String>,
    event_type: String,
    status: String,
    event_time: OffsetDateTime,
    duration_ms: Option<u64>,
    #[serde(default)]
    run_index: u32,
    #[serde(default)]
    iteration_index: u32,
    model_name: Option<String>,
    provider_name: Option<String>,
    mcp_tool_name: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    #[serde(default)]
    cost_micros: u64,
    error_code: Option<String>,
    error_message: Option<String>,
    #[serde(default)]
    attributes: Value,
    content_ref: Option<Uuid>,
}

#[derive(clickhouse::Row, Serialize)]
struct TraceRow {
    #[serde(with = "clickhouse::serde::uuid")]
    event_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    trace_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    span_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    parent_span_id: Option<Uuid>,
    #[serde(with = "clickhouse::serde::uuid")]
    execution_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    workflow_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    workflow_version_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid::option")]
    node_execution_id: Option<Uuid>,
    node_id: Option<String>,
    event_type: String,
    status: String,
    #[serde(with = "clickhouse::serde::time::datetime64::micros")]
    event_time: OffsetDateTime,
    duration_ms: Option<u64>,
    run_index: u32,
    iteration_index: u32,
    model_name: Option<String>,
    provider_name: Option<String>,
    mcp_tool_name: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cost_micros: u64,
    error_code: Option<String>,
    error_message: Option<String>,
    attributes_json: String,
    #[serde(with = "clickhouse::serde::uuid::option")]
    content_ref: Option<Uuid>,
}

impl From<TracePayload> for TraceRow {
    fn from(v: TracePayload) -> Self {
        Self {
            event_id: v.event_id,
            tenant_id: v.tenant_id,
            trace_id: v.trace_id,
            span_id: v.span_id,
            parent_span_id: v.parent_span_id,
            execution_id: v.execution_id,
            workflow_id: v.workflow_id,
            workflow_version_id: v.workflow_version_id,
            node_execution_id: v.node_execution_id,
            node_id: v.node_id,
            event_type: v.event_type,
            status: v.status,
            event_time: v.event_time,
            duration_ms: v.duration_ms,
            run_index: v.run_index,
            iteration_index: v.iteration_index,
            model_name: v.model_name,
            provider_name: v.provider_name,
            mcp_tool_name: v.mcp_tool_name,
            input_tokens: v.input_tokens,
            output_tokens: v.output_tokens,
            cost_micros: v.cost_micros,
            error_code: v.error_code,
            error_message: v.error_message,
            attributes_json: v.attributes.to_string(),
            content_ref: v.content_ref,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings = InfrastructureSettings::from_env()?;
    let clickhouse = agentx_infrastructure::clients::clickhouse(&settings.clickhouse);
    if env::args().nth(1).as_deref() == Some("migrate-clickhouse") {
        run_clickhouse_migrations(&clickhouse).await?;
        return Ok(());
    }
    let pool = mysql::connect(&settings.mysql).await?;
    let consumer = format!(
        "{}-{}",
        env::var("HOSTNAME").unwrap_or_else(|_| "local".into()),
        Uuid::now_v7()
    );
    let state = WriterState {
        pool: pool.clone(),
        redis: settings.redis.clone(),
        clickhouse: clickhouse.clone(),
        consumer,
    };
    ensure_group(&state).await?;
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.register("redis", true).await;
    health.register("clickhouse", true).await;
    health.set_status("mysql", "ready").await;
    health.set_status("redis", "ready").await;
    health.set_status("clickhouse", "ready").await;
    tokio::spawn(relay_loop(state.clone()));
    tokio::spawn(consume_loop(state.clone()));
    tokio::spawn(heartbeat_loop(state));
    agentx_service_kit::serve("trace-writer", axum::Router::new(), health).await
}

async fn run_clickhouse_migrations(client: &clickhouse::Client) -> Result<()> {
    let migration = include_str!("../../../migrations/clickhouse/0001_workflow_trace_events.sql");
    for statement in migration
        .split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        client
            .query(statement)
            .execute()
            .await
            .with_context(|| format!("ClickHouse migration failed: {statement}"))?;
    }
    Ok(())
}
async fn ensure_group(state: &WriterState) -> Result<()> {
    let mut redis = agentx_infrastructure::clients::connect_redis(&state.redis).await?;
    let result: redis::RedisResult<redis::Value> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(TRACE_STREAM)
        .arg(TRACE_GROUP)
        .arg("0")
        .arg("MKSTREAM")
        .query_async(&mut redis)
        .await;
    if let Err(error) = result {
        if !error.to_string().contains("BUSYGROUP") {
            return Err(error.into());
        }
    }
    Ok(())
}
async fn relay_loop(state: WriterState) {
    loop {
        if let Err(error) = relay_batch(&state).await {
            error!(%error,"failed to relay trace outbox");
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
async fn relay_batch(state: &WriterState) -> Result<()> {
    let rows=sqlx::query("SELECT event_id,payload_json FROM trace_delivery_outbox WHERE status='pending' AND available_at<=CURRENT_TIMESTAMP(6) ORDER BY created_at LIMIT 100").fetch_all(&state.pool).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut redis = agentx_infrastructure::clients::connect_redis(&state.redis).await?;
    for row in rows {
        let event_id: Uuid = row.try_get("event_id")?;
        let payload: Value = row.try_get("payload_json")?;
        let stream_id: String = redis
            .xadd(
                TRACE_STREAM,
                "*",
                &[
                    ("eventId", event_id.to_string()),
                    ("payload", payload.to_string()),
                ],
            )
            .await?;
        sqlx::query("UPDATE trace_delivery_outbox SET status='streamed',streamed_at=CURRENT_TIMESTAMP(6),attempt_count=attempt_count+1,last_error=NULL WHERE event_id=? AND status='pending'").bind(event_id).execute(&state.pool).await?;
        sqlx::query("INSERT INTO trace_delivery_offsets(consumer_name,stream_id) VALUES('outbox-relay',?) ON DUPLICATE KEY UPDATE stream_id=VALUES(stream_id)").bind(stream_id).execute(&state.pool).await?;
    }
    Ok(())
}
async fn consume_loop(state: WriterState) {
    loop {
        if let Err(error) = consume_batch(&state).await {
            error!(%error,"failed to consume trace stream");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}
async fn consume_batch(state: &WriterState) -> Result<()> {
    let mut redis = agentx_infrastructure::clients::connect_redis(&state.redis).await?;
    let claimed: StreamAutoClaimReply = redis
        .xautoclaim_options(
            TRACE_STREAM,
            TRACE_GROUP,
            &state.consumer,
            5_000,
            "0-0",
            StreamAutoClaimOptions::default().count(100),
        )
        .await?;
    if !claimed.claimed.is_empty() {
        process_items(state, &mut redis, claimed.claimed).await?;
        return Ok(());
    }
    let options = StreamReadOptions::default()
        .group(TRACE_GROUP, &state.consumer)
        .count(100)
        .block(5000);
    let reply: StreamReadReply = redis
        .xread_options(&[TRACE_STREAM], &[">"], &options)
        .await?;
    for key in reply.keys {
        process_items(state, &mut redis, key.ids).await?;
    }
    Ok(())
}

async fn process_items(
    state: &WriterState,
    redis: &mut redis::aio::ConnectionManager,
    items: Vec<StreamId>,
) -> Result<()> {
    for item in items {
        let Some(value) = item.map.get("payload") else {
            warn!(stream_id=%item.id,"trace stream item has no payload");
            let _: u64 = redis.xack(TRACE_STREAM, TRACE_GROUP, &[&item.id]).await?;
            continue;
        };
        let payload_text = String::from_redis_value(value)?;
        let payload: TracePayload =
            serde_json::from_str(&payload_text).context("invalid trace payload")?;
        let event_id = payload.event_id;
        let mut insert = state.clickhouse.insert("workflow_trace_events")?;
        insert.write(&TraceRow::from(payload)).await?;
        insert.end().await?;
        sqlx::query("UPDATE trace_delivery_outbox SET status='delivered',delivered_at=CURRENT_TIMESTAMP(6),last_error=NULL WHERE event_id=? AND status='streamed'")
            .bind(event_id)
            .execute(&state.pool)
            .await?;
        let _: u64 = redis.xack(TRACE_STREAM, TRACE_GROUP, &[&item.id]).await?;
        let _: u64 = redis.xdel(TRACE_STREAM, &[&item.id]).await?;
    }
    Ok(())
}
async fn heartbeat_loop(state: WriterState) {
    loop {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM tenants")
            .fetch_all(&state.pool)
            .await
        {
            Ok(tenants) => {
                for tenant in tenants {
                    let _=sqlx::query("INSERT INTO runtime_service_heartbeats(tenant_id,service_type,instance_id,status,detail_json,heartbeat_at) VALUES(?,'trace_writer',?,'ready',JSON_OBJECT(),CURRENT_TIMESTAMP(6)) ON DUPLICATE KEY UPDATE status='ready',heartbeat_at=CURRENT_TIMESTAMP(6)").bind(tenant).bind(&state.consumer).execute(&state.pool).await;
                }
            }
            Err(error) => warn!(%error,"failed to update trace writer heartbeat"),
        };
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{TracePayload, TraceRow};
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;
    #[test]
    fn trace_payload_preserves_identity() {
        let id = Uuid::now_v7();
        let payload = TracePayload {
            event_id: id,
            tenant_id: id,
            trace_id: id,
            span_id: id,
            parent_span_id: None,
            execution_id: id,
            workflow_id: id,
            workflow_version_id: id,
            node_execution_id: None,
            node_id: None,
            event_type: "node.started".into(),
            status: "running".into(),
            event_time: OffsetDateTime::now_utc(),
            duration_ms: None,
            run_index: 0,
            iteration_index: 0,
            model_name: None,
            provider_name: None,
            mcp_tool_name: None,
            input_tokens: None,
            output_tokens: None,
            cost_micros: 0,
            error_code: None,
            error_message: None,
            attributes: json!({"safe":true}),
            content_ref: None,
        };
        let row = TraceRow::from(payload);
        assert_eq!(row.event_id, id);
        assert!(row.attributes_json.contains("safe"));
    }
}
