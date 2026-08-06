use std::{sync::Arc, time::Duration};

use agentx_application::OutboxDispatcher;

pub fn start_outbox_relay(
    pool: sqlx::MySqlPool,
    redis: agentx_infrastructure::config::RedisSettings,
) {
    tokio::spawn(async move {
        let dispatcher = agentx_infrastructure::outbox::MySqlOutboxDispatcher::new(pool);
        loop {
            let deliveries = match dispatcher.claim(200, Duration::from_secs(30)).await {
                Ok(deliveries) => deliveries,
                Err(error) => {
                    tracing::error!(%error, "business outbox claim failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if deliveries.is_empty() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
            let mut connection = match agentx_infrastructure::clients::connect_redis(&redis).await {
                Ok(connection) => connection,
                Err(error) => {
                    for delivery in &deliveries {
                        let _ = dispatcher
                            .mark_failed(delivery, &error.to_string(), Duration::from_secs(2))
                            .await;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            for delivery in deliveries {
                let payload =
                    serde_json::to_string(&delivery.payload).unwrap_or_else(|_| "null".into());
                let published = redis::cmd("XADD")
                    .arg("agentx:runtime-events:v1")
                    .arg("MAXLEN")
                    .arg("~")
                    .arg(100_000)
                    .arg("*")
                    .arg("event_id")
                    .arg(delivery.event_id.to_string())
                    .arg("tenant_id")
                    .arg(delivery.tenant_id.to_string())
                    .arg("event_type")
                    .arg(&delivery.event_type)
                    .arg("aggregate_type")
                    .arg(&delivery.aggregate_type)
                    .arg("aggregate_id")
                    .arg(&delivery.aggregate_id)
                    .arg("payload")
                    .arg(payload)
                    .query_async::<String>(&mut connection)
                    .await;
                match published {
                    Ok(_) => {
                        if let Err(error) = dispatcher.mark_published(&delivery).await {
                            tracing::error!(%error, event_id=%delivery.event_id, "business outbox publish acknowledgement failed");
                        }
                    }
                    Err(error) => {
                        let _ = dispatcher
                            .mark_failed(&delivery, &error.to_string(), Duration::from_secs(2))
                            .await;
                    }
                }
            }
        }
    });
}

pub fn start_runtime_projector(
    pool: sqlx::MySqlPool,
    objects: Option<Arc<dyn object_store::ObjectStore>>,
    redis: agentx_infrastructure::config::RedisSettings,
) {
    tokio::spawn(async move {
        let mut projector =
            agentx_infrastructure::runtime_projector::RuntimeEventProjector::new(pool.clone());
        if let Some(objects) = objects {
            projector = projector.with_artifacts(Arc::new(
                agentx_infrastructure::artifact::MySqlObjectArtifactStore::new(pool, objects)
                    .with_quota_admission(agentx_infrastructure::quota::QuotaAdmission::new(redis)),
            ));
        }
        loop {
            match projector.project_batch(200).await {
                Ok(0) => tokio::time::sleep(Duration::from_millis(250)).await,
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error, "runtime business projection failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}
