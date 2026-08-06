use std::{collections::HashMap, sync::Arc};

use agentx_application::ExecutionRuntime;
use agentx_infrastructure::config::RedisSettings;
use agentx_infrastructure::credential::{CredentialKeyring, SecretProvider};
use agentx_infrastructure::runtime_client::GrpcExecutionRuntime;
use object_store::ObjectStore;
use sqlx::MySqlPool;
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

use crate::config::{AuthSettings, ConnectionSettings};

#[derive(Clone)]
pub struct AppState {
    pub pool: MySqlPool,
    pub auth: Arc<AuthSettings>,
    pub credential_keyring: Option<Arc<CredentialKeyring>>,
    pub secret_provider: Option<Arc<dyn SecretProvider>>,
    pub object_store: Option<Arc<dyn ObjectStore>>,
    pub http: reqwest::Client,
    pub connections: Arc<ConnectionSettings>,
    pub clickhouse: Option<clickhouse::Client>,
    pub redis: Option<Arc<RedisSettings>>,
    pub runtime: Option<Arc<dyn ExecutionRuntime>>,
    connection_limits: Arc<Mutex<HashMap<Uuid, Arc<Semaphore>>>>,
}

impl AppState {
    pub fn new(pool: MySqlPool, auth: AuthSettings) -> Self {
        Self {
            pool,
            auth: Arc::new(auth),
            credential_keyring: None,
            secret_provider: None,
            object_store: None,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client configuration is valid"),
            connections: Arc::new(ConnectionSettings {
                timeout_seconds: 10,
                max_concurrency: 4,
                allow_private_networks: false,
                allowed_hosts: Vec::new(),
                allowed_cidrs: Vec::new(),
            }),
            clickhouse: None,
            redis: None,
            runtime: None,
            connection_limits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn with_runtime(mut self, runtime: GrpcExecutionRuntime) -> Self {
        self.runtime = Some(Arc::new(runtime));
        self
    }

    #[must_use]
    pub fn with_m3(mut self, clickhouse: clickhouse::Client, redis: RedisSettings) -> Self {
        self.clickhouse = Some(clickhouse);
        self.redis = Some(Arc::new(redis));
        self
    }

    #[must_use]
    pub fn with_m2(
        mut self,
        credential_keyring: CredentialKeyring,
        object_store: Option<Arc<dyn ObjectStore>>,
        connections: ConnectionSettings,
    ) -> Self {
        self.credential_keyring = Some(Arc::new(credential_keyring));
        self.object_store = object_store;
        self.connections = Arc::new(connections);
        self
    }

    #[must_use]
    pub fn with_m2_external(
        mut self,
        secret_provider: Arc<dyn SecretProvider>,
        object_store: Option<Arc<dyn ObjectStore>>,
        connections: ConnectionSettings,
    ) -> Self {
        self.secret_provider = Some(secret_provider);
        self.object_store = object_store;
        self.connections = Arc::new(connections);
        self
    }

    pub async fn connection_permit(&self, tenant_id: Uuid) -> tokio::sync::OwnedSemaphorePermit {
        let limit = {
            let mut limits = self.connection_limits.lock().await;
            limits
                .entry(tenant_id)
                .or_insert_with(|| Arc::new(Semaphore::new(self.connections.max_concurrency)))
                .clone()
        };
        limit
            .acquire_owned()
            .await
            .expect("connection semaphore remains open")
    }
}
