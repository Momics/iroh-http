//! Slice D of #182 (issue #186) — acceptance criterion #1: pure-Rust
//! [`iroh_http_core::fetch_request`] round-trips a [`hyper::Request<Body>`]
//! through [`iroh_http_core::serve_service`] and returns
//! [`hyper::Response<Body>`] with a typed [`iroh_http_core::FetchError`].
//!
//! No `u64` body handles, no `BodyReader`, no `FfiResponse`. Just `tower`
//! and `hyper`. This is the structural proof that the client side is now
//! shaped exactly like the server side.

mod common;

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body_util::BodyExt;
use iroh_http_core::{fetch_request, serve_service, Body, RemoteNodeId, ServeOptions, StackConfig};
use tower::Service;

#[derive(Clone)]
struct EchoPeerService;

impl Service<hyper::Request<Body>> for EchoPeerService {
    type Response = hyper::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<Body>) -> Self::Future {
        let peer = req
            .extensions()
            .get::<RemoteNodeId>()
            .map(|r| (*r.0).clone())
            .unwrap_or_default();
        let path = req.uri().path().to_string();
        Box::pin(async move {
            let body = format!("path={path} peer={peer}");
            Ok(hyper::Response::builder()
                .status(200)
                .header("content-type", "text/plain")
                .body(Body::full(Bytes::from(body)))
                .expect("static response args are valid"))
        })
    }
}

#[tokio::test]
async fn pure_rust_fetch_round_trips_typed_request_response() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_pk = server_ep.raw().id();
    let server_id_str = server_ep.node_id().to_string();
    let addrs = common::server_addrs(&server_ep);
    let client_id = common::node_id(&client_ep);

    let _handle = serve_service(server_ep.clone(), ServeOptions::default(), EchoPeerService);

    // Build the EndpointAddr by hand — this is the typed contract the
    // pure-Rust API takes. No flat strings, no tickets.
    let mut addr = iroh::EndpointAddr::new(server_pk);
    for a in &addrs {
        addr = addr.with_ip_addr(*a);
    }

    let req = hyper::Request::builder()
        .method("GET")
        .uri("/hello-typed")
        .header(hyper::header::HOST, &server_id_str)
        .body(Body::empty())
        .expect("valid request");

    let cfg = StackConfig::default();
    let resp = fetch_request(&client_ep, &addr, req, &cfg)
        .await
        .expect("typed fetch ok");

    assert_eq!(resp.status().as_u16(), 200);

    // Drain the typed body — no slotmap, no handle store.
    let collected = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let body = String::from_utf8(collected.to_vec()).expect("utf8 body");
    assert!(body.contains("path=/hello-typed"), "body: {body}");
    assert!(body.contains(&format!("peer={client_id}")), "body: {body}");
}

// ── F9: negative tests for FetchError variants ────────────────────────

#[derive(Clone)]
struct SlowPeerService;

impl Service<hyper::Request<Body>> for SlowPeerService {
    type Response = hyper::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: hyper::Request<Body>) -> Self::Future {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            Ok(hyper::Response::builder()
                .status(200)
                .body(Body::empty())
                .expect("valid response"))
        })
    }
}

/// `cfg.timeout` elapses before the slow peer responds → typed `Timeout`.
#[tokio::test]
async fn pure_rust_fetch_timeout_returns_typed_error() {
    use iroh_http_core::FetchError;

    let (server_ep, client_ep) = common::make_pair().await;
    let server_pk = server_ep.raw().id();
    let addrs = common::server_addrs(&server_ep);

    let _handle = serve_service(server_ep.clone(), ServeOptions::default(), SlowPeerService);

    let mut addr = iroh::EndpointAddr::new(server_pk);
    for a in &addrs {
        addr = addr.with_ip_addr(*a);
    }

    let req = hyper::Request::builder()
        .method("GET")
        .uri("/slow")
        .body(Body::empty())
        .expect("valid request");

    let cfg = StackConfig::default().with_timeout(Some(std::time::Duration::from_millis(100)));
    let err = fetch_request(&client_ep, &addr, req, &cfg)
        .await
        .err()
        .expect("expected timeout");
    assert!(
        matches!(err, FetchError::Timeout),
        "expected FetchError::Timeout, got {err:?}"
    );
}

