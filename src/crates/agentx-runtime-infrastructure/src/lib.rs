//! Runtime-plane-only infrastructure for V2 services and operations.

mod clients;
#[path = "mysql.rs"]
mod runtime_mysql;
mod settings;

pub use agentx_mysql_lease as lease;
pub use clients::{connect_runtime_redis, runtime_object_store, runtime_redis_client};
pub use runtime_mysql::{connect_runtime_mysql, migrate_runtime_mysql, ping_runtime_mysql};
pub use settings::{
    MySqlTlsMode, RuntimeInfrastructureSettings, RuntimeMySqlSettings,
    RuntimeObjectStorageSettings, RuntimeRedisSettings,
};
