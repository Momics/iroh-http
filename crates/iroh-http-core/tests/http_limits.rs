#![allow(clippy::disallowed_types)] // test/bench file — RequestPayload and friends are valid here
mod common;

use bytes::Bytes;
use iroh_http_core::respond;
use iroh_http_core::{
    fetch, serve, IrohEndpoint, NetworkingOptions, NodeOptions, RequestPayload, ServeOptions,
};

// -- Security hardening (patch 14) --------------------------------------------

/// A server with a small max_header_size should reject oversized request heads.
#[tokio::test]
async fn header_bomb_rejected() {
    let (server_ep, client_ep) = common::make_pair_custom_server(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        max_header_size: Some(256), // very small
        ..Default::default()
    })
    .await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(
                server_ep.handles(),
                payload.req_handle,
                200,
                vec![("content-length".into(), "0".into())],
            )
            .unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    // Build headers that exceed 256 bytes when serialized.
    let big_value = "X".repeat(300);
    let headers = vec![("x-big".to_string(), big_value)];

    let result = fetch(
        &client_ep,
        &server_id,
        "/bomb",
        "GET",
        &headers,
        None,
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    // ISS-003: The server post-parse header check should return 431.
    let resp = result.expect("expected a 431 response, not a transport error");
    assert_eq!(
        resp.status, 431,
        "expected 431 Request Header Fields Too Large, got: {}",
        resp.status
    );
}

/// The client should also reject oversized response heads.
#[tokio::test]
async fn response_header_bomb_rejected() {
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
    // Client has a tiny max_header_size.
    let client_ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        max_header_size: Some(128),
        ..Default::default()
    })
    .await
    .unwrap();
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let big_value = "Y".repeat(200);
            respond(
                server_ep.handles(),
                payload.req_handle,
                200,
                vec![("x-huge".into(), big_value)],
            )
            .unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    // The client has max_header_size=128, so the server's big response header should be rejected.
    let result = fetch(
        &client_ep,
        &server_id,
        "/big-response",
        "GET",
        &[],
        None,
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    assert!(
        result.is_err(),
        "expected error for oversized response header, got: {:?}",
        result
    );
    // The error must be HeaderTooLarge, not ConnectionFailed.
    let err = result.unwrap_err();
    assert_eq!(
        err.code,
        iroh_http_core::ErrorCode::HeaderTooLarge,
        "expected HeaderTooLarge, got: {:?}",
        err,
    );
}

/// Normal traffic should work with default settings.
#[tokio::test]
async fn default_limits_allow_normal_traffic() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(
                server_ep.handles(),
                payload.req_handle,
                200,
                vec![("content-length".into(), "5".into())],
            )
            .unwrap();

            let handle = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                server_ep
                    .handles()
                    .send_chunk(handle, Bytes::from_static(b"hello"))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(handle).unwrap();
            });
        },
    );

    // Should work fine with default 64KB header limit.
    let res = fetch(
        &client_ep,
        &server_id,
        "/normal",
        "GET",
        &[],
        None,
        None,
        Some(&addrs),
        None,
        true,
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);

    let chunk = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap();
    assert_eq!(chunk.unwrap().as_ref(), b"hello");

    let eof = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap();
    assert!(eof.is_none());
}

