use std::sync::Arc;

use bytes::Bytes;
use object_store::{ObjectStore, path::Path as ObjectPath};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use agentx_runtime_contracts::{
    ContentHash, RuntimeObjectReferenceV1, StorageDomain, TraceContentKindV1,
};

use crate::{engine::ClaimedWorkerAttempt, error::RuntimeResult};

const TRACE_PREVIEW_LIMIT: usize = 16 * 1024;

pub(crate) async fn externalize_attempt_input(
    pool: &MySqlPool,
    objects: &Arc<dyn ObjectStore>,
    claim: &ClaimedWorkerAttempt,
) {
    let result = async {
        let value = serde_json::to_value(&claim.inputs)?;
        let encoded = agentx_runtime_contracts::canonical_bytes(&value)?;
        if !should_externalize(encoded.len()) {
            return Ok::<(), anyhow::Error>(());
        }
        let artifact = persist_content(
            pool,
            objects,
            claim.task.tenant_id,
            claim.task.attempt_id,
            "input",
            encoded,
            "application/json",
        )
        .await?;
        register_artifact(
            pool,
            &artifact,
            claim.task.execution_id,
            claim.task.node_execution_id,
            TraceContentKindV1::AttemptInput,
        )
        .await?;
        emit_input_references(pool, claim, &artifact).await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, attempt_id = %claim.task.attempt_id, "Trace input Artifact externalization failed");
    }
}

fn should_externalize(encoded_len: usize) -> bool {
    encoded_len > TRACE_PREVIEW_LIMIT
}

pub(crate) async fn externalize_plugin_content(
    pool: &MySqlPool,
    objects: &Arc<dyn ObjectStore>,
    claim: &ClaimedWorkerAttempt,
    span_id: Uuid,
    value: &serde_json::Value,
) -> Option<Uuid> {
    let result = async {
        let encoded = agentx_runtime_contracts::canonical_bytes(value)?;
        if !should_externalize(encoded.len()) {
            return Ok::<Option<Uuid>, anyhow::Error>(None);
        }
        let hash = format!("{:x}", Sha256::digest(&encoded));
        let artifact = persist_content(
            pool,
            objects,
            claim.task.tenant_id,
            span_id,
            &format!("plugin-content-{}", &hash[..16]),
            encoded,
            "application/json",
        )
        .await?;
        register_artifact(
            pool,
            &artifact,
            claim.task.execution_id,
            claim.task.node_execution_id,
            TraceContentKindV1::PluginContent,
        )
        .await?;
        Ok(Some(artifact.object_id))
    }
    .await;
    match result {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, %span_id, "Plugin Trace content Artifact externalization failed");
            None
        }
    }
}

pub(crate) async fn externalize_runtime_call_response(
    pool: &MySqlPool,
    objects: &Arc<dyn ObjectStore>,
    claim: &ClaimedWorkerAttempt,
    call_id: Uuid,
    value: &serde_json::Value,
) -> Option<Uuid> {
    let result = async {
        let encoded = agentx_runtime_contracts::canonical_bytes(value)?;
        if !should_externalize(encoded.len()) {
            return Ok::<Option<Uuid>, anyhow::Error>(None);
        }
        let artifact = persist_content(
            pool,
            objects,
            claim.task.tenant_id,
            call_id,
            "provider-response",
            encoded,
            "application/json",
        )
        .await?;
        register_artifact(
            pool,
            &artifact,
            claim.task.execution_id,
            claim.task.node_execution_id,
            TraceContentKindV1::RuntimeResponse,
        )
        .await?;
        Ok(Some(artifact.object_id))
    }
    .await;
    match result {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, %call_id, "Provider response Artifact externalization failed");
            None
        }
    }
}

pub(crate) async fn externalize_http_binary(
    pool: &MySqlPool,
    objects: &Arc<dyn ObjectStore>,
    claim: &ClaimedWorkerAttempt,
    call_id: Uuid,
    bytes: Vec<u8>,
    media_type: &str,
) -> Option<RuntimeObjectReferenceV1> {
    let result = async {
        let artifact = persist_content(
            pool,
            objects,
            claim.task.tenant_id,
            call_id,
            "http-response",
            bytes,
            media_type,
        )
        .await?;
        register_artifact(
            pool,
            &artifact,
            claim.task.execution_id,
            claim.task.node_execution_id,
            TraceContentKindV1::RuntimeResponse,
        )
        .await?;
        Ok::<_, anyhow::Error>(artifact)
    }
    .await;
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(%error, %call_id, "HTTP binary Artifact externalization failed");
            None
        }
    }
}

