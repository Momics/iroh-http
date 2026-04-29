//! Incoming HTTP request — `serve()` implementation.
//!
//! Each accepted QUIC bidirectional stream is driven by hyper's HTTP/1.1
//! server connection.  A `tower::Service` (`IrohHttpService`) bridges between
//! hyper and the existing body-channel + slab infrastructure.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tower::Service;

use crate::{
    base32_encode,
    client::{body_from_reader, pump_hyper_body_to_channel_limited},
    io::IrohStream,
    stream::{HandleStore, ResponseHeadEntry},
    ConnectionEvent, CoreError, IrohEndpoint, RequestPayload,
};

// ── Type aliases ──────────────────────────────────────────────────────────────

use crate::Body;
use crate::BoxError;

// ── Inline error responses (IrohHttpService is infallible) ───────────────
//
// Per ADR-014, `IrohHttpService::Error = Infallible` — every internal failure
// is rendered to an HTTP response inside the service. Layer-level failures
// (timeout, load-shed) are still mapped to 408 / 503 in `TowerErrorHandler`
// because those errors arise *outside* this service.

fn internal_error(detail: &'static [u8]) -> hyper::Response<Body> {
    hyper::Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::full(Bytes::from_static(detail)))
        .expect("static error response args are valid")
}

fn service_unavailable(detail: &'static [u8]) -> hyper::Response<Body> {
    hyper::Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .body(Body::full(Bytes::from_static(detail)))
        .expect("static error response args are valid")
}

// ── ServeOptions ──────────────────────────────────────────────────────────────

/// Options for the HTTP serve loop.
///
/// Passed directly to [`serve()`] or [`serve_with_events()`].  These govern
/// per-request middleware (Tower layers), inbound connection caps, and
/// serve-loop lifecycle — they do **not** affect outgoing fetch calls.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Maximum simultaneous in-flight requests.  Default: 1024.
    pub max_concurrency: Option<usize>,
    /// Consecutive accept-loop errors before the serve loop terminates.  Default: 5.
    pub max_serve_errors: Option<usize>,
    /// Per-request timeout in milliseconds.  Default: 60 000.
    pub request_timeout_ms: Option<u64>,
    /// Maximum connections from a single peer.  Default: 8.
    pub max_connections_per_peer: Option<usize>,
    /// Reject request bodies larger than this many bytes.  Default: 16 MiB.
    pub max_request_body_bytes: Option<usize>,
    /// Graceful shutdown drain window in milliseconds.  Default: 30 000.
    pub drain_timeout_ms: Option<u64>,
    /// Maximum total QUIC connections the server will accept.  Default: unlimited.
    pub max_total_connections: Option<usize>,
    /// When `true` (the default), reject new requests immediately with `503
    /// Service Unavailable` when `max_concurrency` is already reached rather
    /// than queuing them.  Prevents thundering-herd on recovery.
    pub load_shed: Option<bool>,
}

const DEFAULT_CONCURRENCY: usize = 1024;
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_CONNECTIONS_PER_PEER: usize = 8;
const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 30_000;
/// 16 MiB — applied when `max_request_body_bytes` is not explicitly set.
/// Prevents memory exhaustion from unbounded request bodies.
const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;
/// 256 MiB — applied when `max_response_body_bytes` is not explicitly set.
/// Prevents memory exhaustion from a malicious server sending a compressed
/// response that expands to an unbounded size (compression bomb).
pub(crate) const DEFAULT_MAX_RESPONSE_BODY_BYTES: usize = 256 * 1024 * 1024;

// ── ServeHandle ───────────────────────────────────────────────────────────────

pub struct ServeHandle {
    join: tokio::task::JoinHandle<()>,
    shutdown_notify: Arc<tokio::sync::Notify>,
    drain_timeout: std::time::Duration,
    /// Resolves to `true` once the serve task has fully exited.
    done_rx: tokio::sync::watch::Receiver<bool>,
}

impl ServeHandle {
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }
    pub async fn drain(self) {
        self.shutdown();
        let _ = self.join.await;
    }
    pub fn abort(&self) {
        self.join.abort();
    }
    pub fn drain_timeout(&self) -> std::time::Duration {
        self.drain_timeout
    }
    /// Subscribe to the serve-loop-done signal.
    ///
    /// The returned receiver resolves (changes to `true`) once the serve task
    /// has fully exited, including the drain phase.
    pub fn subscribe_done(&self) -> tokio::sync::watch::Receiver<bool> {
        self.done_rx.clone()
    }
}

// ── respond() ────────────────────────────────────────────────────────────────

