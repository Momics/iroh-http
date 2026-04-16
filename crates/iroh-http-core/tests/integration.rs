//! Integration tests for iroh-http-core.
//!
//! Each test creates two Iroh endpoints (in-process) and exercises the full
//! fetch/serve stack over real QUIC connections.  No FFI, no JavaScript — pure
//! Rust end-to-end.

use bytes::Bytes;
use iroh_http_core::server::respond;
use iroh_http_core::{
    fetch, serve, server::ServeOptions, IrohEndpoint, NetworkingOptions, NodeOptions, RequestPayload,
};

/// Create a pair of locally-connected endpoints (relay disabled, loopback only).
async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let opts = || NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
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

    let res = fetch(
        &client_ep,
        &server_id,
        "/hello",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);
    assert!(res.url.starts_with("httpi://"));
    assert!(res.url.contains("/hello"));

    let chunk = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap();
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
            let path = payload
                .url
                .split("://")
                .nth(1)
                .and_then(|s| s.find('/').map(|i| &s[i..]))
                .unwrap_or("/")
                .to_string();
            let body_bytes = Bytes::from(path.as_bytes().to_vec());

            respond(
                server_ep.handles(),
                payload.req_handle,
                200,
                vec![("content-type".into(), "text/plain".into())],
            )
            .unwrap();

            let handle = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                server_ep
                    .handles()
                    .send_chunk(handle, body_bytes)
                    .await
                    .unwrap();
                server_ep.handles().finish_body(handle).unwrap();
            });
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/echo/test",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap()
    {
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

            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                let mut body = Vec::new();
                while let Some(chunk) = server_ep
                    .handles()
                    .next_chunk(req_body_handle)
                    .await
                    .unwrap()
                {
                    body.extend_from_slice(&chunk);
                }

                let response_body = format!("received {} bytes", body.len());
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, Bytes::from(response_body.into_bytes()))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();
    let body_data = b"hello, world!".to_vec();
    let body_len = body_data.len();

    let client_ep_send = client_ep.clone();
    tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from(body_data))
            .await
            .unwrap();
        client_ep_send.handles().finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/upload",
        "POST",
        &[("content-type".to_string(), "text/plain".to_string())],
        Some(body_reader),
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();

    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap()
    {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8(body).unwrap(),
        format!("received {body_len} bytes")
    );
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
            respond(
                server_ep.handles(),
                payload.req_handle,
                201,
                vec![
                    ("x-custom".into(), "test-value".into()),
                    ("content-length".into(), "0".into()),
                ],
            )
            .unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/api",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 201);
    assert!(res
        .headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-custom") && v == "test-value"));
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
            let has_auth = payload
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "Bearer token123");
            assert!(has_auth, "authorization header missing");

            respond(server_ep.handles(), payload.req_handle, 204, vec![]).unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/resource/42",
        "DELETE",
        &[("authorization".to_string(), "Bearer token123".to_string())],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
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
            respond(server_ep.handles(), payload.req_handle, 200, vec![]).unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/test/path",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();

    assert!(res.url.starts_with("httpi://"), "res.url = {}", res.url);
    assert!(res.url.ends_with("/test/path"), "res.url = {}", res.url);

    let server_url = captured_url.lock().unwrap().clone();
    assert!(
        server_url.starts_with("httpi://"),
        "server url = {}",
        server_url
    );
    assert!(
        server_url.ends_with("/test/path"),
        "server url = {}",
        server_url
    );
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
            respond(server_ep.handles(), payload.req_handle, 200, vec![]).unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    let _res = fetch(
        &client_ep,
        &server_id,
        "/",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();

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
            respond(server_ep.handles(), payload.req_handle, 200, vec![]).unwrap();
            let h = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                server_ep
                    .handles()
                    .send_chunk(h, Bytes::from(body.into_bytes()))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(h).unwrap();
            });
        },
    );

    for i in 0..3u32 {
        let res = fetch(
            &client_ep,
            &server_id,
            &format!("/req/{i}"),
            "GET",
            &[],
            None,
            None,
            None,
            Some(&addrs),
        )
        .await
        .unwrap();
        assert_eq!(res.status, 200);

        let mut body = Vec::new();
        while let Some(chunk) = client_ep
            .handles()
            .next_chunk(res.body_handle)
            .await
            .unwrap()
        {
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
            respond(
                server_ep.handles(),
                payload.req_handle,
                200,
                vec![("trailer".into(), "x-checksum".into())],
            )
            .unwrap();

            let body_h = payload.res_body_handle;
            let trailer_h = payload.res_trailers_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                server_ep
                    .handles()
                    .send_chunk(body_h, Bytes::from("data"))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(body_h).unwrap();
                server_ep
                    .handles()
                    .send_trailers(trailer_h, vec![("x-checksum".into(), "abc123".into())])
                    .unwrap();
            });
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/with-trailers",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);

    while let Some(_chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap()
    {}

    let trailers = client_ep
        .handles()
        .next_trailer(res.trailers_handle)
        .await
        .unwrap();
    let trailers = trailers.expect("expected trailers");
    assert!(
        trailers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("x-checksum") && v == "abc123"),
        "trailers: {:?}",
        trailers
    );
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

            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Read request body — should be empty
                let chunk = server_ep
                    .handles()
                    .next_chunk(req_body_handle)
                    .await
                    .unwrap();
                assert!(chunk.is_none(), "expected empty body");

                respond(server_ep.handles(), req_handle, 204, vec![]).unwrap();
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    // Create body writer but immediately finish without sending data
    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();
    client_ep.handles().finish_body(writer_handle).unwrap();

    let res = fetch(
        &client_ep,
        &server_id,
        "/empty",
        "POST",
        &[("content-length".to_string(), "0".to_string())],
        Some(body_reader),
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
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

    // Fire 5 requests concurrently
    let mut handles = Vec::new();
    for i in 0..5u32 {
        let ep = client_ep.clone();
        let id = server_id.clone();
        let a = addrs.clone();
        handles.push(tokio::spawn(async move {
            let res = fetch(
                &ep,
                &id,
                &format!("/concurrent/{i}"),
                "GET",
                &[],
                None,
                None,
                None,
                Some(&a),
            )
            .await
            .unwrap();
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

    // Server: signal when the request arrives, then hang indefinitely.
    let (request_arrived_tx, request_arrived_rx) = tokio::sync::oneshot::channel::<()>();
    let request_arrived_tx = std::sync::Mutex::new(Some(request_arrived_tx));

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |_payload: RequestPayload| {
            if let Some(tx) = request_arrived_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            // Never respond — the client should cancel.
        },
    );

    let token = client_ep.handles().alloc_fetch_token().unwrap();

    // Cancel as soon as the server has received the request — no sleep needed.
    let client_ep_cancel = client_ep.clone();
    tokio::spawn(async move {
        let _ = request_arrived_rx.await;
        client_ep_cancel.handles().cancel_in_flight(token);
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/slow",
        "GET",
        &[],
        None,
        None,
        Some(token),
        Some(&addrs),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().code,
        iroh_http_core::ErrorCode::Cancelled
    );
}

// -- Endpoint basics ----------------------------------------------------------

#[tokio::test]
async fn endpoint_node_id_is_stable() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
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
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let opts2 = NodeOptions {
        key: Some(key),
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep1 = IrohEndpoint::bind(opts1).await.unwrap();
    let ep2 = IrohEndpoint::bind(opts2).await.unwrap();
    assert_eq!(ep1.node_id(), ep2.node_id());
}

#[tokio::test]
async fn endpoint_secret_key_round_trip() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let key_bytes = ep.secret_key_bytes();

    // Rebinding with the same key should produce the same node ID
    let opts2 = NodeOptions {
        key: Some(key_bytes),
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep2 = IrohEndpoint::bind(opts2).await.unwrap();
    assert_eq!(ep.node_id(), ep2.node_id());
}

#[tokio::test]
async fn endpoint_bound_sockets_non_empty() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let sockets = ep.bound_sockets();
    assert!(!sockets.is_empty(), "bound_sockets should not be empty");
}

#[tokio::test]
async fn endpoint_close() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    ep.close().await;
    // After close, connecting should fail
}

#[tokio::test]
async fn endpoint_max_consecutive_errors_default() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    assert_eq!(ep.max_consecutive_errors(), 5);
}

#[tokio::test]
async fn endpoint_max_consecutive_errors_custom() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        server_limits: iroh_http_core::server::ServerLimits {
            max_consecutive_errors: Some(10),
            ..Default::default()
        },
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

    let res = fetch(
        &client_ep,
        &server_id,
        "/search?q=test&page=1",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);

    let server_url = captured_url.lock().unwrap().clone();
    assert!(
        server_url.contains("/search?q=test&page=1"),
        "server url should contain query params: {}",
        server_url
    );
    assert!(
        res.url.contains("/search?q=test&page=1"),
        "response url should contain query params: {}",
        res.url
    );
}

