#![allow(clippy::disallowed_types)] // test/bench file — RequestPayload and friends are valid here
//! Inbound request body decompression — `Content-Encoding: zstd` round-trip.
//!
//! Closes #153 — verifies that a peer-supplied compressed request body is
//! transparently decompressed before it reaches the JS-visible handler.

mod common;

use bytes::Bytes;
use iroh_http_core::respond;
use iroh_http_core::{fetch, serve, RequestPayload, ServeOptions};

#[tokio::test]
async fn request_body_with_content_encoding_zstd_is_decompressed() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    let plaintext = b"hello, decompression world! ".repeat(64);
    let plaintext_len = plaintext.len();

    let server_ep_handler = server_ep.clone();
    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            assert_eq!(payload.method, "POST");

            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;
            let server_ep = server_ep_handler.clone();

            tokio::spawn(async move {
                let mut body = Vec::new();
                while let Some(chunk) = server_ep
                    .handles()
                    .next_chunk(req_body_handle)
                    .await
                    .expect("read request body chunk")
                {
                    body.extend_from_slice(&chunk);
                }

                let response_body = format!("received {} bytes", body.len());
                respond(server_ep.handles(), req_handle, 200, vec![]).expect("write response head");
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, Bytes::from(response_body.into_bytes()))
                    .await
                    .expect("send response body");
                server_ep
                    .handles()
                    .finish_body(res_body_handle)
                    .expect("finish response body");
            });
        },
    );

    // Compress the request body with zstd (default level 3).
    let compressed =
        zstd::stream::encode_all(plaintext.as_slice(), 0).expect("zstd encode succeeds");
    assert!(
        compressed.len() < plaintext_len,
        "compressed body should be smaller than plaintext"
    );

    let (writer_handle, body_reader) = client_ep
        .handles()
        .alloc_body_writer()
        .expect("alloc body writer");

    let client_ep_send = client_ep.clone();
    let compressed_for_send = compressed.clone();
    tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from(compressed_for_send))
            .await
            .expect("send compressed chunk");
        client_ep_send
            .handles()
            .finish_body(writer_handle)
            .expect("finish request body");
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
    .expect("fetch succeeds");

    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .expect("read response body chunk")
    {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8(body).expect("utf8 response"),
        format!("received {plaintext_len} bytes"),
        "handler should have observed the decompressed body length, not the compressed one",
    );
}

#[tokio::test]
async fn request_body_without_content_encoding_passes_through() {
    let (server_ep, client_ep) = common::make_pair().await;
    let server_id = common::node_id(&server_ep);
    let addrs = common::server_addrs(&server_ep);

    let plaintext = b"plain body, no encoding".to_vec();
    let plaintext_len = plaintext.len();

    let server_ep_handler = server_ep.clone();
    serve(
        server_ep.clone(),
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let req_body_handle = payload.req_body_handle;
            let res_body_handle = payload.res_body_handle;
            let req_handle = payload.req_handle;
            let server_ep = server_ep_handler.clone();

            tokio::spawn(async move {
                let mut body = Vec::new();
                while let Some(chunk) = server_ep
                    .handles()
                    .next_chunk(req_body_handle)
                    .await
                    .expect("read chunk")
                {
                    body.extend_from_slice(&chunk);
                }
                let response_body = format!("received {} bytes", body.len());
                respond(server_ep.handles(), req_handle, 200, vec![]).expect("write head");
                server_ep
                    .handles()
                    .send_chunk(res_body_handle, Bytes::from(response_body.into_bytes()))
                    .await
                    .expect("send response");
                server_ep
                    .handles()
                    .finish_body(res_body_handle)
                    .expect("finish");
            });
        },
    );

    let (writer_handle, body_reader) = client_ep
        .handles()
        .alloc_body_writer()
        .expect("alloc body writer");

    let client_ep_send = client_ep.clone();
    tokio::spawn(async move {
        client_ep_send
            .handles()
            .send_chunk(writer_handle, Bytes::from(plaintext))
            .await
            .expect("send plaintext");
        client_ep_send
            .handles()
            .finish_body(writer_handle)
            .expect("finish");
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
    .expect("fetch succeeds");

    assert_eq!(res.status, 200);

    let mut body = Vec::new();
    while let Some(chunk) = client_ep
        .handles()
        .next_chunk(res.body_handle)
        .await
        .expect("read response chunk")
    {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8(body).expect("utf8 response"),
        format!("received {plaintext_len} bytes"),
    );
}
