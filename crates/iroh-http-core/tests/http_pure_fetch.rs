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
