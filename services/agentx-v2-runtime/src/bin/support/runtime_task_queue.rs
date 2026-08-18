#![allow(dead_code)]

use agentx_runtime_contracts::WorkerTaskV1;
use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::streams::{
    StreamAutoClaimOptions, StreamAutoClaimReply, StreamId, StreamReadOptions, StreamReadReply,
};
use redis::{AsyncCommands, FromRedisValue};

pub const TASK_GROUP: &str = "agentx:v2:workers:v1";

#[derive(Debug)]
pub struct TaskQueueItem {
    pub stream: String,
    pub stream_id: String,
    pub task: WorkerTaskV1,
}

pub fn stream_name(capability: &str) -> Result<String> {
    anyhow::ensure!(
        agentx_node_protocol::ALL_RUNTIME_CAPABILITIES.contains(&capability),
        "unsupported Runtime capability {capability}"
    );
    Ok(format!("agentx:v2:tasks:v1:{capability}"))
}

pub async fn ensure_groups(redis: &mut ConnectionManager) -> Result<()> {
    for capability in agentx_node_protocol::ALL_RUNTIME_CAPABILITIES {
        let stream = stream_name(capability)?;
        let result = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(stream)
            .arg(TASK_GROUP)
            .arg("0-0")
            .arg("MKSTREAM")
            .query_async::<String>(redis)
            .await;
        if let Err(error) = result
            && !error.to_string().contains("BUSYGROUP")
        {
            return Err(error).context("failed to create Runtime task consumer group");
        }
    }
    Ok(())
}

pub async fn publish(redis: &mut ConnectionManager, task: &WorkerTaskV1) -> Result<String> {
    let stream = stream_name(task.capability.as_str())?;
    let payload = serde_json::to_string(task)?;
    redis
        .xadd(&stream, "*", &[("task", payload)])
        .await
        .context("failed to append Runtime Worker Task")
}

pub async fn read(
    redis: &mut ConnectionManager,
    capability: &str,
    consumer: &str,
    block_ms: usize,
) -> Result<Vec<TaskQueueItem>> {
    let stream = stream_name(capability)?;
    let claimed: StreamAutoClaimReply = redis
        .xautoclaim_options(
            &stream,
            TASK_GROUP,
            consumer,
            30_000,
            "0-0",
            StreamAutoClaimOptions::default().count(25),
        )
        .await?;
    if !claimed.claimed.is_empty() {
        return decode(&stream, claimed.claimed);
    }
    let options = StreamReadOptions::default()
        .group(TASK_GROUP, consumer)
        .count(25)
        .block(block_ms);
    let reply: StreamReadReply = redis
        .xread_options(&[stream.as_str()], &[">"], &options)
        .await?;
    let mut items = Vec::new();
    for key in reply.keys {
        items.extend(decode(&key.key, key.ids)?);
    }
    Ok(items)
}

pub async fn ack(redis: &mut ConnectionManager, item: &TaskQueueItem) -> Result<()> {
    let _: u64 = redis
        .xack(&item.stream, TASK_GROUP, &[item.stream_id.as_str()])
        .await?;
    let _: u64 = redis.xdel(&item.stream, &[item.stream_id.as_str()]).await?;
    Ok(())
}

fn decode(stream: &str, ids: Vec<StreamId>) -> Result<Vec<TaskQueueItem>> {
    ids.into_iter()
        .map(|id| {
            let payload = id
                .map
                .get("task")
                .context("Runtime task stream entry has no task field")?;
            let payload = String::from_redis_value(payload)?;
            Ok(TaskQueueItem {
                stream: stream.to_owned(),
                stream_id: id.id,
                task: serde_json::from_str(&payload)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::stream_name;

    #[test]
    fn stream_is_versioned_and_partitioned_by_capability() {
        assert_eq!(
            stream_name("builtin").unwrap(),
            "agentx:v2:tasks:v1:builtin"
        );
        assert!(stream_name("unknown").is_err());
    }
}
