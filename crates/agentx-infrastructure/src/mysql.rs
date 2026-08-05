use anyhow::{Context, Result};
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
    sqlx::migrate!("../../migrations/mysql")
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
