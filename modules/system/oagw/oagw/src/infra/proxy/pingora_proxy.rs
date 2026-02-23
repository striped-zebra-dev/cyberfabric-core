use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use pingora_core::protocols::Digest;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_http::ResponseHeader;
use pingora_load_balancing::LoadBalancer;
use pingora_load_balancing::health_check::TcpHealthCheck;
use pingora_load_balancing::selection::RoundRobin;
use pingora_proxy::{HttpProxy, ProxyHttp, Session, http_proxy};
use tokio::sync::watch;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::model::{Endpoint, Scheme};
use crate::domain::services::EndpointSelector;

// ---------------------------------------------------------------------------
// Internal header names (D9)
// ---------------------------------------------------------------------------

const INTERNAL_PREFIX: &str = "x-oagw-internal-";

pub(crate) const H_UPSTREAM_ID: &str = "x-oagw-internal-upstream-id";
pub(crate) const H_ENDPOINT_HOST: &str = "x-oagw-internal-endpoint-host";
pub(crate) const H_ENDPOINT_PORT: &str = "x-oagw-internal-endpoint-port";
pub(crate) const H_ENDPOINT_SCHEME: &str = "x-oagw-internal-endpoint-scheme";
pub(crate) const H_INSTANCE_URI: &str = "x-oagw-internal-instance-uri";

/// Hop-by-hop headers that must not be forwarded in responses (mirrors headers.rs).
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

// ---------------------------------------------------------------------------
// PingoraProxy — ProxyHttp implementation (D3)
// ---------------------------------------------------------------------------

pub struct PingoraProxy {
    connect_timeout: Duration,
    read_timeout: Duration,
}

impl PingoraProxy {
    pub fn new(connect_timeout: Duration, read_timeout: Duration) -> Self {
        Self {
            connect_timeout,
            read_timeout,
        }
    }
}

/// Construct an `HttpProxy` from a `ServerConf` and `PingoraProxy`.
pub fn new_http_proxy(
    conf: &Arc<pingora_core::server::configuration::ServerConf>,
    inner: PingoraProxy,
) -> HttpProxy<PingoraProxy> {
    http_proxy(conf, inner)
}

// ---------------------------------------------------------------------------
// PingoraBackendSelector — default in-process BackendSelector (D2, D3)
// ---------------------------------------------------------------------------

/// Cache entry: load balancer + addr-to-endpoint mapping + shutdown handle.
struct LbEntry {
    lb: Arc<LoadBalancer<RoundRobin>>,
    /// Maps `"host:port"` → `Endpoint` for reverse lookup after `select()`.
    endpoints_by_addr: HashMap<String, Endpoint>,
    /// Dropping this sender signals the health-check background task to stop.
    _shutdown_tx: watch::Sender<bool>,
}

/// Default in-process `BackendSelector` backed by Pingora's `LoadBalancer<RoundRobin>`.
///
/// Lazily constructs a `LoadBalancer` per upstream on first `select()` call,
/// caches it in a `DashMap`, and attaches a `TcpHealthCheck` with 10s interval.
/// Dropping the cache entry (via `invalidate()`) stops the health-check task.
pub struct PingoraEndpointSelector {
    cache: DashMap<Uuid, LbEntry>,
}

impl PingoraEndpointSelector {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
        }
    }

    /// Build a `LoadBalancer<RoundRobin>` from domain endpoints, attach TCP
    /// health check, spawn background health-check task, return cache entry.
    fn build_entry(&self, endpoints: &[Endpoint]) -> Option<LbEntry> {
        let addrs: Vec<String> = endpoints
            .iter()
            .map(|ep| format!("{}:{}", ep.host, ep.port))
            .collect();

        // try_from_iter resolves addresses, builds the selector, and calls
        // update() internally, so backends are ready for select() immediately.
        let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(addrs.iter().map(|s| s.as_str()))
            .map_err(|e| warn!("Failed to create LoadBalancer: {e}"))
            .ok()?;

        let hc = TcpHealthCheck::new();
        lb.set_health_check(hc);
        lb.health_check_frequency = Some(Duration::from_secs(10));

        let lb = Arc::new(lb);

        // Build "host:port" → Endpoint mapping for reverse lookup after select().
        let mut endpoints_by_addr = HashMap::with_capacity(endpoints.len());
        for (ep, addr_str) in endpoints.iter().zip(addrs.iter()) {
            endpoints_by_addr.insert(addr_str.clone(), ep.clone());
        }

        // Spawn background health-check loop; drops when _shutdown_tx is dropped.
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let lb_bg = lb.clone();
        tokio::spawn(async move {
            loop {
                // update() refreshes backends from discovery (no-op for static)
                // and is required before health checks can report status.
                let _ = lb_bg.update().await;

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                    _ = shutdown_rx.changed() => { break; }
                }
            }
        });

        Some(LbEntry {
            lb,
            endpoints_by_addr,
            _shutdown_tx: shutdown_tx,
        })
    }
}