/// Connecting to a node id with no reachable direct addrs surfaces the
/// transport failure as `ConnectionFailed`, never as a stringly-typed
/// `Internal` (regression guard for the substring-classifier removal).
#[tokio::test]
async fn pure_rust_fetch_unreachable_addr_returns_connection_failed() {
    use iroh_http_core::FetchError;

    let (_server_ep, client_ep) = common::make_pair().await;

    // Random node id with a bogus, definitely-unreachable address. We need a
    // PublicKey we don't have a connection to, so generate a throwaway key.
    let bogus_secret = iroh::SecretKey::generate();
    let bogus_pk = bogus_secret.public();

    let mut addr = iroh::EndpointAddr::new(bogus_pk);
    addr = addr.with_ip_addr("127.0.0.1:1".parse().expect("valid socket addr"));

    let req = hyper::Request::builder()
        .method("GET")
        .uri("/nope")
        .body(Body::empty())
        .expect("valid request");

    // Short timeout so the test does not block on dial retries.
    let cfg = StackConfig::default().with_timeout(Some(std::time::Duration::from_secs(2)));
    let err = fetch_request(&client_ep, &addr, req, &cfg)
        .await
        .err()
        .expect("expected connection failure");
    assert!(
        matches!(
            err,
            FetchError::ConnectionFailed { .. } | FetchError::Timeout
        ),
        "expected FetchError::ConnectionFailed (or Timeout if dial slow), got {err:?}"
    );
}

// ── Slice E (#187) acceptance #4: large-body streaming roundtrip ───────

#[derive(Clone)]
struct LargeEchoService;

impl Service<hyper::Request<Body>> for LargeEchoService {
    type Response = hyper::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: hyper::Request<Body>) -> Self::Future {
        Box::pin(async move {
            // Drain the request body, then send it back. Echo proves the
            // body flowed through hyper → channel → service → channel →
            // hyper without dropping bytes.
            let collected = req
                .into_body()
                .collect()
                .await
                .expect("collect req body")
                .to_bytes();
            Ok(hyper::Response::builder()
                .status(200)
                .header("content-type", "application/octet-stream")
                .body(Body::full(collected))
                .expect("static response args are valid"))
        })
    }
}

/// Streams a 10 MiB body request → response and asserts byte-exact
/// roundtrip. This verifies *correctness* end-to-end (request body flows
/// in via `BodyReader: http_body::Body`; response body flows out via the
/// channel pump in `ffi::pumps`) but not bounded memory — both ends use
/// `Body::full` and `BodyExt::collect`, so the full 10 MiB is materialised
/// in memory by design. A real backpressure / peak-memory test would need
/// a slow-yielding stream body and an explicit memory probe; tracked
/// separately.
#[tokio::test]
async fn pure_rust_fetch_round_trips_10mib_body() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_pk = server_ep.raw().id();
    let server_id_str = server_ep.node_id().to_string();
    let addrs = common::server_addrs(&server_ep);

    let _handle = serve_service(server_ep.clone(), ServeOptions::default(), LargeEchoService);

    let mut addr = iroh::EndpointAddr::new(server_pk);
    for a in &addrs {
        addr = addr.with_ip_addr(*a);
    }

    // 10 MiB — bigger than the channel buffer so the test exercises
    // backpressure rather than fitting in a single chunk.
    let payload: Bytes = Bytes::from(vec![0xAB; 10 * 1024 * 1024]);

    let req = hyper::Request::builder()
        .method("POST")
        .uri("/echo")
        .header(hyper::header::HOST, &server_id_str)
        .header(hyper::header::CONTENT_LENGTH, payload.len())
        .body(Body::full(payload.clone()))
        .expect("valid request");

    // Generous outer timeout to avoid hanging CI on a real bug.
    let cfg = StackConfig::default().with_timeout(Some(std::time::Duration::from_secs(60)));
    let resp = fetch_request(&client_ep, &addr, req, &cfg)
        .await
        .expect("typed fetch ok");
    assert_eq!(resp.status().as_u16(), 200);

    let collected = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(collected.len(), payload.len(), "echo length mismatch");
    assert_eq!(collected, payload, "echo content mismatch");
}
