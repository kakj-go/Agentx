use anyhow::{Context, Result};
use object_store::{Certificate, ClientOptions, ObjectStore, aws::AmazonS3Builder};
use secrecy::ExposeSecret;
use sqlx::{
    MySqlPool,
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
};
use std::sync::Arc;

use crate::{ControlMySqlSettings, ControlObjectStorageSettings, MySqlTlsMode};

pub async fn connect_control_mysql(settings: &ControlMySqlSettings) -> Result<MySqlPool> {
    let mut options = MySqlConnectOptions::new()
        .host(&settings.host)
        .port(settings.port)
        .database(&settings.database)
        .username(&settings.username)
        .password(settings.password.expose_secret())
        .ssl_mode(match settings.tls_mode {
            MySqlTlsMode::Disabled => MySqlSslMode::Disabled,
            MySqlTlsMode::Preferred => MySqlSslMode::Preferred,
            MySqlTlsMode::Required => MySqlSslMode::Required,
            MySqlTlsMode::VerifyCa => MySqlSslMode::VerifyCa,
            MySqlTlsMode::VerifyIdentity => MySqlSslMode::VerifyIdentity,
        });
    if let Some(path) = &settings.tls_ca_path {
        options = options.ssl_ca(path);
    }
    if let Some(path) = &settings.tls_client_cert_path {
        options = options.ssl_client_cert(path);
    }
    if let Some(path) = &settings.tls_client_key_path {
        options = options.ssl_client_key(path);
    }
    MySqlPoolOptions::new()
        .max_connections(settings.max_connections)
        .connect_with(options)
        .await
        .context("failed to connect to Control MySQL")
}

pub async fn ping_control_mysql(pool: &MySqlPool) -> Result<()> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .context("Control MySQL health check failed")?;
    Ok(())
}

pub async fn migrate_control_mysql(pool: &MySqlPool) -> Result<()> {
    reject_populated_v2_03_control(pool).await?;
    reject_populated_v2_04_control(pool).await?;
    sqlx::migrate!("../../migrations/control")
        .run(pool)
        .await
        .context("failed to run Control MySQL migrations")
}

async fn reject_populated_v2_04_control(pool: &MySqlPool) -> Result<()> {
    let migration_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !migration_table_exists {
        return Ok(());
    }
    let v2_05_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=5 AND success=TRUE)",
    )
    .fetch_one(pool)
    .await?;
    let v2_04_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=4 AND success=TRUE)",
    )
    .fetch_one(pool)
    .await?;
    if v2_05_applied || !v2_04_applied {
        return Ok(());
    }
    let business_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM applications) + (SELECT COUNT(*) FROM workflow_versions) + (SELECT COUNT(*) FROM application_deployments) + (SELECT COUNT(*) FROM execution_spec_bundles) + (SELECT COUNT(*) FROM runtime_work_package_publications)",
    )
    .fetch_one(pool)
    .await?;
    anyhow::ensure!(
        business_rows == 0,
        "V2-05 refuses to rewrite a populated V2-04 Control domain; rerun deployment with explicit -RecreateV2Data"
    );
    Ok(())
}

async fn reject_populated_v2_03_control(pool: &MySqlPool) -> Result<()> {
    let migration_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !migration_table_exists {
        return Ok(());
    }
    let v2_04_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=4 AND success=TRUE)",
    )
    .fetch_one(pool)
    .await?;
    let v2_03_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=3 AND success=TRUE)",
    )
    .fetch_one(pool)
    .await?;
    if v2_04_applied || !v2_03_applied {
        return Ok(());
    }
    let business_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM applications) + (SELECT COUNT(*) FROM workflow_versions) + (SELECT COUNT(*) FROM application_deployments) + (SELECT COUNT(*) FROM execution_spec_bundles)",
    )
    .fetch_one(pool)
    .await?;
    anyhow::ensure!(
        business_rows == 0,
        "V2-04 refuses to rewrite a populated V2-03 Control domain; rerun deployment with explicit -RecreateV2Data"
    );
    Ok(())
}

