use std::sync::Arc;

use anyhow::{Context, Result};
use clickhouse::Client as ClickHouseClient;
use object_store::{ObjectStore, aws::AmazonS3Builder};
use redis::aio::ConnectionManager;
use secrecy::ExposeSecret;

use crate::config::{ClickHouseSettings, ObjectStorageSettings, RedisSettings};

pub async fn connect_redis(settings: &RedisSettings) -> Result<ConnectionManager> {
    let client = redis::Client::open(settings.url.expose_secret()).context("invalid Redis URL")?;
    ConnectionManager::new(client)
        .await
        .context("failed to connect to Redis")
}

pub fn clickhouse(settings: &ClickHouseSettings) -> ClickHouseClient {
    ClickHouseClient::default()
        .with_url(&settings.url)
        .with_database(&settings.database)
        .with_user(&settings.username)
        .with_password(settings.password.expose_secret())
}

pub fn object_store(settings: &ObjectStorageSettings) -> Result<Arc<dyn ObjectStore>> {
    let store = AmazonS3Builder::new()
        .with_bucket_name(&settings.bucket)
        .with_endpoint(&settings.endpoint)
        .with_region(&settings.region)
        .with_access_key_id(settings.access_key.expose_secret())
        .with_secret_access_key(settings.secret_key.expose_secret())
        .with_allow_http(settings.allow_http)
        .with_virtual_hosted_style_request(false)
        .build()
        .context("failed to build S3-compatible object store")?;
    Ok(Arc::new(store))
}
