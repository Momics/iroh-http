//! Incoming HTTP request — pure-Rust `serve()` implementation.
//!
//! Each accepted QUIC bidirectional stream is driven by hyper's HTTP/1.1
//! server connection. The user supplies a `tower::Service<Request<Body>,
//! Response = Response<Body>, Error = Infallible>`; the per-connection
//! `AddExtensionLayer` makes the authenticated peer id available as a
//! [`RemoteNodeId`] request extension (closes #177).
//!
//! The FFI-shaped callback API ([`crate::ffi::dispatcher::serve_with_callback`])
//! is one specific consumer of this entry — it constructs an
//! `IrohHttpService` around the JS callback and hands it in like any
//! other service.

pub(crate) mod lifecycle;
pub(crate) mod pipeline;
pub(crate) mod stack;

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
use http::StatusCode;
use hyper_util::rt::TokioIo;
use tower::Service;

use crate::{base32_encode, http::transport::io::IrohStream, ConnectionEvent, IrohEndpoint};

use self::lifecycle::{ConnectionTracker, RequestTracker};

// ── Type aliases ──────────────────────────────────────────────────────────────

use crate::Body;
use crate::BoxError;

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

// ── Connection-event callback type ───────────────────────────────────────────

pub(crate) type ConnectionEventFn = Arc<dyn Fn(ConnectionEvent) + Send + Sync>;

// `ConnectionTracker` and `RequestTracker` (formerly inline
// PeerConnectionGuard / TotalGuard / ReqGuard) live in `lifecycle.rs`.
// The FFI dispatcher and respond() live in `crate::ffi::dispatcher`.

/// Authenticated peer node id of the QUIC connection a request arrived
/// on. Inserted as a request extension by the per-connection
/// [`tower_http::add_extension::AddExtensionLayer`] in
/// [`serve_service_with_events`].
///
/// User-facing pure-Rust services consume it with
/// `req.extensions().get::<RemoteNodeId>()`. Closes #177.
#[derive(Clone, Debug)]
pub struct RemoteNodeId(pub Arc<String>);

/// Pure-Rust serve entry — convenience 3-arg wrapper that omits the
/// connection-event callback. Equivalent to `serve_service_with_events(ep,
/// opts, svc, None)`.
pub fn serve_service<S>(endpoint: IrohEndpoint, options: ServeOptions, svc: S) -> ServeHandle
where
    S: Service<
            hyper::Request<Body>,
            Response = hyper::Response<Body>,
            Error = std::convert::Infallible,
        > + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    serve_service_with_events(endpoint, options, svc, None)
}

