use std::env;

use agentx_node_protocol::{NodeProtocolError, ProviderOption, ProviderRequest, ProviderResponse};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    routing::post,
};
use serde_json::json;

#[derive(Clone)]
struct EchoState {
    auth_token: Option<String>,
}

type ProtocolResponse<T> = Result<Json<T>, (StatusCode, Json<NodeProtocolError>)>;

#[tokio::main]
async fn main() -> Result<()> {
    let state = EchoState {
        auth_token: env::var("AGENTX_NODE_PROVIDER_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.is_empty()),
    };
    let router = Router::new()
        .route(
            "/agentx/node/v1/providers/{provider}/invoke",
            post(provider),
        )
        .with_state(state);
    agentx_service_kit::serve("echo-node", router, Default::default()).await
}

async fn provider(
    State(state): State<EchoState>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
    Json(request): Json<ProviderRequest>,
) -> ProtocolResponse<ProviderResponse> {
    authorize(&state, &headers)?;
    validate_protocol(&request.protocol_version)?;
    if provider != request.provider {
        return Err(protocol_error(
            StatusCode::BAD_REQUEST,
            "NODE_PROVIDER_MISMATCH",
            "The provider path does not match the request",
        ));
    }
    Ok(Json(ProviderResponse {
        options: vec![ProviderOption {
            label: format!("{provider}:{}", request.operation),
            value: json!("echo"),
            metadata: json!({"protocolVersion":request.protocol_version}),
        }],
        next_cursor: None,
    }))
}

fn authorize(
    state: &EchoState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<NodeProtocolError>)> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(());
    };
    let actual = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    if constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(protocol_error(
            StatusCode::UNAUTHORIZED,
            "NODE_AUTHENTICATION_FAILED",
            "A valid node service bearer token is required",
        ))
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    let size = left.len().max(right.len());
    for index in 0..size {
        different |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    different == 0
}

fn validate_protocol(version: &str) -> Result<(), (StatusCode, Json<NodeProtocolError>)> {
    if version == agentx_node_protocol::NODE_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(protocol_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_PROTOCOL_VERSION_UNSUPPORTED",
            "The requested node protocol version is not supported",
        ))
    }
}

fn protocol_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<NodeProtocolError>) {
    (
        status,
        Json(NodeProtocolError {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: json!({}),
        }),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use serde_json::json;

    use super::*;

    fn provider_request() -> ProviderRequest {
        ProviderRequest {
            protocol_version: agentx_node_protocol::NODE_PROTOCOL_VERSION.into(),
            provider: "echo".into(),
            operation: "options".into(),
            parameters: json!({}),
            credential_handles: vec![],
        }
    }

    #[tokio::test]
    async fn provider_invoke_returns_one_echo_option() {
        let response = provider(
            State(EchoState { auth_token: None }),
            HeaderMap::new(),
            AxumPath("echo".into()),
            Json(provider_request()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response.options.len(), 1);
        assert_eq!(response.options[0].value, json!("echo"));
        assert_eq!(response.next_cursor, None);
    }

    #[tokio::test]
    async fn provider_authentication_and_protocol_are_enforced() {
        let state = EchoState {
            auth_token: Some("secret".into()),
        };
        let unauthorized = provider(
            State(state.clone()),
            HeaderMap::new(),
            AxumPath("echo".into()),
            Json(provider_request()),
        )
        .await
        .unwrap_err();
        assert_eq!(unauthorized.0, StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer secret".parse().unwrap());
        let mut invalid = provider_request();
        invalid.protocol_version = "0.9".into();
        let rejected = provider(
            State(state),
            headers,
            AxumPath("echo".into()),
            Json(invalid),
        )
        .await
        .unwrap_err();
        assert_eq!(rejected.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(rejected.1.0.code, "NODE_PROTOCOL_VERSION_UNSUPPORTED");
    }

    #[test]
    fn constant_time_comparison_covers_different_lengths() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secrex"));
        assert!(!constant_time_eq(b"secret", b"secret-long"));
    }
}
