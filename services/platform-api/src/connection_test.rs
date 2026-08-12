use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use axum::Json;
use futures::StreamExt;
use reqwest::RequestBuilder;
use serde::Serialize;
use serde_json::Value;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::audit,
    credentials,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    pub status: String,
    pub latency_ms: Option<u64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub checked_at: time::OffsetDateTime,
}

pub async fn run_http_check(
    state: &AppState,
    actor: &AuthActor,
    resource_type: &str,
    resource_id: Uuid,
    url: &str,
    credential_id: Option<Uuid>,
) -> AppResult<Json<HealthCheckResponse>> {
    run_http_check_mode(
        state,
        actor,
        resource_type,
        resource_id,
        url,
        credential_id,
        false,
        None,
    )
    .await
}

pub async fn run_openai_chat_completions_check(
    state: &AppState,
    actor: &AuthActor,
    resource_type: &str,
    resource_id: Uuid,
    url: &str,
    credential_id: Option<Uuid>,
    model_name: &str,
) -> AppResult<Json<HealthCheckResponse>> {
    run_http_check_mode(
        state,
        actor,
        resource_type,
        resource_id,
        url,
        credential_id,
        false,
        Some(model_probe_body(model_name)),
    )
    .await
}

async fn run_http_check_mode(
    state: &AppState,
    actor: &AuthActor,
    resource_type: &str,
    resource_id: Uuid,
    url: &str,
    credential_id: Option<Uuid>,
    validate_openapi: bool,
    body: Option<Value>,
) -> AppResult<Json<HealthCheckResponse>> {
    let _permit = state.connection_permit(actor.tenant_id).await;
    let parsed = validate_target(state, url).await?;
    let sequence_started_at = time::OffsetDateTime::now_utc();
    let sequence = u64::try_from(sequence_started_at.unix_timestamp_nanos()).unwrap_or(u64::MAX);
    let started = Instant::now();
    let mut request = match body {
        Some(body) => state.http.post(parsed).json(&body),
        None => state.http.get(parsed),
    }
    .timeout(Duration::from_secs(state.connections.timeout_seconds));
    if let Some(id) = credential_id {
        request = authorize(
            request,
            credentials::resolve(state, actor.tenant_id, id).await?,
        )?;
    }
    let result = request.send().await;
    let checked_at = time::OffsetDateTime::now_utc();
    let (status, latency, error_code, error_message) = match result {
        Ok(response) if response.status().is_success() => {
            if validate_openapi {
                match read_openapi_document(response).await {
                    Ok(()) => (
                        "healthy".to_owned(),
                        Some(started.elapsed().as_millis() as u64),
                        None,
                        None,
                    ),
                    Err((code, message)) => (
                        "unhealthy".to_owned(),
                        Some(started.elapsed().as_millis() as u64),
                        Some(code.to_owned()),
                        Some(message.to_owned()),
                    ),
                }
            } else {
                (
                    "healthy".to_owned(),
                    Some(started.elapsed().as_millis() as u64),
                    None,
                    None,
                )
            }
        }
        Ok(response) => (
            "unhealthy".to_owned(),
            Some(started.elapsed().as_millis() as u64),
            Some("HTTP_STATUS".to_owned()),
            Some(format!(
                "Remote service returned HTTP {}",
                response.status().as_u16()
            )),
        ),
        Err(error) => (
            "unhealthy".to_owned(),
            Some(started.elapsed().as_millis() as u64),
            Some(
                if error.is_timeout() {
                    "CONNECTION_TIMEOUT"
                } else {
                    "CONNECTION_FAILED"
                }
                .to_owned(),
            ),
            Some(if error.is_timeout() {
                "Connection test timed out".to_owned()
            } else {
                "Remote service could not be reached".to_owned()
            }),
        ),
    };
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO resource_health_checks(id,tenant_id,resource_type,resource_id,check_sequence,status,latency_ms,error_code,error_message,checked_by,checked_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)").bind(Uuid::now_v7()).bind(actor.tenant_id).bind(resource_type).bind(resource_id).bind(sequence).bind(&status).bind(latency).bind(&error_code).bind(&error_message).bind(actor.user_id).bind(checked_at).execute(&mut *tx).await?;
    audit(&mut tx,actor,"resource.connection_tested",resource_type,resource_id,serde_json::json!({"status":status,"errorCode":error_code,"latencyMs":latency,"checkSequence":sequence})).await?;
    tx.commit().await?;
    Ok(Json(HealthCheckResponse {
        status,
        latency_ms: latency,
        error_code,
        error_message,
        checked_at,
    }))
}

fn model_probe_body(model_name: &str) -> Value {
    serde_json::json!({
        "model": model_name,
        "messages": [{"role": "user", "content": "Reply with OK."}],
        "max_tokens": 1,
        "stream": false
    })
}

