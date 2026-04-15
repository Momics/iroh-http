//! Incoming HTTP request — `serve()` implementation.
//!
//! Each accepted QUIC bidirectional stream is driven by hyper's HTTP/1.1
//! server connection.  A `tower::Service` (`RequestService`) bridges between
//! hyper and the existing body-channel + slab infrastructure.

use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tower::Service;

use crate::{
    base32_encode,
    client::{body_from_reader, pump_hyper_body_to_channel_limited},
    io::IrohStream,
    stream::{
        allocate_req_handle, drain_timeout, insert_reader, insert_trailer_receiver,
        insert_trailer_sender, insert_writer, make_body_channel, remove_trailer_sender,
        take_req_sender, ResponseHeadEntry,
    },
    CoreError, IrohEndpoint, RequestPayload,
};

// ── Type aliases ──────────────────────────────────────────────────────────────

type BoxBody = http_body_util::combinators::BoxBody<Bytes, Infallible>;
type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn box_body<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes, Error = Infallible> + Send + Sync + 'static,
{
    body.map_err(|_| unreachable!()).boxed()
}

// ── ServerLimits ──────────────────────────────────────────────────────────────

/// Server-side limits shared between [`NodeOptions`](crate::NodeOptions) and
/// the serve path.
///
/// Embedding this struct in both `NodeOptions` and `EndpointInner` guarantees
/// that adding a new limit field produces a compile error if only one side is
/// updated.
#[derive(Debug, Clone, Default)]
pub struct ServerLimits {
    pub max_concurrency: Option<usize>,
    pub max_consecutive_errors: Option<usize>,
    pub request_timeout_ms: Option<u64>,
    pub max_connections_per_peer: Option<usize>,
    pub max_request_body_bytes: Option<usize>,
    pub drain_timeout_secs: Option<u64>,
}

/// Backward-compatible alias — existing code that names `ServeOptions` keeps
/// compiling without changes.
pub type ServeOptions = ServerLimits;

const DEFAULT_CONCURRENCY: usize = 64;
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_CONNECTIONS_PER_PEER: usize = 8;
const DEFAULT_DRAIN_TIMEOUT_SECS: u64 = 30;

// ── ServeHandle ───────────────────────────────────────────────────────────────

pub struct ServeHandle {
    join: tokio::task::JoinHandle<()>,
    shutdown_notify: Arc<tokio::sync::Notify>,
    drain_timeout: std::time::Duration,
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
}

// ── respond() ────────────────────────────────────────────────────────────────

pub fn respond(
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

    let sender =
        take_req_sender(req_handle).ok_or_else(|| CoreError::invalid_handle(req_handle as u32))?;
    sender
        .send(ResponseHeadEntry { status, headers })
        .map_err(|_| CoreError::internal("serve task dropped before respond"))
}

// ── PeerConnectionGuard ───────────────────────────────────────────────────────

struct PeerConnectionGuard {
    counts: Arc<Mutex<HashMap<iroh::PublicKey, usize>>>,
    peer: iroh::PublicKey,
}

impl PeerConnectionGuard {
    fn acquire(
        counts: &Arc<Mutex<HashMap<iroh::PublicKey, usize>>>,
        peer: iroh::PublicKey,
        max: usize,
    ) -> Option<Self> {
        let mut map = counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = map.entry(peer).or_insert(0);
        if *count >= max {
            return None;
        }
        *count += 1;
        Some(PeerConnectionGuard {
            counts: counts.clone(),
            peer,
        })
    }
}

impl Drop for PeerConnectionGuard {
    fn drop(&mut self) {
        let mut map = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = map.get_mut(&self.peer) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                map.remove(&self.peer);
            }
        }
    }
}

// ── RequestService ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct RequestService {
    on_request: Arc<dyn Fn(RequestPayload) + Send + Sync>,
    ep_idx: u32,
    own_node_id: Arc<String>,
    remote_node_id: Option<String>,
    max_request_body_bytes: Option<usize>,
    max_header_size: Option<usize>,
    #[cfg(feature = "compression")]
    compression: Option<crate::endpoint::CompressionOptions>,
}

impl Service<hyper::Request<Incoming>> for RequestService {
    type Response = hyper::Response<BoxBody>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<Incoming>) -> Self::Future {
        let svc = self.clone();
        Box::pin(async move { svc.handle(req).await })
    }
}