#[async_trait]
impl EndpointSelector for PingoraEndpointSelector {
    async fn select(&self, upstream_id: Uuid, endpoints: &[Endpoint]) -> Option<Endpoint> {
        // Fast path: LB already cached.
        if let Some(entry) = self.cache.get(&upstream_id) {
            let backend = entry.lb.select(b"", 256)?;
            let addr_key = backend.addr.to_string();
            return entry.endpoints_by_addr.get(&addr_key).cloned();
        }

        // Slow path: build and cache.
        let entry = self.build_entry(endpoints)?;
        let backend = entry.lb.select(b"", 256)?;
        let addr_key = backend.addr.to_string();
        let result = entry.endpoints_by_addr.get(&addr_key).cloned();
        self.cache.insert(upstream_id, entry);
        result
    }

    fn invalidate(&self, upstream_id: Uuid) {
        // Removing the entry drops LbEntry, which drops _shutdown_tx,
        // which signals the health-check background task to stop.
        self.cache.remove(&upstream_id);
    }
}

// ---------------------------------------------------------------------------
// Per-request context (D3)
// ---------------------------------------------------------------------------

pub struct ProxyCtx {
    endpoint: Endpoint,
    instance_uri: String,
    retries: u32,
}

impl Default for ProxyCtx {
    fn default() -> Self {
        Self {
            endpoint: Endpoint {
                scheme: Scheme::Https,
                host: String::new(),
                port: 443,
            },
            instance_uri: String::new(),
            retries: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// ProxyHttp trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ProxyHttp for PingoraProxy {
    type CTX = ProxyCtx;

    fn new_ctx(&self) -> Self::CTX {
        ProxyCtx::default()
    }

    /// Extract internal context headers, populate `ProxyCtx`, strip them. (D9)
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<bool> {
        // Read context from internal headers.
        let req = session.req_header();
        if let Some(v) = req
            .headers
            .get(H_ENDPOINT_HOST)
            .and_then(|v| v.to_str().ok())
        {
            ctx.endpoint.host = v.to_string();
        }
        if let Some(v) = req
            .headers
            .get(H_ENDPOINT_PORT)
            .and_then(|v| v.to_str().ok())
            && let Ok(port) = v.parse()
        {
            ctx.endpoint.port = port;
        }
        if let Some(v) = req
            .headers
            .get(H_ENDPOINT_SCHEME)
            .and_then(|v| v.to_str().ok())
        {
            ctx.endpoint.scheme = match v {
                "http" => Scheme::Http,
                "https" => Scheme::Https,
                "wss" => Scheme::Wss,
                "wt" => Scheme::Wt,
                _ => Scheme::Https,
            };
        }
        if let Some(v) = req
            .headers
            .get(H_INSTANCE_URI)
            .and_then(|v| v.to_str().ok())
        {
            ctx.instance_uri = v.to_string();
        }

        // Strip all internal headers before forwarding.
        let to_remove: Vec<http::HeaderName> = session
            .req_header()
            .headers
            .keys()
            .filter(|k| k.as_str().starts_with(INTERNAL_PREFIX))
            .cloned()
            .collect();
        let req_mut = session.req_header_mut();
        for name in &to_remove {
            req_mut.remove_header(name);
        }

        Ok(false) // continue processing
    }

    /// Build `HttpPeer` from the resolved endpoint. (D3, D4, D7)
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<Box<HttpPeer>> {
        let ep = &ctx.endpoint;
        let tls = matches!(ep.scheme, Scheme::Https | Scheme::Wss | Scheme::Wt);

        let mut peer = HttpPeer::new(format!("{}:{}", ep.host, ep.port), tls, ep.host.clone());

        peer.options.connection_timeout = Some(self.connect_timeout);
        peer.options.read_timeout = Some(self.read_timeout);
        peer.options.idle_timeout = Some(Duration::from_secs(90));

        // ALPN: H2H1 for HTTPS, H1 for WebSocket and cleartext.
        peer.options.alpn = if tls && !matches!(ep.scheme, Scheme::Wss) {
            pingora_core::protocols::tls::ALPN::H2H1
        } else {
            pingora_core::protocols::tls::ALPN::H1
        };

        Ok(Box::new(peer))
    }

    /// No-op — headers are already prepared by proxy_request() stages 4-6. (D3)
    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        _upstream_request: &mut pingora_http::RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()> {
        Ok(())
    }

    /// Sanitize response headers: strip hop-by-hop and x-oagw-* headers. (D3)
    async fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()> {
        // Strip hop-by-hop headers.
        for name in HOP_BY_HOP {
            upstream_response.remove_header(*name);
        }

        // Strip x-oagw-* internal headers.
        let to_remove: Vec<http::HeaderName> = upstream_response
            .headers
            .keys()
            .filter(|k| k.as_str().starts_with("x-oagw-"))
            .cloned()
            .collect();
        for name in &to_remove {
            upstream_response.remove_header(name);
        }

        Ok(())
    }

    /// Retry on connection failure, up to 2 retries (3 total attempts). (D6)
    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        mut e: Box<pingora_core::Error>,
    ) -> Box<pingora_core::Error> {
        ctx.retries += 1;
        if ctx.retries <= 2 {
            e.set_retry(true);
        }
        e
    }

    /// Retry on stale pooled connection errors. (D6)
    fn error_while_proxy(
        &self,
        _peer: &HttpPeer,
        _session: &mut Session,
        mut e: Box<pingora_core::Error>,
        _ctx: &mut Self::CTX,
        client_reused: bool,
    ) -> Box<pingora_core::Error> {
        if client_reused {
            e.retry.decide_reuse(true);
        }
        e
    }

    /// Map Pingora error types to HTTP status codes and write RFC 9457 response. (D6)
    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        e: &pingora_core::Error,
        ctx: &mut Self::CTX,
    ) -> pingora_proxy::FailToProxy {
        let (status, detail) = match &e.etype {
            pingora_core::ErrorType::ConnectTimedout => (502, "upstream connection timed out"),
            pingora_core::ErrorType::ConnectRefused => (502, "upstream connection refused"),
            pingora_core::ErrorType::TLSHandshakeFailure
            | pingora_core::ErrorType::TLSHandshakeTimedout => {
                (502, "upstream TLS handshake failed")
            }
            pingora_core::ErrorType::InvalidCert => (502, "upstream certificate invalid"),
            pingora_core::ErrorType::ReadTimedout => (504, "upstream read timed out"),
            pingora_core::ErrorType::WriteTimedout => (504, "upstream write timed out"),
            pingora_core::ErrorType::ConnectionClosed => (502, "upstream connection closed"),
            pingora_core::ErrorType::H2Error | pingora_core::ErrorType::H2Downgrade => {
                (502, "upstream HTTP/2 error")
            }
            _ => (502, "upstream error"),
        };

        // RFC 9457 Problem Details JSON.
        let body = serde_json::json!({
            "type": "about:blank",
            "title": http::StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or("Error"),
            "status": status,
            "detail": detail,
            "instance": ctx.instance_uri,
        });

        let body_bytes = Bytes::from(serde_json::to_vec(&body).unwrap_or_default());

        // Write the error response with correct content-type.
        if let Ok(mut resp) = ResponseHeader::build(status, Some(body_bytes.len())) {
            let _ = resp.insert_header("content-type", "application/problem+json");
            let _ = session.write_response_header(Box::new(resp), false).await;
            let _ = session.write_response_body(Some(body_bytes), true).await;
        } else {
            let _ = session.respond_error(status).await;
        }

        pingora_proxy::FailToProxy {
            error_code: 0,
            can_reuse_downstream: false,
        }
    }

