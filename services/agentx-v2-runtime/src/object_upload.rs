use agentx_runtime_contracts::{
    ContentHash, RuntimeObjectReferenceV1, RuntimeObjectUploadMetadataV1,
    RuntimeObjectUploadReceiptV1, RuntimePublishErrorCodeV1, StorageDomain,
};
use axum::{
    Json,
    extract::{Multipart, State},
    http::HeaderMap,
};
use bytes::Bytes;
use object_store::{WriteMultipart, path::Path as ObjectPath};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    RuntimeState,
    error::{RuntimeError, RuntimeResult},
};

const MAX_OBJECT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const UPLOAD_CONCURRENCY: usize = 4;

enum UploadReservation {
    Upload { temporary_key: String },
    Replay(RuntimeObjectUploadReceiptV1),
}

pub async fn upload_object(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> RuntimeResult<Json<RuntimeObjectUploadReceiptV1>> {
    state.trust.publisher(&headers, "runtime.objects.write")?;
    let mut metadata = None;
    let mut receipt = None;
    while let Some(mut field) = multipart.next_field().await.map_err(invalid_multipart)? {
        match field.name() {
            Some("metadata") if metadata.is_none() && receipt.is_none() => {
                let value = field.bytes().await.map_err(invalid_multipart)?;
                if value.len() > MAX_METADATA_BYTES {
                    return Err(RuntimeError::BadRequest(
                        RuntimePublishErrorCodeV1::UnsupportedApiVersion,
                        "object metadata exceeds the upload limit".into(),
                    ));
                }
                let parsed = serde_json::from_slice::<RuntimeObjectUploadMetadataV1>(&value)
                    .map_err(|_| {
                        RuntimeError::BadRequest(
                            RuntimePublishErrorCodeV1::UnsupportedApiVersion,
                            "invalid object metadata".into(),
                        )
                    })?;
                validate_metadata(&parsed)?;
                metadata = Some(parsed);
            }
            Some("content") if receipt.is_none() => {
                let metadata = metadata.as_ref().ok_or_else(|| {
                    RuntimeError::BadRequest(
                        RuntimePublishErrorCodeV1::ObjectMissing,
                        "metadata must precede content".into(),
                    )
                })?;
                if field.content_type() != Some(metadata.media_type.as_str()) {
                    return Err(RuntimeError::BadRequest(
                        RuntimePublishErrorCodeV1::ObjectMediaTypeMismatch,
                        "multipart content type does not match object metadata".into(),
                    ));
                }
                receipt = Some(stream_upload(&state, metadata.clone(), &mut field).await?);
            }
            Some("metadata" | "content") => {
                return Err(RuntimeError::BadRequest(
                    RuntimePublishErrorCodeV1::IdempotencyConflict,
                    "multipart fields must occur exactly once".into(),
                ));
            }
            _ => {
                return Err(RuntimeError::BadRequest(
                    RuntimePublishErrorCodeV1::ObjectMissing,
                    "unexpected multipart field".into(),
                ));
            }
        }
    }
    receipt.map(Json).ok_or_else(|| {
        RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::ObjectMissing,
            "metadata and content parts are required".into(),
        )
    })
}

