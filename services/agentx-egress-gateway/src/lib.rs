use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env,
    io::BufReader,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use agentx_runtime_contracts::{EgressConnectClaimsV1, EgressRole, verify_egress_connect_token};
use anyhow::{Context as _, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    Method, Request, Response, StatusCode,
    body::Incoming,
    header::{self, HeaderValue},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use ipnet::IpNet;
use rustls::{ServerConfig, pki_types::PrivateKeyDer};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListenerKind {
    Runtime,
    Sandbox,
}

impl ListenerKind {
    fn allowed_roles(self) -> BTreeSet<EgressRole> {
        match self {
            Self::Runtime => BTreeSet::from([
                EgressRole::RuntimeGateway,
                EgressRole::WorkflowRuntime,
                EgressRole::WorkflowWorker,
            ]),
            Self::Sandbox => BTreeSet::from([EgressRole::Sandbox]),
        }
    }
}

#[derive(Clone)]
pub struct GatewayState {
    trusted_keys: Arc<HashMap<String, Vec<u8>>>,
    allowed_ports: Arc<BTreeSet<u16>>,
    blocked_networks: Arc<Vec<IpNet>>,
    docker_desktop_dns: bool,
    used_tokens: Arc<Mutex<HashMap<Uuid, i64>>>,
    sandbox_tunnels: Arc<Semaphore>,
    sandbox_token_usage: Arc<Mutex<HashMap<Uuid, SandboxTokenUsage>>>,
    sandbox_budget: SandboxBudget,
    metrics: agentx_service_kit::MetricsRegistry,
}

#[derive(Clone, Copy)]
struct SandboxBudget {
    max_concurrent_tunnels: usize,
    max_connections: u32,
    max_total_duration: Duration,
}

struct SandboxTokenUsage {
    expires_at: i64,
    connections: u32,
    accumulated_duration: Duration,
    active: HashMap<Uuid, Instant>,
}

struct SandboxTokenLease {
    token_id: Uuid,
    lease_id: Uuid,
    usage: Arc<Mutex<HashMap<Uuid, SandboxTokenUsage>>>,
}

impl Drop for SandboxTokenLease {
    fn drop(&mut self) {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = usage.get_mut(&self.token_id)
            && let Some(started) = token.active.remove(&self.lease_id)
        {
            token.accumulated_duration =
                token.accumulated_duration.saturating_add(started.elapsed());
        }
    }
}

impl GatewayState {
    pub fn from_env(metrics: agentx_service_kit::MetricsRegistry) -> Result<Self> {
        let trusted_keys = serde_json::from_str::<HashMap<String, String>>(
            &env::var("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON")
                .context("AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON is required")?,
        )?
        .into_iter()
        .map(|(kid, pem)| (kid, pem.into_bytes()))
        .collect();
        let allowed_ports = parse_allowed_ports(
            &env::var("AGENTX_EGRESS_ALLOWED_PUBLIC_PORTS").unwrap_or_else(|_| "443".into()),
        )?;
        let blocked_networks = env::var("AGENTX_EGRESS_BLOCKED_CIDRS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse::<IpNet>()
                    .with_context(|| format!("invalid blocked CIDR {value}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let docker_desktop_dns = env::var("AGENTX_EGRESS_ALLOW_DOCKER_DESKTOP_DNS")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let max_sandbox_tunnels = env::var("AGENTX_EGRESS_MAX_SANDBOX_TUNNELS")
            .unwrap_or_else(|_| "64".into())
            .parse::<usize>()
            .context("AGENTX_EGRESS_MAX_SANDBOX_TUNNELS must be a positive integer")?;
        if max_sandbox_tunnels == 0 || max_sandbox_tunnels > 10_000 {
            bail!("AGENTX_EGRESS_MAX_SANDBOX_TUNNELS must be in 1..=10000");
        }
        let sandbox_budget = SandboxBudget {
            max_concurrent_tunnels: bounded_env_usize(
                "AGENTX_EGRESS_SANDBOX_TOKEN_MAX_CONCURRENT_TUNNELS",
                4,
                1,
                64,
            )?,
            max_connections: bounded_env_u32(
                "AGENTX_EGRESS_SANDBOX_TOKEN_MAX_CONNECTIONS",
                32,
                1,
                10_000,
            )?,
            max_total_duration: Duration::from_secs(u64::from(bounded_env_u32(
                "AGENTX_EGRESS_SANDBOX_TOKEN_MAX_TOTAL_SECONDS",
                3_600,
                1,
                3_600,
            )?)),
        };
        Ok(Self {
            trusted_keys: Arc::new(trusted_keys),
            allowed_ports: Arc::new(allowed_ports),
            blocked_networks: Arc::new(blocked_networks),
            docker_desktop_dns,
            used_tokens: Arc::new(Mutex::new(HashMap::new())),
            sandbox_tunnels: Arc::new(Semaphore::new(max_sandbox_tunnels)),
            sandbox_token_usage: Arc::new(Mutex::new(HashMap::new())),
            sandbox_budget,
            metrics,
        })
    }

    #[cfg(test)]
    fn for_test(allowed_ports: BTreeSet<u16>, docker_desktop_dns: bool) -> Self {
        Self {
            trusted_keys: Arc::new(HashMap::new()),
            allowed_ports: Arc::new(allowed_ports),
            blocked_networks: Arc::new(Vec::new()),
            docker_desktop_dns,
            used_tokens: Arc::new(Mutex::new(HashMap::new())),
            sandbox_tunnels: Arc::new(Semaphore::new(64)),
            sandbox_token_usage: Arc::new(Mutex::new(HashMap::new())),
            sandbox_budget: SandboxBudget {
                max_concurrent_tunnels: 4,
                max_connections: 32,
                max_total_duration: Duration::from_secs(3_600),
            },
            metrics: agentx_service_kit::MetricsRegistry::default(),
        }
    }

    fn consume_runtime_token(&self, claims: &EgressConnectClaimsV1) -> Result<()> {
        if claims.role == EgressRole::Sandbox {
            bail!("Sandbox tokens use the bounded reusable-token contract");
        }
        let now = agentx_runtime_contracts::now_unix();
        let mut used = self
            .used_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        used.retain(|_, exp| *exp >= now);
        if used.insert(claims.jti, claims.exp).is_some() {
            bail!("egress token was already used");
        }
        Ok(())
    }

    fn acquire_sandbox_token(&self, claims: &EgressConnectClaimsV1) -> Result<SandboxTokenLease> {
        if claims.role != EgressRole::Sandbox {
            bail!("only Sandbox tokens can acquire a Sandbox token budget");
        }
        let now = agentx_runtime_contracts::now_unix();
        let mut usage = self
            .sandbox_token_usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        usage.retain(|_, token| token.expires_at >= now || !token.active.is_empty());
        let token = usage
            .entry(claims.jti)
            .or_insert_with(|| SandboxTokenUsage {
                expires_at: claims.exp,
                connections: 0,
                accumulated_duration: Duration::ZERO,
                active: HashMap::new(),
            });
        if token.expires_at != claims.exp {
            bail!("Sandbox token identifier was reused with different claims");
        }
        if token.connections >= self.sandbox_budget.max_connections {
            bail!("Sandbox egress token connection budget exhausted");
        }
        if token.active.len() >= self.sandbox_budget.max_concurrent_tunnels {
            bail!("Sandbox egress token concurrency limit reached");
        }
        let elapsed = token
            .active
            .values()
            .fold(token.accumulated_duration, |total, started| {
                total.saturating_add(started.elapsed())
            });
        if elapsed >= self.sandbox_budget.max_total_duration {
            bail!("Sandbox egress token total duration budget exhausted");
        }
        let lease_id = Uuid::now_v7();
        token.connections = token.connections.saturating_add(1);
        token.active.insert(lease_id, Instant::now());
        Ok(SandboxTokenLease {
            token_id: claims.jti,
            lease_id,
            usage: self.sandbox_token_usage.clone(),
        })
    }

    fn sandbox_duration_exhausted(&self, token_id: Uuid) -> bool {
        let usage = self
            .sandbox_token_usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        usage.get(&token_id).is_none_or(|token| {
            token
                .active
                .values()
                .fold(token.accumulated_duration, |total, started| {
                    total.saturating_add(started.elapsed())
                })
                >= self.sandbox_budget.max_total_duration
        })
    }
}

fn bounded_env_usize(name: &str, default: usize, minimum: usize, maximum: usize) -> Result<usize> {
    let value = env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse::<usize>()
        .with_context(|| format!("{name} must be an integer"))?;
    if !(minimum..=maximum).contains(&value) {
        bail!("{name} must be in {minimum}..={maximum}");
    }
    Ok(value)
}

fn bounded_env_u32(name: &str, default: u32, minimum: u32, maximum: u32) -> Result<u32> {
    let value = env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u32>()
        .with_context(|| format!("{name} must be an integer"))?;
    if !(minimum..=maximum).contains(&value) {
        bail!("{name} must be in {minimum}..={maximum}");
    }
    Ok(value)
}

pub fn parse_allowed_ports(value: &str) -> Result<BTreeSet<u16>> {
    let ports = value
        .trim_matches(['[', ']'])
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<u16>()
                .with_context(|| format!("invalid public HTTPS port {value}"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if !ports.contains(&443) {
        bail!("allowed public ports must contain 443");
    }
    if ports.len() > 16 || ports.contains(&0) {
        bail!("allowed public ports must contain at most 16 explicit non-zero ports");
    }
    Ok(ports)
}

pub async fn serve_plain(
    address: SocketAddr,
    state: GatewayState,
    kind: ListenerKind,
    lifecycle: agentx_service_kit::ServiceLifecycle,
) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed binding egress proxy {address}"))?;
    tracing::info!(%address, ?kind, "egress proxy listener started");
    loop {
        tokio::select! {
            () = lifecycle.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let state = state.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| proxy_request(request, state.clone(), kind));
                    if let Err(error) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await
                    {
                        tracing::debug!(%error, %peer, "egress proxy connection closed");
                    }
                });
            }
        }
    }
}

pub async fn serve_tls(
    address: SocketAddr,
    state: GatewayState,
    kind: ListenerKind,
    lifecycle: agentx_service_kit::ServiceLifecycle,
    tls_config: Arc<ServerConfig>,
) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed binding TLS egress proxy {address}"))?;
    let acceptor = TlsAcceptor::from(tls_config);
    tracing::info!(%address, ?kind, "TLS egress proxy listener started");
    loop {
        tokio::select! {
            () = lifecycle.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let state = state.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let result = async {
                        let stream = acceptor.accept(stream).await?;
                        let service = service_fn(move |request| proxy_request(request, state.clone(), kind));
                        hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .with_upgrades()
                            .await?;
                        Ok::<(), anyhow::Error>(())
                    }.await;
                    if let Err(error) = result {
                        tracing::debug!(%error, %peer, "TLS egress proxy connection closed");
                    }
                });
            }
        }
    }
}

pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>> {
    let cert_file = std::fs::File::open(cert_path)
        .with_context(|| format!("failed opening TLS certificate {}", cert_path.display()))?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        bail!("TLS certificate chain is empty");
    }
    let key_file = std::fs::File::open(key_path)
        .with_context(|| format!("failed opening TLS private key {}", key_path.display()))?;
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut BufReader::new(key_file))?
        .ok_or_else(|| anyhow!("TLS private key is missing"))?;
    let config = ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    Ok(Arc::new(config))
}

async fn proxy_request(
    mut request: Request<Incoming>,
    state: GatewayState,
    kind: ListenerKind,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let response = match authorize_request(&request, &state, kind).await {
        Ok(authorized) => {
            let on_upgrade = hyper::upgrade::on(&mut request);
            let tunnel_state = state.clone();
            tokio::spawn(async move {
                match on_upgrade.await {
                    Ok(upgraded) => {
                        if let Err(error) =
                            tunnel(TokioIo::new(upgraded), authorized, tunnel_state).await
                        {
                            tracing::warn!(%error, "egress tunnel failed");
                        }
                    }
                    Err(error) => tracing::debug!(%error, "egress CONNECT upgrade failed"),
                }
            });
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .expect("valid CONNECT response")
        }
        Err(error) => {
            tracing::warn!(decision = "denied", reason = %error, "egress CONNECT rejected");
            state.metrics.add("agentx_egress_denied_total", 1.0).await;
            Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Full::new(Bytes::from_static(b"egress target denied\n")))
                .expect("valid rejection response")
        }
    };
    Ok(response)
}