pub fn respond(
    handles: &HandleStore,
    req_handle: u64,
    status: u16,
    headers: Vec<(String, String)>,
) -> Result<(), CoreError> {
    StatusCode::from_u16(status)
        .map_err(|_| CoreError::invalid_input(format!("invalid HTTP status code: {status}")))?;
    for (name, value) in &headers {
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            CoreError::invalid_input(format!("invalid response header name {:?}", name))
        })?;
        HeaderValue::from_str(value).map_err(|_| {
            CoreError::invalid_input(format!("invalid response header value for {:?}", name))
        })?;
    }

    let sender = handles
        .take_req_sender(req_handle)
        .ok_or_else(|| CoreError::invalid_handle(req_handle))?;
    sender
        .send(ResponseHeadEntry { status, headers })
        .map_err(|_| CoreError::internal("serve task dropped before respond"))
}

// ── PeerConnectionGuard ───────────────────────────────────────────────────────

type ConnectionEventFn = Arc<dyn Fn(ConnectionEvent) + Send + Sync>;

struct PeerConnectionGuard {
    counts: Arc<Mutex<HashMap<iroh::PublicKey, usize>>>,
    peer: iroh::PublicKey,
    peer_id_str: String,
    on_event: Option<ConnectionEventFn>,
}

impl PeerConnectionGuard {
    fn acquire(
        counts: &Arc<Mutex<HashMap<iroh::PublicKey, usize>>>,
        peer: iroh::PublicKey,
        peer_id_str: String,
        max: usize,
        on_event: Option<ConnectionEventFn>,
    ) -> Option<Self> {
        let mut map = counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = map.entry(peer).or_insert(0);
        if *count >= max {
            return None;
        }
        let was_zero = *count == 0;
        *count = count.saturating_add(1);
        let guard = PeerConnectionGuard {
            counts: counts.clone(),
            peer,
            peer_id_str: peer_id_str.clone(),
            on_event: on_event.clone(),
        };
        // Fire connected event on 0 → 1 transition (first connection from this peer).
        if was_zero {
            if let Some(cb) = &on_event {
                cb(ConnectionEvent {
                    peer_id: peer_id_str,
                    connected: true,
                });
            }
        }
        Some(guard)
    }
}

impl Drop for PeerConnectionGuard {
    fn drop(&mut self) {
        let mut map = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = map.get_mut(&self.peer) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                map.remove(&self.peer);
                // Fire disconnected event on 1 → 0 transition (last connection from this peer closed).
                if let Some(cb) = &self.on_event {
                    cb(ConnectionEvent {
                        peer_id: self.peer_id_str.clone(),
                        connected: false,
                    });
                }
            }
        }
    }
}

// ── FFI dispatcher + IrohHttpService ─────────────────────────────────────────
//
// Per ADR-014 the hyper-facing service is split in two:
//
//   * `FfiDispatcher` owns all the JS-bridge concerns — handle allocation,
//     `on_request` callback firing, body-channel pumping, response-head
//     rendezvous, and duplex upgrade hand-off. It is shared across every
//     accepted connection and request via `Arc`.
//   * `IrohHttpService` is the thin `tower::Service` shell. It clones cheaply
//     (Arc bump + Option<String>), patches the per-connection `remote_node_id`
//     in `serve_with_events`, and delegates each request to the dispatcher.
//
// The split keeps the tower::Service contract narrow (Infallible, generic over
// any `http_body::Body`) and isolates the FFI logic so future tests can mock
// `FfiDispatcher::dispatch` without standing up a real connection.

struct FfiDispatcher {
    on_request: Arc<dyn Fn(RequestPayload) + Send + Sync>,
    endpoint: IrohEndpoint,
    own_node_id: Arc<String>,
    max_request_body_bytes: Option<usize>,
    max_header_size: Option<usize>,
    #[cfg(feature = "compression")]
    compression: Option<crate::endpoint::CompressionOptions>,
}

#[derive(Clone)]
struct IrohHttpService {
    dispatcher: Arc<FfiDispatcher>,
    remote_node_id: Option<String>,
}

impl<B> Service<hyper::Request<B>> for IrohHttpService
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: std::fmt::Debug + Send + Sync + 'static,
{
    type Response = hyper::Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<B>) -> Self::Future {
        let dispatcher = self.dispatcher.clone();
        let remote_node_id = self.remote_node_id.clone().unwrap_or_default();
        Box::pin(async move { Ok(dispatcher.dispatch(req, remote_node_id).await) })
    }
}