async fn stream_upload(
    state: &RuntimeState,
    metadata: RuntimeObjectUploadMetadataV1,
    field: &mut axum::extract::multipart::Field<'_>,
) -> RuntimeResult<RuntimeObjectUploadReceiptV1> {
    let request_hash = agentx_runtime_contracts::content_hash(&metadata)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let reservation = reserve_upload(state, &metadata, &request_hash).await?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut writer = match &reservation {
        UploadReservation::Upload { temporary_key } => Some(WriteMultipart::new(
            state
                .objects
                .put_multipart(&ObjectPath::from(temporary_key.clone()))
                .await
                .map_err(|error| RuntimeError::Internal(error.into()))?,
        )),
        UploadReservation::Replay(_) => None,
    };
    while let Some(chunk) = field.chunk().await.map_err(invalid_multipart)? {
        size = size.checked_add(chunk.len() as u64).ok_or_else(|| {
            RuntimeError::BadRequest(
                RuntimePublishErrorCodeV1::ObjectSizeMismatch,
                "runtime object exceeds the upload limit".into(),
            )
        })?;
        if size > MAX_OBJECT_BYTES || size > metadata.size_bytes {
            abort(writer).await;
            return Err(RuntimeError::BadRequest(
                RuntimePublishErrorCodeV1::ObjectSizeMismatch,
                "object size does not match metadata".into(),
            ));
        }
        hasher.update(&chunk);
        if let Some(writer) = writer.as_mut() {
            writer
                .wait_for_capacity(UPLOAD_CONCURRENCY)
                .await
                .map_err(|error| RuntimeError::Internal(error.into()))?;
            writer.put(chunk);
        }
    }
    let digest = ContentHash::parse(format!("sha256:{:x}", hasher.finalize()))
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    if size != metadata.size_bytes || digest != metadata.content_hash {
        abort(writer).await;
        let code = if matches!(reservation, UploadReservation::Replay(_)) {
            RuntimePublishErrorCodeV1::IdempotencyConflict
        } else if size != metadata.size_bytes {
            RuntimePublishErrorCodeV1::ObjectSizeMismatch
        } else {
            RuntimePublishErrorCodeV1::ObjectHashMismatch
        };
        return Err(RuntimeError::BadRequest(
            code,
            "object content does not match immutable metadata".into(),
        ));
    }
    match reservation {
        UploadReservation::Replay(receipt) => Ok(receipt),
        UploadReservation::Upload { temporary_key } => {
            writer
                .expect("upload reservation creates a multipart writer")
                .finish()
                .await
                .map_err(|error| RuntimeError::Internal(error.into()))?;
            finalize_upload(state, metadata, request_hash, temporary_key).await
        }
    }
}

async fn abort(writer: Option<WriteMultipart>) {
    if let Some(writer) = writer {
        let _ = writer.abort().await;
    }
}

async fn reserve_upload(
    state: &RuntimeState,
    metadata: &RuntimeObjectUploadMetadataV1,
    request_hash: &ContentHash,
) -> RuntimeResult<UploadReservation> {
    let mut tx = state.pool.begin().await?;
    if let Some(row) = sqlx::query("SELECT request_hash,CAST(object_key AS CHAR CHARACTER SET utf8mb4) AS object_key,content_hash,size_bytes,media_type,status,temporary_key FROM runtime_objects WHERE tenant_id=? AND idempotency_key=? FOR UPDATE")
        .bind(metadata.tenant_id).bind(&metadata.idempotency_key).fetch_optional(&mut *tx).await?
    {
        if row.try_get::<String, _>("request_hash")? != request_hash.as_str() {
            return Err(RuntimeError::Conflict(RuntimePublishErrorCodeV1::IdempotencyConflict, "idempotency key was already used for different object metadata".into()));
        }
        let reservation = if row.try_get::<String, _>("status")? == "deleted" {
            restart_deleted_upload(&mut tx, metadata, request_hash).await?
        } else {
            reservation_from_row(metadata, row)?
        };
        tx.commit().await?;
        return Ok(reservation);
    }
    if let Some(row) = sqlx::query("SELECT request_hash,CAST(object_key AS CHAR CHARACTER SET utf8mb4) AS object_key,content_hash,size_bytes,media_type,status,temporary_key FROM runtime_objects WHERE tenant_id=? AND object_id=? FOR UPDATE")
        .bind(metadata.tenant_id).bind(metadata.object_id).fetch_optional(&mut *tx).await?
    {
        if row.try_get::<String, _>("content_hash")? != metadata.content_hash.as_str() {
            return Err(RuntimeError::Conflict(RuntimePublishErrorCodeV1::IdempotencyConflict, "object id already identifies different immutable content".into()));
        }
        if row.try_get::<String, _>("request_hash")? != request_hash.as_str() {
            return Err(RuntimeError::Conflict(RuntimePublishErrorCodeV1::IdempotencyConflict, "deleted object can only be restored with its original immutable upload request".into()));
        }
        let reservation = if row.try_get::<String, _>("status")? == "deleted" {
            restart_deleted_upload(&mut tx, metadata, request_hash).await?
        } else {
            reservation_from_row(metadata, row)?
        };
        tx.commit().await?;
        return Ok(reservation);
    }
    let object_key = RuntimeObjectReferenceV1::canonical_key(
        metadata.tenant_id,
        metadata.object_id,
        &metadata.content_hash,
    );
    let temporary_key = format!("temporary/{}/{}", metadata.tenant_id, Uuid::now_v7());
    sqlx::query("INSERT INTO runtime_objects(object_id,tenant_id,object_key,content_hash,size_bytes,media_type,status,temporary_key,idempotency_key,request_hash,temporary_expires_at) VALUES(?,?,?,?,?,?,'uploading',?,?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 1 HOUR))")
        .bind(metadata.object_id).bind(metadata.tenant_id).bind(object_key)
        .bind(metadata.content_hash.as_str()).bind(metadata.size_bytes).bind(&metadata.media_type)
        .bind(&temporary_key).bind(&metadata.idempotency_key).bind(request_hash.as_str())
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(UploadReservation::Upload { temporary_key })
}

