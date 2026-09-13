use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use object_store::{Certificate, ClientOptions, ObjectStore, aws::AmazonS3Builder};
use redis::aio::ConnectionManager;
use secrecy::ExposeSecret;

use crate::{RuntimeObjectStorageSettings, RuntimeRedisSettings};

pub async fn connect_runtime_redis(settings: &RuntimeRedisSettings) -> Result<ConnectionManager> {
    let client = runtime_redis_client(settings)?;
    tokio::time::timeout(Duration::from_secs(5), ConnectionManager::new(client))
        .await
        .context("timed out connecting to Runtime Redis")?
        .context("failed to connect to Runtime Redis")
}

pub fn runtime_redis_client(settings: &RuntimeRedisSettings) -> Result<redis::Client> {
    let url = if let Some(password) = &settings.password {
        let mut parsed = url::Url::parse(settings.url.expose_secret())
            .context("AGENTX_RUNTIME_REDIS_URL is invalid")?;
        parsed
            .set_password(Some(password.expose_secret()))
            .map_err(|()| anyhow::anyhow!("Runtime Redis URL cannot carry a password"))?;
        parsed.to_string()
    } else {
        settings.url.expose_secret().to_owned()
    };
    redis::Client::open(url).context("invalid Runtime Redis URL")
}

pub fn runtime_object_store(
    settings: &RuntimeObjectStorageSettings,
) -> Result<Arc<dyn ObjectStore>> {
    let mut client_options = ClientOptions::new().with_allow_http(settings.allow_http);
    if let Some(path) = &settings.tls_ca_path {
        let pem = std::fs::read(path).context("failed to read Runtime S3 CA certificate")?;
        let certificates =
            Certificate::from_pem_bundle(&pem).context("invalid Runtime S3 CA bundle")?;
        anyhow::ensure!(
            !certificates.is_empty(),
            "Runtime S3 CA bundle contains no certificates"
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
            .context("failed to build Runtime S3 object store")?,
    ))
}