impl FfiDispatcher {
    async fn dispatch<B>(
        self: Arc<Self>,
        mut req: hyper::Request<B>,
        remote_node_id: String,
    ) -> hyper::Response<Body>
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::fmt::Debug + Send + Sync + 'static,
    {
        let handles = self.endpoint.handles();
        let own_node_id = &*self.own_node_id;
        let max_request_body_bytes = self.max_request_body_bytes;
        let max_header_size = self.max_header_size;

        let method = req.method().to_string();
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/")
            .to_string();

        tracing::debug!(
            method = %method,
            path = %path_and_query,
            peer = %remote_node_id,
            "iroh-http: incoming request",
        );
        // Strip any client-supplied peer-id to prevent spoofing,
        // then inject the authenticated identity from the QUIC connection.
        //
        // ISS-011: Use raw byte length for header-size accounting to prevent
        // bypass via non-UTF8 values.  Reject non-UTF8 header values with 400
        // instead of silently converting them to empty strings.

        // First pass: measure header bytes using raw values (before lossy conversion).
        if let Some(limit) = max_header_size {
            let header_bytes: usize = req
                .headers()
                .iter()
                .filter(|(k, _)| !k.as_str().eq_ignore_ascii_case("peer-id"))
                .map(|(k, v)| {
                    k.as_str()
                        .len()
                        .saturating_add(v.as_bytes().len())
                        .saturating_add(4)
                }) // ": " + "\r\n"
                .fold(0usize, |acc, x| acc.saturating_add(x))
                .saturating_add("peer-id".len())
                .saturating_add(remote_node_id.len())
                .saturating_add(4)
                .saturating_add(req.uri().to_string().len())
                .saturating_add(method.len())
                .saturating_add(12); // "HTTP/1.1 \r\n\r\n" overhead
            if header_bytes > limit {
                let resp = hyper::Response::builder()
                    .status(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE)
                    .body(Body::empty())
                    .expect("static response args are valid");
                return resp;
            }
        }

        // Build header list — reject non-UTF8 values instead of silently dropping.
        let mut req_headers: Vec<(String, String)> = Vec::new();
        for (k, v) in req.headers().iter() {
            if k.as_str().eq_ignore_ascii_case("peer-id") {
                continue;
            }
            match v.to_str() {
                Ok(s) => req_headers.push((k.as_str().to_string(), s.to_string())),
                Err(_) => {
                    let resp = hyper::Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::full(Bytes::from_static(b"non-UTF8 header value")))
                        .expect("static response args are valid");
                    return resp;
                }
            }
        }
        req_headers.push(("peer-id".to_string(), remote_node_id.clone()));

        let url = format!("httpi://{own_node_id}{path_and_query}");