async fn restart_deleted_upload(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    metadata: &RuntimeObjectUploadMetadataV1,
    request_hash: &ContentHash,
) -> RuntimeResult<UploadReservation> {
    let temporary_key = format!("temporary/{}/{}", metadata.tenant_id, Uuid::now_v7());
    let changed = sqlx::query("UPDATE runtime_objects SET status='uploading',temporary_key=?,temporary_expires_at=DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 1 HOUR),ready_at=NULL,deleted_at=NULL WHERE tenant_id=? AND object_id=? AND idempotency_key=? AND request_hash=? AND status='deleted'")
        .bind(&temporary_key)
        .bind(metadata.tenant_id)
        .bind(metadata.object_id)
        .bind(&metadata.idempotency_key)
        .bind(request_hash.as_str())
        .execute(&mut **tx)
        .await?;
    if changed.rows_affected() != 1 {
        return Err(RuntimeError::Conflict(
            RuntimePublishErrorCodeV1::IdempotencyConflict,
            "deleted object upload reservation could not be restored".into(),
        ));
    }
    Ok(UploadReservation::Upload { temporary_key })
}

fn reservation_from_row(
    metadata: &RuntimeObjectUploadMetadataV1,
    row: sqlx::mysql::MySqlRow,
) -> RuntimeResult<UploadReservation> {
    if row.try_get::<String, _>("status")? == "ready" {
        Ok(UploadReservation::Replay(receipt_from_row(
            metadata, row, true,
        )?))
    } else {
        Ok(UploadReservation::Upload {
            temporary_key: row
                .try_get::<Option<String>, _>("temporary_key")?
                .ok_or_else(|| {
                    RuntimeError::Conflict(
                        RuntimePublishErrorCodeV1::IdempotencyConflict,
                        "incomplete object upload has no temporary key".into(),
                    )
                })?,
        })
    }
}

async fn finalize_upload(
    state: &RuntimeState,
    metadata: RuntimeObjectUploadMetadataV1,
    request_hash: ContentHash,
    temporary_key: String,
) -> RuntimeResult<RuntimeObjectUploadReceiptV1> {
    let object_key = RuntimeObjectReferenceV1::canonical_key(
        metadata.tenant_id,
        metadata.object_id,
        &metadata.content_hash,
    );
    let temporary_path = ObjectPath::from(temporary_key);
    let final_path = ObjectPath::from(object_key.clone());
    state
        .objects
        .copy(&temporary_path, &final_path)
        .await
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    state
        .objects
        .delete(&temporary_path)
        .await
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    let changed = sqlx::query("UPDATE runtime_objects SET status='ready',temporary_key=NULL,ready_at=UTC_TIMESTAMP(6) WHERE object_id=? AND tenant_id=? AND request_hash=? AND status='uploading'")
        .bind(metadata.object_id).bind(metadata.tenant_id).bind(request_hash.as_str()).execute(&state.pool).await?;
    if changed.rows_affected() != 1 {
        return Err(RuntimeError::Conflict(
            RuntimePublishErrorCodeV1::IdempotencyConflict,
            "object upload reservation was lost".into(),
        ));
    }
    Ok(RuntimeObjectUploadReceiptV1 {
        api_version: 1,
        object: RuntimeObjectReferenceV1 {
            tenant_id: metadata.tenant_id,
            storage_domain: StorageDomain::Runtime,
            object_id: metadata.object_id,
            object_key,
            content_hash: metadata.content_hash,
            size_bytes: metadata.size_bytes,
            media_type: metadata.media_type,
        },
        replayed: false,
        accepted_at: OffsetDateTime::now_utc(),
    })
}

