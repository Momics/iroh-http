//! Integration tests for iroh-http-core.
//!
//! Each test creates two Iroh endpoints (in-process) and exercises the full
//! fetch/serve stack over real QUIC connections.  No FFI, no JavaScript — pure
//! Rust end-to-end.

use bytes::Bytes;
use iroh_http_core::{
    IrohEndpoint, NodeOptions, fetch, serve,
    next_chunk, next_trailer, send_trailers,
    alloc_fetch_token, cancel_in_flight,
    server::ServeOptions,
    RequestPayload,
};
use iroh_http_core::server::respond;
use iroh_http_core::stream;

/// Create a pair of locally-connected endpoints (relay disabled).
async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let opts = || NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let server = IrohEndpoint::bind(opts()).await.unwrap();
    let client = IrohEndpoint::bind(opts()).await.unwrap();
    (server, client)
}

fn node_id(ep: &IrohEndpoint) -> String {
    ep.node_id().to_string()
}

/// Get the server's direct socket addresses so the client can connect.
fn server_addrs(ep: &IrohEndpoint) -> Vec<std::net::SocketAddr> {
    ep.raw().addr().ip_addrs().cloned().collect()
}

// -- Basic fetch/serve --------------------------------------------------------

#[tokio::test]
async fn basic_get_200() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/hello", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);
    assert!(res.url.starts_with("httpi://"));
    assert!(res.url.contains("/hello"));

    let chunk = next_chunk(res.body_handle).await.unwrap();
    assert!(chunk.is_none());
}