/// Pure-Rust serve entry — the canonical inbound API.
///
/// Accepts any `tower::Service<Request<Body>, Response = Response<Body>,
/// Error = Infallible>` (`Clone + Send + Sync + 'static`, with
/// `Send` futures). Each accepted QUIC bidirectional stream is driven by
/// hyper's HTTP/1.1 server connection through the per-connection tower
/// stack composed in [`stack::build_stack`]; the user service sees
/// requests with the authenticated peer id available as a typed
/// [`RemoteNodeId`] request extension.
///
/// `on_connection_event` is called on 0→1 (first connection from a peer)
/// and 1→0 (last connection from a peer closed) count transitions.
///
/// # Security
///
/// Calling this opens a **public endpoint** on the Iroh overlay network.
/// Any peer that knows or discovers your node's public key can connect
/// and send requests. Iroh QUIC authenticates the peer's *identity*
/// cryptographically, but does not enforce *authorization*. Inspect
/// [`RemoteNodeId`] in your service and reject untrusted peers.
pub fn serve_service_with_events<S>(
    endpoint: IrohEndpoint,
    options: ServeOptions,
    svc: S,
    on_connection_event: Option<ConnectionEventFn>,
) -> ServeHandle
where
    S: Service<
            hyper::Request<Body>,
            Response = hyper::Response<Body>,
            Error = std::convert::Infallible,
        > + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
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
    let stack_compression = endpoint.compression().cloned();

    let peer_counts: Arc<Mutex<HashMap<iroh::PublicKey, usize>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let conn_event_fn: Option<ConnectionEventFn> = on_connection_event;

    // In-flight request counter: incremented on accept, decremented on drop.
    // Used for graceful drain (wait until zero or timeout).
    let in_flight: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let drain_notify: Arc<tokio::sync::Notify> = Arc::new(tokio::sync::Notify::new());

    use tower::{limit::ConcurrencyLimitLayer, ServiceBuilder, ServiceExt};
    // SEC-002: build the concurrency limiter as a *layer* once so every
    // per-connection stack we wrap with it shares the same `Arc<Semaphore>`,
    // enforcing a true global request cap across all connections.
    let conc_layer = ConcurrencyLimitLayer::new(max);

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let shutdown_listen = shutdown_notify.clone();
    let drain_dur = drain_timeout;
    // Re-use the endpoint's shared counters so that endpoint_stats() reflects
    // the live connection and request counts at all times.
    let total_connections = endpoint.inner.http.active_connections.clone();
    let total_requests = endpoint.inner.http.active_requests.clone();
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    let endpoint_closed_tx = endpoint.inner.session.closed_tx.clone();

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

            let conn_tracker = match ConnectionTracker::acquire(
                &peer_counts,
                remote_pk,
                remote_id.clone(),
                max_conns_per_peer,
                conn_event_fn.clone(),
                total_connections.clone(),
            ) {
                Some(g) => g,
                None => {
                    tracing::warn!("iroh-http: peer {remote_id} exceeded connection limit");
                    conn.close(0u32.into(), b"too many connections");
                    continue;
                }
            };

            // Build the per-connection service: user-supplied `svc` with the
            // peer's `remote_node_id` injected as a [`RemoteNodeId`] request
            // extension (closes #177), wrapped with the shared concurrency
            // limiter, then type-erased into ServeService. Per ADR-014 D2 /
            // #175 this is the *only* place that names the concrete inner
            // stack — every downstream consumer sees the box.
            let conn_svc: crate::http::server::pipeline::ServeService = ServiceBuilder::new()
                .layer(conc_layer.clone())
                .layer(tower_http::add_extension::AddExtensionLayer::new(
                    RemoteNodeId(Arc::new(remote_id)),
                ))
                .service(svc.clone())
                .boxed_clone();
            let timeout_dur = if request_timeout.is_zero() {
                Duration::MAX
            } else {
                request_timeout
            };

            let conn_requests = total_requests.clone();
            let in_flight_conn = in_flight.clone();
            let drain_notify_conn = drain_notify.clone();
            let stack_compression_conn = stack_compression.clone();
            tokio::spawn(async move {
                // Owns the per-peer count, total-connection counter, and
                // connect/disconnect event firing for this connection's
                // lifetime. See `lifecycle.rs`.
                let _conn_tracker = conn_tracker;

                loop {
                    let (send, recv) = match conn.accept_bi().await {
                        Ok(pair) => pair,
                        Err(_) => break,
                    };

                    let io = TokioIo::new(IrohStream::new(send, recv));
                    let svc = conn_svc.clone();
                    let req_counter = conn_requests.clone();
                    req_counter.fetch_add(1, Ordering::Relaxed);
                    in_flight_conn.fetch_add(1, Ordering::Relaxed);

                    let in_flight_req = in_flight_conn.clone();
                    let drain_notify_req = drain_notify_conn.clone();
                    let req_compression = stack_compression_conn.clone();

                    tokio::spawn(async move {
                        // Owns the per-connection and crate-wide in-flight
                        // counters; notifies drain waiters when in-flight
                        // reaches zero. See `lifecycle.rs`.
                        let _req_tracker =
                            RequestTracker::new(req_counter, in_flight_req, drain_notify_req);
                        // ISS-001: clamp to hyper's minimum safe buffer size of 8192.
                        // ISS-020: a stored value of 0 means "use the default" (64 KB).
                        let effective_header_limit = if max_header_size == 0 {
                            64 * 1024
                        } else {
                            max_header_size.max(8192)
                        };

                        // Build the per-bistream tower pipeline (compression,
                        // decompression, body limit, load-shed, timeout, layer-error
                        // handling) and serve the connection. The full assembly
                        // lives in [`crate::http::server::stack::build_stack`] —
                        // see Slice B of #182 (issue #184). The hyper seam plus
                        // header-limit live in [`crate::http::server::pipeline`].
                        let cfg = crate::http::server::stack::StackConfig {
                            timeout: if timeout_dur == Duration::MAX {
                                None
                            } else {
                                Some(timeout_dur)
                            },
                            max_request_body_bytes,
                            load_shed: load_shed_enabled,
                            compression: req_compression,
                            decompression: true,
                        };
                        crate::http::server::pipeline::serve_bistream(
                            io,
                            svc,
                            effective_header_limit,
                            &cfg,
                        )
                        .await;
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

// ── HandleLayerError — convert tower-layer errors to HTTP responses ──────────
//
// ADR-013 ("Lean on the ecosystem") justification: tower itself, tower-http,
// and hyper-util do not ship an "error → response" adapter. axum has
// `axum::error_handling::HandleErrorLayer`, but pulling axum into the runtime
// just for this seam would invert the dependency direction (axum sits *on
// top of* tower; iroh-http-core lives one level lower). `HandleLayerError` is
// a ~50-line bespoke layer that exists solely because that gap in the
// ecosystem hasn't been filled — every other layer in the serve stack is a
// stock `tower-http` / `tower` building block.
//
// `ConcurrencyLimitLayer`, `TimeoutLayer`, and `LoadShedLayer` return errors
// rather than `Response` values when they reject a request. This adapter
// catches them and renders an HTTP response so hyper only ever sees
// `Ok(Response)`:
//
//   tower::timeout::error::Elapsed      → 408 Request Timeout
//   tower::load_shed::error::Overloaded → 503 Service Unavailable
//   anything else                        → 500 Internal Server Error

/// `Layer` form: insert in any `tower::ServiceBuilder` pipeline that contains
/// a `TimeoutLayer` and/or `LoadShedLayer` to convert their errors into HTTP
/// responses. Wraps the inner service with [`HandleLayerError`].
#[derive(Clone, Default)]
pub(crate) struct HandleLayerErrorLayer;

impl<S> tower::Layer<S> for HandleLayerErrorLayer {
    type Service = HandleLayerError<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HandleLayerError(inner)
    }
}

#[derive(Clone)]
pub(crate) struct HandleLayerError<S>(S);

impl<S, Req> Service<Req> for HandleLayerError<S>
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
