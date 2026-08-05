use std::{sync::Arc, time::Duration};

use agentx_application::{
    CredentialResolver, MemoryOperation, MemoryRequest, MemoryRuntime, RagOperation, RagRequest,
    RagRuntime, RuntimeContext, RuntimeError, RuntimeResult,
};
use agentx_domain::ResourceOperation;
use async_trait::async_trait;
use reqwest::{
    Client, Method, RequestBuilder,
    header::{AUTHORIZATION, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use url::Url;

use crate::runtime_resources::MySqlResourceAuthorizer;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct JsonRuntimeClient {
    http: Client,
    authorizer: MySqlResourceAuthorizer,
    credentials: Arc<dyn CredentialResolver>,
}

impl JsonRuntimeClient {
    fn new(
        authorizer: MySqlResourceAuthorizer,
        credentials: Arc<dyn CredentialResolver>,
    ) -> RuntimeResult<Self> {
        Ok(Self {
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| RuntimeError::new("RUNTIME_CLIENT_INVALID", e.to_string()))?,
            authorizer,
            credentials,
        })
    }
    async fn execute(
        &self,
        context: &RuntimeContext,
        resource: &agentx_domain::ResourceReference,
        method: Method,
        path: &str,
        body: Value,
        kind: &str,
    ) -> RuntimeResult<Value> {
        self.authorizer.authorize_context(context, resource).await?;
        for dependency in &context.resources {
            if dependency.reference.resource_type.as_str() == "credential" {
                self.authorizer
                    .authorize_context(context, &dependency.reference)
                    .await?;
            }
        }
        let snapshot = &context
            .resource(resource)
            .ok_or_else(|| {
                RuntimeError::new(
                    "RESOURCE_SNAPSHOT_MISSING",
                    format!("{kind} snapshot is missing"),
                )
            })?
            .snapshot;
        let mut base = Url::parse(
            snapshot
                .get("endpoint")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RuntimeError::new(format!("{kind}_SNAPSHOT_INVALID"), "Endpoint is missing")
                })?,
        )
        .map_err(|_| {
            RuntimeError::new(format!("{kind}_SNAPSHOT_INVALID"), "Endpoint is invalid")
        })?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(RuntimeError::new(
                format!("{kind}_SNAPSHOT_INVALID"),
                "Endpoint scheme is unsupported",
            ));
        }
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let url = base.join(path.trim_start_matches('/')).map_err(|_| {
            RuntimeError::new(
                format!("{kind}_SNAPSHOT_INVALID"),
                "Operation path is invalid",
            )
        })?;
        let mut request = self
            .http
            .request(method, url)
            .timeout(Duration::from_secs(60))
            .json(&body);
        request = self.authenticate(context, snapshot, request, kind).await?;
        let response = tokio::select! {_=context.cancellation.cancelled()=>return Err(RuntimeError::new("RUNTIME_CANCELLED",format!("{kind} request was cancelled"))),value=request.send()=>value.map_err(|e|transport(kind,e))?};
        if !response.status().is_success() {
            return Err(RuntimeError::new(
                format!("{kind}_HTTP_STATUS"),
                format!("{kind} returned HTTP {}", response.status().as_u16()),
            )
            .retryable(response.status().is_server_error()));
        }
        let bytes = response.bytes().await.map_err(|e| transport(kind, e))?;
        if bytes.len() > MAX_RESPONSE {
            return Err(RuntimeError::new(
                format!("{kind}_RESPONSE_TOO_LARGE"),
                format!("{kind} response exceeded 8 MiB"),
            ));
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            RuntimeError::new(
                format!("{kind}_PROTOCOL_ERROR"),
                format!("{kind} response is not valid JSON"),
            )
        })
    }
    async fn authenticate(
        &self,
        context: &RuntimeContext,
        snapshot: &Value,
        builder: RequestBuilder,
        kind: &str,
    ) -> RuntimeResult<RequestBuilder> {
        let Some(value) = snapshot.get("credentialId").and_then(Value::as_str) else {
            return Ok(builder);
        };
        let id = uuid::Uuid::parse_str(value).map_err(|_| {
            RuntimeError::new(
                format!("{kind}_SNAPSHOT_INVALID"),
                "Credential ID is invalid",
            )
        })?;
        let version = context
            .resources
            .iter()
            .find(|r| r.reference.resource_id == id)
            .and_then(|r| r.snapshot.get("secretVersion"))
            .and_then(Value::as_u64);
        let credential = if let Some(version) = version {
            self.credentials
                .resolve_version(context.tenant_id, id, version)
                .await
        } else {
            self.credentials.resolve(context.tenant_id, id).await
        }
        .map_err(|e| RuntimeError::new(format!("{kind}_CREDENTIAL_INVALID"), e.to_string()))?;
        let secret = std::str::from_utf8(credential.secret.expose()).map_err(|_| {
            RuntimeError::new(
                format!("{kind}_CREDENTIAL_INVALID"),
                "Credential is not UTF-8",
            )
        })?;
        match credential.credential_type.as_str() {
            "bearer" => Ok(builder.bearer_auth(secret)),
            "api_key" => {
                let configuration = snapshot.get("configuration").and_then(Value::as_object);
                let configured_header = configuration
                    .and_then(|value| value.get("credentialHeader"))
                    .and_then(Value::as_str)
                    .unwrap_or("X-API-Key");
                let header =
                    HeaderName::from_bytes(configured_header.as_bytes()).map_err(|_| {
                        RuntimeError::new(
                            format!("{kind}_SNAPSHOT_INVALID"),
                            "Credential header is invalid",
                        )
                    })?;
                if header != AUTHORIZATION && header.as_str() != "x-api-key" {
                    return Err(RuntimeError::new(
                        format!("{kind}_SNAPSHOT_INVALID"),
                        "Credential header must be Authorization or X-API-Key",
                    ));
                }
                let value = if header == AUTHORIZATION {
                    format!("Bearer {secret}")
                } else {
                    secret.to_owned()
                };
                let value = HeaderValue::from_str(&value).map_err(|_| {
                    RuntimeError::new(
                        format!("{kind}_CREDENTIAL_INVALID"),
                        "Credential contains invalid header bytes",
                    )
                })?;
                Ok(builder.header(header, value))
            }
            _ => Err(RuntimeError::new(
                format!("{kind}_CREDENTIAL_UNSUPPORTED"),
                "Credential type is unsupported",
            )),
        }
    }
}