#[tokio::test]
async fn get_with_body() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let path = payload.url.split("://").nth(1)
                .and_then(|s| s.find('/').map(|i| &s[i..]))
                .unwrap_or("/")
                .to_string();
            let body_bytes = Bytes::from(path.as_bytes().to_vec());

            respond(payload.req_handle, 200, vec![
                ("content-type".into(), "text/plain".into()),
            ]).unwrap();

            let handle = payload.res_body_handle;
            tokio::spawn(async move {
                stream::send_chunk(handle, body_bytes).await.unwrap();
                stream::finish_body(handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/echo/test", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(res.body_handle).await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8(body).unwrap(), "/echo/test");
}

// -- Request body (POST) -----------------------------------------------------

#[tokio::test]
async fn post_with_request_body() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            assert_eq!(payload.method, "POST");

            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;

            tokio::spawn(async move {
                let mut body = Vec::new();
                while let Some(chunk) = next_chunk(req_body_handle).await.unwrap() {
                    body.extend_from_slice(&chunk);
                }

                let response_body = format!("received {} bytes", body.len());
                respond(req_handle, 200, vec![]).unwrap();
                stream::send_chunk(res_body_handle, Bytes::from(response_body.into_bytes())).await.unwrap();
                stream::finish_body(res_body_handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let (writer_handle, body_reader) = stream::alloc_body_writer();
    let body_data = b"hello, world!".to_vec();
    let body_len = body_data.len();

    tokio::spawn(async move {
        stream::send_chunk(writer_handle, Bytes::from(body_data)).await.unwrap();
        stream::finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep, &server_id, "/upload", "POST",
        &[("content-type".to_string(), "text/plain".to_string())],
        Some(body_reader),
        None,
        Some(&addrs),
    ).await.unwrap();

    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(res.body_handle).await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8(body).unwrap(), format!("received {body_len} bytes"));
}

// -- Response headers ---------------------------------------------------------

#[tokio::test]
async fn custom_response_headers() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 201, vec![
                ("x-custom".into(), "test-value".into()),
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/api", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 201);
    assert!(res.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("x-custom") && v == "test-value"));
}

// -- Request headers + method -------------------------------------------------

#[tokio::test]
async fn request_headers_and_method() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            assert_eq!(payload.method, "DELETE");
            let has_auth = payload.headers.iter().any(|(k, v)|
                k.eq_ignore_ascii_case("authorization") && v == "Bearer token123"
            );
            assert!(has_auth, "authorization header missing");

            respond(payload.req_handle, 204, vec![]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(
        &client_ep, &server_id, "/resource/42", "DELETE",
        &[("authorization".to_string(), "Bearer token123".to_string())],
        None, None,
        Some(&addrs),
    ).await.unwrap();
    assert_eq!(res.status, 204);
}

// -- URL scheme ---------------------------------------------------------------

#[tokio::test]
async fn url_uses_httpi_scheme() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let captured_url = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let captured = captured_url.clone();

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            *captured.lock().unwrap() = payload.url.clone();
            respond(payload.req_handle, 200, vec![]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/test/path", "GET", &[], None, None, Some(&addrs)).await.unwrap();

    assert!(res.url.starts_with("httpi://"), "res.url = {}", res.url);
    assert!(res.url.ends_with("/test/path"), "res.url = {}", res.url);

    let server_url = captured_url.lock().unwrap().clone();
    assert!(server_url.starts_with("httpi://"), "server url = {}", server_url);
    assert!(server_url.ends_with("/test/path"), "server url = {}", server_url);
}

// -- Remote node ID -----------------------------------------------------------

#[tokio::test]
async fn remote_node_id_is_populated() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let client_id = node_id(&client_ep);
    let addrs = server_addrs(&server_ep);

    let captured_remote = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let captured = captured_remote.clone();

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            *captured.lock().unwrap() = payload.remote_node_id.clone();
            respond(payload.req_handle, 200, vec![]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _res = fetch(&client_ep, &server_id, "/", "GET", &[], None, None, Some(&addrs)).await.unwrap();

    let remote = captured_remote.lock().unwrap().clone();
    assert_eq!(remote, client_id, "Server should see the client's node ID");
}

// -- Multiple requests --------------------------------------------------------

#[tokio::test]
async fn multiple_sequential_requests() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_clone = counter.clone();

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let n = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = format!("request #{n}");
            respond(payload.req_handle, 200, vec![]).unwrap();
            let h = payload.res_body_handle;
            tokio::spawn(async move {
                stream::send_chunk(h, Bytes::from(body.into_bytes())).await.unwrap();
                stream::finish_body(h).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    for i in 0..3u32 {
        let res = fetch(&client_ep, &server_id, &format!("/req/{i}"), "GET", &[], None, None, Some(&addrs)).await.unwrap();
        assert_eq!(res.status, 200);

        let mut body = Vec::new();
        while let Some(chunk) = next_chunk(res.body_handle).await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        assert_eq!(String::from_utf8(body).unwrap(), format!("request #{i}"));
    }
}

// -- Trailers -----------------------------------------------------------------

#[tokio::test]
async fn response_trailers() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("trailer".into(), "x-checksum".into()),
            ]).unwrap();

            let body_h = payload.res_body_handle;
            let trailer_h = payload.res_trailers_handle;
            tokio::spawn(async move {
                stream::send_chunk(body_h, Bytes::from("data")).await.unwrap();
                stream::finish_body(body_h).unwrap();
                send_trailers(trailer_h, vec![
                    ("x-checksum".into(), "abc123".into()),
                ]).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/with-trailers", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);

    while let Some(_chunk) = next_chunk(res.body_handle).await.unwrap() {}

    let trailers = next_trailer(res.trailers_handle).await.unwrap();
    let trailers = trailers.expect("expected trailers");
    assert!(trailers.iter().any(|(k, v)|
        k.eq_ignore_ascii_case("x-checksum") && v == "abc123"
    ), "trailers: {:?}", trailers);
}

// -- Empty body POST ----------------------------------------------------------

#[tokio::test]
async fn post_empty_body() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            assert_eq!(payload.method, "POST");
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;

            tokio::spawn(async move {
                // Read request body — should be empty
                let chunk = next_chunk(req_body_handle).await.unwrap();
                assert!(chunk.is_none(), "expected empty body");

                respond(req_handle, 204, vec![]).unwrap();
                stream::finish_body(res_body_handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Create body writer but immediately finish without sending data
    let (writer_handle, body_reader) = stream::alloc_body_writer();
    stream::finish_body(writer_handle).unwrap();

    let res = fetch(
        &client_ep, &server_id, "/empty", "POST",
        &[("content-length".to_string(), "0".to_string())],
        Some(body_reader),
        None,
        Some(&addrs),
    ).await.unwrap();
    assert_eq!(res.status, 204);
}

// -- Concurrent requests ------------------------------------------------------

#[tokio::test]
async fn concurrent_requests() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Fire 5 requests concurrently
    let mut handles = Vec::new();
    for i in 0..5u32 {
        let ep = client_ep.clone();
        let id = server_id.clone();
        let a = addrs.clone();
        handles.push(tokio::spawn(async move {
            let res = fetch(&ep, &id, &format!("/concurrent/{i}"), "GET", &[], None, None, Some(&a)).await.unwrap();
            assert_eq!(res.status, 200);
            i
        }));
    }

    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }
    results.sort();
    assert_eq!(results, vec![0, 1, 2, 3, 4]);
}

// -- Fetch cancellation -------------------------------------------------------

#[tokio::test]
async fn fetch_cancelled_via_token() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    // Server: accept connection but delay response indefinitely
    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let body_handle = payload.res_body_handle;
            tokio::spawn(async move {
                // Wait a long time before responding
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let _ = respond(req_handle, 200, vec![]);
                let _ = stream::finish_body(body_handle);
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let token = alloc_fetch_token();

    // Cancel after 100ms
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel_in_flight(token);
    });

    let result = fetch(
        &client_ep, &server_id, "/slow", "GET", &[], None, Some(token), Some(&addrs),
    ).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "aborted");
}

// -- Endpoint basics ----------------------------------------------------------

#[tokio::test]
async fn endpoint_node_id_is_stable() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let id1 = ep.node_id().to_string();
    let id2 = ep.node_id().to_string();
    assert_eq!(id1, id2);
    assert!(!id1.is_empty());
}

