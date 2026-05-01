#![allow(clippy::disallowed_types)] // test/bench file — RequestPayload and friends are valid here
mod common;

use bytes::Bytes;
use iroh_http_core::respond;
use iroh_http_core::{fetch, serve, RequestPayload, ServeOptions};

// -- Basic fetch/serve --------------------------------------------------------

#[tokio::test]
async fn basic_get_200() {
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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
    )
    .await
    .unwrap();
    assert_eq!(res.status, 204);
}

// -- URL scheme ---------------------------------------------------------------

#[tokio::test]
async fn url_uses_httpi_scheme() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let client_id = common::node_id(&client_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
    )
    .await
    .unwrap();

    let remote = captured_remote.lock().unwrap().clone();
    assert_eq!(remote, client_id, "Server should see the client's node ID");
}

// -- Multiple requests --------------------------------------------------------

#[tokio::test]
async fn multiple_sequential_requests() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
            Some(&addrs),
            None,
            true,
None, // max_response_body_bytes
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

// -- Empty body POST ----------------------------------------------------------

#[tokio::test]
async fn post_empty_body() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
    )
    .await
    .unwrap();
    assert_eq!(res.status, 204);
}

// -- Concurrent requests ------------------------------------------------------

#[tokio::test]
async fn concurrent_requests() {
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
                Some(&a),
                None,
                true,
None, // max_response_body_bytes
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

// -- URL with query params and fragments --------------------------------------

#[tokio::test]
async fn url_with_query_params() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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

// -- Additional coverage tests -----------------------------------------------

/// Round-trip a 1 MB body to verify streaming works for large payloads.
#[tokio::test]
async fn large_body_round_trip() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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
    let (ep_a, ep_b) = common::make_pair().await;
    let id_a = common::node_id(&ep_a);
    let id_b = common::node_id(&ep_b);
    let addrs_a = common::server_addrs(&ep_a);
    let addrs_b = common::server_addrs(&ep_b);

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
        fetch(
            &ep_a,
            &id_b,
            "/who",
            "GET",
            &[],
            None,
            None,
            Some(&addrs_b),
            None,
            true,
            None, // max_response_body_bytes
        ),
        fetch(
            &ep_b,
            &id_a,
            "/who",
            "GET",
            &[],
            None,
            None,
            Some(&addrs_a),
            None,
            true,
            None, // max_response_body_bytes
        ),
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
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

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
        Some(&addrs),
        None,
        true,
None, // max_response_body_bytes
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

// ── URL scheme validation ─────────────────────────────────────────────────────

#[tokio::test]
async fn fetch_rejects_https_scheme() {
    let (server_ep, client_ep) = common::make_pair().await;
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
        true,
None, // max_response_body_bytes
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
    let (server_ep, client_ep) = common::make_pair().await;
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
        true,
None, // max_response_body_bytes
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("httpi://"),
        "error should mention httpi://, got: {err}"
    );
}