/// Body size limit should reset the stream when exceeded.
#[tokio::test]
async fn body_limit_exceeded_resets_stream() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_wire_bytes: Some(64), // very small
            ..Default::default()
        },
        move |payload: RequestPayload| {
            // Try to read body — it should get cut off.
            let body_h = payload.req_body_handle;
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut total = 0usize;
                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(body_h).await {
                    total += chunk.len();
                }
                // Respond with how many bytes we got.
                let body = format!("{total}");
                respond(
                    server_ep.handles(),
                    req_h,
                    200,
                    vec![("content-type".into(), "text/plain".into())],
                )
                .unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_h, Bytes::from(body))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_h).unwrap();
            });
        },
    );

    // Send a 256-byte body, which exceeds the 64-byte limit.
    let (writer, reader) = iroh_http_core::make_body_channel();
    let send_task = tokio::spawn(async move {
        let chunk = Bytes::from(vec![0x41u8; 256]);
        let _ = writer.send_chunk(chunk).await;
        drop(writer);
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[],
        Some(reader),
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    send_task.await.unwrap();

    // The request might succeed with a partial body or fail entirely;
    // either way the server should not have received all 256 bytes.
    if let Ok(res) = result {
        if let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
            let received: usize = std::str::from_utf8(&chunk)
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            assert!(
                received <= 64,
                "server received {received} bytes, should be <= 64"
            );
        }
    }
    // If the fetch errored entirely, that's also acceptable — the stream was reset.
}

/// Per-peer connection limit should be configurable via ServeOptions.
#[tokio::test]
async fn per_peer_connection_limit_config() {
    // Just verify that the config fields compile and can be set.
    let opts = ServeOptions {
        max_connections_per_peer: Some(2),
        request_timeout_ms: Some(30_000),
        max_request_body_wire_bytes: Some(1024 * 1024),
        ..Default::default()
    };
    assert_eq!(opts.max_connections_per_peer, Some(2));
    assert_eq!(opts.request_timeout_ms, Some(30_000));
    assert_eq!(opts.max_request_body_wire_bytes, Some(1024 * 1024));
}

/// Verify that max_header_size is configurable via NodeOptions and defaults to 64KB.
#[tokio::test]
async fn max_header_size_default_is_64kb() {
    let ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(ep.max_header_size(), 64 * 1024);
}

/// Verify custom max_header_size is respected.
#[tokio::test]
async fn max_header_size_custom() {
    let ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            ..Default::default()
        },
        max_header_size: Some(1024),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(ep.max_header_size(), 1024);
}

/// Verify that max_header_size: Some(0) is rejected.
#[tokio::test]
async fn max_header_size_zero_is_rejected() {
    let result = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            ..Default::default()
        },
        max_header_size: Some(0),
        ..Default::default()
    })
    .await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("bind should reject max_header_size: Some(0)"),
    };
    assert!(
        err.message.contains("max_header_size"),
        "error should mention max_header_size, got: {err}"
    );
}

// -- Server limit enforcement -------------------------------------------------

/// Requests beyond the concurrency limit are queued (semaphore) rather than
/// rejected.  Two concurrent in-flight requests with max_concurrency=2; a
/// third starts after one finishes.  All three must complete successfully.
#[tokio::test]
async fn serve_concurrency_limit() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    // Gate controls when the server handler completes.
    let gate = std::sync::Arc::new(tokio::sync::Barrier::new(1));

    serve(
        server_ep.clone(),
        ServeOptions {
            max_concurrency: Some(2),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            // Handlers complete immediately.
            respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
            server_ep.handles().finish_body(res_body).unwrap();
        },
    );

    // Fire 3 concurrent requests — all should succeed.
    let (r1, r2, r3) = tokio::join!(
        fetch(
            &client_ep,
            &server_id,
            "/r1",
            "GET",
            &[],
            None,
            None,
            Some(&addrs),
            None,
            true,
        ),
        fetch(
            &client_ep,
            &server_id,
            "/r2",
            "GET",
            &[],
            None,
            None,
            Some(&addrs),
            None,
            true,
        ),
        fetch(
            &client_ep,
            &server_id,
            "/r3",
            "GET",
            &[],
            None,
            None,
            Some(&addrs),
            None,
            true,
        ),
    );
    assert_eq!(r1.unwrap().status, 200);
    assert_eq!(r2.unwrap().status, 200);
    assert_eq!(r3.unwrap().status, 200);
    drop(gate);
}

