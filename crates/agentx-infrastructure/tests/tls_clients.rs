use std::{
    future::Future,
    io::BufReader,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use agentx_infrastructure::{
    clients,
    config::{
        ClickHouseSettings, MySqlSettings, MySqlTlsMode, ObjectStorageSettings, RedisSettings,
    },
    mysql,
};
use bytes::Bytes;
use clickhouse::Compression;
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use object_store::path::Path;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair, SanType};
use secrecy::SecretString;
use tempfile::TempDir;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_rustls::{TlsAcceptor, rustls};

struct Certificates {
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    server: Vec<u8>,
    server_key: Vec<u8>,
    client: Vec<u8>,
    client_key: Vec<u8>,
}

struct CertificateFiles {
    _directory: TempDir,
    ca: std::path::PathBuf,
    wrong_ca: std::path::PathBuf,
    client: std::path::PathBuf,
    client_key: std::path::PathBuf,
}

struct HttpsFixture {
    address: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for HttpsFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[test]
fn system_ca_store_contains_valid_certificates() {
    install_crypto_provider();
    let native = rustls_native_certs::load_native_certs();
    assert!(
        !native.certs.is_empty(),
        "the operating system did not provide any CA certificates"
    );
    let mut roots = rustls::RootCertStore::empty();
    let (valid, _) = roots.add_parsable_certificates(native.certs);
    assert!(valid > 0, "the operating system CA store was not parseable");
}

#[tokio::test]
async fn clickhouse_and_s3_enforce_private_ca_auth_and_session_token() {
    install_crypto_provider();
    let certificates = certificates();
    let files = write_certificates(&certificates);

    let clickhouse = start_https_fixture(&certificates, |request| {
        let user = request
            .headers()
            .get("x-clickhouse-user")
            .and_then(|value| value.to_str().ok());
        let password = request
            .headers()
            .get("x-clickhouse-key")
            .and_then(|value| value.to_str().ok());
        if user != Some("agentx") || password != Some("clickhouse-secret") {
            return (StatusCode::UNAUTHORIZED, b"authentication failed".to_vec());
        }
        (StatusCode::OK, vec![1])
    })
    .await;

    let clickhouse_settings = ClickHouseSettings {
        url: format!("https://localhost:{}", clickhouse.address.port()),
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("clickhouse-secret".to_owned()),
        tls_ca_path: Some(files.ca.clone()),
    };
    let result = clients::clickhouse(&clickhouse_settings)
        .expect("build ClickHouse client")
        .with_compression(Compression::None)
        .query("SELECT 1")
        .fetch_one::<u8>()
        .await
        .expect("private CA ClickHouse request");
    assert_eq!(result, 1);

    let wrong_ca = ClickHouseSettings {
        tls_ca_path: Some(files.wrong_ca.clone()),
        ..clickhouse_settings.clone()
    };
    assert!(
        clients::clickhouse(&wrong_ca)
            .expect("build wrong-CA ClickHouse client")
            .with_compression(Compression::None)
            .query("SELECT 1")
            .fetch_one::<u8>()
            .await
            .is_err()
    );
    let wrong_auth = ClickHouseSettings {
        password: SecretString::from("wrong".to_owned()),
        ..clickhouse_settings
    };
    assert!(
        clients::clickhouse(&wrong_auth)
            .expect("build wrong-auth ClickHouse client")
            .with_compression(Compression::None)
            .query("SELECT 1")
            .fetch_one::<u8>()
            .await
            .is_err()
    );

    let s3 = start_https_fixture(&certificates, |request| {
        let authorization = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let session_token = request
            .headers()
            .get("x-amz-security-token")
            .and_then(|value| value.to_str().ok());
        if !authorization.contains("Credential=access-key/")
            || session_token != Some("session-token")
        {
            return (StatusCode::FORBIDDEN, b"invalid credentials".to_vec());
        }
        (StatusCode::OK, Vec::new())
    })
    .await;

    let s3_settings = ObjectStorageSettings {
        endpoint: format!("https://localhost:{}", s3.address.port()),
        bucket: "agentx-test".into(),
        region: "us-east-1".into(),
        access_key: SecretString::from("access-key".to_owned()),
        secret_key: SecretString::from("secret-key".to_owned()),
        session_token: Some(SecretString::from("session-token".to_owned())),
        allow_http: false,
        path_style: true,
        tls_ca_path: Some(files.ca.clone()),
    };
    let store = clients::object_store(&s3_settings).expect("build S3 client");
    let path = Path::from("deployment-doctor/private-ca.txt");
    store
        .put(&path, Bytes::from_static(b"private-ca").into())
        .await
        .expect("S3 private CA and session token request");
    store.delete(&path).await.expect("S3 delete request");

    let wrong_ca = ObjectStorageSettings {
        tls_ca_path: Some(files.wrong_ca.clone()),
        ..s3_settings.clone()
    };
    assert!(
        clients::object_store(&wrong_ca)
            .expect("build wrong-CA S3 client")
            .put(&path, Bytes::from_static(b"wrong-ca").into())
            .await
            .is_err()
    );
    let wrong_auth = ObjectStorageSettings {
        access_key: SecretString::from("wrong-access-key".to_owned()),
        ..s3_settings
    };
    assert!(
        clients::object_store(&wrong_auth)
            .expect("build wrong-auth S3 client")
            .put(&path, Bytes::from_static(b"wrong-auth").into())
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_and_redis_enforce_private_ca_mtls_and_authentication() {
    install_crypto_provider();
    let certificates = certificates();
    let files = write_certificates(&certificates);

    let mysql_container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stderr("port: 3306"))
        .with_env_var("MYSQL_DATABASE", "agentx")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "mysql-secret")
        .with_env_var("MYSQL_ROOT_PASSWORD", "mysql-root-secret")
        .with_copy_to("/certs/ca.pem", certificates.ca.clone())
        .with_copy_to("/certs/server.pem", certificates.server.clone())
        .with_copy_to("/certs/server-key.pem", certificates.server_key.clone())
        .with_cmd([
            "--ssl-ca=/certs/ca.pem",
            "--ssl-cert=/certs/server.pem",
            "--ssl-key=/certs/server-key.pem",
            "--require-secure-transport=ON",
        ])
        .start()
        .await
        .expect("start TLS MySQL");
    let mysql_port = mysql_container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("MySQL port");
    let mysql_settings = MySqlSettings {
        host: "127.0.0.1".into(),
        port: mysql_port,
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("mysql-secret".to_owned()),
        max_connections: 2,
        tls_mode: MySqlTlsMode::VerifyIdentity,
        tls_ca_path: Some(files.ca.clone()),
        tls_client_cert_path: None,
        tls_client_key_path: None,
    };
    let pool = match mysql::connect(&mysql_settings).await {
        Ok(pool) => pool,
        Err(error) => {
            let logs = mysql_container
                .stderr_to_vec()
                .await
                .unwrap_or_else(|_| b"<logs unavailable>".to_vec());
            panic!(
                "MySQL private CA connection failed: {error:#}\n{}",
                String::from_utf8_lossy(&logs)
            );
        }
    };
    mysql::ping(&pool).await.expect("MySQL TLS SELECT 1");
    drop(pool);
    assert!(
        mysql::connect(&MySqlSettings {
            tls_ca_path: Some(files.wrong_ca.clone()),
            ..mysql_settings.clone()
        })
        .await
        .is_err()
    );
    assert!(
        mysql::connect(&MySqlSettings {
            password: SecretString::from("wrong".to_owned()),
            ..mysql_settings.clone()
        })
        .await
        .is_err()
    );

    let root = mysql::connect(&MySqlSettings {
        username: "root".into(),
        password: SecretString::from("mysql-root-secret".to_owned()),
        ..mysql_settings.clone()
    })
    .await
    .expect("MySQL root TLS connection");
    sqlx::query("ALTER USER 'agentx'@'%' REQUIRE X509")
        .execute(&root)
        .await
        .expect("require MySQL client certificate");
    drop(root);
    assert!(mysql::connect(&mysql_settings).await.is_err());
    let mtls = MySqlSettings {
        tls_client_cert_path: Some(files.client.clone()),
        tls_client_key_path: Some(files.client_key.clone()),
        ..mysql_settings
    };
    mysql::ping(&mysql::connect(&mtls).await.expect("MySQL mTLS connection"))
        .await
        .expect("MySQL mTLS SELECT 1");

    let redis_container = GenericImage::new("redis", "7.4-alpine")
        .with_exposed_port(6379.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
        .with_copy_to("/certs/ca.pem", certificates.ca.clone())
        .with_copy_to("/certs/server.pem", certificates.server.clone())
        .with_copy_to("/certs/server-key.pem", certificates.server_key.clone())
        .with_cmd([
            "redis-server",
            "--port",
            "0",
            "--tls-port",
            "6379",
            "--tls-cert-file",
            "/certs/server.pem",
            "--tls-key-file",
            "/certs/server-key.pem",
            "--tls-ca-cert-file",
            "/certs/ca.pem",
            "--tls-auth-clients",
            "yes",
            "--requirepass",
            "redis-secret",
        ])
        .start()
        .await
        .expect("start TLS Redis");
    let redis_port = redis_container
        .get_host_port_ipv4(6379.tcp())
        .await
        .expect("Redis port");
    let redis_settings = RedisSettings {
        url: SecretString::from(format!("rediss://127.0.0.1:{redis_port}/")),
        password: Some(SecretString::from("redis-secret".to_owned())),
        tls_ca_path: Some(files.ca.clone()),
        tls_client_cert_path: Some(files.client.clone()),
        tls_client_key_path: Some(files.client_key.clone()),
    };
    let mut redis = clients::connect_redis(&redis_settings)
        .await
        .expect("Redis mTLS connection");
    let pong: String = redis::cmd("PING")
        .query_async(&mut redis)
        .await
        .expect("Redis mTLS PING");
    assert_eq!(pong, "PONG");
    assert!(
        connection_fails(clients::connect_redis(&RedisSettings {
            tls_ca_path: Some(files.wrong_ca.clone()),
            ..redis_settings.clone()
        }))
        .await
    );
    assert!(
        connection_fails(clients::connect_redis(&RedisSettings {
            password: Some(SecretString::from("wrong".to_owned())),
            ..redis_settings.clone()
        }))
        .await
    );
    assert!(
        connection_fails(clients::connect_redis(&RedisSettings {
            tls_client_cert_path: None,
            tls_client_key_path: None,
            ..redis_settings
        }))
        .await
    );
}

async fn connection_fails<T, E>(future: impl Future<Output = Result<T, E>>) -> bool {
    match tokio::time::timeout(Duration::from_secs(10), future).await {
        Ok(Ok(_)) => false,
        Ok(Err(_)) | Err(_) => true,
    }
}

async fn start_https_fixture(
    certificates: &Certificates,
    handler: impl Fn(&Request<Incoming>) -> (StatusCode, Vec<u8>) + Send + Sync + 'static,
) -> HttpsFixture {
    let certificate_chain =
        rustls_pemfile::certs(&mut BufReader::new(certificates.server.as_slice()))
            .collect::<Result<Vec<_>, _>>()
            .expect("parse server certificate");
    let private_key =
        rustls_pemfile::private_key(&mut BufReader::new(certificates.server_key.as_slice()))
            .expect("parse server key")
            .expect("server key");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificate_chain, private_key)
        .expect("TLS server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind TLS fixture");
    let address = listener.local_addr().expect("TLS fixture address");
    let handler = Arc::new(handler);
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(stream).await else {
                    return;
                };
                let service = service_fn(move |request| {
                    let (status, body) = handler(&request);
                    async move {
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(status)
                                .header("etag", "\"agentx-tls-fixture\"")
                                .body(Full::new(Bytes::from(body)))
                                .expect("fixture response"),
                        )
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    HttpsFixture { address, task }
}

fn certificates() -> Certificates {
    let (ca, ca_key) = certificate_authority("Agentx TLS Test CA");
    let (wrong_ca, _) = certificate_authority("Agentx Wrong TLS Test CA");

    let mut server_params =
        CertificateParams::new(vec!["localhost".into()]).expect("server certificate parameters");
    server_params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_key = KeyPair::generate().expect("server key");
    let server = server_params
        .signed_by(&server_key, &ca, &ca_key)
        .expect("server certificate");

    let mut client_params = CertificateParams::new(vec!["agentx-client".into()])
        .expect("client certificate parameters");
    client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client_key = KeyPair::generate().expect("client key");
    let client = client_params
        .signed_by(&client_key, &ca, &ca_key)
        .expect("client certificate");

    Certificates {
        ca: ca.pem().into_bytes(),
        wrong_ca: wrong_ca.pem().into_bytes(),
        server: server.pem().into_bytes(),
        server_key: server_key.serialize_pem().into_bytes(),
        client: client.pem().into_bytes(),
        client_key: client_key.serialize_pem().into_bytes(),
    }
}

fn certificate_authority(name: &str) -> (rcgen::Certificate, KeyPair) {
    let mut parameters = CertificateParams::new(Vec::<String>::new()).expect("CA parameters");
    parameters
        .distinguished_name
        .push(rcgen::DnType::CommonName, name);
    parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let key = KeyPair::generate().expect("CA key");
    let certificate = parameters.self_signed(&key).expect("CA certificate");
    (certificate, key)
}

fn write_certificates(certificates: &Certificates) -> CertificateFiles {
    let directory = tempfile::tempdir().expect("certificate directory");
    let ca = directory.path().join("ca.pem");
    let wrong_ca = directory.path().join("wrong-ca.pem");
    let client = directory.path().join("client.pem");
    let client_key = directory.path().join("client-key.pem");
    std::fs::write(&ca, &certificates.ca).expect("write CA");
    std::fs::write(&wrong_ca, &certificates.wrong_ca).expect("write wrong CA");
    std::fs::write(&client, &certificates.client).expect("write client certificate");
    std::fs::write(&client_key, &certificates.client_key).expect("write client key");
    CertificateFiles {
        _directory: directory,
        ca,
        wrong_ca,
        client,
        client_key,
    }
}

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