        // ISS-015: strict duplex upgrade validation — require CONNECT method +
        // Upgrade: iroh-duplex + Connection: upgrade headers.
        let has_upgrade_header = req_headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("iroh-duplex")
        });
        let has_connection_upgrade = req_headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("connection")
                && v.split(',')
                    .any(|tok| tok.trim().eq_ignore_ascii_case("upgrade"))
        });
        let is_connect = req.method() == http::Method::CONNECT;

        let is_bidi = if has_upgrade_header {
            if !has_connection_upgrade || !is_connect {
                let resp = hyper::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::full(Bytes::from_static(
                        b"duplex upgrade requires CONNECT method with Connection: upgrade header",
                    )))
                    .expect("static response args are valid");
                return resp;
            }
            true
        } else {
            false
        };

        // For duplex: capture the upgrade future BEFORE consuming the request.
        let upgrade_future = if is_bidi {
            Some(hyper::upgrade::on(&mut req))
        } else {
            None
        };

        // ── Allocate channels ────────────────────────────────────────────────

        // Request body: writer pumped from hyper; reader given to JS.
        let mut guard = handles.insert_guard();
        let (req_body_writer, req_body_reader) = handles.make_body_channel();
        let req_body_handle = match guard.insert_reader(req_body_reader) {
            Ok(h) => h,
            Err(_) => return service_unavailable(b"server handle table full"),
        };

        // Response body: writer given to JS (sendChunk); reader feeds hyper response.
        let (res_body_writer, res_body_reader) = handles.make_body_channel();
        let res_body_handle = match guard.insert_writer(res_body_writer) {
            Ok(h) => h,
            Err(_) => return service_unavailable(b"server handle table full"),
        };

        // ── Allocate response-head rendezvous ────────────────────────────────

        let (head_tx, head_rx) = tokio::sync::oneshot::channel::<ResponseHeadEntry>();
        let req_handle = match guard.allocate_req_handle(head_tx) {
            Ok(h) => h,
            Err(_) => return service_unavailable(b"server handle table full"),
        };

        guard.commit();

        // RAII guard: remove the req_handle slab entry on all exit paths
        // (413 early-return, timeout drop, "JS handler dropped", normal completion).
        // If respond() already consumed the entry, take_req_sender returns None — safe no-op.
        struct ReqHeadCleanup {
            endpoint: IrohEndpoint,
            req_handle: u64,
        }
        impl Drop for ReqHeadCleanup {
            fn drop(&mut self) {
                self.endpoint.handles().take_req_sender(self.req_handle);
            }
        }
        let _req_head_cleanup = ReqHeadCleanup {
            endpoint: self.endpoint.clone(),
            req_handle,
        };

        // ── Pump request body ────────────────────────────────────────────────

        // For duplex: keep req_body_writer to move into the upgrade spawn below.
        // For regular: consume it immediately into the pump task.
        // ISS-004: create an overflow channel so the serve path can return 413.
        let (body_overflow_tx, body_overflow_rx) = if !is_bidi && max_request_body_bytes.is_some() {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        let duplex_req_body_writer = if !is_bidi {
            let body = req.into_body();
            let frame_timeout = handles.drain_timeout();
            tokio::spawn(pump_hyper_body_to_channel_limited(
                body,
                req_body_writer,
                max_request_body_bytes,
                frame_timeout,
                body_overflow_tx,
            ));
            None
        } else {
            // Duplex: discard the HTTP preamble body (empty before 101).
            drop(req.into_body());
            Some(req_body_writer)
        };

        // ── Fire on_request callback ─────────────────────────────────────────

        on_request_fire(
            &self.on_request,
            req_handle,
            req_body_handle,
            res_body_handle,
            method,
            url,
            req_headers,
            remote_node_id,
            is_bidi,
        );

        // ── Await response head from JS (race against body overflow) ─────────
        //
        // ISS-004: if the request body exceeds maxRequestBodyBytes, return 413
        // immediately without waiting for the JS handler to respond.

        let response_head = if let Some(overflow_rx) = body_overflow_rx {
            tokio::select! {
                biased;
                Ok(()) = overflow_rx => {
                    // Body too large: ReqHeadCleanup RAII guard will remove the slab
                    // entry when this function exits (issue-7 fix).
                    let resp = hyper::Response::builder()
                        .status(StatusCode::PAYLOAD_TOO_LARGE)
                        .body(Body::full(Bytes::from_static(b"request body too large")))
                        .expect("valid 413 response");
                    return resp;
                }
                head = head_rx => {
                    match head {
                        Ok(h) => h,
                        Err(_) => return internal_error(b"JS handler dropped without responding"),
                    }
                }
            }
        } else {
            match head_rx.await {
                Ok(h) => h,
                Err(_) => return internal_error(b"JS handler dropped without responding"),
            }
        };

        // ── Duplex path: honor handler status, upgrade only on 101 ──────────────
        //
        // ISS-002: the handler may reject the duplex request by returning any
        // non-101 status.  Only perform the QUIC stream pump when the handler
        // explicitly returns 101 Switching Protocols.

        if let Some(upgrade_fut) = upgrade_future {
            let req_body_writer =
                duplex_req_body_writer.expect("duplex path always has req_body_writer");

            // If the handler returned a non-101 status, send that response and
            // do NOT perform the upgrade.  Drop the upgrade future and writer.
            if response_head.status != StatusCode::SWITCHING_PROTOCOLS.as_u16() {
                drop(upgrade_fut);
                drop(req_body_writer);
                let mut resp_builder = hyper::Response::builder().status(response_head.status);
                for (k, v) in &response_head.headers {
                    resp_builder = resp_builder.header(k.as_str(), v.as_str());
                }
                let resp = match resp_builder.body(Body::empty()) {
                    Ok(r) => r,
                    Err(_) => return internal_error(b"failed to build response head from JS"),
                };
                return resp;
            }

            // Spawn the upgrade pump after hyper delivers the 101.
            //
            // Both directions are wired to the channels already sent to JS:
            //   recv_io → req_body_writer  (JS reads via req_body_handle)
            //   res_body_reader → send_io  (JS writes via res_body_handle)
            tokio::spawn(async move {
                match upgrade_fut.await {
                    Err(e) => tracing::warn!("iroh-http: duplex upgrade error: {e}"),
                    Ok(upgraded) => {
                        let io = TokioIo::new(upgraded);
                        crate::stream::pump_duplex(io, req_body_writer, res_body_reader).await;
                    }
                }
            });

            // ISS-015: emit both Connection and Upgrade headers in 101 response.
            let resp = hyper::Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header(hyper::header::CONNECTION, "Upgrade")
                .header(hyper::header::UPGRADE, "iroh-duplex")
                .body(Body::empty())
                .expect("static response args are valid");
            return resp;
        }

        // ── Regular HTTP response ─────────────────────────────────────────────

        let body_stream = body_from_reader(res_body_reader);

        let mut resp_builder = hyper::Response::builder().status(response_head.status);
        for (k, v) in &response_head.headers {
            resp_builder = resp_builder.header(k.as_str(), v.as_str());
        }

        #[cfg(feature = "compression")]
        let resp_builder = resp_builder; // CompressionLayer in ServiceBuilder handles this

        match resp_builder.body(Body::new(body_stream)) {
            Ok(r) => r,
            Err(_) => internal_error(b"failed to build response head from JS"),
        }
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn on_request_fire(
    cb: &Arc<dyn Fn(RequestPayload) + Send + Sync>,
    req_handle: u64,
    req_body_handle: u64,
    res_body_handle: u64,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    remote_node_id: String,
    is_bidi: bool,
) {
    cb(RequestPayload {
        req_handle,
        req_body_handle,
        res_body_handle,
        method,
        url,
        headers,
        remote_node_id,
        is_bidi,
    });
}

// ── serve() ───────────────────────────────────────────────────────────────────

/// Start the serve accept loop.
///
/// This is the 3-argument form for backward compatibility.
/// Use `serve_with_events` to also receive peer connect/disconnect callbacks.
///
/// # Security
///
/// Calling `serve()` opens a **public endpoint** on the Iroh overlay network.
/// Unlike regular HTTP (where you choose whether to bind on `0.0.0.0`), any
/// peer that knows or discovers your node's public key can connect and send
/// requests. Iroh QUIC authenticates the peer's *identity* cryptographically,
/// but does not enforce *authorization*.
///
/// Always inspect `RequestPayload::peer_id` (exposed as the `Peer-Id` request
/// header at the FFI layer) and reject requests from untrusted peers:
///
/// ```ignore
/// serve(endpoint, ServeOptions::default(), |payload| {
///     if !ALLOWED_PEERS.contains(&payload.peer_id) {
///         respond(handles, payload.req_handle, 403, vec![]).ok();
///         return;
///     }
///     // ... handle request
/// });
/// ```
pub fn serve<F>(endpoint: IrohEndpoint, options: ServeOptions, on_request: F) -> ServeHandle
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    serve_with_events(endpoint, options, on_request, None)
}

