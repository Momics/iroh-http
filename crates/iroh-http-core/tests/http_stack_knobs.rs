#![allow(clippy::disallowed_types)] // test/bench file — RequestPayload and friends are valid here
//! Regression tests for #189 — StackConfig knobs threaded through the FFI.
//!
//! AC#1: per-call timeout fires against a hung server
//! AC#2: fetch with decompress=false returns raw zstd bytes
//! AC#3: serve with decompression=false passes raw compressed body to handler
//! AC#4: per-call max_response_body_bytes overrides endpoint default

mod common;

use std::time::Duration;

use bytes::Bytes;
use iroh_http_core::respond;
use iroh_http_core::{fetch, serve, RequestPayload, ServeOptions};

// ── AC#1: per-call fetch timeout ─────────────────────────────────────────────

/// A server that starts sending the response head but then hangs without
/// A server that accepts the request but never sends back a response head.
/// The client issues `fetch` with a 200ms timeout and must receive a timeout
/// error well within 500ms.
#[tokio::test]
async fn fetch_timeout_fires_against_hung_server() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    // Server receives the request but parks forever — never calls `respond`.
    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            // Never calls respond — hold open so the connection stays alive.
            tokio::spawn(async move {
                let _keep = (payload.req_handle, payload.res_body_handle);
                std::future::pending::<()>().await;
            });
        },
    );

    let start = std::time::Instant::now();
    let result = fetch(
        &client_ep,
        &server_id,
        "/hang",
        "GET",
        &[],
        None,
        None,
        Some(&addrs),
        Some(Duration::from_millis(200)), // 200ms timeout
        true,
        None, // max_response_body_bytes
    )
    .await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "expected timeout error, got: {:?}", result);
    assert!(
        elapsed < Duration::from_millis(500),
        "timeout should fire within 500ms, took {elapsed:?}"
    );
}

// ── AC#2: fetch decompress=false returns raw zstd bytes ──────────────────────

/// When `decompress=false` the client must receive the raw zstd-compressed
/// response bytes without the decompression layer running.
///
/// Setup: server sends a zstd-compressed body with `Content-Encoding: zstd`.
/// Client fetches with `decompress=false`. The received bytes must be
/// identical to the compressed payload, not the plaintext.
#[tokio::test]
async fn fetch_decompress_false_returns_raw_zstd_bytes() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    let plaintext = b"hello, raw compression world! ".repeat(64);
    let compressed =
        zstd::stream::encode_all(plaintext.as_slice(), 0).expect("zstd encode succeeds");
    let compressed_len = compressed.len();
    let plaintext_len = plaintext.len();
    assert!(
        compressed_len < plaintext_len,
        "compressed body should be smaller than plaintext"
    );

    let compressed_for_server = compressed.clone();
    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body_handle = payload.res_body_handle;
            let server_ep = server_ep.clone();
            let body = Bytes::from(compressed_for_server.clone());
            tokio::spawn(async move {
                respond(
                    server_ep.handles(),
                    req_handle,
                    200,
                    vec![
                        ("content-encoding".to_string(), "zstd".to_string()),
                        ("content-length".to_string(), body.len().to_string()),
                    ],
                )
                .unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, body)
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    // Fetch with Accept-Encoding: identity to suppress server zstd; the server
    // still sends its pre-compressed body with Content-Encoding: zstd.
    let res = fetch(
        &client_ep,
        &server_id,
        "/raw",
        "GET",
        &[("accept-encoding".to_string(), "identity".to_string())],
        None,
        None,
        Some(&addrs),
        None,
        false, // decompress = false → receive raw bytes
        None,  // max_response_body_bytes
    )
    .await
    .expect("fetch should succeed");

    assert_eq!(res.status, 200);

    let mut received = Vec::new();
    while let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
        received.extend_from_slice(&chunk);
    }

    assert_eq!(
        received,
        compressed,
        "with decompress=false the client should receive the raw zstd bytes, \
         got {} bytes (expected {compressed_len}, plaintext was {plaintext_len})",
        received.len(),
    );
}

// ── AC#3: serve decompression=false forwards raw compressed request body ─────

