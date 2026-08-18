//! Control-plane-only infrastructure construction.

pub mod artifact;
pub mod credential;
mod mysql;
pub mod outbox;
mod settings;

pub use artifact::MySqlControlArtifactStore;
pub use credential::{
    CredentialKeyring, EncryptedSecret, PlainSecret, SecretProvider, VaultSecretProvider,
};
pub use mysql::{
    connect_control_mysql, control_object_store, migrate_control_mysql, ping_control_mysql,
};
pub use outbox::{MySqlControlOutbox, MySqlControlOutboxDispatcher};
pub use settings::{
    ControlInfrastructureSettings, ControlMySqlSettings, ControlObjectStorageSettings, MySqlTlsMode,
};

pub use object_store::ObjectStore;