#[derive(Clone)]
pub struct LightRagRuntime(JsonRuntimeClient);
impl LightRagRuntime {
    pub fn new(
        authorizer: MySqlResourceAuthorizer,
        credentials: Arc<dyn CredentialResolver>,
    ) -> RuntimeResult<Self> {
        JsonRuntimeClient::new(authorizer, credentials).map(Self)
    }
}

#[async_trait]
impl RagRuntime for LightRagRuntime {
    async fn execute(&self, context: &RuntimeContext, request: RagRequest) -> RuntimeResult<Value> {
        let snapshot = &context
            .resource(&request.resource)
            .ok_or_else(|| {
                RuntimeError::new("RESOURCE_SNAPSHOT_MISSING", "RAG snapshot is missing")
            })?
            .snapshot;
        let config = snapshot
            .get("configuration")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if matches!(
            request.operation,
            RagOperation::Insert | RagOperation::Delete
        ) {
            require_write_access(snapshot, request.resource.operation, "RAG")?;
        }
        let (method, key, default) = rag_route(&request.operation);
        let path = config
            .get("paths")
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .unwrap_or(default);
        let input = rag_payload(&request.operation, request.input)?;
        self.0
            .execute(context, &request.resource, method, path, input, "RAG")
            .await
    }
}

#[derive(Clone)]
pub struct Mem0Runtime(JsonRuntimeClient);
impl Mem0Runtime {
    pub fn new(
        authorizer: MySqlResourceAuthorizer,
        credentials: Arc<dyn CredentialResolver>,
    ) -> RuntimeResult<Self> {
        JsonRuntimeClient::new(authorizer, credentials).map(Self)
    }
}

#[async_trait]
impl MemoryRuntime for Mem0Runtime {
    async fn execute(
        &self,
        context: &RuntimeContext,
        request: MemoryRequest,
    ) -> RuntimeResult<Value> {
        let snapshot = &context
            .resource(&request.resource)
            .ok_or_else(|| {
                RuntimeError::new("RESOURCE_SNAPSHOT_MISSING", "Memory snapshot is missing")
            })?
            .snapshot;
        let namespace = snapshot
            .get("externalNamespace")
            .cloned()
            .unwrap_or(Value::Null);
        let config = snapshot
            .get("configuration")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if matches!(
            request.operation,
            MemoryOperation::Add | MemoryOperation::Update | MemoryOperation::Delete
        ) {
            require_write_access(snapshot, request.resource.operation, "MEMORY")?;
        }
        let (method, default_path, body) =
            memory_route(&request.operation, request.input, namespace)?;
        let path = memory_path(&request.operation, &default_path, &config);
        self.0
            .execute(context, &request.resource, method, path, body, "MEMORY")
            .await
    }
}