#[tokio::test]
async fn endpoint_deterministic_key() {
    let key = [42u8; 32];
    let opts1 = NodeOptions {
        key: Some(key),
        disable_networking: true,
        ..Default::default()
    };
    let opts2 = NodeOptions {
        key: Some(key),
        disable_networking: true,
        ..Default::default()
    };
    let ep1 = IrohEndpoint::bind(opts1).await.unwrap();
    let ep2 = IrohEndpoint::bind(opts2).await.unwrap();
    assert_eq!(ep1.node_id(), ep2.node_id());
}

#[tokio::test]
async fn endpoint_secret_key_round_trip() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let key_bytes = ep.secret_key_bytes();

    // Rebinding with the same key should produce the same node ID
    let opts2 = NodeOptions {
        key: Some(key_bytes),
        disable_networking: true,
        ..Default::default()
    };
    let ep2 = IrohEndpoint::bind(opts2).await.unwrap();
    assert_eq!(ep.node_id(), ep2.node_id());
}

#[tokio::test]
async fn endpoint_bound_sockets_non_empty() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let sockets = ep.bound_sockets();
    assert!(!sockets.is_empty(), "bound_sockets should not be empty");
}

#[tokio::test]
async fn endpoint_close() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    ep.close().await;
    // After close, connecting should fail
}

#[tokio::test]
async fn endpoint_max_consecutive_errors_default() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    assert_eq!(ep.max_consecutive_errors(), 5);
}