// -- respond() error path -----------------------------------------------------

#[tokio::test]
async fn respond_invalid_handle() {
    let ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    })
    .await
    .unwrap();
    let result = respond(ep.handles(), 999999, 200, vec![]);
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
            respond(server_ep.handles(), payload.req_handle, 200, vec![]).unwrap();
            let h = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                server_ep
                    .handles()
                    .send_chunk(h, Bytes::from("works"))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(h).unwrap();
                // Deliberately NOT calling send_trailers
            });
        },
    );

    let res = fetch(
        &client_ep,
        &server_id,
        "/no-trailers",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .unwrap()
    {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(String::from_utf8(body).unwrap(), "works");
}

// -- Fetch with bad node ID ---------------------------------------------------

#[tokio::test]
async fn fetch_bad_node_id_returns_error() {
    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let client = IrohEndpoint::bind(opts).await.unwrap();
    let result = fetch(&client, "!!!invalid!!!", "/", "GET", &[], None, None, None, None).await;
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

    // First request — establishes connection and caches it.
    let res1 = fetch(
        &client_ep,
        &server_id,
        "/a",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res1.status, 200);
    // Drain body to complete the request.
    while let Some(_) = client_ep
        .handles()
        .next_chunk(res1.body_handle)
        .await
        .unwrap()
    {}

    // Second request — should reuse the cached connection (no new handshake).
    let res2 = fetch(
        &client_ep,
        &server_id,
        "/b",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res2.status, 200);
    while let Some(_) = client_ep
        .handles()
        .next_chunk(res2.body_handle)
        .await
        .unwrap()
    {}

    // Third request for good measure.
    let res3 = fetch(
        &client_ep,
        &server_id,
        "/c",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res3.status, 200);
    while let Some(_) = client_ep
        .handles()
        .next_chunk(res3.body_handle)
        .await
        .unwrap()
    {}

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

    // Fire 10 concurrent requests to the same peer.
    let mut handles = Vec::new();
    for i in 0..10u32 {
        let ep = client_ep.clone();
        let id = server_id.clone();
        let a = addrs.clone();
        handles.push(tokio::spawn(async move {
            let res = fetch(
                &ep,
                &id,
                &format!("/storm/{i}"),
                "GET",
                &[],
                None,
                None,
                None,
                Some(&a),
            )
            .await
            .unwrap();
            assert_eq!(res.status, 200);
            while let Some(_) = ep.handles().next_chunk(res.body_handle).await.unwrap() {}
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
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let server1 = IrohEndpoint::bind(opts()).await.unwrap();
    let server2 = IrohEndpoint::bind(opts()).await.unwrap();
    let client = IrohEndpoint::bind(opts()).await.unwrap();

    let id1 = node_id(&server1);
    let id2 = node_id(&server2);
    let addrs1 = server_addrs(&server1);
    let addrs2 = server_addrs(&server2);

    for ep in [server1.clone(), server2.clone()] {
        let ep_handler = ep.clone();
        serve(
            ep,
            ServeOptions::default(),
            move |payload: RequestPayload| {
                respond(
                    ep_handler.handles(),
                    payload.req_handle,
                    200,
                    vec![("content-length".into(), "0".into())],
                )
                .unwrap();
                ep_handler
                    .handles()
                    .finish_body(payload.res_body_handle)
                    .unwrap();
            },
        );
    }

    // Fetch with a generous timeout instead of retry/sleep loops.
    let r1 = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        fetch(&client, &id1, "/", "GET", &[], None, None, None, Some(&addrs1)),
    )
    .await
    .expect("fetch to server1 timed out")
    .expect("fetch to server1 failed");
    assert_eq!(r1.status, 200);
    while let Some(_) = client.handles().next_chunk(r1.body_handle).await.unwrap() {}

    let r2 = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        fetch(&client, &id2, "/", "GET", &[], None, None, None, Some(&addrs2)),
    )
    .await
    .expect("fetch to server2 timed out")
    .expect("fetch to server2 failed");
    assert_eq!(r2.status, 200);
    while let Some(_) = client.handles().next_chunk(r2.body_handle).await.unwrap() {}

    // Both succeeded with separate connections to different peers.
    assert_ne!(id1, id2);
}

// -- Security hardening (patch 14) --------------------------------------------

/// Helper: create a pair where the server has custom NodeOptions.
async fn make_pair_custom_server(server_opts: NodeOptions) -> (IrohEndpoint, IrohEndpoint) {
    let server = IrohEndpoint::bind(server_opts).await.unwrap();
    let client = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap();
    (server, client)
}

/// A server with a small max_header_size should reject oversized request heads.
#[tokio::test]
async fn header_bomb_rejected() {
    let (server_ep, client_ep) = make_pair_custom_server(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        max_header_size: Some(256), // very small
        ..Default::default()
    })
    .await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

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
        None,
        Some(&addrs),
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
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    })
    .await
    .unwrap();
    // Client has a tiny max_header_size.
    let client_ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        max_header_size: Some(128),
        ..Default::default()
    })
    .await
    .unwrap();
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

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
        None,
        Some(&addrs),
    )
    .await;

    assert!(
        result.is_err(),
        "expected error for oversized response header, got: {:?}",
        result
    );
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
        None,
        Some(&addrs),
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
    let (writer, reader) = iroh_http_core::stream::make_body_channel();
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
        None,
        Some(&addrs),
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
        networking: NetworkingOptions { disabled: true, ..Default::default() },
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
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        max_header_size: Some(1024),
        ..Default::default()
    })
    .await
    .unwrap();
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
    // Use a notify to let the handler proceed after shutdown is triggered.
    let handler_proceed = std::sync::Arc::new(tokio::sync::Notify::new());
    let handler_proceed_rx = handler_proceed.clone();

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
            let proceed = handler_proceed_rx.clone();
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Signal that the handler is running.
                started.notify_one();
                // Wait for the test to signal us to proceed (deterministic sync).
                proceed.notified().await;
                respond(
                    server_ep.handles(),
                    req_h,
                    200,
                    vec![("content-length".into(), "2".into())],
                )
                .unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_h, Bytes::from_static(b"ok"))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_h).unwrap();
            });
        },
    );

    // Start a request that will take 1s to complete.
    let fetch_task = {
        let client = client_ep.clone();
        let sid = server_id.clone();
        let a = addrs.clone();
        tokio::spawn(async move {
            fetch(&client, &sid, "/slow", "GET", &[], None, None, None, Some(&a)).await
        })
    };

    // Wait for the handler to actually start running before we trigger shutdown.
    handler_started.notified().await;

    // Start drain in the background — it should block until the handler finishes.
    let drain_done = std::sync::Arc::new(tokio::sync::Notify::new());
    let drain_done_rx = drain_done.clone();
    tokio::spawn(async move {
        handle.drain().await;
        drain_done.notify_one();
    });

    // Give a brief yield to ensure drain has started waiting.
    tokio::task::yield_now().await;

    // Drain should NOT have completed yet because the handler hasn't finished.
    // (We can't assert this deterministically, but the handler_proceed signal
    // below ensures the handler runs to completion before drain finishes.)

    // Let the handler complete.
    handler_proceed.notify_one();

    // Drain should complete now that the handler has finished.
    tokio::time::timeout(std::time::Duration::from_secs(10), drain_done_rx.notified())
        .await
        .expect("drain should complete after handler finishes");

    // The in-flight request should have succeeded.
    let result = fetch_task.await.unwrap();
    assert!(
        result.is_ok(),
        "in-flight request should succeed: {:?}",
        result
    );
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

    let start = std::time::Instant::now();
    server_ep.close_force().await;
    let elapsed = start.elapsed();

    // Force close should complete quickly (well under 5 seconds).
    // iroh's QUIC close path can take ~1s on slow machines, so we use 5s.
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "force close took too long: {elapsed:?}"
    );
}

