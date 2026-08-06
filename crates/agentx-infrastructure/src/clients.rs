use std::{io::BufReader, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clickhouse::Client as ClickHouseClient;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client as HyperClient, rt::TokioExecutor};
use object_store::{Certificate, ClientOptions, ObjectStore, aws::AmazonS3Builder};
use redis::{
    IntoConnectionInfo,
    aio::{ConnectionManager, ConnectionManagerConfig},
};
use secrecy::ExposeSecret;

use crate::config::{ClickHouseSettings, ObjectStorageSettings, RedisSettings};

pub async fn connect_redis(settings: &RedisSettings) -> Result<ConnectionManager> {
    let mut connection_info = settings
        .url
        .expose_secret()
        .into_connection_info()
        .context("invalid Redis URL")?;
    if let Some(password) = &settings.password {
        connection_info.redis.password = Some(password.expose_secret().to_owned());
    }
    let tls_configured = settings.tls_ca_path.is_some()
        || settings.tls_client_cert_path.is_some()
        || settings.tls_client_key_path.is_some();
    let client = if tls_configured {
        let root_cert = read_optional(&settings.tls_ca_path, "Redis CA")?;
        let client_tls = match (
            &settings.tls_client_cert_path,
            &settings.tls_client_key_path,
        ) {
            (Some(cert), Some(key)) => Some(redis::ClientTlsConfig {
                client_cert: std::fs::read(cert)
                    .context("failed to read Redis client certificate")?,
                client_key: std::fs::read(key).context("failed to read Redis client key")?,
            }),
            (None, None) => None,
            _ => anyhow::bail!("Redis client certificate and key must be configured together"),
        };
        redis::Client::build_with_tls(
            connection_info,
            redis::TlsCertificates {
                client_tls,
                root_cert,
            },
        )
        .context("invalid Redis TLS configuration")?
    } else {
        redis::Client::open(connection_info).context("invalid Redis configuration")?
    };
    let manager_config = ConnectionManagerConfig::new()
        .set_number_of_retries(2)
        .set_max_delay(500)
        .set_connection_timeout(Duration::from_secs(2))
        .set_response_timeout(Duration::from_secs(5));
    ConnectionManager::new_with_config(client, manager_config)
        .await
        .context("failed to connect to Redis")
}

pub fn clickhouse(settings: &ClickHouseSettings) -> Result<ClickHouseClient> {
    let client = if let Some(path) = &settings.tls_ca_path {
        let mut roots = rustls::RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        let (valid_native, _) = roots.add_parsable_certificates(native.certs);
        anyhow::ensure!(
            valid_native > 0,
            "no valid system CA certificates were loaded"
        );

        let pem = std::fs::File::open(path).context("failed to read ClickHouse CA certificate")?;
        let certificates = rustls_pemfile::certs(&mut BufReader::new(pem))
            .collect::<std::io::Result<Vec<_>>>()
            .context("invalid ClickHouse CA PEM")?;
        anyhow::ensure!(
            !certificates.is_empty(),
            "ClickHouse CA PEM contains no certificates"
        );
        let (valid_custom, _) = roots.add_parsable_certificates(certificates);
        anyhow::ensure!(
            valid_custom > 0,
            "ClickHouse CA PEM contains no valid certificates"
        );

        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();
        let http = HyperClient::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_millis(2_500))
            .pool_max_idle_per_host(4)
            .build(connector);
        ClickHouseClient::with_http_client(http)
    } else {
        ClickHouseClient::default()
    };
    Ok(client
        .with_url(&settings.url)
        .with_database(&settings.database)
        .with_user(&settings.username)
        .with_password(settings.password.expose_secret()))
}

pub fn object_store(settings: &ObjectStorageSettings) -> Result<Arc<dyn ObjectStore>> {
    let mut client_options = ClientOptions::new().with_allow_http(settings.allow_http);
    if let Some(path) = &settings.tls_ca_path {
        let pem = std::fs::read(path).context("failed to read S3 CA certificate")?;
        let certificates = Certificate::from_pem_bundle(&pem).context("invalid S3 CA bundle")?;
        anyhow::ensure!(
            !certificates.is_empty(),
            "S3 CA bundle contains no certificates"
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
    let store = builder
        .build()
        .context("failed to build S3-compatible object store")?;
    Ok(Arc::new(store))
}

fn read_optional(path: &Option<std::path::PathBuf>, description: &str) -> Result<Option<Vec<u8>>> {
    path.as_ref()
        .map(|path| {
            std::fs::read(path).with_context(|| format!("failed to read {description} certificate"))
        })
        .transpose()
}
