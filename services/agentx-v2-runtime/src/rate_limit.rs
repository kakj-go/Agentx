use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use sha2::{Digest, Sha256};

const DEFAULT_REQUESTS_PER_SECOND: u32 = 50;
const DEFAULT_BURST: u32 = 100;
const MAX_TRACKED_CALLERS: usize = 10_000;

#[derive(Clone)]
pub struct LocalCallerRateLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    requests_per_second: f64,
    burst: f64,
    callers: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    tokens: f64,
    updated_at: Instant,
}

impl LocalCallerRateLimiter {
    pub fn from_env() -> anyhow::Result<Self> {
        let requests_per_second = parse_positive(
            "AGENTX_RUNTIME_GATEWAY_RATE_LIMIT_RPS",
            DEFAULT_REQUESTS_PER_SECOND,
        )?;
        let burst = parse_positive("AGENTX_RUNTIME_GATEWAY_RATE_LIMIT_BURST", DEFAULT_BURST)?;
        anyhow::ensure!(
            burst >= requests_per_second,
            "AGENTX_RUNTIME_GATEWAY_RATE_LIMIT_BURST must be at least the RPS limit"
        );
        Ok(Self::new(requests_per_second, burst))
    }

    #[must_use]
    pub fn new(requests_per_second: u32, burst: u32) -> Self {
        Self {
            inner: Arc::new(Inner {
                requests_per_second: f64::from(requests_per_second),
                burst: f64::from(burst),
                callers: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn allow(&self, key: String, now: Instant) -> bool {
        let mut callers = self.inner.callers.lock().expect("rate limiter mutex");
        if callers.len() >= MAX_TRACKED_CALLERS && !callers.contains_key(&key) {
            let idle_before = now.checked_sub(Duration::from_secs(300)).unwrap_or(now);
            callers.retain(|_, bucket| bucket.updated_at >= idle_before);
        }
        let bucket = callers.entry(key).or_insert(Bucket {
            tokens: self.inner.burst,
            updated_at: now,
        });
        let elapsed = now
            .saturating_duration_since(bucket.updated_at)
            .as_secs_f64();
        bucket.tokens =
            (bucket.tokens + elapsed * self.inner.requests_per_second).min(self.inner.burst);
        bucket.updated_at = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }
}

pub async fn enforce(
    State(limiter): State<LocalCallerRateLimiter>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let key = caller_key(&request);
    if limiter.allow(key, Instant::now()) {
        return next.run(request).await;
    }
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", "1")],
        Json(json!({
            "code": "RATE_LIMITED",
            "message": "Runtime Gateway caller rate limit exceeded"
        })),
    )
        .into_response()
}

fn caller_key(request: &Request<Body>) -> String {
    let identity = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .or_else(|| request.uri().path().strip_prefix("/webhooks/"))
        .or_else(|| request.uri().path().strip_prefix("/waits/"))
        .unwrap_or("anonymous");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn parse_positive(name: &str, default: u32) -> anyhow::Result<u32> {
    let value = match env::var(name) {
        Ok(value) => value.parse::<u32>()?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(value > 0, "{name} must be positive");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_is_enforced_per_caller_and_refills() {
        let limiter = LocalCallerRateLimiter::new(50, 100);
        let now = Instant::now();
        for _ in 0..100 {
            assert!(limiter.allow("caller-a".into(), now));
        }
        assert!(!limiter.allow("caller-a".into(), now));
        assert!(limiter.allow("caller-b".into(), now));
        assert!(limiter.allow("caller-a".into(), now + Duration::from_millis(20)));
    }
}
