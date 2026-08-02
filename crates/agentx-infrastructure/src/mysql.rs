use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use sqlx::{
    MySqlPool,
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
};

use crate::config::MySqlSettings;

pub async fn connect(settings: &MySqlSettings) -> Result<MySqlPool> {
    let options = MySqlConnectOptions::new()
        .host(&settings.host)
        .port(settings.port)
        .database(&settings.database)
        .username(&settings.username)
        .password(settings.password.expose_secret());

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
