use anyhow::{Context, Result};
use redis::streams::{
    StreamAutoClaimOptions, StreamAutoClaimReply, StreamId, StreamReadOptions, StreamReadReply,
};
use redis::{AsyncCommands, FromRedisValue};

use crate::{
    clients::connect_redis,
    config::RedisSettings,
    runtime_repository::{DispatchMessage, OutboxDelivery},
};

pub const RUNTIME_GROUP: &str = "agentx-runtime-workers-v1";

#[derive(Clone)]
pub struct RuntimeQueue {
    settings: RedisSettings,
}

#[derive(Clone, Debug)]
pub struct QueueItem {
    pub stream: String,
    pub stream_id: String,
    pub message: DispatchMessage,
}

impl RuntimeQueue {
    #[must_use]
    pub fn new(settings: RedisSettings) -> Self {
        Self { settings }
    }

    pub async fn ensure_groups(&self) -> Result<()> {
        let mut connection = connect_redis(&self.settings).await?;
        for capability in agentx_node_protocol::ALL_RUNTIME_CAPABILITIES {
            let stream = stream_name(capability);
            let result = redis::cmd("XGROUP")
                .arg("CREATE")
                .arg(&stream)
                .arg(RUNTIME_GROUP)
                .arg("0-0")
                .arg("MKSTREAM")
                .query_async::<String>(&mut connection)
                .await;
            if let Err(error) = result
                && !error.to_string().contains("BUSYGROUP")
            {
                return Err(error).context("failed to create runtime consumer group");
            }
        }
        Ok(())
    }

    pub async fn publish(&self, delivery: &OutboxDelivery) -> Result<String> {
        let mut connection = connect_redis(&self.settings).await?;
        let stream = stream_name(&delivery.capability);
        let payload = serde_json::to_string(&delivery.payload)?;
        connection
            .xadd(&stream, "*", &[("payload", payload)])
            .await
            .context("failed to publish runtime dispatch")
    }

    pub async fn read(
        &self,
        capabilities: &[String],
        consumer: &str,
        block_ms: usize,
    ) -> Result<Vec<QueueItem>> {
        let mut connection = connect_redis(&self.settings).await?;
        let streams = capabilities
            .iter()
            .map(|capability| stream_name(capability))
            .collect::<Vec<_>>();
        for stream in &streams {
            let claimed: StreamAutoClaimReply = connection
                .xautoclaim_options(
                    stream,
                    RUNTIME_GROUP,
                    consumer,
                    30_000,
                    "0-0",
                    StreamAutoClaimOptions::default().count(25),
                )
                .await?;
            if !claimed.claimed.is_empty() {
                return decode_items(stream, claimed.claimed);
            }
        }
        let stream_refs = streams.iter().map(String::as_str).collect::<Vec<_>>();
        let ids = vec![">"; streams.len()];
        let options = StreamReadOptions::default()
            .group(RUNTIME_GROUP, consumer)
            .count(25)
            .block(block_ms);
        let reply: StreamReadReply = connection
            .xread_options(&stream_refs, &ids, &options)
            .await?;
        let mut items = Vec::new();
        for key in reply.keys {
            items.extend(decode_items(&key.key, key.ids)?);
        }
        Ok(items)
    }

    pub async fn ack(&self, item: &QueueItem) -> Result<()> {
        let mut connection = connect_redis(&self.settings).await?;
        let _: u64 = connection
            .xack(&item.stream, RUNTIME_GROUP, &[&item.stream_id])
            .await?;
        let _: u64 = connection.xdel(&item.stream, &[&item.stream_id]).await?;
        Ok(())
    }
}

fn decode_items(stream: &str, items: Vec<StreamId>) -> Result<Vec<QueueItem>> {
    items
        .into_iter()
        .map(|item| {
            let payload = item
                .map
                .get("payload")
                .context("runtime queue item has no payload")?;
            let payload = String::from_redis_value(payload)?;
            Ok(QueueItem {
                stream: stream.into(),
                stream_id: item.id,
                message: serde_json::from_str(&payload)?,
            })
        })
        .collect()
}

#[must_use]
pub fn stream_name(capability: &str) -> String {
    debug_assert!(agentx_node_protocol::ALL_RUNTIME_CAPABILITIES.contains(&capability));
    format!("agentx:runtime:{capability}")
}

#[cfg(test)]
mod tests {
    use super::stream_name;

    #[test]
    fn capabilities_are_isolated_by_stream() {
        assert_eq!(stream_name("builtin"), "agentx:runtime:builtin");
        assert_ne!(stream_name("builtin"), stream_name("remote_action"));
    }
}
