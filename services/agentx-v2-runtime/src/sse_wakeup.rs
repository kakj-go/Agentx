use std::time::Duration;

use futures::StreamExt;
use redis::AsyncCommands;
use tokio::sync::mpsc;
use uuid::Uuid;

use agentx_runtime_infrastructure::{RuntimeRedisSettings, runtime_redis_client};

const CHANNEL_PREFIX: &str = "agentx:v2:invocation:wakeup:";

#[derive(Clone, Default)]
pub struct SseWakeup {
    client: Option<redis::Client>,
}

impl SseWakeup {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            client: Some(runtime_redis_client(&RuntimeRedisSettings::from_env()?)?),
        })
    }

    pub const fn disabled() -> Self {
        Self { client: None }
    }

    pub fn subscribe(&self, invocation_id: Uuid) -> Option<mpsc::Receiver<()>> {
        subscribe(self.client.clone(), invocation_id)
    }
}

pub async fn publish(client: Option<&redis::Client>, invocation_id: Uuid) {
    let Some(client) = client else { return };
    let result = async {
        let mut connection = client.get_multiplexed_async_connection().await?;
        connection
            .publish::<_, _, u64>(channel(invocation_id), invocation_id.to_string())
            .await
    }
    .await;
    if let Err(error) = result {
        tracing::debug!(%error, %invocation_id, "Redis SSE wakeup publish failed; MySQL remains authoritative");
    }
}

pub async fn publish_connection(
    connection: &mut redis::aio::ConnectionManager,
    invocation_id: Uuid,
) -> redis::RedisResult<()> {
    connection
        .publish::<_, _, u64>(channel(invocation_id), invocation_id.to_string())
        .await?;
    Ok(())
}

pub fn subscribe(client: Option<redis::Client>, invocation_id: Uuid) -> Option<mpsc::Receiver<()>> {
    let (sender, receiver) = mpsc::channel(1);
    let client = client?;
    tokio::spawn(async move {
        loop {
            let result: redis::RedisResult<()> = async {
                let mut pubsub = client.get_async_pubsub().await?;
                pubsub.subscribe(channel(invocation_id)).await?;
                let mut messages = pubsub.on_message();
                while messages.next().await.is_some() {
                    let _ = sender.try_send(());
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(%error, %invocation_id, "Redis SSE wakeup subscription interrupted; MySQL polling continues");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    Some(receiver)
}

fn channel(invocation_id: Uuid) -> String {
    format!("{CHANNEL_PREFIX}{invocation_id}")
}