impl RequestService {
    async fn handle(
        self,
        mut req: hyper::Request<Incoming>,
    ) -> Result<hyper::Response<BoxBody>, BoxError> {
        let ep_idx = self.ep_idx;
        let own_node_id = &*self.own_node_id;
        let remote_node_id = self.remote_node_id.clone().unwrap_or_default();
        let max_request_body_bytes = self.max_request_body_bytes;
        let max_header_size = self.max_header_size;

        let method = req.method().to_string();
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/")
            .to_string();
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
                .map(|(k, v)| k.as_str().len() + v.as_bytes().len() + 4) // ": " + "\r\n"
                .sum::<usize>()
                + "peer-id".len() + remote_node_id.len() + 4
                + req.uri().to_string().len()
                + method.len()
                + 12; // "HTTP/1.1 \r\n\r\n" overhead
            if header_bytes > limit {
                let resp = hyper::Response::builder()
                    .status(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE)
                    .body(box_body(http_body_util::Empty::new()))
                    .unwrap();
                return Ok(resp);
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
                        .body(box_body(http_body_util::Full::new(Bytes::from_static(
                            b"non-UTF8 header value",
                        ))))
                        .unwrap();
                    return Ok(resp);
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
                    .body(box_body(http_body_util::Full::new(Bytes::from_static(
                        b"duplex upgrade requires CONNECT method with Connection: upgrade header",
                    ))))
                    .unwrap();
                return Ok(resp);
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
        let (req_body_writer, req_body_reader) = make_body_channel();
        let req_body_handle = insert_reader(ep_idx, req_body_reader);

        // Response body: writer given to JS (sendChunk); reader feeds hyper response.
        let (res_body_writer, res_body_reader) = make_body_channel();
        let res_body_handle = insert_writer(ep_idx, res_body_writer);

        // ── Trailer channels (non-duplex only) ───────────────────────────────

        let (req_trailers_handle, res_trailers_handle, req_trailer_tx, opt_res_trailer_rx) =
            if !is_bidi {
                // Request trailers: pump delivers them; JS reads via nextTrailer.
                let (rq_tx, rq_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
                let rq_h = insert_trailer_receiver(ep_idx, rq_rx);
                // Response trailers: JS delivers via sendTrailers; pump appends to body.
                let (rs_tx, rs_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
                let rs_h = insert_trailer_sender(ep_idx, rs_tx);
                (rq_h, rs_h, Some(rq_tx), Some(rs_rx))
            } else {
                (0u64, 0u64, None, None)
            };

        // ── Allocate response-head rendezvous ────────────────────────────────

        let (head_tx, head_rx) = tokio::sync::oneshot::channel::<ResponseHeadEntry>();
        let req_handle = allocate_req_handle(ep_idx, head_tx);

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
            let trailer_tx = req_trailer_tx.expect("non-duplex has req_trailer_tx");
            tokio::spawn(pump_hyper_body_to_channel_limited(
                body,
                req_body_writer,
                trailer_tx,
                max_request_body_bytes,
                drain_timeout(),
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
            req_trailers_handle,
            res_trailers_handle,
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
                _ = overflow_rx => {
                    // Body too large: head_rx is dropped automatically on return,
                    // causing the JS respond() call to fail gracefully.
                    let resp = hyper::Response::builder()
                        .status(StatusCode::PAYLOAD_TOO_LARGE)
                        .body(box_body(http_body_util::Full::new(Bytes::from_static(
                            b"request body too large",
                        ))))
                        .expect("valid 413 response");
                    return Ok(resp);
                }
                head = head_rx => {
                    head.map_err(|_| -> BoxError { "JS handler dropped without responding".into() })?
                }
            }
        } else {
            head_rx
                .await
                .map_err(|_| -> BoxError { "JS handler dropped without responding".into() })?
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
                let resp = resp_builder
                    .body(box_body(http_body_util::Empty::new()))
                    .map_err(|e| -> BoxError { e.into() })?;
                return Ok(resp);
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
                        let (mut recv_io, mut send_io) = tokio::io::split(io);

                        tokio::join!(
                            // upgraded recv → req_body channel (JS reads via req_body_handle)
                            async {
                                use tokio::io::AsyncReadExt;
                                let mut buf = vec![0u8; 16 * 1024];
                                loop {
                                    match recv_io.read(&mut buf).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(n) => {
                                            if req_body_writer
                                                .send_chunk(Bytes::copy_from_slice(&buf[..n]))
                                                .await
                                                .is_err()
                                            {
                                                break;
                                            }
                                        }
                                    }
                                }
                                // Dropping writer signals EOF on req_body_handle.
                                drop(req_body_writer);
                            },
                            // res_body channel → upgraded send (JS writes via res_body_handle)
                            async {
                                use tokio::io::AsyncWriteExt;
                                loop {
                                    match res_body_reader.next_chunk().await {
                                        None => break,
                                        Some(chunk) => {
                                            if send_io.write_all(&chunk).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                                let _ = send_io.shutdown().await;
                            },
                        );
                    }
                }
            });

            // ISS-015: emit both Connection and Upgrade headers in 101 response.
            let resp = hyper::Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header(hyper::header::CONNECTION, "Upgrade")
                .header(hyper::header::UPGRADE, "iroh-duplex")
                .body(box_body(http_body_util::Empty::new()))
                .unwrap();
            return Ok(resp);
        }

        // ── Regular HTTP response ─────────────────────────────────────────────

        let has_trailer_hdr = response_head
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("trailer"));
        let trailer_rx_for_body = if has_trailer_hdr {
            opt_res_trailer_rx
        } else {
            remove_trailer_sender(res_trailers_handle);
            None
        };

        let body_stream = body_from_reader(res_body_reader, trailer_rx_for_body);

        let mut resp_builder = hyper::Response::builder().status(response_head.status);
        for (k, v) in &response_head.headers {
            resp_builder = resp_builder.header(k.as_str(), v.as_str());
        }

        #[cfg(feature = "compression")]
        let resp_builder = resp_builder; // CompressionLayer in ServiceBuilder handles this

        let resp = resp_builder
            .body(box_body(body_stream))
            .map_err(|e| -> BoxError { e.into() })?;

        Ok(resp)
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn on_request_fire(
    cb: &Arc<dyn Fn(RequestPayload) + Send + Sync>,
    req_handle: u64,
    req_body_handle: u64,
    res_body_handle: u64,
    req_trailers_handle: u64,
    res_trailers_handle: u64,
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
        req_trailers_handle,
        res_trailers_handle,
        method,
        url,
        headers,
        remote_node_id,
        is_bidi,
    });
}

