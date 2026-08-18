use std::{env, fmt, sync::Arc, time::Duration};

use agentx_runtime_contracts::{
    EGRESS_RUNTIME_TOKEN_TTL_SECONDS, EGRESS_TOKEN_AUDIENCE, EGRESS_TOKEN_ISSUER,
    EgressConnectClaimsV1, EgressMode, EgressRole, issue_egress_connect_token, now_unix,
};
use anyhow::{Context, Result, bail};
use reqwest::{
    Method, Response, Url,
    header::{
        AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderName, HeaderValue,
        TRANSFER_ENCODING,
    },
};
use serde::Serialize;
use uuid::Uuid;

const MAX_PROVIDER_REDIRECTS: usize = 5;
const MAX_PROVIDER_CONNECT_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct EgressRequestContext {
    pub tenant_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
}

impl EgressRequestContext {
    #[must_use]
    pub const fn request(tenant_id: Uuid, request_id: Uuid) -> Self {
        Self {
            tenant_id,
            execution_id: None,
            request_id: Some(request_id),
        }
    }

    #[must_use]
    pub const fn execution(tenant_id: Uuid, execution_id: Uuid) -> Self {
        Self {
            tenant_id,
            execution_id: Some(execution_id),
            request_id: None,
        }
    }
}

#[derive(Clone)]
pub struct ProviderHttpClient {
    role: EgressRole,
    proxy_url: Url,
    key_id: Arc<str>,
    private_key_pem: Arc<Vec<u8>>,
    direct: reqwest::Client,
}

pub struct ProviderRequestBuilder {
    provider: ProviderHttpClient,
    context: EgressRequestContext,
    timeout: Duration,
    inner: reqwest::RequestBuilder,
}

#[derive(Debug)]
pub enum ProviderRequestError {
    Request(reqwest::Error),
    Policy(anyhow::Error),
    Timeout,
    RedirectLimit,
    UncloneableRedirect,
}

impl ProviderRequestError {
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout) || matches!(self, Self::Request(error) if error.is_timeout())
    }

    #[must_use]
    pub fn is_connect(&self) -> bool {
        matches!(self, Self::Request(error) if error.is_connect())
    }
}

impl fmt::Display for ProviderRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => error.fmt(formatter),
            Self::Policy(error) => write!(formatter, "provider redirect was denied: {error}"),
            Self::Timeout => formatter.write_str("provider request timed out"),
            Self::RedirectLimit => formatter.write_str("provider redirect limit exceeded"),
            Self::UncloneableRedirect => {
                formatter.write_str("provider redirect cannot replay a streaming request body")
            }
        }
    }
}

impl std::error::Error for ProviderRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Policy(error) => Some(error.as_ref()),
            Self::Timeout | Self::RedirectLimit | Self::UncloneableRedirect => None,
        }
    }
}

impl ProviderRequestBuilder {
    #[must_use]
    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<axum::http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<axum::http::Error>,
    {
        self.inner = self.inner.header(key, value);
        self
    }

    #[must_use]
    pub fn bearer_auth<T>(mut self, token: T) -> Self
    where
        T: fmt::Display,
    {
        self.inner = self.inner.bearer_auth(token);
        self
    }

    #[must_use]
    pub fn json<T: Serialize + ?Sized>(mut self, json: &T) -> Self {
        self.inner = self.inner.json(json);
        self
    }

    pub async fn send(self) -> Result<Response, ProviderRequestError> {
        let started = tokio::time::Instant::now();
        let mut request = self.inner.build().map_err(ProviderRequestError::Request)?;
        for redirect_count in 0..=MAX_PROVIDER_REDIRECTS {
            let redirect_replay = request
                .try_clone()
                .ok_or(ProviderRequestError::UncloneableRedirect)?;
            let mut connect_attempt = 0;
            let response = loop {
                connect_attempt += 1;
                let elapsed = started.elapsed();
                let remaining = self
                    .timeout
                    .checked_sub(elapsed)
                    .ok_or(ProviderRequestError::Timeout)?;
                *request.timeout_mut() = Some(remaining);
                let connect_replay = request
                    .try_clone()
                    .ok_or(ProviderRequestError::UncloneableRedirect)?;
                let client = self
                    .provider
                    .client_for_url(request.url(), self.context)
                    .map_err(ProviderRequestError::Policy)?;
                match tokio::time::timeout(remaining, client.execute(request)).await {
                    Ok(Ok(response)) => break response,
                    Ok(Err(error))
                        if error.is_connect()
                            && connect_attempt < MAX_PROVIDER_CONNECT_ATTEMPTS =>
                    {
                        request = connect_replay;
                    }
                    Ok(Err(error)) => return Err(ProviderRequestError::Request(error)),
                    Err(_) => return Err(ProviderRequestError::Timeout),
                }
            };
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Ok(response);
            };
            if redirect_count == MAX_PROVIDER_REDIRECTS {
                return Err(ProviderRequestError::RedirectLimit);
            }
            let location = location.to_str().map_err(|error| {
                ProviderRequestError::Policy(anyhow::anyhow!("invalid redirect Location: {error}"))
            })?;
            request = prepare_redirect_request(
                redirect_replay,
                response.url(),
                response.status(),
                location,
            )?;
        }
        Err(ProviderRequestError::RedirectLimit)
    }
}