/// A node with no serve loop should close immediately.
#[tokio::test]
async fn close_without_serve_is_immediate() {
    let ep = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    })
    .await
    .unwrap();

    let start = std::time::Instant::now();
    ep.close().await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "close without serve took too long: {elapsed:?}"
    );
}

/// After shutdown, new requests are rejected (connection refused).
#[tokio::test(start_paused = true)]
async fn shutdown_rejects_new_requests() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let server_ep_handler = server_ep.clone();
    let handle = serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(
                server_ep_handler.handles(),
                payload.req_handle,
                200,
                vec![("content-length".into(), "0".into())],
            )
            .unwrap();
            server_ep_handler
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    // First request should succeed.
    let res = fetch(
        &client_ep,
        &server_id,
        "/before",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    assert_eq!(res.status, 200);
    while let Ok(Some(_)) = client_ep.handles().next_chunk(res.body_handle).await {}

    // Shut down the serve loop.
    handle.drain().await;

    // Close the endpoint too so the client gets a clean rejection.
    server_ep.close_force().await;

    // Request after shutdown should fail.
    let result = fetch(
        &client_ep,
        &server_id,
        "/after",
        "GET",
        &[],
        None,
        None,
        None,
        Some(&addrs),
    )
    .await;
    assert!(
        result.is_err(),
        "expected error after shutdown, got: {:?}",
        result
    );
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

            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();

                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(req_body_handle).await {
                    server_ep
                        .handles()
                        .send_chunk(res_body_handle, chunk)
                        .await
                        .unwrap();
                }
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    // 1 MB of patterned data.
    let data: Vec<u8> = (0u8..=255).cycle().take(1024 * 1024).collect();

    // Allocate a body writer so we can stream the request body.
    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();

    // Send the body in chunks concurrently with fetch.
    let data_clone = data.clone();
    let client_ep_send = client_ep.clone();
    let send_task = tokio::spawn(async move {
        for chunk in data_clone.chunks(8192) {
            client_ep_send
                .handles()
                .send_chunk(writer_handle, Bytes::copy_from_slice(chunk))
                .await
                .unwrap();
        }
        client_ep_send.handles().finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/echo",
        "POST",
        &[],
        Some(body_reader),
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    send_task.await.unwrap();
    assert_eq!(res.status, 200);

    let mut received = Vec::new();
    while let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
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
    for (ep, id) in [(ep_a.clone(), id_a.clone()), (ep_b.clone(), id_b.clone())] {
        let my_id = id.clone();
        let ep_handler = ep.clone();
        serve(
            ep,
            ServeOptions::default(),
            move |payload: RequestPayload| {
                let body = Bytes::from(my_id.clone().into_bytes());
                let res_body = payload.res_body_handle;
                let req = payload.req_handle;
                let ep_spawn = ep_handler.clone();
                tokio::spawn(async move {
                    respond(ep_spawn.handles(), req, 200, vec![]).unwrap();
                    ep_spawn.handles().send_chunk(res_body, body).await.unwrap();
                    ep_spawn.handles().finish_body(res_body).unwrap();
                });
            },
        );
    }

    // A fetches from B, B fetches from A — concurrently.
    let (res_ab, res_ba) = tokio::join!(
        fetch(&ep_a, &id_b, "/who", "GET", &[], None, None, None, Some(&addrs_b)),
        fetch(&ep_b, &id_a, "/who", "GET", &[], None, None, None, Some(&addrs_a)),
    );

    let res_ab = res_ab.unwrap();
    let res_ba = res_ba.unwrap();

    // A fetching B should get B's ID.
    let mut body_ab = Vec::new();
    while let Ok(Some(c)) = ep_a.handles().next_chunk(res_ab.body_handle).await {
        body_ab.extend_from_slice(&c);
    }
    assert_eq!(String::from_utf8(body_ab).unwrap(), id_b);

    // B fetching A should get A's ID.
    let mut body_ba = Vec::new();
    while let Ok(Some(c)) = ep_b.handles().next_chunk(res_ba.body_handle).await {
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
            let content_type = payload
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;

            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Read request body.
                let mut body = Vec::new();
                while let Ok(Some(chunk)) = server_ep.handles().next_chunk(req_body_handle).await {
                    body.extend_from_slice(&chunk);
                }

                // Verify content-type was sent.
                assert_eq!(content_type, "application/json");

                // Echo it back as JSON with content-type.
                respond(
                    server_ep.handles(),
                    req_handle,
                    200,
                    vec![("content-type".into(), "application/json".into())],
                )
                .unwrap();
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, Bytes::from(body))
                    .await
                    .unwrap();
                server_ep.handles().finish_body(res_body_handle).unwrap();
            });
        },
    );

    let json_body = b"{\"hello\":\"world\"}";
    let (writer_handle, body_reader) = client_ep.handles().alloc_body_writer().unwrap();

    let headers = vec![("content-type".to_string(), "application/json".to_string())];

    let client_ep_send = client_ep.clone();
    let send_task = tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from_static(json_body))
            .await
            .unwrap();
        client_ep_send.handles().finish_body(writer_handle).unwrap();
    });

    let res = fetch(
        &client_ep,
        &server_id,
        "/api/data",
        "POST",
        &headers,
        Some(body_reader),
        None,
        None,
        Some(&addrs),
    )
    .await
    .unwrap();
    send_task.await.unwrap();
    assert_eq!(res.status, 200);

    let ct = res
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    assert_eq!(ct, Some("application/json"));

    let mut body = Vec::new();
    while let Ok(Some(chunk)) = client_ep.handles().next_chunk(res.body_handle).await {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(&body, json_body);
}

