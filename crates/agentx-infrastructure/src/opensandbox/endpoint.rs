use std::{collections::BTreeMap, net::IpAddr};

use agentx_application::{RuntimeError, RuntimeResult};
use ipnet::IpNet;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use url::Url;

const ALLOWED_EXECD_HEADERS: [&str; 3] = [
    "x-execd-access-token",
    "opensandbox-ingress-to",
    "opensandbox-secure-access",
];

#[derive(Clone, Debug)]
pub struct EndpointPolicy {
    lifecycle_host: String,
    allowed_hosts: Vec<String>,
    allowed_cidrs: Vec<IpNet>,
    allow_http: bool,
}

impl EndpointPolicy {
    pub fn new(
        lifecycle: &Url,
        allowed_hosts: Vec<String>,
        allowed_cidrs: Vec<IpNet>,
        allow_http: bool,
    ) -> RuntimeResult<Self> {
        let lifecycle_host = lifecycle
            .host_str()
            .ok_or_else(|| protocol_error("OpenSandbox lifecycle endpoint has no host"))?
            .to_ascii_lowercase();
        Ok(Self {
            lifecycle_host,
            allowed_hosts,
            allowed_cidrs,
            allow_http,
        })
    }

    pub fn validate(
        &self,
        raw: &str,
        headers: &BTreeMap<String, String>,
    ) -> RuntimeResult<(Url, HeaderMap)> {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with("//") || raw.contains(['\\', '\r', '\n', '\t', ' ']) {
            return Err(protocol_error(
                "OpenSandbox returned an invalid execd endpoint",
            ));
        }
        let mut url = if raw.contains("://") {
            Url::parse(raw)
        } else {
            Url::parse(&format!(
                "{}://{raw}",
                if self.allow_http { "http" } else { "https" }
            ))
        }
        .map_err(|_| protocol_error("OpenSandbox returned an invalid execd endpoint"))?;
        if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
            return Err(protocol_error(
                "OpenSandbox execd endpoint contains forbidden URL components",
            ));
        }
        if url.scheme() != "https" && !(self.allow_http && url.scheme() == "http") {
            return Err(protocol_error(
                "OpenSandbox execd endpoint uses a forbidden scheme",
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| protocol_error("OpenSandbox execd endpoint has no host"))?
            .to_ascii_lowercase();
        let host_allowed = host == self.lifecycle_host
            || self.allowed_hosts.contains(&host)
            || host.parse::<IpAddr>().ok().is_some_and(|ip| {
                self.allowed_cidrs
                    .iter()
                    .any(|network| network.contains(&ip))
            });
        if !host_allowed {
            return Err(protocol_error(
                "OpenSandbox execd endpoint host is outside the configured allowlist",
            ));
        }
        if url.query().is_some() {
            return Err(protocol_error(
                "OpenSandbox execd endpoint contains a forbidden query",
            ));
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        let mut safe_headers = HeaderMap::new();
        for (name, value) in headers {
            let normalized = name.to_ascii_lowercase();
            if !ALLOWED_EXECD_HEADERS.contains(&normalized.as_str()) {
                return Err(protocol_error(
                    "OpenSandbox returned a forbidden execd header",
                ));
            }
            if value.contains(['\r', '\n']) {
                return Err(protocol_error(
                    "OpenSandbox returned an invalid execd header value",
                ));
            }
            safe_headers.insert(
                HeaderName::from_bytes(normalized.as_bytes()).map_err(|_| {
                    protocol_error("OpenSandbox returned an invalid execd header name")
                })?,
                HeaderValue::from_str(value).map_err(|_| {
                    protocol_error("OpenSandbox returned an invalid execd header value")
                })?,
            );
        }
        Ok((url, safe_headers))
    }

    pub fn is_lifecycle_origin(&self, url: &Url, lifecycle: &Url) -> bool {
        url.scheme() == lifecycle.scheme()
            && url.host_str().map(str::to_ascii_lowercase) == Some(self.lifecycle_host.clone())
            && url.port_or_known_default() == lifecycle.port_or_known_default()
    }
}

fn protocol_error(message: &str) -> RuntimeError {
    RuntimeError::new("SANDBOX_PROTOCOL_UNSUPPORTED", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cross_origin_and_untrusted_headers() {
        let policy = EndpointPolicy::new(
            &Url::parse("http://127.0.0.1:8080/v1/").unwrap(),
            vec![],
            vec![],
            true,
        )
        .unwrap();
        assert!(
            policy
                .validate("http://169.254.169.254:44772", &BTreeMap::new())
                .is_err()
        );
        assert!(
            policy
                .validate(
                    "http://127.0.0.1:44772",
                    &BTreeMap::from([("Authorization".into(), "secret".into())])
                )
                .is_err()
        );
        assert!(
            policy
                .validate(
                    "http://127.0.0.1:44772",
                    &BTreeMap::from([("X-EXECD-ACCESS-TOKEN".into(), "token".into())])
                )
                .is_ok()
        );
        assert!(
            policy
                .validate(
                    "127.0.0.1:8080/v1/sandboxes/sbx/proxy/44772",
                    &BTreeMap::from([
                        ("OpenSandbox-Ingress-To".into(), "sbx-44772".into()),
                        ("OpenSandbox-Secure-Access".into(), "token".into()),
                    ])
                )
                .is_ok()
        );
    }

    #[test]
    fn normalizes_scheme_less_endpoints_and_rejects_query_injection() {
        let lifecycle = Url::parse("http://host.docker.internal:18080/v1/").unwrap();
        let policy = EndpointPolicy::new(
            &lifecycle,
            vec!["host.docker.internal".into()],
            vec![],
            true,
        )
        .unwrap();
        let (endpoint, _) = policy
            .validate(
                "host.docker.internal:18080/v1/sandboxes/sbx/proxy/44772",
                &BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(
            endpoint.as_str(),
            "http://host.docker.internal:18080/v1/sandboxes/sbx/proxy/44772/"
        );
        assert!(policy.is_lifecycle_origin(&endpoint, &lifecycle));
        assert!(
            policy
                .validate(
                    "host.docker.internal:18080/proxy/44772?target=metadata",
                    &BTreeMap::new()
                )
                .is_err()
        );
    }
}
