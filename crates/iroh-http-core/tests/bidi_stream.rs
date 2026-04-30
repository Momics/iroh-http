//! Session bidirectional stream tests.
//!
//! Exercises [`Session::connect`], [`Session::create_bidi_stream`],
//! [`Session::next_bidi_stream`], and [`Session::close`] over real QUIC
//! connections.

use bytes::Bytes;
use iroh_http_core::{IrohEndpoint, NetworkingOptions, NodeOptions, Session};

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
    let b_ep_spawn = b_ep.clone();
    let b_handle = tokio::spawn(async move {
        let session_b = Session::accept(b_ep_spawn.clone()).await.unwrap().unwrap();
        let stream = session_b.next_bidi_stream().await.unwrap().unwrap();

        // Read all data from A.
        let mut received = Vec::new();
        while let Some(chunk) = b_ep_spawn
            .handles()
            .next_chunk(stream.read_handle)
            .await
            .unwrap()
        {
            received.extend_from_slice(&chunk);
        }

        // Echo it back (reversed).
        received.reverse();
        b_ep_spawn
            .handles()
            .send_chunk(stream.write_handle, Bytes::from(received.clone()))
            .await
            .unwrap();
        b_ep_spawn
            .handles()
            .finish_body(stream.write_handle)
            .unwrap();

        // Do NOT close here — it abruptly kills the connection
        // before the pump task can flush. Let the test end naturally.
        (session_b, received)
    });

    // A connects and opens a bidi stream.
    let session_a = Session::connect(a_ep.clone(), &b_id, Some(&b_addrs))
        .await
        .unwrap();
    let stream_a = session_a.create_bidi_stream().await.unwrap();

    // Write 3 chunks.
    let chunks: &[&[u8]] = &[b"hello", b" ", b"world"];
    for chunk in chunks {
        a_ep.handles()
            .send_chunk(stream_a.write_handle, Bytes::from(chunk.to_vec()))
            .await
            .unwrap();
    }
    a_ep.handles().finish_body(stream_a.write_handle).unwrap();

    // Read the echoed response.
    let mut response = Vec::new();
    while let Some(chunk) = a_ep
        .handles()
        .next_chunk(stream_a.read_handle)
        .await
        .unwrap()
    {
        response.extend_from_slice(&chunk);
    }

    let expected: Vec<u8> = b"hello world".iter().rev().cloned().collect();
    assert_eq!(response, expected);

    // Verify B received the correct data.
    let (session_b, b_received) = b_handle.await.unwrap();
    assert_eq!(b_received, expected);

    // Now it's safe to close — both sides have finished reading.
    session_b.close(0, "").ok();
    session_a.close(0, "").ok();
}

// -- Multiple streams on one session ------------------------------------------

#[tokio::test]
async fn session_multiple_bidi_streams() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    let b_ep_spawn = b_ep.clone();
    let b_handle = tokio::spawn(async move {
        let session_b = Session::accept(b_ep_spawn.clone()).await.unwrap().unwrap();

        for i in 0u8..3 {
            let stream = session_b.next_bidi_stream().await.unwrap().unwrap();
            let mut data = Vec::new();
            while let Some(chunk) = b_ep_spawn
                .handles()
                .next_chunk(stream.read_handle)
                .await
                .unwrap()
            {
                data.extend_from_slice(&chunk);
            }
            let mut reply = vec![i];
            reply.extend_from_slice(&data);
            b_ep_spawn
                .handles()
                .send_chunk(stream.write_handle, Bytes::from(reply))
                .await
                .unwrap();
            b_ep_spawn
                .handles()
                .finish_body(stream.write_handle)
                .unwrap();
        }

        session_b
    });

    let session_a = Session::connect(a_ep.clone(), &b_id, Some(&b_addrs))
        .await
        .unwrap();

    for i in 0u8..3 {
        let stream = session_a.create_bidi_stream().await.unwrap();
        let msg = format!("stream-{i}");
        a_ep.handles()
            .send_chunk(stream.write_handle, Bytes::from(msg.clone().into_bytes()))
            .await
            .unwrap();
        a_ep.handles().finish_body(stream.write_handle).unwrap();

        let mut reply = Vec::new();
        while let Some(chunk) = a_ep.handles().next_chunk(stream.read_handle).await.unwrap() {
            reply.extend_from_slice(&chunk);
        }
        assert_eq!(reply[0], i);
        assert_eq!(&reply[1..], msg.as_bytes());
    }

    let session_b = b_handle.await.unwrap();
    session_b.close(0, "").ok();
    session_a.close(0, "").ok();
}

