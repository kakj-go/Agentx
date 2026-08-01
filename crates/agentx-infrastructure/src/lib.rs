//! Infrastructure adapters live here as the project grows.

pub mod clickhouse {
    pub struct ClickHouseTraceSink;
}

pub mod mysql {
    pub struct MySqlRepositories;
}

pub mod object_storage {
    pub struct ObjectStorageAdapter;
}

pub mod redis {
    pub struct RedisExecutionQueue;
}

pub mod sandbox {
    pub struct CubeSandboxAdapter;
}
