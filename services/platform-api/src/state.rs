use std::sync::Arc;

use sqlx::MySqlPool;

use crate::config::AuthSettings;

#[derive(Clone)]
pub struct AppState {
    pub pool: MySqlPool,
    pub auth: Arc<AuthSettings>,
}

impl AppState {
    pub fn new(pool: MySqlPool, auth: AuthSettings) -> Self {
        Self {
            pool,
            auth: Arc::new(auth),
        }
    }
}