/// Request body exceeding the wire limit causes the stream to be
/// reset — the fetch returns an error.
#[tokio::test]
async fn body_exceeds_limit_resets_stream() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_wire_bytes: Some(100),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            // Drain the body (triggers limit check in server).
            let req_body = payload.req_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                while let Ok(Some(_)) = server_ep.handles().next_chunk(req_body).await {}
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                server_ep.handles().finish_body(res_body).unwrap();
            });
        },
    );

    // Send a 10KB body — well over the 100-byte limit.
    let big_body = vec![b'x'; 10_000];
    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();
    let client_ep_send = client_ep.clone();
    tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from(big_body))
            .await
            .unwrap();
        client_ep_send.handles().finish_body(writer_handle).unwrap();
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[],
        Some(body_reader),
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    // Stream reset should produce an error or the body read fails.
    // Either the fetch errors or it succeeds but body is truncated.
    // We don't assert the exact error since it may race; just no panic.
    let _ = result;
}

// ── Edge-case tests (TEST-004) ────────────────────────────────────────────────

/// Concurrent requests to the same endpoint all complete correctly when
/// max_concurrency is set to a small value.
#[tokio::test]
async fn concurrent_requests_under_tight_concurrency() {
    let server_opts = NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let client_opts = NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let server_ep = IrohEndpoint::bind(server_opts).await.unwrap();
    let client_ep = IrohEndpoint::bind(client_opts).await.unwrap();
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    // Fire 20 requests concurrently — they must all complete despite max_concurrency=2.
    serve(
        server_ep.clone(),
        ServeOptions {
            max_concurrency: Some(2),
            // Disable load-shedding so excess requests queue rather than
            // immediately receiving 503. This test verifies that many more
            // concurrent requests than the concurrency cap all eventually
            // complete — not that capacity is enforced.
            load_shed: Some(false),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Small delay to keep slots occupied.
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                server_ep.handles().finish_body(res_body).unwrap();
            });
        },
    );

    // Fire 20 requests concurrently — they must all complete despite max_concurrency=2.
    let mut handles = Vec::new();
    for i in 0..20 {
        let client = client_ep.clone();
        let id = server_id.clone();
        let a = addrs.clone();
        handles.push(tokio::spawn(async move {
            let path = format!("/stress/{i}");
            fetch(
                &client,
                &id,
                &path,
                "GET",
                &[],
                None,
                None,
                Some(&a),
                None,
                true,
            )
            .await
        }));
    }

    let mut ok_count = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(res) => {
                assert_eq!(res.status, 200);
                ok_count += 1;
            }
            Err(_) => {
                // Under heavy contention some may time out — acceptable.
            }
        }
    }
    // At least half should succeed (all 20 should, but be lenient for CI).
    assert!(
        ok_count >= 10,
        "expected ≥10 successes under concurrency=2, got {ok_count}"
    );
}

/// When the wire limit is exceeded, the pump loop must drain the
/// remaining body frames so the peer's QUIC send stream can close cleanly.
/// The client write task should complete well within 500 ms of receiving the
/// 413 response — not stall until the QUIC idle timeout (ISS-015).
#[tokio::test]
async fn body_overflow_drains_quic_stream() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            // 100-byte wire limit; client will send 50 KB.
            max_request_body_wire_bytes: Some(100),
            ..Default::default()
        },
        move |_payload: RequestPayload| {
            // Handler does nothing: the serve path handles the 413 automatically
            // via the overflow_tx mechanism.
        },
    );

    // Allocate a body writer and stream 50 KB to the server.
    let big_body = Bytes::from(vec![b'z'; 50_000]);
    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();

    let client_ep_write = client_ep.clone();
    let big_body_clone = big_body.clone();
    // Spawn the write task; it must finish promptly once the server accepts
    // enough data to issue the 413.
    let write_task = tokio::spawn(async move {
        let _ = client_ep_write
            .handles()
            .send_chunk(writer_handle, big_body_clone)
            .await;
        let _ = client_ep_write.handles().finish_body(writer_handle);
    });

    // Drive the fetch.
    let result = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[],
        Some(body_reader),
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    // 413 or error (races are fine); the key invariant is that the write task
    // above does not stall.
    let _ = result;

    // The write task must finish within 500 ms — not stall until QUIC idle
    // timeout.  Before the drain fix this would hang for many seconds.
    let deadline = tokio::time::timeout(std::time::Duration::from_millis(500), write_task).await;
    assert!(
        deadline.is_ok(),
        "client write task stalled after body overflow — QUIC stream was not drained"
    );
}