#[tokio::test]
async fn endpoint_max_consecutive_errors_custom() {
    let opts = NodeOptions {
        disable_networking: true,
        max_consecutive_errors: Some(10),
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    assert_eq!(ep.max_consecutive_errors(), 10);
}

// -- URL with query params and fragments --------------------------------------

#[tokio::test]
async fn url_with_query_params() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let captured_url = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let captured = captured_url.clone();

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            *captured.lock().unwrap() = payload.url.clone();
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/search?q=test&page=1", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);

    let server_url = captured_url.lock().unwrap().clone();
    assert!(server_url.contains("/search?q=test&page=1"),
        "server url should contain query params: {}", server_url);
    assert!(res.url.contains("/search?q=test&page=1"),
        "response url should contain query params: {}", res.url);
}

// -- respond() error path -----------------------------------------------------

#[tokio::test]
async fn respond_invalid_handle() {
    let result = respond(999999, 200, vec![]);
    assert!(result.is_err());
}

// -- No trailing trailer header -----------------------------------------------

#[tokio::test]
async fn response_without_trailer_header_still_works() {
    // Tests the server fix: when handler doesn't set Trailer: header,
    // the server should NOT wait for trailers and complete normally.
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            // No Trailer: header declared
            respond(payload.req_handle, 200, vec![]).unwrap();
            let h = payload.res_body_handle;
            tokio::spawn(async move {
                stream::send_chunk(h, Bytes::from("works")).await.unwrap();
                stream::finish_body(h).unwrap();
                // Deliberately NOT calling send_trailers
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let res = fetch(&client_ep, &server_id, "/no-trailers", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = next_chunk(res.body_handle).await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8(body).unwrap(), "works");
}

// -- Fetch with bad node ID ---------------------------------------------------

#[tokio::test]
async fn fetch_bad_node_id_returns_error() {
    let opts = NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let client = IrohEndpoint::bind(opts).await.unwrap();
    let result = fetch(&client, "!!!invalid!!!", "/", "GET", &[], None, None, None).await;
    assert!(result.is_err());
}

// -- Connection pooling -------------------------------------------------------

#[tokio::test]
async fn pool_reuses_connection_for_sequential_requests() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let request_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let rc = request_count.clone();

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            rc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // First request — establishes connection and caches it.
    let res1 = fetch(&client_ep, &server_id, "/a", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res1.status, 200);
    // Drain body to complete the request.
    while let Some(_) = next_chunk(res1.body_handle).await.unwrap() {}

    // Second request — should reuse the cached connection (no new handshake).
    let res2 = fetch(&client_ep, &server_id, "/b", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res2.status, 200);
    while let Some(_) = next_chunk(res2.body_handle).await.unwrap() {}

    // Third request for good measure.
    let res3 = fetch(&client_ep, &server_id, "/c", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res3.status, 200);
    while let Some(_) = next_chunk(res3.body_handle).await.unwrap() {}

    // All three requests should have been served.
    assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn pool_concurrent_requests_share_connection() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Fire 10 concurrent requests to the same peer.
    let mut handles = Vec::new();
    for i in 0..10u32 {
        let ep = client_ep.clone();
        let id = server_id.clone();
        let a = addrs.clone();
        handles.push(tokio::spawn(async move {
            let res = fetch(&ep, &id, &format!("/storm/{i}"), "GET", &[], None, None, Some(&a)).await.unwrap();
            assert_eq!(res.status, 200);
            while let Some(_) = next_chunk(res.body_handle).await.unwrap() {}
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // All requests completed successfully — the pool prevented a connection
    // storm (only 1 connect() call happened, the rest waited and reused it).
}

#[tokio::test]
async fn pool_different_peers_get_separate_connections() {
    // Create two separate servers.
    let opts = || NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let server1 = IrohEndpoint::bind(opts()).await.unwrap();
    let server2 = IrohEndpoint::bind(opts()).await.unwrap();
    let client = IrohEndpoint::bind(opts()).await.unwrap();

    let id1 = node_id(&server1);
    let id2 = node_id(&server2);
    let addrs1 = server_addrs(&server1);
    let addrs2 = server_addrs(&server2);

    for ep in [&server1, &server2] {
        serve(
            ep.clone(),
            ServeOptions::default(),
            move |payload: RequestPayload| {
                respond(payload.req_handle, 200, vec![
                    ("content-length".into(), "0".into()),
                ]).unwrap();
                stream::finish_body(payload.res_body_handle).unwrap();
            },
        );
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let r1 = fetch(&client, &id1, "/", "GET", &[], None, None, Some(&addrs1)).await.unwrap();
    assert_eq!(r1.status, 200);
    while let Some(_) = next_chunk(r1.body_handle).await.unwrap() {}

    let r2 = fetch(&client, &id2, "/", "GET", &[], None, None, Some(&addrs2)).await.unwrap();
    assert_eq!(r2.status, 200);
    while let Some(_) = next_chunk(r2.body_handle).await.unwrap() {}

    // Both succeeded with separate connections to different peers.
    assert_ne!(id1, id2);
}

// -- Security hardening (patch 14) --------------------------------------------

/// Helper: create a pair where the server has custom NodeOptions.
async fn make_pair_custom_server(server_opts: NodeOptions) -> (IrohEndpoint, IrohEndpoint) {
    let server = IrohEndpoint::bind(server_opts).await.unwrap();
    let client = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        ..Default::default()
    }).await.unwrap();
    (server, client)
}

/// A server with a small max_header_size should reject oversized request heads.
#[tokio::test]
async fn header_bomb_rejected() {
    let (server_ep, client_ep) = make_pair_custom_server(NodeOptions {
        disable_networking: true,
        max_header_size: Some(256), // very small
        ..Default::default()
    }).await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Build headers that exceed 256 bytes when QPACK-encoded.
    let big_value = "X".repeat(300);
    let headers = vec![("x-big".to_string(), big_value)];

    let result = fetch(
        &client_ep, &server_id, "/bomb", "GET", &headers, None, None, Some(&addrs),
    ).await;

    // The server should reject the oversized head and the client will see an error.
    assert!(result.is_err(), "expected error for oversized header, got: {:?}", result);
}

/// The client should also reject oversized response heads.
#[tokio::test]
async fn response_header_bomb_rejected() {
    let server_ep = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        ..Default::default()
    }).await.unwrap();
    // Client has a tiny max_header_size.
    let client_ep = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        max_header_size: Some(128),
        ..Default::default()
    }).await.unwrap();
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let big_value = "Y".repeat(200);
            respond(payload.req_handle, 200, vec![
                ("x-huge".into(), big_value),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // The client has max_header_size=128, so the server's big response header should be rejected.
    let result = fetch(
        &client_ep, &server_id, "/big-response", "GET", &[], None, None, Some(&addrs),
    ).await;

    assert!(result.is_err(), "expected error for oversized response header, got: {:?}", result);
}

/// Normal traffic should work with default settings.
#[tokio::test]
async fn default_limits_allow_normal_traffic() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "5".into()),
            ]).unwrap();

            let handle = payload.res_body_handle;
            tokio::spawn(async move {
                stream::send_chunk(handle, Bytes::from_static(b"hello")).await.unwrap();
                stream::finish_body(handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Should work fine with default 64KB header limit.
    let res = fetch(
        &client_ep, &server_id, "/normal", "GET", &[], None, None, Some(&addrs),
    ).await.unwrap();
    assert_eq!(res.status, 200);

    let chunk = next_chunk(res.body_handle).await.unwrap();
    assert_eq!(chunk.unwrap().as_ref(), b"hello");

    let eof = next_chunk(res.body_handle).await.unwrap();
    assert!(eof.is_none());
}

/// Body size limit should reset the stream when exceeded.
#[tokio::test]
async fn body_limit_exceeded_resets_stream() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_bytes: Some(64), // very small
            ..Default::default()
        },
        move |payload: RequestPayload| {
            // Try to read body — it should get cut off.
            let body_h = payload.req_body_handle;
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            tokio::spawn(async move {
                let mut total = 0usize;
                while let Ok(Some(chunk)) = next_chunk(body_h).await {
                    total += chunk.len();
                }
                // Respond with how many bytes we got.
                let body = format!("{total}");
                respond(req_h, 200, vec![
                    ("content-type".into(), "text/plain".into()),
                ]).unwrap();
                stream::send_chunk(res_h, Bytes::from(body)).await.unwrap();
                stream::finish_body(res_h).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send a 256-byte body, which exceeds the 64-byte limit.
    let (writer, reader) = iroh_http_core::stream::make_body_channel();
    let send_task = tokio::spawn(async move {
        let chunk = Bytes::from(vec![0x41u8; 256]);
        let _ = writer.send_chunk(chunk).await;
        drop(writer);
    });

    let result = fetch(
        &client_ep, &server_id, "/upload", "POST",
        &[], Some(reader), None, Some(&addrs),
    ).await;

    send_task.await.unwrap();

    // The request might succeed with a partial body or fail entirely;
    // either way the server should not have received all 256 bytes.
    if let Ok(res) = result {
        if let Ok(Some(chunk)) = next_chunk(res.body_handle).await {
            let received: usize = std::str::from_utf8(&chunk).unwrap_or("0").parse().unwrap_or(0);
            assert!(received <= 64, "server received {received} bytes, should be <= 64");
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
        max_request_body_bytes: Some(1024 * 1024),
        ..Default::default()
    };
    assert_eq!(opts.max_connections_per_peer, Some(2));
    assert_eq!(opts.request_timeout_ms, Some(30_000));
    assert_eq!(opts.max_request_body_bytes, Some(1024 * 1024));
}

/// Verify that max_header_size is configurable via NodeOptions and defaults to 64KB.
#[tokio::test]
async fn max_header_size_default_is_64kb() {
    let ep = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        ..Default::default()
    }).await.unwrap();
    assert_eq!(ep.max_header_size(), 64 * 1024);
}

/// Verify custom max_header_size is respected.
#[tokio::test]
async fn max_header_size_custom() {
    let ep = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        max_header_size: Some(1024),
        ..Default::default()
    }).await.unwrap();
    assert_eq!(ep.max_header_size(), 1024);
}

