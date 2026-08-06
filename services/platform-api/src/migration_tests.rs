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

#[test]
fn migration_cli_accepts_only_a_positive_through_bound() {
    assert_eq!(
        super::migration_command::parse_migration_upper_bound(&[]).unwrap(),
        None
    );
    assert_eq!(
        super::migration_command::parse_migration_upper_bound(&["--through".into(), "16".into()])
            .unwrap(),
        Some(16)
    );
    assert!(super::migration_command::parse_migration_upper_bound(&["--through".into()]).is_err());
    assert!(
        super::migration_command::parse_migration_upper_bound(&["--through".into(), "0".into()])
            .is_err()
    );
    assert!(super::migration_command::parse_migration_upper_bound(&["--unknown".into()]).is_err());
}

async fn start_mysql() -> (
    testcontainers::ContainerAsync<GenericImage>,
    sqlx::MySqlPool,
) {
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
    for _ in 0..30 {
        if let Ok(pool) = mysql::connect(&settings).await {
            return (container, pool);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("connect to test MySQL");
}

#[tokio::test]
async fn m7_expand_and_contract_migrations_are_staged_and_guarded() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = start_mysql().await;

    mysql::run_migrations_through(&pool, 16)
        .await
        .expect("apply through M7 expand");
    let versions: String = sqlx::query_scalar(
        "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE version IN (16,17) AND success=TRUE",
    )
    .fetch_one(&pool)
    .await
    .expect("read staged migration versions");
    assert_eq!(versions, "16");
    let contract_table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='release_schema_contract'",
    )
    .fetch_one(&pool)
    .await
    .expect("read contract table state");
    assert_eq!(contract_table, 0);

    let mut connection = pool.acquire().await.expect("acquire test connection");
    sqlx::query("SET FOREIGN_KEY_CHECKS=0")
        .execute(&mut *connection)
        .await
        .expect("disable test foreign keys");
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,provider,secret_ref,provider_version,algorithm,key_id,nonce,ciphertext,created_by) VALUES(UNHEX(REPEAT('01',16)),UNHEX(REPEAT('02',16)),UNHEX(REPEAT('03',16)),1,'vault_kv_v2',NULL,NULL,NULL,NULL,NULL,NULL,UNHEX(REPEAT('04',16)))")
        .execute(&mut *connection)
        .await
        .expect("insert invalid expanded secret row");
    sqlx::query("SET FOREIGN_KEY_CHECKS=1")
        .execute(&mut *connection)
        .await
        .expect("restore test foreign keys");
    drop(connection);
    let error = super::migration_command::validate_m7_contract_inputs(&pool)
        .await
        .expect_err("invalid external Secret reference must block contract");
    assert!(error.to_string().contains("invalid credential secret rows"));

    sqlx::query("DELETE FROM credential_secret_versions")
        .execute(&pool)
        .await
        .expect("remove invalid test row");
    super::migration_command::validate_m7_contract_inputs(&pool)
        .await
        .expect("valid expanded data permits contract");
    mysql::run_migrations(&pool)
        .await
        .expect("apply M7 contract");
    mysql::run_migrations(&pool)
        .await
        .expect("M7 contract is idempotent");
    let versions: String = sqlx::query_scalar(
        "SELECT GROUP_CONCAT(version ORDER BY version) FROM _sqlx_migrations WHERE version IN (16,17) AND success=TRUE",
    )
    .fetch_one(&pool)
    .await
    .expect("read completed migration versions");
    assert_eq!(versions, "16,17");
    let marker: String = sqlx::query_scalar("SELECT CONCAT(schema_version,':',minimum_application_version) FROM release_schema_contract WHERE contract_name='m7-runtime-integration'")
        .fetch_one(&pool)
        .await
        .expect("read M7 contract marker");
    assert_eq!(marker, "17:0.1.0");
}

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