struct AuthorizedTarget {
    claims: EgressConnectClaimsV1,
    host: String,
    port: u16,
    addresses: Vec<SocketAddr>,
    _sandbox_permit: Option<OwnedSemaphorePermit>,
    _sandbox_token_lease: Option<SandboxTokenLease>,
}

async fn authorize_request<B>(
    request: &Request<B>,
    state: &GatewayState,
    kind: ListenerKind,
) -> Result<AuthorizedTarget> {
    if request.method() != Method::CONNECT {
        bail!("only HTTP CONNECT is supported");
    }
    let authority = request
        .uri()
        .authority()
        .ok_or_else(|| anyhow!("CONNECT authority is missing"))?;
    if authority.as_str().contains('@') {
        bail!("userinfo is forbidden");
    }
    let host = authority.host().trim_end_matches('.').to_ascii_lowercase();
    let port = authority
        .port_u16()
        .ok_or_else(|| anyhow!("CONNECT port is required"))?;
    validate_host_name(&host)?;
    if !state.allowed_ports.contains(&port) {
        bail!("target port is not allowed");
    }
    let token = proxy_token(request, kind)?;
    let claims = verify_egress_connect_token(
        &token,
        &state.trusted_keys,
        &kind.allowed_roles(),
        Some((&host, port)),
    )
    .map_err(|_| anyhow!("invalid egress token"))?;
    if claims.role != EgressRole::Sandbox {
        state.consume_runtime_token(&claims)?;
    }
    let literal = host.parse::<IpAddr>().ok();
    let addresses = if let Some(ip) = literal {
        validate_ip(ip, false, state)?;
        vec![SocketAddr::new(ip, port)]
    } else {
        let resolved =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((&*host, port)))
                .await
                .map_err(|_| anyhow!("DNS resolution timed out"))??
                .collect::<Vec<_>>();
        validate_resolved_addresses(&resolved, state)?;
        let mut unique = HashSet::new();
        resolved
            .into_iter()
            .filter(|address| unique.insert(*address))
            .collect()
    };
    let sandbox_permit = if kind == ListenerKind::Sandbox {
        Some(
            state
                .sandbox_tunnels
                .clone()
                .try_acquire_owned()
                .map_err(|_| anyhow!("Sandbox egress tunnel concurrency limit reached"))?,
        )
    } else {
        None
    };
    let sandbox_token_lease = if kind == ListenerKind::Sandbox {
        Some(state.acquire_sandbox_token(&claims)?)
    } else {
        None
    };
    Ok(AuthorizedTarget {
        claims,
        host,
        port,
        addresses,
        _sandbox_permit: sandbox_permit,
        _sandbox_token_lease: sandbox_token_lease,
    })
}