pub async fn persist_upload(
    state: &RuntimeState,
    metadata: RuntimeObjectUploadMetadataV1,
    content: Bytes,
) -> RuntimeResult<RuntimeObjectUploadReceiptV1> {
    validate_metadata(&metadata)?;
    if content.len() as u64 != metadata.size_bytes || content.len() as u64 > MAX_OBJECT_BYTES {
        return Err(RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::ObjectSizeMismatch,
            "object size does not match metadata".into(),
        ));
    }
    let digest = ContentHash::parse(format!("sha256:{:x}", Sha256::digest(&content)))
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    if digest != metadata.content_hash {
        return Err(RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::ObjectHashMismatch,
            "object content hash does not match metadata".into(),
        ));
    }
    let request_hash = agentx_runtime_contracts::content_hash(&metadata)
        .map_err(|error| RuntimeError::Internal(error.into()))?;
    match reserve_upload(state, &metadata, &request_hash).await? {
        UploadReservation::Replay(receipt) => Ok(receipt),
        UploadReservation::Upload { temporary_key } => {
            state
                .objects
                .put(&ObjectPath::from(temporary_key.clone()), content.into())
                .await
                .map_err(|error| RuntimeError::Internal(error.into()))?;
            finalize_upload(state, metadata, request_hash, temporary_key).await
        }
    }
}

fn validate_metadata(metadata: &RuntimeObjectUploadMetadataV1) -> RuntimeResult<()> {
    if metadata.api_version != 1 {
        return Err(RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::UnsupportedApiVersion,
            "object upload API version is unsupported".into(),
        ));
    }
    if metadata.size_bytes > MAX_OBJECT_BYTES {
        return Err(RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::ObjectSizeMismatch,
            "runtime object exceeds the upload limit".into(),
        ));
    }
    if metadata.media_type.trim().is_empty()
        || metadata.media_type.len() > 255
        || metadata.media_type.contains(['\r', '\n'])
    {
        return Err(RuntimeError::BadRequest(
            RuntimePublishErrorCodeV1::ObjectMediaTypeMismatch,
            "object media type is invalid".into(),
        ));
    }
    if metadata.idempotency_key.trim().is_empty() || metadata.idempotency_key.len() > 192 {
        return Err(RuntimeError::Conflict(
            RuntimePublishErrorCodeV1::IdempotencyConflict,
            "object idempotency key is invalid".into(),
        ));
    }
    Ok(())
}

fn invalid_multipart(_: axum::extract::multipart::MultipartError) -> RuntimeError {
    RuntimeError::BadRequest(
        RuntimePublishErrorCodeV1::ObjectHashMismatch,
        "invalid multipart body".into(),
    )
}

fn receipt_from_row(
    metadata: &RuntimeObjectUploadMetadataV1,
    row: sqlx::mysql::MySqlRow,
    replayed: bool,
) -> RuntimeResult<RuntimeObjectUploadReceiptV1> {
    Ok(RuntimeObjectUploadReceiptV1 {
        api_version: 1,
        object: RuntimeObjectReferenceV1 {
            tenant_id: metadata.tenant_id,
            storage_domain: StorageDomain::Runtime,
            object_id: metadata.object_id,
            object_key: row.try_get("object_key")?,
            content_hash: ContentHash::parse(row.try_get::<String, _>("content_hash")?)
                .map_err(|error| RuntimeError::Internal(error.into()))?,
            size_bytes: row.try_get("size_bytes")?,
            media_type: row.try_get("media_type")?,
        },
        replayed,
        accepted_at: OffsetDateTime::now_utc(),
    })
}