pub(crate) async fn register_artifact(
    pool: &MySqlPool,
    artifact: &RuntimeObjectReferenceV1,
    execution_id: Uuid,
    node_execution_id: Uuid,
    content_kind: TraceContentKindV1,
) -> anyhow::Result<()> {
    let sha256 = artifact
        .content_hash
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| anyhow::anyhow!("Runtime Artifact hash is invalid"))?;
    let role = serde_json::to_value(content_kind)?
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Trace content kind is invalid"))?
        .to_owned();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key) VALUES(?,?,?,?,?,?) ON DUPLICATE KEY UPDATE id=id")
        .bind(artifact.object_id).bind(artifact.tenant_id).bind(&artifact.media_type)
        .bind(artifact.size_bytes).bind(sha256).bind(&artifact.object_key)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT IGNORE INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'execution',?,?)")
        .bind(artifact.tenant_id).bind(artifact.object_id).bind(execution_id.to_string()).bind(&role)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT IGNORE INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'node_execution',?,?)")
        .bind(artifact.tenant_id).bind(artifact.object_id).bind(node_execution_id.to_string()).bind(&role)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{TRACE_PREVIEW_LIMIT, should_externalize};

    #[test]
    fn provider_responses_crossing_the_trace_preview_limit_are_externalized() {
        assert!(!should_externalize(TRACE_PREVIEW_LIMIT));
        assert!(should_externalize(TRACE_PREVIEW_LIMIT + 1));
    }
}

pub(crate) async fn persist_content(
    pool: &MySqlPool,
    objects: &Arc<dyn ObjectStore>,
    tenant_id: Uuid,
    attempt_id: Uuid,
    role: &str,
    encoded: Vec<u8>,
    media_type: &str,
) -> anyhow::Result<RuntimeObjectReferenceV1> {
    let object_id = agentx_runtime_contracts::deterministic_uuid(
        attempt_id,
        format!("agentx-trace-content-v1:{role}").as_bytes(),
    );
    let content_hash = ContentHash::parse(format!("sha256:{:x}", Sha256::digest(&encoded)))?;
    let object_key = RuntimeObjectReferenceV1::canonical_key(tenant_id, object_id, &content_hash);
    if let Some(row) = sqlx::query("SELECT content_hash,size_bytes,media_type,status FROM runtime_objects WHERE tenant_id=? AND object_id=?")
        .bind(tenant_id).bind(object_id).fetch_optional(pool).await?
    {
        anyhow::ensure!(
            row.try_get::<String, _>("content_hash")? == content_hash.as_str()
                && row.try_get::<u64, _>("size_bytes")? == encoded.len() as u64
                && row.try_get::<String, _>("media_type")? == media_type
                && row.try_get::<String, _>("status")? == "ready",
            "Trace Artifact identity conflicts with existing content"
        );
    } else {
        let path = ObjectPath::from(object_key.clone());
        objects.put(&path, Bytes::from(encoded.clone()).into()).await?;
        let request_hash = agentx_runtime_contracts::content_hash(&json!({
            "attemptId":attempt_id,"role":role,"contentHash":content_hash,"sizeBytes":encoded.len()
        }))?;
        if let Err(error) = sqlx::query("INSERT INTO runtime_objects(object_id,tenant_id,object_key,content_hash,size_bytes,media_type,status,idempotency_key,request_hash,temporary_expires_at,ready_at) VALUES(?,?,?,?,?,?,'ready',?,?,UTC_TIMESTAMP(6),UTC_TIMESTAMP(6))")
            .bind(object_id).bind(tenant_id).bind(&object_key).bind(content_hash.as_str())
            .bind(encoded.len() as u64).bind(media_type).bind(format!("trace-content:{attempt_id}:{role}"))
            .bind(request_hash.as_str()).execute(pool).await
        {
            let _ = objects.delete(&path).await;
            return Err(error.into());
        }
    }
    Ok(RuntimeObjectReferenceV1 {
        tenant_id,
        storage_domain: StorageDomain::Runtime,
        object_id,
        object_key,
        content_hash,
        size_bytes: encoded.len() as u64,
        media_type: media_type.into(),
    })
}