fn proxy_token<B>(request: &Request<B>, kind: ListenerKind) -> Result<String> {
    let value = request
        .headers()
        .get(header::PROXY_AUTHORIZATION)
        .ok_or_else(|| anyhow!("Proxy-Authorization is required"))?
        .to_str()
        .context("Proxy-Authorization is not valid ASCII")?;
    if let Some(token) = value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
    {
        return Ok(token.to_owned());
    }
    if kind == ListenerKind::Sandbox
        && let Some(encoded) = value.strip_prefix("Basic ")
        && let Ok(decoded) = STANDARD.decode(encoded)
        && let Ok(credentials) = String::from_utf8(decoded)
        && let Some((username, token)) = credentials.split_once(':')
        && username == "agentx"
        && !token.is_empty()
    {
        return Ok(token.to_owned());
    }
    bail!("Proxy-Authorization is invalid for this listener")
}

fn validate_resolved_addresses(addresses: &[SocketAddr], state: &GatewayState) -> Result<()> {
    if addresses.is_empty() {
        bail!("DNS returned no address");
    }
    for address in addresses {
        validate_ip(address.ip(), true, state)?;
    }
    Ok(())
}

fn validate_host_name(host: &str) -> Result<()> {
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".svc")
        || host.ends_with(".cluster.local")
        || host.eq_ignore_ascii_case("kubernetes")
        || host.eq_ignore_ascii_case("kubernetes.default")
        || host.eq_ignore_ascii_case("kubernetes.default.svc")
    {
        bail!("cluster and local host names are forbidden");
    }
    if host
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\' || byte == b'/')
    {
        bail!("target host contains forbidden characters");
    }
    Ok(())
}