/// When `ServeOptions::decompression = Some(false)` the server's handler
/// must see the raw compressed bytes, not the decompressed plaintext.
///
/// Setup: client sends a zstd-compressed POST body with
/// `Content-Encoding: zstd`. The server is configured with
/// `decompression: Some(false)`. The handler records how many bytes it
/// received; the test asserts it equals the *compressed* length, not the
/// plaintext length.
#[tokio::test]
async fn serve_decompression_false_passes_raw_compressed_body_to_handler() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    let plaintext = b"serve-side raw compression test! ".repeat(64);
    let plaintext_len = plaintext.len();
    let compressed =
        zstd::stream::encode_all(plaintext.as_slice(), 0).expect("zstd encode succeeds");
    let compressed_len = compressed.len();
    assert!(
        compressed_len < plaintext_len,
        "compressed body should be smaller"
    );

    serve(
        server_ep.clone(),
        ServeOptions {
            // Opt out of server-side decompression.
            decompression: Some(false),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut body = Vec::new();
                while let Some(chunk) = server_ep
                    .handles()
                    .next_chunk(req_body_handle)
                    .await
                    .expect("read request body")
                {
                    body.extend_from_slice(&chunk);
                }
                // Echo back how many bytes the handler saw.
                let body_len_str = body.len().to_string();
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, Bytes::from(body_len_str.into_bytes()))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    let (writer_handle, body_reader) = client_ep
        .handles()
        .alloc_body_writer()
        .expect("alloc body writer");

    let client_ep_send = client_ep.clone();
    let compressed_clone = compressed.clone();
    tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from(compressed_clone))
            .await
            .unwrap();
        client_ep_send.handles().finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[
            (
                "content-type".to_string(),
                "application/octet-stream".to_string(),
            ),
            ("content-encoding".to_string(), "zstd".to_string()),
        ],
        Some(body_reader),
        None,
        Some(&addrs),
        None,
        true,
        None, // max_response_body_bytes
    )
    .await
    .expect("fetch should succeed");

    assert_eq!(res.status, 200);

    let mut response_body = Vec::new();
    while let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
        response_body.extend_from_slice(&chunk);
    }
    let handler_saw: usize = String::from_utf8(response_body)
        .expect("utf8")
        .parse()
        .expect("integer");

    assert_eq!(
        handler_saw, compressed_len,
        "with decompression=false the handler should see compressed ({compressed_len}) bytes, \
         not decompressed ({plaintext_len})"
    );
}

// ── AC#4: per-call max_response_body_bytes overrides endpoint default ─────────

/// The endpoint default is 1 MiB. The server responds with 5 MiB. A fetch
/// with `max_response_body_bytes = Some(10 * 1024 * 1024)` (10 MiB) must
/// succeed and receive all 5 MiB, proving the per-call limit overrides the
/// endpoint-wide one.
#[tokio::test]
async fn per_call_max_response_body_bytes_overrides_endpoint_default() {
    use iroh_http_core::{IrohEndpoint, NetworkingOptions, NodeOptions};

    // Bind a server with a very large endpoint limit (128 MiB) and a client
    // with a very small endpoint limit (1 MiB). Then issue a fetch with an
    // explicit per-call limit of 10 MiB for a 5 MiB response. The per-call
    // limit must win — the fetch must succeed even though the endpoint default
    // (1 MiB) would reject it.
    let server_ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap();

    let client_ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        // Set endpoint-wide default to 1 MiB so a 5 MiB response would
        // be rejected by default.
        max_response_body_bytes: Some(1024 * 1024),
        ..Default::default()
    })
    .await
    .unwrap();

    let server_id = server_ep.node_id().to_string();
    let addrs: Vec<std::net::SocketAddr> = server_ep.raw().addr().ip_addrs().cloned().collect();

    // 5 MiB of payload.
    let payload_5mib = vec![b'x'; 5 * 1024 * 1024];
    let payload_len = payload_5mib.len();

    let server_ep_clone = server_ep.clone();
    let payload_clone = payload_5mib.clone();
    serve(
        server_ep,
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body_handle = payload.res_body_handle;
            let server_ep = server_ep_clone.clone();
            let body_bytes = Bytes::from(payload_clone.clone());
            tokio::spawn(async move {
                respond(
                    server_ep.handles(),
                    req_handle,
                    200,
                    vec![("content-length".to_string(), body_bytes.len().to_string())],
                )
                .unwrap();
                // Send in 1 MiB chunks to stay under any per-chunk limit.
                let chunk_size = 1024 * 1024;
                let mut offset = 0;
                while offset < body_bytes.len() {
                    let end = (offset + chunk_size).min(body_bytes.len());
                    let chunk = body_bytes.slice(offset..end);
                    server_ep
                        .handles()
                        .send_chunk(res_body_handle, chunk)
                        .await
                        .unwrap();
                    offset = end;
                }
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/big",
        "GET",
        &[],
        None,
        None,
        Some(&addrs),
        None,
        true,
        Some(10 * 1024 * 1024), // 10 MiB per-call limit overrides 1 MiB endpoint default
    )
    .await
    .expect("per-call 10 MiB limit should allow 5 MiB response");

    assert_eq!(res.status, 200);

    let mut received_bytes = 0usize;
    while let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
        received_bytes += chunk.len();
    }
    assert_eq!(
        received_bytes, payload_len,
        "should receive all {payload_len} bytes; \
         per-call 10 MiB limit must override the 1 MiB endpoint default"
    );
}