// -- Backpressure -------------------------------------------------------------

#[tokio::test]
async fn session_bidi_stream_backpressure() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    // ISS-022: replace sleep-based coordination with a notify signal.
    // The reader (B) waits until the writer (A) has sent all chunks, creating
    // genuine backpressure without relying on wall-clock timing.
    let (all_written_tx, all_written_rx) = tokio::sync::oneshot::channel::<()>();

    let b_ep_spawn = b_ep.clone();
    let b_handle = tokio::spawn(async move {
        let session_b = Session::accept(b_ep_spawn.clone()).await.unwrap().unwrap();
        let stream = session_b.next_bidi_stream().await.unwrap().unwrap();

        // Deliberately delay reading to create backpressure — but use a signal
        // rather than a sleep so the test completes as fast as the pipe allows.
        let _ = all_written_rx.await;

        let mut total = 0usize;
        while let Some(chunk) = b_ep_spawn
            .handles()
            .next_chunk(stream.read_handle)
            .await
            .unwrap()
        {
            total += chunk.len();
        }

        b_ep_spawn
            .handles()
            .finish_body(stream.write_handle)
            .unwrap();
        (session_b, total)
    });

    let session_a = Session::connect(a_ep.clone(), &b_id, Some(&b_addrs))
        .await
        .unwrap();
    let stream = session_a.create_bidi_stream().await.unwrap();

    // Write many chunks — this should not OOM or buffer unboundedly.
    let chunk = Bytes::from(vec![0xABu8; 1024]);
    let num_chunks = 200;
    for _ in 0..num_chunks {
        a_ep.handles()
            .send_chunk(stream.write_handle, chunk.clone())
            .await
            .unwrap();
    }
    a_ep.handles().finish_body(stream.write_handle).unwrap();
    // Signal the reader that all chunks have been written.
    let _ = all_written_tx.send(());

    let (session_b, total) = b_handle.await.unwrap();
    assert_eq!(total, 1024 * num_chunks);

    // Read the (empty) response — B just closes the write side.
    let eof = a_ep.handles().next_chunk(stream.read_handle).await.unwrap();
    assert!(eof.is_none());

    session_b.close(0, "").ok();
    session_a.close(0, "").ok();
}

// -- Clean close --------------------------------------------------------------

#[tokio::test]
async fn session_bidi_stream_clean_close() {
    let (a_ep, b_ep) = make_pair().await;
    let b_id = node_id(&b_ep);
    let b_addrs = direct_addrs(&b_ep);

    let b_ep_spawn = b_ep.clone();
    let b_handle = tokio::spawn(async move {
        let session_b = Session::accept(b_ep_spawn.clone()).await.unwrap().unwrap();
        let stream = session_b.next_bidi_stream().await.unwrap().unwrap();

        // Finish both sides.
        b_ep_spawn
            .handles()
            .finish_body(stream.write_handle)
            .unwrap();
        let eof = b_ep_spawn
            .handles()
            .next_chunk(stream.read_handle)
            .await
            .unwrap();
        assert!(eof.is_none());

        session_b
    });

    let session_a = Session::connect(a_ep.clone(), &b_id, Some(&b_addrs))
        .await
        .unwrap();
    let stream = session_a.create_bidi_stream().await.unwrap();

    a_ep.handles().finish_body(stream.write_handle).unwrap();
    let eof = a_ep.handles().next_chunk(stream.read_handle).await.unwrap();
    assert!(eof.is_none());

    let session_b = b_handle.await.unwrap();
    session_b.close(0, "").ok();
    session_a.close(0, "").ok();
}