async fn read_openapi_document(
    response: reqwest::Response,
) -> Result<(), (&'static str, &'static str)> {
    const MAX_DOCUMENT_BYTES: usize = 5 * 1024 * 1024;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| ("OPENAPI_READ_FAILED", "OpenAPI document could not be read"))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_DOCUMENT_BYTES {
            return Err((
                "OPENAPI_DOCUMENT_TOO_LARGE",
                "OpenAPI document exceeds 5 MiB",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let document: Value = serde_json::from_slice(&bytes)
        .or_else(|_| serde_yaml::from_slice(&bytes))
        .map_err(|_| {
            (
                "OPENAPI_INVALID",
                "Response is not valid OpenAPI JSON or YAML",
            )
        })?;
    let object = document
        .as_object()
        .ok_or(("OPENAPI_INVALID", "OpenAPI document must be an object"))?;
    if !object.contains_key("openapi") && !object.contains_key("swagger") {
        return Err((
            "OPENAPI_INVALID",
            "Document does not declare an OpenAPI or Swagger version",
        ));
    }
    Ok(())
}

pub(crate) async fn validate_target(state: &AppState, value: &str) -> AppResult<Url> {
    let url = Url::parse(value)
        .map_err(|_| AppError::bad_request("INVALID_ENDPOINT", "Endpoint is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::bad_request(
            "INVALID_ENDPOINT",
            "Only HTTP and HTTPS endpoints are supported",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::bad_request("INVALID_ENDPOINT", "Endpoint requires a host"))?
        .to_ascii_lowercase();
    if state
        .connections
        .allowed_hosts
        .iter()
        .any(|allowed| allowed == &host)
    {
        return Ok(url);
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| {
            AppError::bad_request("ENDPOINT_DNS_FAILED", "Endpoint host cannot be resolved")
        })?;
    for address in addresses {
        let ip = address.ip();
        let explicitly_allowed = state
            .connections
            .allowed_cidrs
            .iter()
            .any(|network| network.contains(&ip));
        if !explicitly_allowed
            && (always_blocked(ip) || (blocked(ip) && !state.connections.allow_private_networks))
        {
            return Err(AppError::forbidden(
                "Endpoint resolves to a blocked network address",
            ));
        }
    }
    Ok(url)
}
fn always_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast()
        }
        IpAddr::V6(ip) => ip.is_unicast_link_local() || ip.is_unspecified() || ip.is_multicast(),
    }
}
fn blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    }
}
pub(crate) fn authorize(
    mut request: RequestBuilder,
    credential: credentials::ResolvedCredential,
) -> AppResult<RequestBuilder> {
    let value: Value = serde_json::from_slice(credential.secret.expose()).map_err(|_| {
        AppError::service_unavailable("CREDENTIAL_FORMAT_INVALID", "Credential content is invalid")
    })?;
    request = match credential.credential_type.as_str() {
        "api_key" => request.header("x-api-key", secret_string(&value)?),
        "bearer" => request.bearer_auth(secret_string(&value)?),
        "basic" => {
            let username = value
                .get("username")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::service_unavailable(
                        "CREDENTIAL_FORMAT_INVALID",
                        "Basic credential requires username",
                    )
                })?;
            let password = value
                .get("password")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::service_unavailable(
                        "CREDENTIAL_FORMAT_INVALID",
                        "Basic credential requires password",
                    )
                })?;
            request.basic_auth(username, Some(password))
        }
        "custom_json" => request,
        _ => {
            return Err(AppError::service_unavailable(
                "CREDENTIAL_FORMAT_INVALID",
                "Credential type is unsupported",
            ));
        }
    };
    Ok(request)
}
fn secret_string(value: &Value) -> AppResult<&str> {
    value
        .as_str()
        .or_else(|| value.get("value").and_then(Value::as_str))
        .ok_or_else(|| {
            AppError::service_unavailable(
                "CREDENTIAL_FORMAT_INVALID",
                "Credential requires a string value",
            )
        })
}

#[cfg(test)]
mod tests {
    use axum::{Router, response::IntoResponse, routing::get};

    use super::{always_blocked, blocked, model_probe_body, read_openapi_document};

    async fn response(body: &'static str) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake server");
        let address = listener.local_addr().expect("fake server address");
        let app = Router::new().route(
            "/",
            get(move || async move { ([("content-type", "text/plain")], body).into_response() }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fake response");
        });
        reqwest::get(format!("http://{address}/"))
            .await
            .expect("fetch fake response")
    }

    #[test]
    fn blocks_local_private_and_metadata_networks() {
        assert!(blocked("127.0.0.1".parse().unwrap()));
        assert!(blocked("10.0.0.1".parse().unwrap()));
        assert!(blocked("169.254.169.254".parse().unwrap()));
        assert!(blocked("::1".parse().unwrap()));
        assert!(!blocked("1.1.1.1".parse().unwrap()));
        assert!(always_blocked("169.254.169.254".parse().unwrap()));
        assert!(always_blocked("224.0.0.1".parse().unwrap()));
        assert!(!always_blocked("10.0.0.1".parse().unwrap()));
        assert!(!always_blocked("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn model_probe_uses_openai_chat_completions_shape() {
        assert_eq!(
            model_probe_body("upstream-model"),
            serde_json::json!({
                "model": "upstream-model",
                "messages": [{"role": "user", "content": "Reply with OK."}],
                "max_tokens": 1,
                "stream": false
            })
        );
    }

    #[tokio::test]
    async fn accepts_json_and_yaml_openapi_documents() {
        assert!(
            read_openapi_document(response(r#"{"openapi":"3.1.0","paths":{}}"#).await)
                .await
                .is_ok()
        );
        assert!(
            read_openapi_document(response("swagger: '2.0'\npaths: {}\n").await)
                .await
                .is_ok()
        );
        assert_eq!(
            read_openapi_document(response("not: [valid").await)
                .await
                .unwrap_err()
                .0,
            "OPENAPI_INVALID"
        );
    }
}
