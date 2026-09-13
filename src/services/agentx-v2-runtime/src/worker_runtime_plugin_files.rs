use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use agentx_runtime_contracts::{
    ContentHash, RuntimeObjectReferenceV1, StorageDomain, TraceContentKindV1,
};
use futures::TryStreamExt;
use object_store::{MultipartUpload, path::Path as ObjectPath};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

use super::{ClaimedWorkerAttempt, RuntimeWorker};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TRANSFER_BYTES: u64 = 128 * 1024 * 1024;
const CHUNK_BYTES: usize = 256 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactDescriptor {
    artifact_id: Uuid,
    file_name: String,
    content_type: String,
    size_bytes: u64,
    sha256: String,
}

pub(super) struct InvocationFiles {
    directory: tempfile::TempDir,
    allowed: BTreeMap<Uuid, ArtifactDescriptor>,
    created: BTreeMap<Uuid, RuntimeObjectReferenceV1>,
    temporary: BTreeMap<Uuid, PathBuf>,
    transferred: u64,
}

impl InvocationFiles {
    pub(super) fn new() -> std::io::Result<Self> {
        let root = std::env::var_os("AGENTX_PLUGIN_WORK_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("agentx-plugin-workspaces"));
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            directory: tempfile::Builder::new()
                .prefix("invocation-")
                .tempdir_in(root)?,
            allowed: BTreeMap::new(),
            created: BTreeMap::new(),
            temporary: BTreeMap::new(),
            transferred: 0,
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn observe(&mut self, value: &Value) {
        match value {
            Value::Object(values) => {
                if let Ok(reference) = serde_json::from_value::<ArtifactDescriptor>(value.clone()) {
                    self.allowed.insert(reference.artifact_id, reference);
                } else {
                    for value in values.values() {
                        self.observe(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.observe(value);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle(
        &mut self,
        worker: &RuntimeWorker,
        tenant: Uuid,
        invocation: Uuid,
        claim: Option<&ClaimedWorkerAttempt>,
        method: &str,
        input: Value,
        index: u32,
    ) -> Result<Value, (i64, String)> {
        let result = match method {
            "host.artifacts.read" => self.read(worker, tenant, claim, input).await,
            "host.artifacts.put" => {
                self.put(worker, tenant, invocation, claim, input, index)
                    .await
            }
            _ => return Err((-32601, "Unknown Artifact operation".into())),
        };
        result.map_err(|error| (-32020, format!("PLUGIN_ARTIFACT_ERROR: {error}")))
    }

    fn reserve(&mut self, size: u64) -> anyhow::Result<()> {
        anyhow::ensure!(size <= MAX_FILE_BYTES, "Artifact exceeds 64 MiB");
        let total = self
            .transferred
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("Artifact transfer overflow"))?;
        anyhow::ensure!(
            total <= MAX_TRANSFER_BYTES,
            "Invocation Artifact transfers exceed 128 MiB"
        );
        self.transferred = total;
        Ok(())
    }

    async fn read(
        &mut self,
        worker: &RuntimeWorker,
        tenant: Uuid,
        claim: Option<&ClaimedWorkerAttempt>,
        input: Value,
    ) -> anyhow::Result<Value> {
        let requested: ArtifactDescriptor = serde_json::from_value(input)?;
        let allowed = self
            .allowed
            .get(&requested.artifact_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Artifact is not an input or a result of this invocation")
            })?
            .clone();
        anyhow::ensure!(
            requested.sha256 == allowed.sha256
                && requested.size_bytes == allowed.size_bytes
                && requested.content_type == allowed.content_type,
            "Artifact metadata differs from its authorized reference"
        );
        if let Some(path) = self.temporary.get(&requested.artifact_id).cloned() {
            self.reserve(allowed.size_bytes)?;
            let relative = format!("input-{}.bin", Uuid::now_v7());
            let destination = self.path().join(&relative);
            tokio::fs::copy(path, &destination).await?;
            let (size, hash) = hash_file(&mut tokio::fs::File::open(destination).await?).await?;
            anyhow::ensure!(
                size == allowed.size_bytes && hash == allowed.sha256,
                "Temporary Artifact failed content verification"
            );
            return Ok(
                json!({"path":relative,"fileName":allowed.file_name,"contentType":allowed.content_type,"sizeBytes":size,"sha256":hash}),
            );
        }
        let key = if let Some(created) = self.created.get(&requested.artifact_id) {
            created.object_key.clone()
        } else {
            let claim = claim.ok_or_else(|| {
                anyhow::anyhow!("Design operations can only read their own temporary Artifacts")
            })?;
            let row = sqlx::query("SELECT a.storage_key,a.size_bytes,a.sha256,a.content_type FROM artifacts a WHERE a.tenant_id=? AND a.id=? AND a.deleted_at IS NULL AND EXISTS(SELECT 1 FROM artifact_references r WHERE r.tenant_id=a.tenant_id AND r.artifact_id=a.id AND ((r.owner_type='execution' AND r.owner_id=?) OR (r.owner_type='node_execution' AND EXISTS(SELECT 1 FROM node_executions n WHERE n.tenant_id=a.tenant_id AND n.execution_id=? AND r.owner_id=BIN_TO_UUID(n.id)))))")
                .bind(tenant).bind(requested.artifact_id).bind(claim.task.execution_id.to_string()).bind(claim.task.execution_id)
                .fetch_optional(&worker.pool).await?.ok_or_else(|| anyhow::anyhow!("Artifact is unavailable to this execution"))?;
            anyhow::ensure!(
                row.try_get::<u64, _>("size_bytes")? == allowed.size_bytes
                    && row.try_get::<String, _>("sha256")? == allowed.sha256
                    && row.try_get::<String, _>("content_type")? == allowed.content_type,
                "Artifact metadata verification failed"
            );
            row.try_get::<String, _>("storage_key")?
        };
        self.reserve(allowed.size_bytes)?;
        let relative = format!("input-{}.bin", Uuid::now_v7());
        let path = self.path().join(&relative);
        let result = download(worker, &key, &path, &allowed).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        result?;
        Ok(
            json!({"path":relative,"fileName":allowed.file_name,"contentType":allowed.content_type,"sizeBytes":allowed.size_bytes,"sha256":allowed.sha256}),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn put(
        &mut self,
        worker: &RuntimeWorker,
        tenant: Uuid,
        invocation: Uuid,
        claim: Option<&ClaimedWorkerAttempt>,
        input: Value,
        index: u32,
    ) -> anyhow::Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct FileInput {
            path: String,
            file_name: String,
            content_type: String,
        }
        let input: FileInput = serde_json::from_value(input)?;
        anyhow::ensure!(
            !input.file_name.trim().is_empty()
                && input.file_name.len() <= 255
                && !input.file_name.contains(['/', '\\', '\0']),
            "Artifact fileName is invalid"
        );
        anyhow::ensure!(
            !input.content_type.trim().is_empty() && input.content_type.len() <= 255,
            "Artifact contentType is invalid"
        );
        let path = scoped_file(self.path(), &input.path).await?;
        let mut file = tokio::fs::File::open(&path).await?;
        let metadata = file.metadata().await?;
        anyhow::ensure!(metadata.is_file(), "Artifact path must be a regular file");
        self.reserve(metadata.len())?;
        let (size, digest) = hash_file(&mut file).await?;
        anyhow::ensure!(
            size == metadata.len(),
            "Artifact file changed while being read"
        );
        file.rewind().await?;
        let object_id = agentx_runtime_contracts::deterministic_uuid(
            invocation,
            format!("plugin-file-v2:{index}").as_bytes(),
        );
        let value = serde_json::to_value(ArtifactDescriptor {
            artifact_id: object_id,
            file_name: input.file_name.clone(),
            content_type: input.content_type.clone(),
            size_bytes: size,
            sha256: digest.clone(),
        })?;
        if claim.is_none() {
            let snapshot = self.path().join(format!("provider-{object_id}.bin"));
            tokio::fs::copy(&path, &snapshot).await?;
            let (copied, hash) = hash_file(&mut tokio::fs::File::open(&snapshot).await?).await?;
            anyhow::ensure!(
                copied == size && hash == digest,
                "Provider file changed during snapshot creation"
            );
            self.temporary.insert(object_id, snapshot);
            self.observe(&value);
            return Ok(value);
        }
        let content_hash = ContentHash::parse(format!("sha256:{digest}"))?;
        let key = RuntimeObjectReferenceV1::canonical_key(tenant, object_id, &content_hash);
        upload(worker, &key, &mut file, size, &digest).await?;
        let reference = RuntimeObjectReferenceV1 {
            tenant_id: tenant,
            storage_domain: StorageDomain::Runtime,
            object_id,
            object_key: key.clone(),
            content_hash,
            size_bytes: size,
            media_type: input.content_type.clone(),
        };
        let request_hash = agentx_runtime_contracts::content_hash(&reference)?;
        let inserted = sqlx::query("INSERT INTO runtime_objects(object_id,tenant_id,object_key,content_hash,size_bytes,media_type,status,idempotency_key,request_hash,temporary_expires_at,ready_at) VALUES(?,?,?,?,?,?,'ready',?,?,DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 1 HOUR),UTC_TIMESTAMP(6))")
            .bind(object_id).bind(tenant).bind(&key).bind(reference.content_hash.as_str()).bind(size).bind(&input.content_type)
            .bind(format!("plugin-file:{invocation}:{index}")).bind(request_hash.as_str()).execute(&worker.pool).await;
        if let Err(error) = inserted {
            let _ = worker.objects.delete(&ObjectPath::from(key)).await;
            return Err(error.into());
        }
        if let Some(claim) = claim {
            crate::trace_artifact::register_artifact(
                &worker.pool,
                &reference,
                claim.task.execution_id,
                claim.task.node_execution_id,
                TraceContentKindV1::RuntimeResponse,
            )
            .await?;
        }
        self.observe(&value);
        self.created.insert(object_id, reference);
        Ok(value)
    }
}

async fn scoped_file(root: &Path, value: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(value);
    anyhow::ensure!(
        !value.is_empty()
            && !value.contains(['\\', ':'])
            && relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Artifact path must be relative to this invocation"
    );
    let root = tokio::fs::canonicalize(root).await?;
    let mut path = root.clone();
    for part in relative.components() {
        path.push(part.as_os_str());
        anyhow::ensure!(
            !tokio::fs::symlink_metadata(&path)
                .await?
                .file_type()
                .is_symlink(),
            "Artifact symbolic links are not allowed"
        );
    }
    let path = tokio::fs::canonicalize(path).await?;
    anyhow::ensure!(
        path.starts_with(root),
        "Artifact path leaves the invocation directory"
    );
    Ok(path)
}

async fn hash_file(file: &mut tokio::fs::File) -> anyhow::Result<(u64, String)> {
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut hasher = Sha256::new();
    let mut size = 0;
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        size += read as u64;
        anyhow::ensure!(size <= MAX_FILE_BYTES, "Artifact exceeds 64 MiB");
        hasher.update(&buffer[..read]);
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

struct PendingUpload(Option<Box<dyn MultipartUpload>>);

impl Drop for PendingUpload {
    fn drop(&mut self) {
        if let Some(mut upload) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(5), upload.abort()).await;
            });
        }
    }
}

async fn upload(
    worker: &RuntimeWorker,
    key: &str,
    file: &mut tokio::fs::File,
    size: u64,
    expected: &str,
) -> anyhow::Result<()> {
    if size == 0 {
        let mut byte = [0_u8; 1];
        anyhow::ensure!(
            file.read(&mut byte).await? == 0,
            "Artifact file grew during upload"
        );
        worker
            .objects
            .put(&ObjectPath::from(key), bytes::Bytes::new().into())
            .await?;
        return Ok(());
    }
    let mut pending = PendingUpload(Some(
        worker.objects.put_multipart(&ObjectPath::from(key)).await?,
    ));
    {
        let upload = pending.0.as_mut().expect("upload is active");
        let mut sent = 0;
        let mut hasher = Sha256::new();
        loop {
            // S3 requires every non-final part to contain at least 5 MiB.
            let mut buffer = Vec::with_capacity(8 * 1024 * 1024);
            file.take(8 * 1024 * 1024).read_to_end(&mut buffer).await?;
            if buffer.is_empty() {
                break;
            }
            sent += buffer.len() as u64;
            anyhow::ensure!(sent <= size, "Artifact file grew during upload");
            hasher.update(&buffer);
            upload.put_part(buffer.into()).await?;
        }
        anyhow::ensure!(
            sent == size && format!("{:x}", hasher.finalize()) == expected,
            "Artifact file changed during upload"
        );
        upload.complete().await?;
    }
    pending.0.take();
    Ok(())
}

async fn download(
    worker: &RuntimeWorker,
    key: &str,
    path: &Path,
    expected: &ArtifactDescriptor,
) -> anyhow::Result<()> {
    let mut stream = worker
        .objects
        .get(&ObjectPath::from(key))
        .await?
        .into_stream();
    let mut file = tokio::fs::File::create_new(path).await?;
    let mut size = 0;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.try_next().await? {
        size += chunk.len() as u64;
        anyhow::ensure!(
            size <= expected.size_bytes,
            "Artifact content exceeds its declared size"
        );
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    anyhow::ensure!(
        size == expected.size_bytes && format!("{:x}", hasher.finalize()) == expected.sha256,
        "Artifact content failed size or hash verification"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_paths_are_invocation_scoped_and_budgeted() {
        let mut files = InvocationFiles::new().unwrap();
        tokio::fs::write(files.path().join("result.bin"), b"value")
            .await
            .unwrap();
        assert!(scoped_file(files.path(), "result.bin").await.is_ok());
        for invalid in ["../result.bin", "/etc/passwd", "C:/test", "a\\b", ""] {
            assert!(
                scoped_file(files.path(), invalid).await.is_err(),
                "{invalid}"
            );
        }
        assert!(files.reserve(MAX_FILE_BYTES + 1).is_err());
        files.reserve(MAX_FILE_BYTES).unwrap();
        files.reserve(MAX_FILE_BYTES).unwrap();
        assert!(files.reserve(1).is_err());
        let path = files.path().to_owned();
        drop(files);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn streams_files_larger_than_rpc_frames_and_rejects_forged_reads() {
        let pool = sqlx::mysql::MySqlPoolOptions::new()
            .connect_lazy("mysql://user:password@127.0.0.1/unused")
            .unwrap();
        let worker = RuntimeWorker::new_with_provider(
            pool,
            std::sync::Arc::new(object_store::memory::InMemory::new()),
            std::sync::Arc::new(super::super::tests::NoProvider),
        );
        let mut files = InvocationFiles::new().unwrap();
        let bytes = vec![0x6a; 10 * 1024 * 1024];
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let source = files.path().join("source.bin");
        tokio::fs::write(&source, &bytes).await.unwrap();
        let mut file = tokio::fs::File::open(&source).await.unwrap();
        upload(&worker, "roundtrip", &mut file, bytes.len() as u64, &digest)
            .await
            .unwrap();
        let reference = ArtifactDescriptor {
            artifact_id: Uuid::now_v7(),
            file_name: "source.bin".into(),
            content_type: "application/octet-stream".into(),
            size_bytes: bytes.len() as u64,
            sha256: digest,
        };
        let target = files.path().join("target.bin");
        download(&worker, "roundtrip", &target, &reference)
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), bytes);
        let input = serde_json::to_value(&reference).unwrap();
        assert!(
            files
                .read(&worker, Uuid::now_v7(), None, input.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("not an input")
        );
        files.observe(&input);
        let mut forged = input;
        forged["sha256"] = json!("0".repeat(64));
        assert!(
            files
                .read(&worker, Uuid::now_v7(), None, forged)
                .await
                .unwrap_err()
                .to_string()
                .contains("metadata differs")
        );
        let temporary = files.put(&worker, Uuid::now_v7(), Uuid::now_v7(), None,
            json!({"path":"source.bin","fileName":"source.bin","contentType":"application/octet-stream"}), 1).await.unwrap();
        let copy = files
            .read(&worker, Uuid::now_v7(), None, temporary)
            .await
            .unwrap();
        assert_eq!(copy["sizeBytes"], bytes.len() as u64);
        assert_eq!(
            tokio::fs::read(files.path().join(copy["path"].as_str().unwrap()))
                .await
                .unwrap(),
            bytes
        );
    }
}