    /// Log upstream connection info. (D3)
    async fn connected_to_upstream(
        &self,
        _session: &mut Session,
        reused: bool,
        peer: &HttpPeer,
        #[cfg(unix)] _fd: std::os::unix::io::RawFd,
        #[cfg(windows)] _sock: std::os::windows::io::RawSocket,
        _digest: Option<&Digest>,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()> {
        info!(
            reused,
            peer = %peer,
            instance = %ctx.instance_uri,
            "Connected to upstream"
        );
        Ok(())
    }

    /// Log request summary with timing. (D3)
    async fn logging(
        &self,
        session: &mut Session,
        e: Option<&pingora_core::Error>,
        _ctx: &mut Self::CTX,
    ) {
        let status = session
            .as_downstream()
            .response_written()
            .map(|r| r.status.as_u16())
            .unwrap_or(0);
        let method = session.req_header().method.as_str();
        let path = session.req_header().uri.path();

        if let Some(err) = e {
            warn!(method, path, status, error = %err, "Proxy request failed");
        } else {
            info!(method, path, status, "Proxy request completed");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{Endpoint, Scheme};

    fn init_crypto() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    fn ep(host: &str, port: u16, scheme: Scheme) -> Endpoint {
        Endpoint {
            scheme,
            host: host.to_string(),
            port,
        }
    }

    // Note: PingoraBackendSelector uses Pingora's LoadBalancer which resolves
    // addresses via ToSocketAddrs during construction. Tests must use real IP
    // addresses (e.g. 127.0.0.1) with distinct ports to differentiate endpoints.

    #[tokio::test]
    async fn select_round_robin_distribution() {
        init_crypto();
        let selector = PingoraEndpointSelector::new();
        let id = Uuid::new_v4();
        let endpoints = vec![
            ep("127.0.0.1", 10001, Scheme::Https),
            ep("127.0.0.1", 10002, Scheme::Https),
        ];

        let mut port_a = 0u32;
        let mut port_b = 0u32;
        for _ in 0..4 {
            let selected = selector.select(id, &endpoints).await.unwrap();
            match selected.port {
                10001 => port_a += 1,
                10002 => port_b += 1,
                other => panic!("unexpected port: {other}"),
            }
        }
        assert!(port_a > 0, "port 10001 should be selected at least once");
        assert!(port_b > 0, "port 10002 should be selected at least once");
    }

    #[tokio::test]
    async fn invalidate_causes_rebuild() {
        init_crypto();
        let selector = PingoraEndpointSelector::new();
        let id = Uuid::new_v4();

        let v1 = vec![ep("127.0.0.1", 20001, Scheme::Https)];
        let selected = selector.select(id, &v1).await.unwrap();
        assert_eq!(selected.port, 20001);

        selector.invalidate(id);

        let v2 = vec![ep("127.0.0.1", 20002, Scheme::Https)];
        let selected = selector.select(id, &v2).await.unwrap();
        assert_eq!(selected.port, 20002);
    }

    #[tokio::test]
    async fn select_single_endpoint() {
        init_crypto();
        let selector = PingoraEndpointSelector::new();
        let id = Uuid::new_v4();
        let endpoints = vec![ep("127.0.0.1", 30001, Scheme::Http)];

        let selected = selector.select(id, &endpoints).await.unwrap();
        assert_eq!(selected.host, "127.0.0.1");
        assert_eq!(selected.port, 30001);
        assert_eq!(selected.scheme, Scheme::Http);
    }

    /// Endpoints in an upstream share scheme/port (by design).
    /// Verify the scheme survives the Pingora Backend round-trip.
    #[tokio::test]
    async fn select_preserves_scheme() {
        init_crypto();
        let selector = PingoraEndpointSelector::new();
        let id = Uuid::new_v4();
        // All endpoints share the same scheme (upstream-level invariant).
        // Use different ports to distinguish endpoints.
        let endpoints = vec![
            ep("127.0.0.1", 40001, Scheme::Https),
            ep("127.0.0.1", 40002, Scheme::Https),
        ];

        let mut found_1 = false;
        let mut found_2 = false;
        for _ in 0..20 {
            let selected = selector.select(id, &endpoints).await.unwrap();
            assert_eq!(selected.scheme, Scheme::Https, "scheme must be preserved");
            assert_eq!(selected.host, "127.0.0.1", "host must be preserved");
            match selected.port {
                40001 => found_1 = true,
                40002 => found_2 = true,
                other => panic!("unexpected port: {other}"),
            }
            if found_1 && found_2 {
                break;
            }
        }
        assert!(found_1, "should have selected port 40001");
        assert!(found_2, "should have selected port 40002");
    }
}
