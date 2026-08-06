use agentx_infrastructure::{
    config::{MySqlSettings, MySqlTlsMode, RedisSettings},
    mysql,
    quota::{
        ARTIFACT_BYTES, QuotaAdmission, QuotaReservation, SUPPORTED_DIMENSIONS, reap_expired,
        release_scope_with_admission, reserve_with_admission,
    },
};
use rust_decimal::Decimal;
use secrecy::SecretString;
use sqlx::{MySqlPool, Row};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

#[tokio::test]
async fn quota_dimensions_are_authoritative_isolated_and_reconcilable() {
    let mysql_container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("MySQL container should start");
    let redis_container = GenericImage::new("redis", "7.4-alpine")
        .with_exposed_port(6379.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
        .start()
        .await
        .expect("Redis container should start");
    let mysql_port = mysql_container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("mapped MySQL port");
    let redis_port = redis_container
        .get_host_port_ipv4(6379.tcp())
        .await
        .expect("mapped Redis port");
    let pool = connect_with_retry(&MySqlSettings {
        host: "127.0.0.1".into(),
        port: mysql_port,
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("agentx-test-password".to_owned()),
        max_connections: 5,
        tls_mode: MySqlTlsMode::Disabled,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    })
    .await;
    mysql::run_migrations(&pool).await.expect("run migrations");
    let admission = QuotaAdmission::new(RedisSettings {
        url: SecretString::from(format!("redis://127.0.0.1:{redis_port}/")),
        password: None,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    });

    let first_tenant = Uuid::now_v7();
    let first_user = Uuid::now_v7();
    let second_tenant = Uuid::now_v7();
    let second_user = Uuid::now_v7();
    seed_policies(&pool, first_tenant, first_user).await;
    seed_policies(&pool, second_tenant, second_user).await;

    for (tenant, scope) in [(first_tenant, "tenant-a"), (second_tenant, "tenant-b")] {
        for dimension in SUPPORTED_DIMENSIONS {
            reserve_dimension(&pool, &admission, tenant, dimension, scope, 1)
                .await
                .unwrap_or_else(|error| panic!("reserve {tenant}/{dimension}: {error}"));
        }
    }
    let active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM quota_reservations WHERE status='active'")
            .fetch_one(&pool)
            .await
            .expect("active reservations");
    assert_eq!(active, (SUPPORTED_DIMENSIONS.len() * 2) as i64);

    let first_limit = reserve_dimension(
        &pool,
        &admission,
        first_tenant,
        "execution_concurrency",
        "tenant-a-over-limit",
        1000,
    )
    .await
    .expect_err("first tenant must hit its own hard limit");
    assert!(first_limit.to_string().contains("QUOTA_EXCEEDED"));
    reserve_dimension(
        &pool,
        &admission,
        second_tenant,
        "execution_concurrency",
        "tenant-b-extra",
        999,
    )
    .await
    .expect("first tenant exhaustion must not affect second tenant");

    sqlx::query("INSERT INTO quota_usage_ledger(id,tenant_id,dimension_key,scope_type,scope_id,amount,idempotency_key) VALUES(?,?,'tokens','fixture','used',900,'tokens-used')")
        .bind(Uuid::now_v7())
        .bind(first_tenant)
        .execute(&pool)
        .await
        .expect("token usage");
    let token_limit = reserve_dimension(
        &pool,
        &admission,
        first_tenant,
        "tokens",
        "tokens-over-limit",
        100,
    )
    .await
    .expect_err("period usage and active reservation must share the limit");
    assert!(token_limit.to_string().contains("QUOTA_EXCEEDED"));

    sqlx::query("INSERT INTO quota_usage_ledger(id,tenant_id,dimension_key,scope_type,scope_id,amount,idempotency_key) VALUES(?,?,'artifact_bytes','artifact','created',900,'artifact-created'),(?,?,'artifact_bytes','artifact','deleted',-500,'artifact-deleted')")
        .bind(Uuid::now_v7())
        .bind(first_tenant)
        .bind(Uuid::now_v7())
        .bind(first_tenant)
        .execute(&pool)
        .await
        .expect("artifact usage delta");
    reserve_dimension(
        &pool,
        &admission,
        first_tenant,
        ARTIFACT_BYTES,
        "artifact-after-delete",
        500,
    )
    .await
    .expect("artifact deletion ledger must return capacity");

    let mut tx = pool.begin().await.expect("release transaction");
    let released = release_scope_with_admission(
        &mut tx,
        first_tenant,
        "fixture",
        "tenant-a",
        Some(&admission),
    )
    .await
    .expect("release all first tenant dimensions");
    tx.commit().await.expect("commit release");
    assert_eq!(released, SUPPORTED_DIMENSIONS.len() as u64);
    reserve_dimension(
        &pool,
        &admission,
        first_tenant,
        "node_concurrency",
        "crashed-attempt",
        1,
    )
    .await
    .expect("crashed attempt reservation");
    sqlx::query("UPDATE quota_reservations SET expires_at=DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 1 SECOND) WHERE tenant_id=? AND scope_type='fixture' AND scope_id='crashed-attempt'")
        .bind(first_tenant)
        .execute(&pool)
        .await
        .expect("expire crashed attempt reservation");
    assert_eq!(reap_expired(&pool).await.expect("reap crashed attempt"), 1);
    let rebuilt = admission
        .rebuild_from_mysql(&pool)
        .await
        .expect("rebuild Redis counters from MySQL");
    let authoritative_active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quota_reservations WHERE status='active' AND expires_at>CURRENT_TIMESTAMP(6)")
        .fetch_one(&pool)
        .await
        .expect("authoritative active reservations");
    assert_eq!(rebuilt, authoritative_active as u64);

    let drift: i64 = sqlx::query("SELECT COUNT(*) count FROM quota_reservations WHERE tenant_id=? AND scope_type='fixture' AND scope_id='tenant-a' AND status='active'")
        .bind(first_tenant)
        .fetch_one(&pool)
        .await
        .expect("released reservation drift")
        .try_get("count")
        .expect("drift count");
    assert_eq!(drift, 0);
}

async fn reserve_dimension(
    pool: &MySqlPool,
    admission: &QuotaAdmission,
    tenant_id: Uuid,
    dimension: &str,
    scope_id: &str,
    amount: u64,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    reserve_with_admission(
        &mut tx,
        &QuotaReservation {
            tenant_id,
            dimension,
            scope_type: "fixture",
            scope_id,
            amount: Decimal::from(amount),
            ttl_seconds: 300,
            fail_closed: true,
        },
        Some(admission),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn seed_policies(pool: &MySqlPool, tenant: Uuid, user: Uuid) {
    sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,? ,?)")
        .bind(tenant)
        .bind(format!("Quota {tenant}"))
        .bind(format!("quota {tenant}"))
        .execute(pool)
        .await
        .expect("quota tenant");
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,?,?,?)")
        .bind(user)
        .bind(tenant)
        .bind(format!("quota-{tenant}"))
        .bind(format!("quota-{tenant}"))
        .bind("Quota User")
        .execute(pool)
        .await
        .expect("quota user");
    for dimension in SUPPORTED_DIMENSIONS {
        let period =
            matches!(*dimension, "tokens" | "cost_micros" | "agent_iterations").then_some(3600_u64);
        sqlx::query("INSERT INTO quota_policies(tenant_id,dimension_key,hard_limit,period_seconds,updated_by) VALUES(?,?,1000,?,?)")
            .bind(tenant)
            .bind(dimension)
            .bind(period)
            .bind(user)
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("quota policy {dimension}: {error}"));
    }
}

async fn connect_with_retry(settings: &MySqlSettings) -> MySqlPool {
    let mut last_error = None;
    for _ in 0..60 {
        match mysql::connect(settings).await {
            Ok(pool) => return pool,
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    panic!("connect MySQL: {:#}", last_error.expect("connection error"));
}
