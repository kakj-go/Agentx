use agentx_infrastructure::{
    config::{MySqlSettings, MySqlTlsMode},
    mysql,
};
use secrecy::SecretString;
use std::{borrow::Cow, path::Path, time::Duration};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

#[tokio::test]
async fn mysql_migrations_upgrade_an_m1_schema_to_mcp_and_skill_workspace() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("MySQL container should start");
    let port = container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("mapped MySQL port");
    let settings = MySqlSettings {
        host: "127.0.0.1".to_owned(),
        port,
        database: "agentx".to_owned(),
        username: "agentx".to_owned(),
        password: SecretString::from("agentx-test-password".to_owned()),
        max_connections: 5,
        tls_mode: MySqlTlsMode::Disabled,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    };
    let mut pool = None;
    for _ in 0..30 {
        match mysql::connect(&settings).await {
            Ok(value) => {
                pool = Some(value);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let pool = pool.expect("connect to test MySQL");
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/mysql"
    ));
    let all = sqlx::migrate::Migrator::new(path)
        .await
        .expect("load migrations");
    let m1 = sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            all.migrations
                .iter()
                .filter(|migration| migration.version <= 3)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    m1.run(&pool).await.expect("apply M1 migrations");
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='mcp_servers'",
    )
    .fetch_one(&pool)
    .await
    .expect("query pre-upgrade schema");
    assert_eq!(before, 0);

    all.run(&pool).await.expect("append M2.1 migrations");
    let current_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('mcp_servers','mcp_tools','skill_workspace_entries')",
    )
    .fetch_one(&pool)
    .await
    .expect("query M2.1 schema");
    assert_eq!(current_tables, 3);
    let removed_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('tools','tool_versions')",
    )
    .fetch_one(&pool)
    .await
    .expect("query removed schema");
    assert_eq!(removed_tables, 0);
}