fn validate_ip(ip: IpAddr, from_dns: bool, state: &GatewayState) -> Result<()> {
    let docker_synthetic = "198.18.0.0/15".parse::<IpNet>().expect("static CIDR");
    if docker_synthetic.contains(&ip) {
        if from_dns && state.docker_desktop_dns {
            return Ok(());
        }
        bail!("benchmark address range is forbidden");
    }
    if !is_public_ip(ip)
        || state
            .blocked_networks
            .iter()
            .any(|network| network.contains(&ip))
    {
        bail!("non-public or cluster address is forbidden");
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map_or_else(|| is_public_v6(ip), is_public_v4),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let global_unicast = (segments[0] & 0xe000) == 0x2000;
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || !global_unicast
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x0064 && segments[1] == 0xff9b)
        || (segments[0] == 0x2001 && segments[1] <= 0x01ff)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x2001 && segments[1] == 0x0002)
        || segments[0] == 0x2002
        || (segments[0] == 0x3fff && segments[1] <= 0x0fff)
        || (segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0))
}

async fn tunnel<T>(client: T, target: AuthorizedTarget, state: GatewayState) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let started = Instant::now();
    let upstream = connect_verified(&target.addresses).await?;
    let activity = Arc::new(Mutex::new(Instant::now()));
    let mut client = ActivityStream::new(client, activity.clone());
    let mut upstream = ActivityStream::new(upstream, activity.clone());
    state.metrics.add("agentx_egress_active_tunnels", 1.0).await;
    state.metrics.add("agentx_egress_allowed_total", 1.0).await;
    tracing::info!(
        role = ?target.claims.role,
        tenant_id = %target.claims.tenant_id,
        execution_id = ?target.claims.execution_id,
        request_id = ?target.claims.request_id,
        target_host = %target.host,
        target_port = target.port,
        decision = "allowed",
        "egress tunnel opened"
    );
    let transfer = async {
        let copy = tokio::io::copy_bidirectional(&mut client, &mut upstream);
        tokio::pin!(copy);
        let sandbox = target.claims.role == EgressRole::Sandbox;
        let mut idle_check = tokio::time::interval(if sandbox {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(30)
        });
        loop {
            tokio::select! {
                result = &mut copy => return result.map_err(Into::into),
                _ = idle_check.tick() => {
                    let idle = activity.lock().unwrap_or_else(std::sync::PoisonError::into_inner).elapsed();
                    if idle >= IDLE_TIMEOUT {
                        bail!("egress tunnel idle timeout");
                    }
                    if sandbox && state.sandbox_duration_exhausted(target.claims.jti) {
                        bail!("Sandbox egress token total duration budget exhausted");
                    }
                }
            }
        }
    };
    let maximum_tunnel_duration = if target.claims.role == EgressRole::Sandbox {
        let remaining_token_lifetime = target
            .claims
            .exp
            .saturating_sub(agentx_runtime_contracts::now_unix())
            .max(1) as u64;
        TUNNEL_TIMEOUT.min(Duration::from_secs(remaining_token_lifetime))
    } else {
        TUNNEL_TIMEOUT
    };
    let result = match tokio::time::timeout(maximum_tunnel_duration, transfer).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!("egress tunnel maximum duration exceeded")),
    };
    state
        .metrics
        .add("agentx_egress_active_tunnels", -1.0)
        .await;
    if let Ok((sent, received)) = result {
        state
            .metrics
            .add("agentx_egress_bytes_total", (sent + received) as f64)
            .await;
        tracing::info!(
            role = ?target.claims.role,
            tenant_id = %target.claims.tenant_id,
            execution_id = ?target.claims.execution_id,
            request_id = ?target.claims.request_id,
            target_host = %target.host,
            target_port = target.port,
            duration_ms = started.elapsed().as_millis(),
            bytes_up = sent,
            bytes_down = received,
            "egress tunnel closed"
        );
    }
    result.map(|_| ())
}

async fn connect_verified(addresses: &[SocketAddr]) -> Result<TcpStream> {
    let connect = async {
        let mut last_error = None;
        for address in addresses {
            match TcpStream::connect(address).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.map_or_else(|| anyhow!("no verified target address"), Into::into))
    };
    tokio::time::timeout(CONNECT_TIMEOUT, connect)
        .await
        .map_err(|_| anyhow!("target connect timed out"))?
}