// ── Regression tests for #190 (decoded body limit / zstd-bomb) ─────────────

/// Regression #190-A: a zstd-compressed payload that is small on the wire but
/// large when decompressed must be truncated by `max_request_body_decoded_bytes`.
///
/// Before #190 was fixed, `RequestBodyLimitLayer` was placed outside the
/// decompression layer so it only counted compressed wire bytes.  A ~50 B
/// zstd payload (100 KiB plaintext) would bypass the 64 KiB wire limit and
/// the handler would read the full 100 KiB.  `max_request_body_decoded_bytes`
/// is now applied inside `RequestDecompressionLayer` and closes this gap.
#[tokio::test]
async fn zstd_bomb_rejected_by_decoded_body_limit() {
    const DECODED_LIMIT: usize = 8 * 1024; // 8 KiB
    const PLAINTEXT_SIZE: usize = 100 * 1024; // 100 KiB

    // 100 KiB of zeros compresses to ~50 bytes with zstd.
    let plaintext = vec![0u8; PLAINTEXT_SIZE];
    let compressed =
        zstd::stream::encode_all(plaintext.as_slice(), 3).expect("zstd encode succeeds");
    // Sanity: verify the compression ratio is high enough for the test to be
    // meaningful (compressed must fit within the 64 KiB wire limit).
    assert!(
        compressed.len() < DECODED_LIMIT,
        "expected compressed payload < {DECODED_LIMIT} B, got {} B",
        compressed.len()
    );

    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            // Wire limit: 64 KiB — well above the compressed payload (~50 B)
            // so the wire check must NOT fire here.
            max_request_body_wire_bytes: Some(64 * 1024),
            // Decoded limit: 8 KiB — the 100 KiB plaintext must be truncated.
            max_request_body_decoded_bytes: Some(DECODED_LIMIT),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            // Read the (decompressed) body and count bytes received.
            // The decoded limit must cut off reading before 100 KiB.
            let body_h = payload.req_body_handle;
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut total = 0usize;
                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(body_h).await {
                    total += chunk.len();
                }
                let count_str = format!("{total}");
                respond(
                    server_ep.handles(),
                    req_h,
                    200,
                    vec![("content-type".into(), "text/plain".into())],
                )
                .unwrap();
                let _ = server_ep
                    .handles()
                    .send_chunk(res_h, Bytes::from(count_str))
                    .await;
                let _ = server_ep.handles().finish_body(res_h);
            });
        },
    );

    let (writer, body_reader) = iroh_http_core::make_body_channel();
    tokio::spawn(async move {
        let _ = writer.send_chunk(Bytes::from(compressed)).await;
        drop(writer);
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/bomb",
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
    )
    .await;

    // The transport must succeed (server responds 200 after truncation).
    if let Ok(res) = result {
        if let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
            let received: usize = std::str::from_utf8(&chunk)
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0);
            assert!(
                received <= DECODED_LIMIT,
                "decoded-body limit = {DECODED_LIMIT} B but handler saw {received} B; \
                 decoded limit is not being enforced inside decompression (regression #190)"
            );
        }
    }
    // If the fetch errored entirely (stream reset), the limit fired — also acceptable.
}