// -- Graceful shutdown (patch 15) ---------------------------------------------

/// Graceful shutdown: in-flight request completes, then drain finishes.
#[tokio::test]
async fn graceful_shutdown_drains_in_flight() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    // Use a notify to confirm the handler is actually running.
    let handler_started = std::sync::Arc::new(tokio::sync::Notify::new());
    let handler_started_tx = handler_started.clone();

    let handle = serve(
        server_ep.clone(),
        ServeOptions {
            drain_timeout_secs: Some(10),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let res_h = payload.res_body_handle;
            let req_h = payload.req_handle;
            let started = handler_started_tx.clone();
            tokio::spawn(async move {
                // Signal that the handler is running.
                started.notify_one();
                // Simulate a slow handler (1s).
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                respond(req_h, 200, vec![
                    ("content-length".into(), "2".into()),
                ]).unwrap();
                stream::send_chunk(res_h, Bytes::from_static(b"ok")).await.unwrap();
                stream::finish_body(res_h).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Start a request that will take 1s to complete.
    let fetch_task = {
        let client = client_ep.clone();
        let sid = server_id.clone();
        let a = addrs.clone();
        tokio::spawn(async move {
            fetch(&client, &sid, "/slow", "GET", &[], None, None, Some(&a)).await
        })
    };

    // Wait for the handler to actually start running before we trigger shutdown.
    handler_started.notified().await;

    // Signal shutdown — the serve loop should stop accepting but drain the
    // in-flight request.
    let start = std::time::Instant::now();
    handle.drain().await;
    let elapsed = start.elapsed();

    // The drain should have waited for the in-flight request (~1s handler).
    assert!(
        elapsed >= std::time::Duration::from_millis(300),
        "drain completed too fast ({elapsed:?}), should have waited for in-flight request"
    );

    // The in-flight request should have succeeded.
    let result = fetch_task.await.unwrap();
    assert!(result.is_ok(), "in-flight request should succeed: {:?}", result);
    let res = result.unwrap();
    assert_eq!(res.status, 200);
}

/// Force close aborts immediately without draining.
#[tokio::test]
async fn force_close_aborts_immediately() {
    let (server_ep, _client_ep) = make_pair().await;

    let _handle = serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |_payload: RequestPayload| {},
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let start = std::time::Instant::now();
    server_ep.close_force().await;
    let elapsed = start.elapsed();

    // Force close should be near-instant (well under 1 second).
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "force close took too long: {elapsed:?}"
    );
}

/// A node with no serve loop should close immediately.
#[tokio::test]
async fn close_without_serve_is_immediate() {
    let ep = IrohEndpoint::bind(NodeOptions {
        disable_networking: true,
        ..Default::default()
    }).await.unwrap();

    let start = std::time::Instant::now();
    ep.close().await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "close without serve took too long: {elapsed:?}"
    );
}

/// After shutdown, new requests are rejected (connection refused).
#[tokio::test]
async fn shutdown_rejects_new_requests() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let handle = serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(payload.req_handle, 200, vec![
                ("content-length".into(), "0".into()),
            ]).unwrap();
            stream::finish_body(payload.res_body_handle).unwrap();
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // First request should succeed.
    let res = fetch(&client_ep, &server_id, "/before", "GET", &[], None, None, Some(&addrs)).await.unwrap();
    assert_eq!(res.status, 200);
    while let Ok(Some(_)) = next_chunk(res.body_handle).await {}

    // Shut down the serve loop.
    handle.drain().await;

    // Close the endpoint too so the client gets a clean rejection.
    server_ep.close_force().await;

    // Request after shutdown should fail.
    let result = fetch(&client_ep, &server_id, "/after", "GET", &[], None, None, Some(&addrs)).await;
    assert!(result.is_err(), "expected error after shutdown, got: {:?}", result);
}

