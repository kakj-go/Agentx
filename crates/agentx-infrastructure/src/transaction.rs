use agentx_application::TransactionManager;
use anyhow::Result;
use async_trait::async_trait;
use sqlx::{MySql, MySqlPool, Transaction};

#[derive(Clone)]
pub struct MySqlTransactionManager {
    pool: MySqlPool,
}

impl MySqlTransactionManager {
    #[must_use]
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TransactionManager for MySqlTransactionManager {
    type Transaction = Transaction<'static, MySql>;

    async fn begin(&self) -> Result<Self::Transaction> {
        Ok(self.pool.begin().await?)
    }
    async fn commit(&self, transaction: Self::Transaction) -> Result<()> {
        Ok(transaction.commit().await?)
    }
    async fn rollback(&self, transaction: Self::Transaction) -> Result<()> {
        Ok(transaction.rollback().await?)
    }
}
