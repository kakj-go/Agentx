use std::time::Duration;

use agentx_runtime_contracts::{
    ObservabilityAggregateRequestV1, ObservabilityDimensionV1, ObservabilityMetricV1,
};
use clickhouse::Client;
use serde_json::json;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    AggregateRow, TraceConflictRow, TraceRow, aggregate_query_sql, existing_trace_hash,
    timestamp_micros,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clickhouse_trace_queries_decode_aggregate_and_exclude_conflicts() {
    let container = GenericImage::new("clickhouse/clickhouse-server", "25.3")
        .with_exposed_port(8123.tcp())
        .with_wait_for(WaitFor::seconds(3))
        .with_env_var("CLICKHOUSE_DB", "agentx_observability")
        .with_env_var("CLICKHOUSE_USER", "observability_migrate")
        .with_env_var("CLICKHOUSE_PASSWORD", "agentxtestpassword")
        .with_env_var("CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT", "1")
        .start()
        .await
        .expect("ClickHouse container should start");
    let port = container.get_host_port_ipv4(8123.tcp()).await.unwrap();
    let admin = clickhouse_with_retry(port).await;
    execute_migration(
        &admin,
        include_str!("../../../migrations/observability/0001_initial.sql"),
    )
    .await;
    execute_migration(
        &admin,
        include_str!("../../../migrations/observability/0002_query_and_observability.sql"),
    )
    .await;

    let tenant_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let conflict_event_id = Uuid::now_v7();
    let clean_event_id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    assert_eq!(
        existing_trace_hash(&admin, clean_event_id).await.unwrap(),
        None,
        "an empty ClickHouse result must not be interpreted as an existing hash"
    );
    let mut insert = admin.insert("workflow_trace_events").unwrap();
    insert
        .write(&trace_row(
            conflict_event_id,
            tenant_id,
            execution_id,
            1,
            now,
            3,
            9,
        ))
        .await
        .unwrap();
    insert
        .write(&trace_row(
            clean_event_id,
            tenant_id,
            execution_id,
            2,
            now,
            5,
            11,
        ))
        .await
        .unwrap();
    insert.end().await.unwrap();
    assert_eq!(
        existing_trace_hash(&admin, clean_event_id).await.unwrap(),
        Some(hash('a'))
    );
    let mut conflicts = admin.insert("trace_ingest_conflicts").unwrap();
    conflicts
        .write(&TraceConflictRow {
            event_id: conflict_event_id,
            tenant_id,
            execution_id,
            existing_hash: hash('a'),
            conflicting_hash: hash('b'),
            stream_id: "1-0".into(),
        })
        .await
        .unwrap();
    conflicts.end().await.unwrap();

    let request = ObservabilityAggregateRequestV1 {
        api_version: 1,
        tenant_id,
        from: now - time::Duration::hours(1),
        to: now + time::Duration::hours(1),
        dimensions: vec![ObservabilityDimensionV1::Status],
        metrics: vec![
            ObservabilityMetricV1::Count,
            ObservabilityMetricV1::CostMicros,
            ObservabilityMetricV1::InputTokens,
        ],
        filters: json!({}),
        limit: 100,
    };
    let sql = aggregate_query_sql(&request).unwrap();
    let rows = admin
        .query(&sql)
        .bind(tenant_id)
        .bind(timestamp_micros(request.from).unwrap())
        .bind(timestamp_micros(request.to).unwrap())
        .bind(tenant_id)
        .bind(request.limit)
        .fetch_all::<AggregateRow>()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let dimensions: serde_json::Value = serde_json::from_str(&rows[0].dimensions).unwrap();
    let metrics: serde_json::Value = serde_json::from_str(&rows[0].metrics).unwrap();
    assert_eq!(dimensions["status"], "succeeded");
    assert_eq!(metrics["count"], "1");
    assert_eq!(metrics["costMicros"], "11");
    assert_eq!(metrics["inputTokens"], "5");

    let watermark = admin
        .query("SELECT max(execution_sequence) FROM workflow_trace_events FINAL WHERE tenant_id=? AND execution_id=? AND event_id NOT IN (SELECT event_id FROM trace_ingest_conflicts WHERE tenant_id=?)")
        .bind(tenant_id)
        .bind(execution_id)
        .bind(tenant_id)
        .fetch_one::<u64>()
        .await
        .unwrap();
    assert_eq!(watermark, 2);
}

fn trace_row(
    event_id: Uuid,
    tenant_id: Uuid,
    execution_id: Uuid,
    sequence: u64,
    occurred_at: OffsetDateTime,
    input_tokens: u64,
    cost_micros: u64,
) -> TraceRow {
    TraceRow {
        event_id,
        tenant_id,
        execution_id,
        execution_sequence: sequence,
        trace_id: Uuid::now_v7(),
        span_id: Uuid::now_v7(),
        parent_span_id: None,
        node_execution_id: None,
        attempt_id: None,
        runtime_call_id: None,
        workflow_id: None,
        application_id: None,
        resource_type: Some("model".into()),
        resource_id: None,
        resource_version: Some("v1".into()),
        event_type: "runtime_call.completed".into(),
        status: "succeeded".into(),
        error_code: None,
        duration_ms: Some(4),
        input_tokens: Some(input_tokens),
        output_tokens: Some(2),
        cost_micros,
        attributes_json: json!({"provider":"fixture"}).to_string(),
        content_ref: None,
        content_hash: hash('a'),
        occurred_at,
    }
}

fn hash(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

async fn clickhouse_with_retry(port: u16) -> Client {
    let client = Client::default()
        .with_url(format!("http://127.0.0.1:{port}"))
        .with_database("agentx_observability")
        .with_user("observability_migrate")
        .with_password("agentxtestpassword");
    let mut last_error = None;
    for _ in 0..60 {
        match client.query("SELECT 1").fetch_one::<u8>().await {
            Ok(1) => return client,
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("ClickHouse did not become ready: {last_error:?}");
}

async fn execute_migration(client: &Client, migration: &str) {
    let migration = migration
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for statement in migration
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        client.query(statement).execute().await.unwrap();
    }
}