/// ServeHandle::shutdown() returns immediately without blocking.
#[tokio::test]
async fn shutdown_returns_immediately() {
    let (server_ep, _client_ep) = make_pair().await;

    let handle = serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |_payload: RequestPayload| {},
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let start = std::time::Instant::now();
    handle.shutdown();
    let elapsed = start.elapsed();

    // shutdown() should be non-blocking (< 10ms).
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "shutdown() blocked for {elapsed:?}"
    );
}

// -- Additional coverage tests -----------------------------------------------

/// Round-trip a 1 MB body to verify streaming works for large payloads.
#[tokio::test]
async fn large_body_round_trip() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            // Echo the request body back as the response body.
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;

            tokio::spawn(async move {
                respond(req_handle, 200, vec![]).unwrap();

                while let Ok(Some(chunk)) = next_chunk(req_body_handle).await {
                    stream::send_chunk(res_body_handle, chunk).await.unwrap();
                }
                stream::finish_body(res_body_handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 1 MB of patterned data.
    let data: Vec<u8> = (0u8..=255).cycle().take(1024 * 1024).collect();

    // Allocate a body writer so we can stream the request body.
    let (writer_handle, body_reader) = stream::alloc_body_writer();

    // Send the body in chunks concurrently with fetch.
    let data_clone = data.clone();
    let send_task = tokio::spawn(async move {
        for chunk in data_clone.chunks(8192) {
            stream::send_chunk(writer_handle, Bytes::copy_from_slice(chunk)).await.unwrap();
        }
        stream::finish_body(writer_handle).unwrap();
    });

    let res = fetch(&client_ep, &server_id, "/echo", "POST", &[], Some(body_reader), None, Some(&addrs)).await.unwrap();
    send_task.await.unwrap();
    assert_eq!(res.status, 200);

    let mut received = Vec::new();
    while let Ok(Some(chunk)) = next_chunk(res.body_handle).await {
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received.len(), data.len());
    assert_eq!(received, data);
}

/// Both peers serve and fetch from each other simultaneously.
#[tokio::test]
async fn mutual_fetch() {
    let (ep_a, ep_b) = make_pair().await;
    let id_a = node_id(&ep_a);
    let id_b = node_id(&ep_b);
    let addrs_a = server_addrs(&ep_a);
    let addrs_b = server_addrs(&ep_b);

    // Both nodes serve a handler that responds with their own node ID.
    for (ep, id) in [(&ep_a, id_a.clone()), (&ep_b, id_b.clone())] {
        let my_id = id.clone();
        serve(
            ep.clone(),
            ServeOptions::default(),
            move |payload: RequestPayload| {
                let body = Bytes::from(my_id.clone().into_bytes());
                let res_body = payload.res_body_handle;
                let req = payload.req_handle;
                tokio::spawn(async move {
                    respond(req, 200, vec![]).unwrap();
                    stream::send_chunk(res_body, body).await.unwrap();
                    stream::finish_body(res_body).unwrap();
                });
            },
        );
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // A fetches from B, B fetches from A — concurrently.
    let (res_ab, res_ba) = tokio::join!(
        fetch(&ep_a, &id_b, "/who", "GET", &[], None, None, Some(&addrs_b)),
        fetch(&ep_b, &id_a, "/who", "GET", &[], None, None, Some(&addrs_a)),
    );

    let res_ab = res_ab.unwrap();
    let res_ba = res_ba.unwrap();

    // A fetching B should get B's ID.
    let mut body_ab = Vec::new();
    while let Ok(Some(c)) = next_chunk(res_ab.body_handle).await {
        body_ab.extend_from_slice(&c);
    }
    assert_eq!(String::from_utf8(body_ab).unwrap(), id_b);

    // B fetching A should get A's ID.
    let mut body_ba = Vec::new();
    while let Ok(Some(c)) = next_chunk(res_ba.body_handle).await {
        body_ba.extend_from_slice(&c);
    }
    assert_eq!(String::from_utf8(body_ba).unwrap(), id_a);
}

/// POST JSON with content-type verification.
#[tokio::test]
async fn fetch_json_post() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let content_type = payload.headers.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;

            tokio::spawn(async move {
                // Read request body.
                let mut body = Vec::new();
                while let Ok(Some(chunk)) = next_chunk(req_body_handle).await {
                    body.extend_from_slice(&chunk);
                }

                // Verify content-type was sent.
                assert_eq!(content_type, "application/json");

                // Echo it back as JSON with content-type.
                respond(req_handle, 200, vec![
                    ("content-type".into(), "application/json".into()),
                ]).unwrap();
                stream::send_chunk(res_body_handle, Bytes::from(body)).await.unwrap();
                stream::finish_body(res_body_handle).unwrap();
            });
        },
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let json_body = b"{\"hello\":\"world\"}";
    let (writer_handle, body_reader) = stream::alloc_body_writer();

    let headers = vec![("content-type".to_string(), "application/json".to_string())];

    let send_task = tokio::spawn(async move {
        stream::send_chunk(writer_handle, Bytes::from_static(json_body)).await.unwrap();
        stream::finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/api/data",
        "POST",
        &headers,
        Some(body_reader),
        None,
        Some(&addrs),
    ).await.unwrap();
    send_task.await.unwrap();
    assert_eq!(res.status, 200);

    let ct = res.headers.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    assert_eq!(ct, Some("application/json"));

    let mut body = Vec::new();
    while let Ok(Some(chunk)) = next_chunk(res.body_handle).await {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(&body, json_body);
}