pub fn control_object_store(
    settings: &ControlObjectStorageSettings,
) -> Result<Arc<dyn ObjectStore>> {
    let mut client_options = ClientOptions::new().with_allow_http(settings.allow_http);
    if let Some(path) = &settings.tls_ca_path {
        let pem = std::fs::read(path).context("failed to read Control S3 CA certificate")?;
        let certificates =
            Certificate::from_pem_bundle(&pem).context("invalid Control S3 CA bundle")?;
        anyhow::ensure!(
            !certificates.is_empty(),
            "Control S3 CA bundle contains no certificates"
        );
        for certificate in certificates {
            client_options = client_options.with_root_certificate(certificate);
        }
    }
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&settings.bucket)
        .with_endpoint(&settings.endpoint)
        .with_region(&settings.region)
        .with_access_key_id(settings.access_key.expose_secret())
        .with_secret_access_key(settings.secret_key.expose_secret())
        .with_virtual_hosted_style_request(!settings.path_style)
        .with_client_options(client_options);
    if let Some(token) = &settings.session_token {
        builder = builder.with_token(token.expose_secret());
    }
    Ok(Arc::new(
        builder
            .build()
            .context("failed to build Control S3 object store")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{migrate_control_mysql, reject_populated_v2_04_control};
    use sqlx::{MySqlPool, mysql::MySqlPoolOptions};
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };

    #[tokio::test]
    async fn populated_v2_04_control_requires_an_explicit_recreate() {
        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx_control")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(3306.tcp()).await.unwrap();
        let pool = connect(port, "agentx_control").await;
        sqlx::raw_sql(
            "CREATE TABLE _sqlx_migrations(version BIGINT PRIMARY KEY,success BOOLEAN NOT NULL);\
             INSERT INTO _sqlx_migrations VALUES(4,TRUE);\
             CREATE TABLE applications(id BIGINT PRIMARY KEY);\
             CREATE TABLE workflow_versions(id BIGINT PRIMARY KEY);\
             CREATE TABLE application_deployments(id BIGINT PRIMARY KEY);\
             CREATE TABLE execution_spec_bundles(id BIGINT PRIMARY KEY);\
             CREATE TABLE runtime_work_package_publications(id BIGINT PRIMARY KEY);\
             INSERT INTO applications VALUES(1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let error = reject_populated_v2_04_control(&pool)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("-RecreateV2Data"), "{error}");
        sqlx::query("DELETE FROM applications")
            .execute(&pool)
            .await
            .unwrap();
        reject_populated_v2_04_control(&pool).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn empty_control_migrations_are_concurrent_and_replayable() {
        let container = mysql_container("agentx_control_migrations").await;
        let port = container.get_host_port_ipv4(3306.tcp()).await.unwrap();
        let pool = connect(port, "agentx_control_migrations").await;

        let (first, second) =
            tokio::join!(migrate_control_mysql(&pool), migrate_control_mysql(&pool));
        first.unwrap();
        second.unwrap();
        migrate_control_mysql(&pool).await.unwrap();

        let applied: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=6 AND success=TRUE",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(applied, 1);
        let status_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='runtime_projection_status')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(status_exists);
        let index_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.statistics WHERE table_schema=DATABASE() AND table_name='outbox' AND index_name='idx_v206_control_outbox_claim')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(index_exists);
    }

    async fn mysql_container(database: &str) -> testcontainers::ContainerAsync<GenericImage> {
        GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", database)
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .unwrap()
    }

    async fn connect(port: u16, database: &str) -> MySqlPool {
        let url = format!("mysql://agentx:agentx-test-password@127.0.0.1:{port}/{database}");
        for _ in 0..40 {
            if let Ok(pool) = MySqlPoolOptions::new().connect(&url).await {
                return pool;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("MySQL test container did not become ready")
    }
}