struct ActivityStream<T> {
    inner: T,
    activity: Arc<Mutex<Instant>>,
}

impl<T> ActivityStream<T> {
    fn new(inner: T, activity: Arc<Mutex<Instant>>) -> Self {
        Self { inner, activity }
    }

    fn mark(&self) {
        *self
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for ActivityStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buffer);
        if matches!(result, Poll::Ready(Ok(()))) && buffer.filled().len() > before {
            self.mark();
        }
        result
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for ActivityStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buffer);
        if matches!(result, Poll::Ready(Ok(written)) if written > 0) {
            self.mark();
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub fn proxy_authorization(token: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(&format!("Bearer {token}"))
        .context("egress token contains invalid header characters")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, net::IpAddr};

    use agentx_runtime_contracts::{
        EGRESS_TOKEN_AUDIENCE, EGRESS_TOKEN_ISSUER, EgressConnectClaimsV1, EgressMode, EgressRole,
        issue_egress_connect_token,
    };
    use hyper::{Method, Request, header};
    use uuid::Uuid;

    use super::{
        GatewayState, ListenerKind, SandboxBudget, authorize_request, is_public_ip,
        parse_allowed_ports, validate_host_name, validate_ip, validate_resolved_addresses,
    };

    const PRIVATE_KEY: &[u8] = include_bytes!(
        "../../../crates/agentx-runtime-contracts/tests/fixtures/service-private.pem"
    );
    const PUBLIC_KEY: &[u8] = include_bytes!(
        "../../../crates/agentx-runtime-contracts/tests/fixtures/service-public.pem"
    );

    fn egress_claims(role: EgressRole, jti: Uuid) -> EgressConnectClaimsV1 {
        let now = agentx_runtime_contracts::now_unix();
        EgressConnectClaimsV1 {
            iss: EGRESS_TOKEN_ISSUER.into(),
            aud: EGRESS_TOKEN_AUDIENCE.into(),
            role,
            tenant_id: Uuid::now_v7(),
            execution_id: Some(Uuid::now_v7()),
            request_id: None,
            egress_mode: EgressMode::PublicHttps,
            target_host: if role == EgressRole::Sandbox {
                "*".into()
            } else {
                "api.example.com".into()
            },
            target_port: 443,
            iat: now,
            exp: now + 60,
            jti,
        }
    }

    #[test]
    fn public_port_configuration_is_explicit_and_keeps_https() {
        assert_eq!(
            parse_allowed_ports("443,8443").unwrap(),
            BTreeSet::from([443, 8443])
        );
        assert!(parse_allowed_ports("8443").is_err());
        assert!(parse_allowed_ports("443,0").is_err());
        assert!(parse_allowed_ports("443,1-65535").is_err());
    }

    #[test]
    fn local_cluster_metadata_and_reserved_addresses_are_blocked() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
        ] {
            assert!(!is_public_ip(value.parse::<IpAddr>().unwrap()), "{value}");
        }
        for value in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(is_public_ip(value.parse::<IpAddr>().unwrap()), "{value}");
        }
    }

    #[test]
    fn docker_desktop_synthetic_range_only_allows_dns_results() {
        let state = GatewayState::for_test(BTreeSet::from([443]), true);
        let ip = "198.18.10.2".parse().unwrap();
        assert!(validate_ip(ip, true, &state).is_ok());
        assert!(validate_ip(ip, false, &state).is_err());
        let strict = GatewayState::for_test(BTreeSet::from([443]), false);
        assert!(validate_ip(ip, true, &strict).is_err());
    }

    #[test]
    fn cluster_names_and_userinfo_are_not_valid_targets() {
        for host in [
            "localhost",
            "api.default.svc",
            "api.cluster.local",
            "kubernetes.default",
        ] {
            assert!(validate_host_name(host).is_err(), "{host}");
        }
        assert!(validate_host_name("api.openai.com").is_ok());
    }

    #[test]
    fn runtime_tokens_are_single_use() {
        let state = GatewayState::for_test(BTreeSet::from([443]), false);
        let claims = egress_claims(EgressRole::WorkflowWorker, Uuid::now_v7());
        state.consume_runtime_token(&claims).unwrap();
        assert!(state.consume_runtime_token(&claims).is_err());
    }

    #[test]
    fn sandbox_tokens_have_per_token_concurrency_and_connection_budgets() {
        let mut state = GatewayState::for_test(BTreeSet::from([443]), false);
        state.sandbox_budget = SandboxBudget {
            max_concurrent_tunnels: 2,
            max_connections: 3,
            max_total_duration: std::time::Duration::from_secs(60),
        };
        let claims = egress_claims(EgressRole::Sandbox, Uuid::now_v7());
        let first = state.acquire_sandbox_token(&claims).unwrap();
        let second = state.acquire_sandbox_token(&claims).unwrap();
        assert!(state.acquire_sandbox_token(&claims).is_err());
        drop(first);
        let third = state.acquire_sandbox_token(&claims).unwrap();
        drop((second, third));
        assert!(state.acquire_sandbox_token(&claims).is_err());
    }

    #[test]
    fn sandbox_tokens_have_a_total_duration_budget() {
        let mut state = GatewayState::for_test(BTreeSet::from([443]), false);
        state.sandbox_budget.max_total_duration = std::time::Duration::ZERO;
        let claims = egress_claims(EgressRole::Sandbox, Uuid::now_v7());
        assert!(state.acquire_sandbox_token(&claims).is_err());
    }

    #[test]
    fn any_private_dns_answer_rejects_the_entire_resolution() {
        let state = GatewayState::for_test(BTreeSet::from([443]), false);
        let answers = [
            "1.1.1.1:443".parse().unwrap(),
            "10.0.0.1:443".parse().unwrap(),
        ];
        assert!(validate_resolved_addresses(&answers, &state).is_err());
        assert!(
            validate_resolved_addresses(&["[2606:4700:4700::1111]:443".parse().unwrap()], &state)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn connect_authorization_enforces_listener_role_port_target_and_replay() {
        let mut state = GatewayState::for_test(BTreeSet::from([443]), false);
        state.trusted_keys = std::sync::Arc::new(std::collections::HashMap::from([(
            "workflow-worker-current".into(),
            PUBLIC_KEY.to_vec(),
        )]));
        let mut claims = egress_claims(EgressRole::WorkflowWorker, Uuid::now_v7());
        claims.target_host = "1.1.1.1".into();
        let token =
            issue_egress_connect_token("workflow-worker-current", PRIVATE_KEY, &claims).unwrap();
        let request = Request::builder()
            .method(Method::CONNECT)
            .uri("1.1.1.1:443")
            .header(header::PROXY_AUTHORIZATION, format!("Bearer {token}"))
            .body(())
            .unwrap();
        assert!(
            authorize_request(&request, &state, ListenerKind::Runtime)
                .await
                .is_ok()
        );
        assert!(
            authorize_request(&request, &state, ListenerKind::Runtime)
                .await
                .is_err()
        );
        assert!(
            authorize_request(&request, &state, ListenerKind::Sandbox)
                .await
                .is_err()
        );

        let illegal_port = Request::builder()
            .method(Method::CONNECT)
            .uri("1.1.1.1:80")
            .header(header::PROXY_AUTHORIZATION, "Bearer ignored")
            .body(())
            .unwrap();
        assert!(
            authorize_request(&illegal_port, &state, ListenerKind::Runtime)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn private_literal_is_rejected_after_a_valid_target_bound_token() {
        let mut state = GatewayState::for_test(BTreeSet::from([443]), false);
        state.trusted_keys = std::sync::Arc::new(std::collections::HashMap::from([(
            "runtime-gateway-current".into(),
            PUBLIC_KEY.to_vec(),
        )]));
        let mut claims = egress_claims(EgressRole::RuntimeGateway, Uuid::now_v7());
        claims.target_host = "10.0.0.1".into();
        let token =
            issue_egress_connect_token("runtime-gateway-current", PRIVATE_KEY, &claims).unwrap();
        let request = Request::builder()
            .method(Method::CONNECT)
            .uri("10.0.0.1:443")
            .header(header::PROXY_AUTHORIZATION, format!("Bearer {token}"))
            .body(())
            .unwrap();
        assert!(
            authorize_request(&request, &state, ListenerKind::Runtime)
                .await
                .is_err()
        );
    }
}