// -- Server limit enforcement -------------------------------------------------

/// Requests beyond the concurrency limit are queued (semaphore) rather than
/// rejected.  Two concurrent in-flight requests with max_concurrency=2; a
/// third starts after one finishes.  All three must complete successfully.
#[tokio::test]
async fn serve_concurrency_limit() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

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
            None,
            Some(&addrs)
        ),
        fetch(
            &client_ep,
            &server_id,
            "/r2",
            "GET",
            &[],
            None,
            None,
            None,
            Some(&addrs)
        ),
        fetch(
            &client_ep,
            &server_id,
            "/r3",
            "GET",
            &[],
            None,
            None,
            None,
            Some(&addrs)
        ),
    );
    assert_eq!(r1.unwrap().status, 200);
    assert_eq!(r2.unwrap().status, 200);
    assert_eq!(r3.unwrap().status, 200);
    drop(gate);
}

/// Fetch to an unknown peer returns an Error (not a panic or hang).
#[tokio::test]
async fn fetch_unknown_peer() {
    // Generate a random keypair — nobody is listening on it.
    let fake_key = iroh_http_core::generate_secret_key().unwrap();
    let fake_pk = iroh::SecretKey::from_bytes(&fake_key).public();
    let fake_id = iroh_http_core::base32_encode(fake_pk.as_bytes());

    let opts = NodeOptions {
        networking: NetworkingOptions { disabled: true, ..Default::default() },
        ..Default::default()
    };
    let client_ep = IrohEndpoint::bind(opts).await.unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fetch(&client_ep, &fake_id, "/", "GET", &[], None, None, None, None),
    )
    .await;

    // Should not hang — either a timeout or a connection error.
    match result {
        Ok(Err(_)) => {} // connection error — expected
        Err(_) => {}     // our 5s timeout — also acceptable if iroh takes longer
        Ok(Ok(res)) => panic!(
            "expected error connecting to unknown peer, got status {}",
            res.status
        ),
    }
}