/// Start the serve accept loop with an optional peer connection event callback.
///
/// `on_connection_event` is called on 0→1 (first connection from a peer) and
/// 1→0 (last connection from a peer closed) count transitions.
pub fn serve_with_events<F>(
    endpoint: IrohEndpoint,
    options: ServeOptions,
    on_request: F,
    on_connection_event: Option<ConnectionEventFn>,
) -> ServeHandle
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let max = options.max_concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let max_errors = options.max_serve_errors.unwrap_or(5);
    let request_timeout = options
        .request_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS));
    let max_conns_per_peer = options
        .max_connections_per_peer
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_PEER);
    let max_request_body_bytes = options
        .max_request_body_bytes
        .or(Some(DEFAULT_MAX_REQUEST_BODY_BYTES));
    let max_total_connections = options.max_total_connections;
    let drain_timeout =
        Duration::from_millis(options.drain_timeout_ms.unwrap_or(DEFAULT_DRAIN_TIMEOUT_MS));
    // Load-shed is opt-out — default `true` (reject immediately when at capacity).
    let load_shed_enabled = options.load_shed.unwrap_or(true);
    let max_header_size = endpoint.max_header_size();
    #[cfg(feature = "compression")]
    let compression = endpoint.compression().cloned();
    let own_node_id = Arc::new(endpoint.node_id().to_string());
    let on_request = Arc::new(on_request) as Arc<dyn Fn(RequestPayload) + Send + Sync>;

    let peer_counts: Arc<Mutex<HashMap<iroh::PublicKey, usize>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let conn_event_fn: Option<ConnectionEventFn> = on_connection_event;

    // In-flight request counter: incremented on accept, decremented on drop.
    // Used for graceful drain (wait until zero or timeout).
    let in_flight: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let drain_notify: Arc<tokio::sync::Notify> = Arc::new(tokio::sync::Notify::new());

    let dispatcher = Arc::new(FfiDispatcher {
        on_request,
        endpoint: endpoint.clone(),
        own_node_id,
        max_request_body_bytes,
        max_header_size: if max_header_size == 0 {
            None
        } else {
            Some(max_header_size)
        },
        #[cfg(feature = "compression")]
        compression,
    });

    let base_svc = IrohHttpService {
        dispatcher,
        remote_node_id: None,
    };

    use tower::{limit::ConcurrencyLimitLayer, Layer};
    // SEC-002: build the concurrency limiter once so all clones share one
    // Arc<Semaphore>, enforcing a true global request cap across every
    // connection and request task.
    let shared_conc = ConcurrencyLimitLayer::new(max).layer(base_svc);

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let shutdown_listen = shutdown_notify.clone();
    let drain_dur = drain_timeout;
    // Re-use the endpoint's shared counters so that endpoint_stats() reflects
    // the live connection and request counts at all times.
    let total_connections = endpoint.inner.active_connections.clone();
    let total_requests = endpoint.inner.active_requests.clone();
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    let endpoint_closed_tx = endpoint.inner.closed_tx.clone();

    let in_flight_drain = in_flight.clone();
    let drain_notify_drain = drain_notify.clone();

    let join = tokio::spawn(async move {
        let ep = endpoint.raw().clone();
        let mut consecutive_errors: usize = 0;

        loop {
            let incoming = tokio::select! {
                biased;
                _ = shutdown_listen.notified() => {
                    tracing::info!("iroh-http: serve loop shutting down");
                    break;
                }
                inc = ep.accept() => match inc {
                    Some(i) => i,
                    None => {
                        tracing::info!("iroh-http: endpoint closed (accept returned None)");
                        let _ = endpoint_closed_tx.send(true);
                        break;
                    }
                }
            };

            let conn = match incoming.await {
                Ok(c) => {
                    consecutive_errors = 0;
                    c
                }
                Err(e) => {
                    consecutive_errors = consecutive_errors.saturating_add(1);
                    tracing::warn!(
                        "iroh-http: accept error ({consecutive_errors}/{max_errors}): {e}"
                    );
                    if consecutive_errors >= max_errors {
                        tracing::error!("iroh-http: too many accept errors — shutting down");
                        break;
                    }
                    continue;
                }
            };

            let remote_pk = conn.remote_id();

            // Enforce total connection limit.
            if let Some(max_total) = max_total_connections {
                let current = total_connections.load(Ordering::Relaxed);
                if current >= max_total {
                    tracing::warn!(
                        "iroh-http: total connection limit reached ({current}/{max_total})"
                    );
                    conn.close(0u32.into(), b"server at capacity");
                    continue;
                }
            }

            let remote_id = base32_encode(remote_pk.as_bytes());

            let guard = match PeerConnectionGuard::acquire(
                &peer_counts,
                remote_pk,
                remote_id.clone(),
                max_conns_per_peer,
                conn_event_fn.clone(),
            ) {
                Some(g) => g,
                None => {
                    tracing::warn!("iroh-http: peer {remote_id} exceeded connection limit");
                    conn.close(0u32.into(), b"too many connections");
                    continue;
                }
            };

            let mut conn_conc = shared_conc.clone();
            conn_conc.get_mut().remote_node_id = Some(remote_id);

            let timeout_dur = if request_timeout.is_zero() {
                Duration::MAX
            } else {
                request_timeout
            };

            let conn_total = total_connections.clone();
            let conn_requests = total_requests.clone();
            let in_flight_conn = in_flight.clone();
            let drain_notify_conn = drain_notify.clone();
            conn_total.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let _guard = guard;
                // Decrement total connection count when this task exits.
                struct TotalGuard(Arc<AtomicUsize>);
                impl Drop for TotalGuard {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::Relaxed);
                    }
                }
                let _total_guard = TotalGuard(conn_total);

                loop {
                    let (send, recv) = match conn.accept_bi().await {
                        Ok(pair) => pair,
                        Err(_) => break,
                    };

                    let io = TokioIo::new(IrohStream::new(send, recv));
                    let svc = conn_conc.clone();
                    let req_counter = conn_requests.clone();
                    req_counter.fetch_add(1, Ordering::Relaxed);
                    in_flight_conn.fetch_add(1, Ordering::Relaxed);

                    let in_flight_req = in_flight_conn.clone();
                    let drain_notify_req = drain_notify_conn.clone();

                    tokio::spawn(async move {
                        // Decrement request count when this task exits.
                        struct ReqGuard {
                            counter: Arc<AtomicUsize>,
                            in_flight: Arc<AtomicUsize>,
                            drain_notify: Arc<tokio::sync::Notify>,
                        }
                        impl Drop for ReqGuard {
                            fn drop(&mut self) {
                                self.counter.fetch_sub(1, Ordering::Relaxed);
                                if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                                    // Last in-flight request completed — signal drain.
                                    self.drain_notify.notify_waiters();
                                }
                            }
                        }
                        let _req_guard = ReqGuard {
                            counter: req_counter,
                            in_flight: in_flight_req,
                            drain_notify: drain_notify_req,
                        };
                        // ISS-001: clamp to hyper's minimum safe buffer size of 8192.
                        // ISS-020: a stored value of 0 means "use the default" (64 KB).
                        let effective_header_limit = if max_header_size == 0 {
                            64 * 1024
                        } else {
                            max_header_size.max(8192)
                        };

                        // Build the Tower reliability service stack and serve the connection.
                        //
                        // Layer ordering (outermost first):
                        //   [CompressionLayer →] HandleLayerError → [LoadShed →] Timeout → IrohHttpService
                        //
                        // Both the compression layer (cfg-gated + runtime-opt) and the
                        // load-shed layer (runtime-opt) are conditionally inserted via
                        // `option_layer`, so the four feature × runtime combinations
                        // collapse to a single expression. `IrohHttpService::Error` is
                        // `Infallible`, so the only fallible boundary in the stack is
                        // the `TimeoutLayer` / `LoadShedLayer` pair; `HandleLayerError`
                        // converts their `Elapsed` / `Overloaded` errors into 408 / 503
                        // responses so hyper only ever sees `Ok(Response)`.

                        use tower::{timeout::TimeoutLayer, ServiceBuilder};

                        #[cfg(feature = "compression")]
                        let compression_layer = {
                            use http::{Extensions, HeaderMap, Version};
                            use tower_http::compression::{
                                predicate::{Predicate, SizeAbove},
                                CompressionLayer, CompressionLevel,
                            };
                            svc.get_ref().dispatcher.compression.as_ref().map(|comp| {
                                let mut layer = CompressionLayer::new().zstd(true);
                                if let Some(level) = comp.level {
                                    layer =
                                        layer.quality(CompressionLevel::Precise(level as i32));
                                }
                                let not_pre_compressed =
                                    |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
                                        !h.contains_key(http::header::CONTENT_ENCODING)
                                    };
                                let not_no_transform =
                                    |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
                                        h.get(http::header::CACHE_CONTROL)
                                            .and_then(|v| v.to_str().ok())
                                            .map(|v| {
                                                !v.split(',').any(|d| {
                                                    d.trim().eq_ignore_ascii_case("no-transform")
                                                })
                                            })
                                            .unwrap_or(true)
                                    };
                                let not_opaque_content_type =
                                    |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
                                        let ct = match h
                                            .get(http::header::CONTENT_TYPE)
                                            .and_then(|v| v.to_str().ok())
                                        {
                                            Some(v) => v,
                                            None => return true,
                                        };
                                        let media = ct.split(';').next().unwrap_or(ct).trim();
                                        let skip = [
                                            "application/zstd",
                                            "application/octet-stream",
                                        ];
                                        if skip.iter().any(|s| media.eq_ignore_ascii_case(s)) {
                                            return false;
                                        }
                                        if let Some(top) = media.split('/').next() {
                                            let top = top.trim();
                                            if top.eq_ignore_ascii_case("image")
                                                || top.eq_ignore_ascii_case("audio")
                                                || top.eq_ignore_ascii_case("video")
                                            {
                                                return false;
                                            }
                                        }
                                        true
                                    };
                                let predicate = SizeAbove::new(
                                    comp.min_body_bytes.min(u16::MAX as usize) as u16,
                                )
                                .and(not_pre_compressed)
                                .and(not_no_transform)
                                .and(not_opaque_content_type);
                                layer.compress_when(predicate)
                            })
                        };
                        #[cfg(not(feature = "compression"))]
                        let compression_layer: Option<tower::layer::util::Identity> = None;

                        let load_shed_layer = if load_shed_enabled {
                            Some(tower::load_shed::LoadShedLayer::new())
                        } else {
                            None
                        };

                        let core_stack = ServiceBuilder::new()
                            .layer(HandleLayerErrorLayer)
                            .option_layer(load_shed_layer)
                            .layer(TimeoutLayer::new(timeout_dur))
                            .service(svc);

                        let mut builder = hyper::server::conn::http1::Builder::new();
                        builder
                            .max_buf_size(effective_header_limit)
                            .max_headers(128);

                        #[cfg(feature = "compression")]
                        let result = {
                            use tower_http::decompression::RequestDecompressionLayer;
                            let req_decomp = RequestDecompressionLayer::new();
                            if let Some(comp) = compression_layer {
                                let stack = ServiceBuilder::new()
                                    .layer(req_decomp)
                                    .layer(comp)
                                    .service(core_stack);
                                builder
                                    .serve_connection(io, TowerToHyperService::new(stack))
                                    .with_upgrades()
                                    .await
                            } else {
                                let stack =
                                    ServiceBuilder::new().layer(req_decomp).service(core_stack);
                                builder
                                    .serve_connection(io, TowerToHyperService::new(stack))
                                    .with_upgrades()
                                    .await
                            }
                        };
                        #[cfg(not(feature = "compression"))]
                        let result = {
                            let _ = compression_layer;
                            builder
                                .serve_connection(io, TowerToHyperService::new(core_stack))
                                .with_upgrades()
                                .await
                        };

                        if let Err(e) = result {
                            tracing::debug!("iroh-http: http1 connection error: {e}");
                        }
                    });
                }
            });
        }

        // Graceful drain: wait for all in-flight requests to finish,
        // or give up after `drain_timeout`.
        //
        // Loop avoids the race between `in_flight == 0` check and `notified()`:
        // if the last request finishes between the load and the await, the loop
        // re-checks immediately after the timeout wakes it.
        let deadline = tokio::time::Instant::now()
            .checked_add(drain_dur)
            .expect("drain duration overflow");
        loop {
            if in_flight_drain.load(Ordering::Acquire) == 0 {
                tracing::info!("iroh-http: all in-flight requests drained");
                break;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::warn!("iroh-http: drain timed out after {}s", drain_dur.as_secs());
                break;
            }
            tokio::select! {
                _ = drain_notify_drain.notified() => {}
                _ = tokio::time::sleep(remaining) => {}
            }
        }
        let _ = done_tx.send(true);
    });

    ServeHandle {
        join,
        shutdown_notify,
        drain_timeout: drain_dur,
        done_rx,
    }
}