async fn emit_input_references(
    pool: &MySqlPool,
    claim: &ClaimedWorkerAttempt,
    artifact: &RuntimeObjectReferenceV1,
) -> RuntimeResult<()> {
    let row = sqlx::query("SELECT a.attempt_number,n.node_name FROM node_attempts a JOIN node_executions n ON n.id=a.node_execution_id WHERE a.tenant_id=? AND a.id=?")
        .bind(claim.task.tenant_id).bind(claim.task.attempt_id).fetch_one(pool).await?;
    let attempt_number: u16 = row.try_get("attempt_number")?;
    let mut tx = pool.begin().await?;
    let mut node = crate::trace_delivery::TraceDraft::span(
        claim.task.tenant_id,
        claim.task.execution_id,
        claim.task.node_execution_id,
        Some((
            claim.task.execution_id,
            agentx_runtime_contracts::TraceSpanKindV1::Execution,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Node,
        row.try_get::<String, _>("node_name")?,
        agentx_runtime_contracts::TraceEventKindV1::Updated,
        "node.input_externalized",
        "running",
    );
    node.node_execution_id = Some(claim.task.node_execution_id);
    node.attempt_id = Some(claim.task.attempt_id);
    node.content_ref = Some(artifact.object_id);
    node.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::NodeInput);
    node.attributes = json!({"contentExternalized":true});
    crate::trace_delivery::enqueue_best_effort(&mut tx, node).await;
    let mut attempt = crate::trace_delivery::TraceDraft::span(
        claim.task.tenant_id,
        claim.task.execution_id,
        claim.task.attempt_id,
        Some((
            claim.task.node_execution_id,
            agentx_runtime_contracts::TraceSpanKindV1::Node,
        )),
        agentx_runtime_contracts::TraceSpanKindV1::Attempt,
        format!("Attempt {attempt_number}"),
        agentx_runtime_contracts::TraceEventKindV1::Updated,
        "attempt.input_externalized",
        "running",
    );
    attempt.node_execution_id = Some(claim.task.node_execution_id);
    attempt.attempt_id = Some(claim.task.attempt_id);
    attempt.content_ref = Some(artifact.object_id);
    attempt.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::AttemptInput);
    attempt.attributes = json!({"contentExternalized":true,"encodedBytes":artifact.size_bytes});
    crate::trace_delivery::enqueue_best_effort(&mut tx, attempt).await;
    if let Some(lease_id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM sandbox_leases WHERE tenant_id=? AND attempt_id=? ORDER BY created_at DESC LIMIT 1")
        .bind(claim.task.tenant_id).bind(claim.task.attempt_id).fetch_optional(&mut *tx).await?
    {
        let mut sandbox = crate::trace_delivery::TraceDraft::span(
            claim.task.tenant_id, claim.task.execution_id, lease_id,
            Some((claim.task.attempt_id, agentx_runtime_contracts::TraceSpanKindV1::Attempt)),
            agentx_runtime_contracts::TraceSpanKindV1::Sandbox, "OpenSandbox execution",
            agentx_runtime_contracts::TraceEventKindV1::Updated, "sandbox.input_externalized", "running",
        );
        sandbox.node_execution_id = Some(claim.task.node_execution_id);
        sandbox.attempt_id = Some(claim.task.attempt_id);
        sandbox.sandbox_lease_id = Some(lease_id);
        sandbox.content_ref = Some(artifact.object_id);
        sandbox.content_kind = Some(agentx_runtime_contracts::TraceContentKindV1::SandboxRequest);
        sandbox.attributes = json!({"contentExternalized":true});
        crate::trace_delivery::enqueue_best_effort(&mut tx, sandbox).await;
    }
    tx.commit().await?;
    Ok(())
}