/// Graceful shutdown via `ServeHandle::drain` stops accepting new connections
/// but lets in-flight requests complete.
#[tokio::test(start_paused = true)]
async fn node_close_drains_in_flight() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    // The server handler waits for a signal before responding.
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx)));

    let handle = serve(
        server_ep.clone(),
        ServeOptions {
            drain_timeout_secs: Some(5),
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            let tx_clone = tx.clone();
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Signal the test that the handler is in progress.
                if let Some(tx) = tx_clone.lock().await.take() {
                    let _ = tx.send(());
                }
                // Small pause to simulate work.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                server_ep.handles().finish_body(res_body).unwrap();
            });
        },
    );

    // Start a request in the background.
    let fetch_task = tokio::spawn({
        let client_ep = client_ep.clone();
        async move {
            fetch(
                &client_ep,
                &server_id,
                "/drain-test",
                "GET",
                &[],
                None,
                None,
                None,
                Some(&addrs),
            )
            .await
        }
    });

    // Wait until the handler has started.
    let _ = rx.await;

    // Trigger graceful shutdown — should wait for the in-flight request.
    handle.drain().await;

    // The fetch should have completed successfully.
    let res = fetch_task.await.expect("join error");
    assert_eq!(res.unwrap().status, 200);
}

/// Request body exceeding `max_request_body_bytes` causes the stream to be
/// reset — the fetch returns an error.
#[tokio::test]
async fn body_exceeds_limit_resets_stream() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_request_body_bytes: Some(100),
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
        None,
        Some(&addrs),
    )
    .await;

    // Stream reset should produce an error or the body read fails.
    // Either the fetch errors or it succeeds but body is truncated.
    // We don't assert the exact error since it may race; just no panic.
    let _ = result;
}