// ── TowerErrorHandler — maps Tower layer errors to HTTP responses ─────────────
//
// `ConcurrencyLimitLayer`, `TimeoutLayer`, and `LoadShedLayer` return errors
// rather than `Response` values when they reject a request.  `TowerErrorHandler`
// wraps the composed service and converts those errors to proper HTTP responses:
//
//   tower::timeout::error::Elapsed     → 408 Request Timeout
//   tower::load_shed::error::Overloaded → 503 Service Unavailable
//   anything else                       → 500 Internal Server Error
//
// This allows the whole stack to satisfy hyper's requirement that the service
// returns `Ok(Response)` — errors crash the connection instead of producing a
// status code.

#[derive(Clone)]
struct TowerErrorHandler<S>(S);

/// `Layer` adapter that wraps a fallible inner service with [`TowerErrorHandler`].
///
/// Composes inside [`tower::ServiceBuilder`] so the serve-loop wiring stays
/// a single expression. Per ADR-014 the only fallible boundary in the serve
/// stack is the `TimeoutLayer` / `LoadShedLayer` pair; this layer converts
/// their `Elapsed` / `Overloaded` errors into 408 / 503 responses so hyper
/// only ever sees `Ok(Response)`.
#[derive(Clone, Default)]
struct HandleLayerErrorLayer;