fn rag_route(operation: &RagOperation) -> (Method, &'static str, &'static str) {
    match operation {
        RagOperation::Query => (Method::POST, "query", "query"),
        RagOperation::Retrieve => (Method::POST, "retrieve", "query"),
        RagOperation::Insert => (Method::POST, "insert", "documents/text"),
        RagOperation::Delete => (Method::DELETE, "delete", "documents/delete_document"),
        RagOperation::HealthCheck => (Method::GET, "health", "health"),
    }
}

fn rag_payload(operation: &RagOperation, input: Value) -> RuntimeResult<Value> {
    let invalid = || {
        RuntimeError::new(
            "RAG_INPUT_INVALID",
            "LightRAG input does not match the fixed operation contract",
        )
    };
    match operation {
        RagOperation::Query | RagOperation::Retrieve => {
            let mut body = match input {
                Value::String(query) => json!({"query":query}),
                Value::Object(value) if value.get("query").and_then(Value::as_str).is_some() => {
                    Value::Object(value)
                }
                _ => return Err(invalid()),
            };
            if matches!(operation, RagOperation::Retrieve) {
                body["only_need_context"] = Value::Bool(true);
            }
            Ok(body)
        }
        RagOperation::Insert => match input {
            Value::Object(value)
                if value
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty()) =>
            {
                Ok(Value::Object(value))
            }
            _ => Err(invalid()),
        },
        RagOperation::Delete => match input {
            Value::Object(value)
                if value
                    .get("doc_ids")
                    .and_then(Value::as_array)
                    .is_some_and(|ids| !ids.is_empty()) =>
            {
                Ok(Value::Object(value))
            }
            _ => Err(invalid()),
        },
        RagOperation::HealthCheck => Ok(Value::Null),
    }
}

fn operation_key(operation: &MemoryOperation) -> &'static str {
    match operation {
        MemoryOperation::Get => "get",
        MemoryOperation::Search => "search",
        MemoryOperation::Add => "add",
        MemoryOperation::Update => "update",
        MemoryOperation::Delete => "delete",
    }
}

fn memory_path<'a>(
    operation: &MemoryOperation,
    default_path: &'a str,
    config: &'a Value,
) -> &'a str {
    let has_dynamic_id = matches!(operation, MemoryOperation::Update | MemoryOperation::Delete)
        || matches!(operation, MemoryOperation::Get) && default_path != "memories";
    if has_dynamic_id {
        return default_path;
    }
    config
        .get("paths")
        .and_then(|value| value.get(operation_key(operation)))
        .and_then(Value::as_str)
        .unwrap_or(default_path)
}

fn memory_route(
    operation: &MemoryOperation,
    input: Value,
    namespace: Value,
) -> RuntimeResult<(Method, String, Value)> {
    let invalid = || {
        RuntimeError::new(
            "MEMORY_INPUT_INVALID",
            "Mem0 input does not match the fixed v2.0.15 operation contract",
        )
    };
    let mut body = match input {
        Value::Object(value) => Value::Object(value),
        _ => return Err(invalid()),
    };
    let object = body.as_object_mut().ok_or_else(invalid)?;
    let namespace = namespace.as_str().unwrap_or_default();
    match operation {
        MemoryOperation::Get => {
            let path = object
                .remove("memoryId")
                .or_else(|| object.remove("id"))
                .and_then(|value| value.as_str().map(str::to_owned))
                .map(|id| -> RuntimeResult<String> {
                    Ok(format!(
                        "memories/{}",
                        memory_id_segment(&id).ok_or_else(invalid)?
                    ))
                })
                .transpose()?
                .unwrap_or_else(|| "memories".to_owned());
            Ok((Method::GET, path, Value::Object(object.clone())))
        }
        MemoryOperation::Search => {
            if object.get("query").and_then(Value::as_str).is_none() {
                return Err(invalid());
            }
            let filters = object.entry("filters").or_insert_with(|| json!({}));
            let filters = filters.as_object_mut().ok_or_else(invalid)?;
            filters
                .entry("user_id")
                .or_insert_with(|| Value::String(namespace.to_owned()));
            Ok((Method::POST, "search".to_owned(), body))
        }
        MemoryOperation::Add => {
            if object.get("messages").and_then(Value::as_array).is_none() {
                return Err(invalid());
            }
            object
                .entry("user_id")
                .or_insert_with(|| Value::String(namespace.to_owned()));
            Ok((Method::POST, "memories".to_owned(), body))
        }
        MemoryOperation::Update | MemoryOperation::Delete => {
            let id = object
                .remove("memoryId")
                .or_else(|| object.remove("id"))
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(invalid)?;
            let path = format!("memories/{}", memory_id_segment(&id).ok_or_else(invalid)?);
            let method = if matches!(operation, MemoryOperation::Update) {
                Method::PUT
            } else {
                Method::DELETE
            };
            Ok((method, path, body))
        }
    }
}