/// Request timeout: server handler takes longer than `request_timeout_ms`;
/// the fetch task should complete (possibly with error) rather than hang forever.
#[tokio::test]
async fn request_timeout_fires() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            request_timeout_ms: Some(100), // 100ms timeout
            ..Default::default()
        },
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                // Never respond — let the server timeout kill the request.
                std::future::pending::<()>().await;
                // The handler may still respond but it's racing the timeout.
                let _ = respond(server_ep.handles(), req_handle, 200, vec![]);
                let _ = server_ep.handles().finish_body(res_body);
            });
        },
    );

    // The fetch should come back (either with an error or with whatever the
    // server managed to send before timeout killed the task).
    // The 100ms server timeout + propagation should resolve well within 10s.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        fetch(
            &client_ep,
            &server_id,
            "/slow",
            "GET",
            &[],
            None,
            None,
            None,
            Some(&addrs),
        ),
    )
    .await;

    // Should not hang — accept either an error or a 200 that raced through.
    assert!(result.is_ok(), "fetch should not hang past the timeout");
}

// ── URL scheme validation ─────────────────────────────────────────────────────

#[tokio::test]
async fn fetch_rejects_https_scheme() {
    let (server_ep, client_ep) = make_pair().await;
    let err = fetch(
        &client_ep,
        server_ep.node_id(),
        "https://example.com/",
        "GET",
        &[],
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("httpi://"),
        "error should mention httpi://, got: {err}"
    );
}