fn prepare_redirect_request(
    mut request: reqwest::Request,
    response_url: &Url,
    status: reqwest::StatusCode,
    location: &str,
) -> Result<reqwest::Request, ProviderRequestError> {
    let next_url = response_url
        .join(location)
        .context("invalid provider redirect URL")
        .and_then(|url| validate_provider_url(url.as_str()))
        .map_err(ProviderRequestError::Policy)?;
    let cross_origin = origin(&next_url) != origin(request.url());
    *request.url_mut() = next_url;
    if cross_origin {
        request.headers_mut().remove(AUTHORIZATION);
        request.headers_mut().remove(COOKIE);
    }
    if status == reqwest::StatusCode::SEE_OTHER
        || ((status == reqwest::StatusCode::MOVED_PERMANENTLY
            || status == reqwest::StatusCode::FOUND)
            && request.method() == Method::POST)
    {
        *request.method_mut() = Method::GET;
        *request.body_mut() = None;
        request.headers_mut().remove(CONTENT_LENGTH);
        request.headers_mut().remove(CONTENT_TYPE);
        request.headers_mut().remove(TRANSFER_ENCODING);
    }
    Ok(request)
}

impl ProviderHttpClient {
    pub fn from_env(role: EgressRole) -> Result<Self> {
        let proxy_url = env::var("AGENTX_EGRESS_PROXY_URL")
            .context("AGENTX_EGRESS_PROXY_URL is required")?
            .parse::<Url>()
            .context("AGENTX_EGRESS_PROXY_URL is invalid")?;
        if proxy_url.scheme() != "http" || proxy_url.host_str().is_none() {
            bail!("AGENTX_EGRESS_PROXY_URL must be an internal HTTP proxy URL");
        }
        let key_id =
            env::var("AGENTX_EGRESS_JWT_KEY_ID").context("AGENTX_EGRESS_JWT_KEY_ID is required")?;
        if key_id.trim().is_empty() {
            bail!("AGENTX_EGRESS_JWT_KEY_ID cannot be empty");
        }
        let private_key_pem = env::var("AGENTX_EGRESS_JWT_PRIVATE_KEY_PEM")
            .context("AGENTX_EGRESS_JWT_PRIVATE_KEY_PEM is required")?
            .into_bytes();
        let direct = provider_builder()?
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            role,
            proxy_url,
            key_id: key_id.into(),
            private_key_pem: Arc::new(private_key_pem),
            direct,
        })
    }

    pub fn get(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: Duration,
    ) -> Result<ProviderRequestBuilder> {
        self.request(Method::GET, endpoint, context, timeout)
    }

    pub fn post(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: Duration,
    ) -> Result<ProviderRequestBuilder> {
        self.request(Method::POST, endpoint, context, timeout)
    }

    pub fn request(
        &self,
        method: Method,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: Duration,
    ) -> Result<ProviderRequestBuilder> {
        let url = validate_provider_url(endpoint)?;
        let inner = self.direct.request(method, url).timeout(timeout);
        Ok(ProviderRequestBuilder {
            provider: self.clone(),
            context,
            timeout,
            inner,
        })
    }

    fn client_for_url(&self, url: &Url, context: EgressRequestContext) -> Result<reqwest::Client> {
        if is_managed_cluster_fixture(url) {
            return Ok(self.direct.clone());
        }
        let host = url
            .host_str()
            .context("provider endpoint host is required")?
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let port = url
            .port_or_known_default()
            .context("provider port is required")?;
        let now = now_unix();
        let claims = EgressConnectClaimsV1 {
            iss: EGRESS_TOKEN_ISSUER.into(),
            aud: EGRESS_TOKEN_AUDIENCE.into(),
            role: self.role,
            tenant_id: context.tenant_id,
            execution_id: context.execution_id,
            request_id: context.request_id,
            egress_mode: EgressMode::PublicHttps,
            target_host: host,
            target_port: port,
            iat: now,
            exp: now + EGRESS_RUNTIME_TOKEN_TTL_SECONDS,
            jti: Uuid::now_v7(),
        };
        let token = issue_egress_connect_token(&self.key_id, &self.private_key_pem, &claims)
            .context("failed signing egress CONNECT token")?;
        let proxy_auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .context("egress CONNECT token is not a valid header")?;
        let proxy = reqwest::Proxy::all(self.proxy_url.clone())?.custom_http_auth(proxy_auth);
        let client = provider_builder()?
            .proxy(proxy)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(client)
    }
}