// ── serve() ───────────────────────────────────────────────────────────────────

pub fn serve<F>(endpoint: IrohEndpoint, options: ServeOptions, on_request: F) -> ServeHandle
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let max = options.max_concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let max_errors = options.max_consecutive_errors.unwrap_or(5);
    let request_timeout = options
        .request_timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS));
    let max_conns_per_peer = options
        .max_connections_per_peer
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_PEER);
    let max_request_body_bytes = options.max_request_body_bytes;
    let drain_timeout = std::time::Duration::from_secs(
        options
            .drain_timeout_secs
            .unwrap_or(DEFAULT_DRAIN_TIMEOUT_SECS),
    );
    let max_header_size = endpoint.max_header_size();
    #[cfg(feature = "compression")]
    let compression = endpoint.compression().cloned();
    let ep_idx = endpoint.inner.endpoint_idx;
    let own_node_id = Arc::new(endpoint.node_id().to_string());
    let on_request = Arc::new(on_request) as Arc<dyn Fn(RequestPayload) + Send + Sync>;

    let peer_counts: Arc<Mutex<HashMap<iroh::PublicKey, usize>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Drain semaphore: one permit per in-flight REQUEST (bi-stream), not per connection.
    // Drain waits for acquire_many(max) which returns only when all requests finish.
    let drain_semaphore = Arc::new(tokio::sync::Semaphore::new(max));

    let base_svc = RequestService {
        on_request,
        ep_idx,
        own_node_id,
        remote_node_id: None,
        max_request_body_bytes,
        max_header_size: if max_header_size == 0 {
            None
        } else {
            Some(max_header_size)
        },
        #[cfg(feature = "compression")]
        compression,
    };

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let shutdown_listen = shutdown_notify.clone();
    let drain_sem = drain_semaphore.clone();
    let drain_dur = drain_timeout;

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
                    None => break,
                }
            };

            let conn = match incoming.await {
                Ok(c) => {
                    consecutive_errors = 0;
                    c
                }
                Err(e) => {
                    consecutive_errors += 1;
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
            let guard =
                match PeerConnectionGuard::acquire(&peer_counts, remote_pk, max_conns_per_peer) {
                    Some(g) => g,
                    None => {
                        tracing::warn!(
                            "iroh-http: peer {} exceeded connection limit",
                            base32_encode(remote_pk.as_bytes())
                        );
                        conn.close(0u32.into(), b"too many connections");
                        continue;
                    }
                };

            let remote_id = base32_encode(remote_pk.as_bytes());
            let mut peer_svc = base_svc.clone();
            peer_svc.remote_node_id = Some(remote_id);

            let timeout_dur = if request_timeout.is_zero() {
                std::time::Duration::MAX
            } else {
                request_timeout
            };

            let conn_drain = drain_semaphore.clone();
            tokio::spawn(async move {
                let _guard = guard;

                loop {
                    let (send, recv) = match conn.accept_bi().await {
                        Ok(pair) => pair,
                        Err(_) => break,
                    };

                    // Acquire one concurrency slot per request (bi-stream).
                    // Dropping the permit when hyper finishes the request signals drain().
                    let permit = match conn_drain.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break, // semaphore closed → shutting down
                    };

                    let io = TokioIo::new(IrohStream::new(send, recv));
                    let svc = peer_svc.clone();

                    tokio::spawn(async move {
                        let _permit = permit;
                        // Build the hyper-facing service, optionally wrapping with
                        // CompressionLayer (zstd-only, for responses ≥ min_body_bytes).
                        #[cfg(feature = "compression")]
                        let hyper_svc = {
                            use tower_http::compression::{predicate::SizeAbove, CompressionLayer};
                            let min_bytes = svc
                                .compression
                                .as_ref()
                                .map(|c| c.min_body_bytes)
                                .unwrap_or(512);
                            let mut layer = CompressionLayer::new().zstd(true);
                            if let Some(level) = svc.compression.as_ref().and_then(|c| c.level) {
                                use tower_http::compression::CompressionLevel;
                                layer = layer.quality(CompressionLevel::Precise(level as i32));
                            }
                            TowerToHyperService::new(
                                tower::ServiceBuilder::new()
                                    .layer(layer.compress_when(SizeAbove::new(min_bytes as u16)))
                                    .service(TimeoutService::new(svc, timeout_dur)),
                            )
                        };
                        #[cfg(not(feature = "compression"))]
                        let hyper_svc =
                            TowerToHyperService::new(TimeoutService::new(svc, timeout_dur));

                        // ISS-001: clamp to hyper's minimum safe buffer size of 8192.
                        // ISS-020: a stored value of 0 means "use the default" (64 KB).
                        let effective_header_limit = if max_header_size == 0 {
                            64 * 1024
                        } else {
                            max_header_size.max(8192)
                        };
                        let result = hyper::server::conn::http1::Builder::new()
                            .max_buf_size(effective_header_limit)
                            .max_headers(128)
                            .serve_connection(io, hyper_svc)
                            .with_upgrades()
                            .await;
                        if let Err(e) = result {
                            tracing::debug!("iroh-http: http1 connection error: {e}");
                        }
                    });
                }
            });
        }

        let drain_result =
            tokio::time::timeout(drain_dur, drain_sem.acquire_many(max as u32)).await;
        match drain_result {
            Ok(Ok(_)) => tracing::info!("iroh-http: all in-flight requests drained"),
            Ok(Err(_)) => tracing::warn!("iroh-http: semaphore closed during drain"),
            Err(_) => tracing::warn!("iroh-http: drain timed out after {}s", drain_dur.as_secs()),
        }
    });

    ServeHandle {
        join,
        shutdown_notify,
        drain_timeout: drain_dur,
    }
}

// ── TimeoutService — thin per-request timeout wrapper ────────────────────────

#[derive(Clone)]
struct TimeoutService<S> {
    inner: S,
    timeout: std::time::Duration,
}

impl<S> TimeoutService<S> {
    fn new(inner: S, timeout: std::time::Duration) -> Self {
        Self { inner, timeout }
    }
}

impl<S, Req> Service<Req> for TimeoutService<S>
where
    // ISS-007: constrain S::Response to hyper::Response<BoxBody> so we can return
    // a concrete 408 response on timeout rather than a generic error string.
    S: Service<Req, Response = hyper::Response<BoxBody>>,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
{
    type Response = hyper::Response<BoxBody>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let fut = self.inner.call(req);
        let timeout = self.timeout;
        Box::pin(async move {
            match tokio::time::timeout(timeout, fut).await {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => {
                    // ISS-007: return a proper HTTP 408 Request Timeout instead of a
                    // generic error string so adapters can relay the status code.
                    Ok(hyper::Response::builder()
                        .status(StatusCode::REQUEST_TIMEOUT)
                        .body(box_body(http_body_util::Full::new(Bytes::from_static(
                            b"request timed out",
                        ))))
                        .expect("valid 408 response"))
                }
            }
        })
    }
}