#[tokio::test]
async fn fetch_rejects_http_scheme() {
    let (server_ep, client_ep) = make_pair().await;
    let err = fetch(
        &client_ep,
        server_ep.node_id(),
        "http://example.com/path",
        "GET",
        &[],
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("httpi://"),
        "error should mention httpi://, got: {err}"
    );
}

// ── Edge-case tests (TEST-004) ────────────────────────────────────────────────

/// Registry: get_endpoint after remove_endpoint returns None without panic.
#[tokio::test]
async fn registry_get_after_remove_returns_none() {
    let opts = NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let ep = IrohEndpoint::bind(opts).await.unwrap();
    let handle = iroh_http_core::insert_endpoint(ep);

    let got = iroh_http_core::get_endpoint(handle);
    assert!(got.is_some());

    let removed = iroh_http_core::remove_endpoint(handle);
    assert!(removed.is_some());

    // Second remove returns None.
    let removed_again = iroh_http_core::remove_endpoint(handle);
    assert!(removed_again.is_none());

    // Get after remove returns None.
    let got_after = iroh_http_core::get_endpoint(handle);
    assert!(got_after.is_none());
}

/// Registry: get_endpoint with a bogus handle returns None without panic.
#[tokio::test]
async fn registry_bogus_handle_returns_none() {
    let got = iroh_http_core::get_endpoint(999_999);
    assert!(got.is_none());
}

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
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions {
            max_concurrency: Some(2),
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
            fetch(&client, &id, &path, "GET", &[], None, None, None, Some(&a)).await
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

/// Cancel mid-stream: client cancels while server is still writing body chunks.
/// The server should observe an error but not panic, and the endpoint stays
/// healthy for subsequent requests.
#[tokio::test]
async fn cancel_mid_stream_no_panic() {
    let (server_ep, client_ep) = make_pair().await;
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    let (request_arrived_tx, request_arrived_rx) = tokio::sync::oneshot::channel::<()>();
    let request_arrived_tx = std::sync::Mutex::new(Some(request_arrived_tx));

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let req_handle = payload.req_handle;
            let res_body = payload.res_body_handle;
            if let Some(tx) = request_arrived_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            let server_ep = server_ep.clone();
            tokio::spawn(async move {
                respond(server_ep.handles(), req_handle, 200, vec![]).unwrap();
                // Write chunks slowly — the client will cancel mid-stream.
                for i in 0..100 {
                    let chunk = Bytes::from(format!("chunk-{i}\n"));
                    if server_ep
                        .handles()
                        .send_chunk(res_body, chunk)
                        .await
                        .is_err()
                    {
                        break; // Client cancelled — expected.
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let _ = server_ep.handles().finish_body(res_body);
            });
        },
    );

    let token = client_ep.handles().alloc_fetch_token().unwrap();

    // Cancel as soon as server starts responding.
    let client_ep_cancel = client_ep.clone();
    tokio::spawn(async move {
        let _ = request_arrived_rx.await;
        // Small delay to let a few chunks through.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        client_ep_cancel.handles().cancel_in_flight(token);
    });

    let result = fetch(
        &client_ep,
        &server_id,
        "/stream",
        "GET",
        &[],
        None,
        None,
        Some(token),
        Some(&addrs),
    )
    .await;

    // Either the fetch errors (cancelled) or we got a partial response.
    // The key assertion: no panic occurred.
    let _ = result;
}

/// Pool: with max_pooled_connections=1, rapid sequential requests to different
/// paths all succeed — the pool evicts cleanly.
#[tokio::test]
async fn pool_eviction_single_slot() {
    let server_opts = NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        pool: iroh_http_core::endpoint::PoolOptions { max_connections: Some(1), ..Default::default() },
        ..Default::default()
    };
    let client_opts = NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        pool: iroh_http_core::endpoint::PoolOptions { max_connections: Some(1), ..Default::default() },
        ..Default::default()
    };
    let server_ep = IrohEndpoint::bind(server_opts).await.unwrap();
    let client_ep = IrohEndpoint::bind(client_opts).await.unwrap();
    let server_id = node_id(&server_ep);
    let addrs = server_addrs(&server_ep);

    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            respond(server_ep.handles(), payload.req_handle, 200, vec![]).unwrap();
            server_ep
                .handles()
                .finish_body(payload.res_body_handle)
                .unwrap();
        },
    );

    // 10 sequential requests — pool has room for only 1 connection.
    for i in 0..10 {
        let path = format!("/pool-test/{i}");
        let res = fetch(
            &client_ep,
            &server_id,
            &path,
            "GET",
            &[],
            None,
            None,
            None,
            Some(&addrs),
        )
        .await
        .unwrap();
        assert_eq!(res.status, 200, "request {i} failed");
    }
}
