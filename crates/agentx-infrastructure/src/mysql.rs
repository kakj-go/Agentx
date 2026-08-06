use std::borrow::Cow;

use anyhow::{Context, Result, ensure};
use secrecy::ExposeSecret;
use sqlx::{
    MySqlPool,
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
};

use crate::config::{MySqlSettings, MySqlTlsMode};

pub async fn connect(settings: &MySqlSettings) -> Result<MySqlPool> {
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
        .context("failed to connect to MySQL")
}

pub async fn run_migrations(pool: &MySqlPool) -> Result<()> {
    run_migrations_internal(pool, None).await
}

pub async fn run_migrations_through(pool: &MySqlPool, through: i64) -> Result<()> {
    run_migrations_internal(pool, Some(through)).await
}

async fn run_migrations_internal(pool: &MySqlPool, through: Option<i64>) -> Result<()> {
    let all = sqlx::migrate!("../../migrations/mysql");
    let migrator = if let Some(through) = through {
        ensure!(through > 0, "migration upper bound must be positive");
        ensure!(
            all.migrations
                .iter()
                .any(|migration| migration.version == through),
            "migration {through} does not exist"
        );
        sqlx::migrate::Migrator {
            migrations: Cow::Owned(
                all.migrations
                    .iter()
                    .filter(|migration| migration.version <= through)
                    .cloned()
                    .collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        }
    } else {
        all
    };
    migrator
        .run(pool)
        .await
        .context("failed to run MySQL migrations")
}

pub async fn ping(pool: &MySqlPool) -> Result<()> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .context("MySQL health check failed")?;
    Ok(())
}
