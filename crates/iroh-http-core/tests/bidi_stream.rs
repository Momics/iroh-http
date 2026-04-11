//! Session bidirectional stream tests.
//!
//! Exercises `session_connect`, `session_create_bidi_stream`,
//! `session_next_bidi_stream`, and `session_close` over real QUIC connections.

use bytes::Bytes;
use iroh_http_core::{
    IrohEndpoint, NodeOptions,
    session_connect, session_create_bidi_stream, session_next_bidi_stream,
    session_accept, session_close,
    next_chunk, send_chunk, finish_body,
};

/// Create a pair of locally-connected endpoints (relay disabled).
async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let opts = || NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let a = IrohEndpoint::bind(opts()).await.unwrap();
    let b = IrohEndpoint::bind(opts()).await.unwrap();
    (a, b)
}

fn node_id(ep: &IrohEndpoint) -> String {
    ep.node_id().to_string()
}

fn direct_addrs(ep: &IrohEndpoint) -> Vec<std::net::SocketAddr> {
    ep.raw().addr().ip_addrs().cloned().collect()
}

// -- Round-trip ---------------------------------------------------------------

#[tokio::test]
async fn session_bidi_stream_round_trip() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    // Spawn B's accept loop in the background.
    // B does NOT close the session — A reads all data before the test ends.
    let b_handle = tokio::spawn(async move {
        let session_b = session_accept(&b_ep).await.unwrap().unwrap();
        let stream = session_next_bidi_stream(session_b).await.unwrap().unwrap();

        // Read all data from A.
        let mut received = Vec::new();
        while let Some(chunk) = next_chunk(stream.read_handle).await.unwrap() {
            received.extend_from_slice(&chunk);
        }

        // Echo it back (reversed).
        received.reverse();
        send_chunk(stream.write_handle, Bytes::from(received.clone())).await.unwrap();
        finish_body(stream.write_handle).unwrap();

        // Do NOT session_close here — it abruptly kills the connection
        // before the pump task can flush. Let the test end naturally.
        (session_b, received)
    });

    // A connects and opens a bidi stream.
    let session_a = session_connect(&a_ep, &b_id, Some(&b_addrs)).await.unwrap();
    let stream_a = session_create_bidi_stream(session_a).await.unwrap();

    // Write 3 chunks.
    let chunks: &[&[u8]] = &[b"hello", b" ", b"world"];
    for chunk in chunks {
        send_chunk(stream_a.write_handle, Bytes::from(chunk.to_vec())).await.unwrap();
    }
    finish_body(stream_a.write_handle).unwrap();

    // Read the echoed response.
    let mut response = Vec::new();
    while let Some(chunk) = next_chunk(stream_a.read_handle).await.unwrap() {
        response.extend_from_slice(&chunk);
    }

    let expected: Vec<u8> = b"hello world".iter().rev().cloned().collect();
    assert_eq!(response, expected);

    // Verify B received the correct data.
    let (session_b, b_received) = b_handle.await.unwrap();
    assert_eq!(b_received, expected);

    // Now it's safe to close — both sides have finished reading.
    session_close(session_b).ok();
    session_close(session_a).ok();
}

// -- Multiple streams on one session ------------------------------------------

#[tokio::test]
async fn session_multiple_bidi_streams() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    let b_handle = tokio::spawn(async move {
        let session_b = session_accept(&b_ep).await.unwrap().unwrap();

        for i in 0u8..3 {
            let stream = session_next_bidi_stream(session_b).await.unwrap().unwrap();
            let mut data = Vec::new();
            while let Some(chunk) = next_chunk(stream.read_handle).await.unwrap() {
                data.extend_from_slice(&chunk);
            }
            let mut reply = vec![i];
            reply.extend_from_slice(&data);
            send_chunk(stream.write_handle, Bytes::from(reply)).await.unwrap();
            finish_body(stream.write_handle).unwrap();
        }

        session_b
    });

    let session_a = session_connect(&a_ep, &b_id, Some(&b_addrs)).await.unwrap();

    for i in 0u8..3 {
        let stream = session_create_bidi_stream(session_a).await.unwrap();
        let msg = format!("stream-{i}");
        send_chunk(stream.write_handle, Bytes::from(msg.clone().into_bytes())).await.unwrap();
        finish_body(stream.write_handle).unwrap();

        let mut reply = Vec::new();
        while let Some(chunk) = next_chunk(stream.read_handle).await.unwrap() {
            reply.extend_from_slice(&chunk);
        }
        assert_eq!(reply[0], i);
        assert_eq!(&reply[1..], msg.as_bytes());
    }

    let session_b = b_handle.await.unwrap();
    session_close(session_b).ok();
    session_close(session_a).ok();
}

// -- Backpressure -------------------------------------------------------------

#[tokio::test]
async fn session_bidi_stream_backpressure() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    let b_handle = tokio::spawn(async move {
        let session_b = session_accept(&b_ep).await.unwrap().unwrap();
        let stream = session_next_bidi_stream(session_b).await.unwrap().unwrap();

        // Deliberately delay reading to create backpressure.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mut total = 0usize;
        while let Some(chunk) = next_chunk(stream.read_handle).await.unwrap() {
            total += chunk.len();
        }

        finish_body(stream.write_handle).unwrap();
        (session_b, total)
    });

    let session_a = session_connect(&a_ep, &b_id, Some(&b_addrs)).await.unwrap();
    let stream = session_create_bidi_stream(session_a).await.unwrap();

    // Write many chunks — this should not OOM or buffer unboundedly.
    let chunk = Bytes::from(vec![0xABu8; 1024]);
    let num_chunks = 200;
    for _ in 0..num_chunks {
        send_chunk(stream.write_handle, chunk.clone()).await.unwrap();
    }
    finish_body(stream.write_handle).unwrap();

    let (session_b, total) = b_handle.await.unwrap();
    assert_eq!(total, 1024 * num_chunks);

    // Read the (empty) response — B just closes the write side.
    let eof = next_chunk(stream.read_handle).await.unwrap();
    assert!(eof.is_none());

    session_close(session_b).ok();
    session_close(session_a).ok();
}

// -- Clean close --------------------------------------------------------------

#[tokio::test]
async fn session_bidi_stream_clean_close() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    let b_handle = tokio::spawn(async move {
        let session_b = session_accept(&b_ep).await.unwrap().unwrap();
        let stream = session_next_bidi_stream(session_b).await.unwrap().unwrap();

        // Finish both sides.
        finish_body(stream.write_handle).unwrap();
        let eof = next_chunk(stream.read_handle).await.unwrap();
        assert!(eof.is_none());

        session_b
    });

    let session_a = session_connect(&a_ep, &b_id, Some(&b_addrs)).await.unwrap();
    let stream = session_create_bidi_stream(session_a).await.unwrap();

    finish_body(stream.write_handle).unwrap();
    let eof = next_chunk(stream.read_handle).await.unwrap();
    assert!(eof.is_none());

    let session_b = b_handle.await.unwrap();
    session_close(session_b).ok();
    session_close(session_a).ok();
}