fn origin(url: &Url) -> (&str, Option<&str>, Option<u16>) {
    (url.scheme(), url.host_str(), url.port_or_known_default())
}

pub fn validate_provider_url(endpoint: &str) -> Result<Url> {
    let url = Url::parse(endpoint).context("provider endpoint is invalid")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("provider endpoint must be an HTTP(S) URL with a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("provider endpoint userinfo is forbidden");
    }
    if url.fragment().is_some() {
        bail!("provider endpoint fragment is forbidden");
    }
    if url.scheme() == "http" && !is_managed_cluster_fixture(&url) {
        bail!("public provider endpoints must use HTTPS");
    }
    Ok(url)
}

fn is_managed_cluster_fixture(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "http"
        && (host.ends_with(".svc")
            || host.ends_with(".svc.cluster.local")
            || host == "echo-mcp"
            || host == "lightrag"
            || host == "mem0")
}

fn provider_builder() -> Result<reqwest::ClientBuilder> {
    agentx_service_kit::reqwest_client_builder_with_ca("AGENTX_RUNTIME_PROVIDER_TLS_CA_PATH")
}

#[cfg(test)]
mod tests {
    use reqwest::{Method, StatusCode, header};

    use super::{prepare_redirect_request, validate_provider_url};

    #[test]
    fn public_endpoints_require_https_and_forbid_userinfo() {
        assert!(validate_provider_url("https://api.example.com/v1").is_ok());
        assert!(validate_provider_url("http://api.example.com/v1").is_err());
        assert!(validate_provider_url("https://user:password@api.example.com/v1").is_err());
        assert!(validate_provider_url("https://api.example.com/v1#secret").is_err());
    }

    #[test]
    fn managed_cluster_fixtures_keep_their_http_path() {
        assert!(
            validate_provider_url("http://echo-mcp.agentx-v2-deps.svc.cluster.local:8090/mcp")
                .is_ok()
        );
        assert!(validate_provider_url("http://10.0.0.1:8090/mcp").is_err());
    }

    #[test]
    fn cross_origin_redirect_revalidates_and_drops_credentials() {
        let request = reqwest::Client::new()
            .post("https://provider.example/v1/chat")
            .bearer_auth("provider-secret")
            .header(header::COOKIE, "session=secret")
            .json(&serde_json::json!({"prompt":"hello"}))
            .build()
            .unwrap();
        let redirected = prepare_redirect_request(
            request,
            &"https://provider.example/v1/chat".parse().unwrap(),
            StatusCode::FOUND,
            "https://other.example/v2/chat",
        )
        .unwrap();
        assert_eq!(redirected.url().host_str(), Some("other.example"));
        assert_eq!(redirected.method(), Method::GET);
        assert!(!redirected.headers().contains_key(header::AUTHORIZATION));
        assert!(!redirected.headers().contains_key(header::COOKIE));
        assert!(redirected.body().is_none());
    }

    #[test]
    fn same_origin_temporary_redirect_preserves_request_semantics() {
        let request = reqwest::Client::new()
            .post("https://provider.example/v1/chat")
            .bearer_auth("provider-secret")
            .body("payload")
            .build()
            .unwrap();
        let redirected = prepare_redirect_request(
            request,
            &"https://provider.example/v1/chat".parse().unwrap(),
            StatusCode::TEMPORARY_REDIRECT,
            "/v2/chat",
        )
        .unwrap();
        assert_eq!(redirected.url().path(), "/v2/chat");
        assert_eq!(redirected.method(), Method::POST);
        assert!(redirected.headers().contains_key(header::AUTHORIZATION));
        assert!(redirected.body().is_some());
    }
}