fn memory_id_segment(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(value)
}

fn require_write_access(
    snapshot: &Value,
    operation: ResourceOperation,
    kind: &str,
) -> RuntimeResult<()> {
    if !matches!(
        operation,
        ResourceOperation::Write | ResourceOperation::Manage
    ) {
        return Err(RuntimeError::new(
            format!("{kind}_WRITE_DENIED"),
            format!("{kind} write operation is not granted"),
        ));
    }
    if kind != "MEMORY" || snapshot.get("accessMode").and_then(Value::as_str) == Some("read_write")
    {
        return Ok(());
    }
    Err(RuntimeError::new(
        format!("{kind}_WRITE_DENIED"),
        format!("{kind} resource is read-only"),
    ))
}

fn transport(kind: &str, error: reqwest::Error) -> RuntimeError {
    if error.is_timeout() {
        RuntimeError::new(
            format!("{kind}_TIMEOUT"),
            format!("{kind} request timed out"),
        )
        .retryable(true)
    } else {
        RuntimeError::new(
            format!("{kind}_UNAVAILABLE"),
            format!("{kind} service could not be reached"),
        )
        .retryable(true)
        .outcome_unknown(error.is_request())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_lightrag_and_mem0_routes_match_the_supported_contract() {
        assert_eq!(
            rag_route(&RagOperation::Query),
            (Method::POST, "query", "query")
        );
        assert_eq!(
            rag_route(&RagOperation::Delete),
            (Method::DELETE, "delete", "documents/delete_document")
        );
        let get = memory_route(
            &MemoryOperation::Get,
            json!({"memoryId":"memory-1"}),
            json!("fixture"),
        )
        .unwrap();
        assert_eq!(get.0, Method::GET);
        assert_eq!(get.1, "memories/memory-1");
        assert_eq!(
            memory_route(
                &MemoryOperation::Search,
                json!({"query":"agentx"}),
                json!("fixture")
            )
            .unwrap()
            .1,
            "search"
        );
        assert_eq!(
            memory_route(
                &MemoryOperation::Update,
                json!({"memoryId":"memory-1","text":"updated"}),
                json!("fixture")
            )
            .unwrap()
            .0,
            Method::PUT
        );
        assert_eq!(
            memory_route(
                &MemoryOperation::Delete,
                json!({"id":"memory-1"}),
                json!("fixture")
            )
            .unwrap()
            .0,
            Method::DELETE
        );
        let overrides = json!({"paths":{"get":"custom/get","search":"custom/search","update":"custom/update","delete":"custom/delete"}});
        assert_eq!(
            memory_path(&MemoryOperation::Search, "search", &overrides),
            "custom/search"
        );
        assert_eq!(
            memory_path(&MemoryOperation::Get, "memories/memory-1", &overrides),
            "memories/memory-1"
        );
        assert_eq!(
            memory_path(&MemoryOperation::Update, "memories/memory-1", &overrides),
            "memories/memory-1"
        );
        assert_eq!(
            memory_path(&MemoryOperation::Delete, "memories/memory-1", &overrides),
            "memories/memory-1"
        );
    }

    #[test]
    fn lightrag_payloads_match_v1_5_5_models() {
        assert_eq!(
            rag_payload(&RagOperation::Query, json!("What is Agentx?")).unwrap(),
            json!({"query":"What is Agentx?"})
        );
        assert_eq!(
            rag_payload(
                &RagOperation::Retrieve,
                json!({"query":"What is Agentx?","mode":"naive"})
            )
            .unwrap(),
            json!({"query":"What is Agentx?","mode":"naive","only_need_context":true})
        );
        assert!(
            rag_payload(
                &RagOperation::Insert,
                json!({"text":"Agentx runtime fixture","file_source":"m5.txt"})
            )
            .is_ok()
        );
        assert!(rag_payload(&RagOperation::Insert, json!({"input":"wrong"})).is_err());
        assert!(rag_payload(&RagOperation::Delete, json!({"doc_ids":["doc-1"]})).is_ok());
    }

    #[test]
    fn rag_and_memory_writes_require_write_grants_and_namespace_access() {
        let denied = require_write_access(&json!({}), ResourceOperation::Read, "RAG").unwrap_err();
        assert_eq!(denied.code, "RAG_WRITE_DENIED");
        assert!(require_write_access(&json!({}), ResourceOperation::Write, "RAG").is_ok());
        assert!(
            require_write_access(
                &json!({"accessMode":"read"}),
                ResourceOperation::Write,
                "MEMORY"
            )
            .is_err()
        );
        assert!(
            require_write_access(
                &json!({"accessMode":"read_write"}),
                ResourceOperation::Write,
                "MEMORY"
            )
            .is_ok()
        );
    }
}