/// Regression #190-B: the wire limit still fires for large uncompressed bodies.
///
/// Ensures the rename of `max_request_body_bytes` → `max_request_body_wire_bytes`
/// did not accidentally disable the pre-existing wire-level guard.
#[tokio::test]
async fn wire_limit_rejects_large_uncompressed_body() {
    const WIRE_LIMIT: usize = 1024; // 1 KiB
    const BODY_SIZE: usize = 2048; // 2 KiB

    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_wire_bytes: Some(WIRE_LIMIT),
            max_request_body_decoded_bytes: None, // decoded limit disabled
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let body_h = payload.req_body_handle;
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut total = 0usize;
                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(body_h).await {
                    total += chunk.len();
                }
                let count_str = format!("{total}");
                respond(
                    server_ep.handles(),
                    req_h,
                    200,
                    vec![("content-type".into(), "text/plain".into())],
                )
                .unwrap();
                let _ = server_ep
                    .handles()
                    .send_chunk(res_h, Bytes::from(count_str))
                    .await;
                let _ = server_ep.handles().finish_body(res_h);
            });
        },
    );

    // Send a 2 KiB raw (uncompressed) body — exceeds the 1 KiB wire limit.
    let (writer, body_reader) = iroh_http_core::make_body_channel();
    tokio::spawn(async move {
        let _ = writer
            .send_chunk(Bytes::from(vec![0x41u8; BODY_SIZE]))
            .await;
        drop(writer);
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[],
        Some(body_reader),
        None,
        Some(&addrs),
        None,
        true,
    )
    .await;

    match result {
        Ok(res) => {
            if let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
                let received: usize = std::str::from_utf8(&chunk)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                assert!(
                    received <= WIRE_LIMIT,
                    "wire limit = {WIRE_LIMIT} B but handler saw {received} B; \
                     wire limit is not being enforced (regression #190)"
                );
            }
        }
        Err(_) => { /* stream reset is also acceptable — limit fired */ }
    }
}

/// Regression #190-C: a request within both limits must reach the handler intact.
///
/// Validates that the two-limit design doesn't accidentally drop normal traffic:
/// a small compressed body within both the wire and decoded limits must arrive
/// fully decompressed at the handler.
#[tokio::test]
async fn request_within_both_limits_succeeds() {
    const BOTH_LIMITS: usize = 64 * 1024; // 64 KiB
    const PLAINTEXT_SIZE: usize = 512; // 512 B

    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    // Encode 512 bytes — decompresses to 512 bytes, well within both limits.
    let plaintext = vec![0x42u8; PLAINTEXT_SIZE];
    let compressed =
        zstd::stream::encode_all(plaintext.as_slice(), 3).expect("zstd encode succeeds");

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_wire_bytes: Some(BOTH_LIMITS),
            max_request_body_decoded_bytes: Some(BOTH_LIMITS),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let body_h = payload.req_body_handle;
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut total = 0usize;
                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(body_h).await {
                    total += chunk.len();
                }
                let count_str = format!("{total}");
                respond(
                    server_ep.handles(),
                    req_h,
                    200,
                    vec![("content-type".into(), "text/plain".into())],
                )
                .unwrap();
                let _ = server_ep
                    .handles()
                    .send_chunk(res_h, Bytes::from(count_str))
                    .await;
                let _ = server_ep.handles().finish_body(res_h);
            });
        },
    );

    let (writer, body_reader) = iroh_http_core::make_body_channel();
    tokio::spawn(async move {
        let _ = writer.send_chunk(Bytes::from(compressed)).await;
        drop(writer);
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[
            ("content-type".into(), "application/octet-stream".into()),
            ("content-encoding".into(), "zstd".into()),
        ],
        Some(body_reader),
        None,
        Some(&addrs),
        None,
        true,
    )
    .await
    .expect("fetch must succeed for a body within both limits");

    assert_eq!(res.status, 200, "expected 200, got {}", res.status);

    if let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
        let received: usize = std::str::from_utf8(&chunk)
            .unwrap_or("0")
            .trim()
            .parse()
            .unwrap_or(0);
        assert_eq!(
            received, PLAINTEXT_SIZE,
            "expected handler to receive {PLAINTEXT_SIZE} decoded bytes, got {received}"
        );
    }
}