impl<S> tower::Layer<S> for HandleLayerErrorLayer {
    type Service = TowerErrorHandler<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TowerErrorHandler(inner)
    }
}

impl<S, Req> Service<Req> for TowerErrorHandler<S>
where
    S: Service<Req, Response = hyper::Response<Body>>,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
{
    type Response = hyper::Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // If ConcurrencyLimitLayer is saturated AND LoadShed is present, it
        // returns Pending from poll_ready — LoadShed converts that to an
        // immediate Err(Overloaded). If LoadShed is absent, poll_ready blocks
        // until a slot opens. In both cases the inner service signals readiness
        // here; layer errors are handled in `call`, never surfaced via
        // `poll_ready`.
        match self.0.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Ok(())),
        }
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let fut = self.0.call(req);
        Box::pin(async move {
            match fut.await {
                Ok(r) => Ok(r),
                Err(e) => {
                    let e = e.into();
                    let status = if e.is::<tower::timeout::error::Elapsed>() {
                        StatusCode::REQUEST_TIMEOUT
                    } else if e.is::<tower::load_shed::error::Overloaded>() {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        tracing::warn!("iroh-http: unexpected tower error: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    };
                    let body_bytes: &'static [u8] = match status {
                        StatusCode::REQUEST_TIMEOUT => b"request timed out",
                        StatusCode::SERVICE_UNAVAILABLE => b"server at capacity",
                        _ => b"internal server error",
                    };
                    Ok(hyper::Response::builder()
                        .status(status)
                        .body(Body::full(Bytes::from_static(body_bytes)))
                        .expect("valid error response"))
                }
            }
        })
    }
}
