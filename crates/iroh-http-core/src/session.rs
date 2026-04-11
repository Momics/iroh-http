//! Session — a QUIC connection to a single remote peer.
//!
//! `session_connect` establishes (or reuses a pooled) connection and returns
//! an opaque handle.  `session_create_bidi_stream` / `session_next_bidi_stream`
//! open or accept raw bidirectional streams on that connection with data pumped
//! through the body-channel slab (same mechanism as fetch/serve).

use std::sync::{Mutex, OnceLock};

use iroh::endpoint::Connection;
use slab::Slab;

use crate::{
    parse_node_addr, FfiDuplexStream, IrohEndpoint, ALPN_DUPLEX,
    stream::{make_body_channel, insert_reader, insert_writer, BodyReader, BodyWriter},
};

const READ_BUF: usize = 16 * 1024;

// ── Session slab ─────────────────────────────────────────────────────────────

/// A live session — wraps a QUIC `Connection` to one peer.
struct Session {
    conn: Connection,
}

fn session_slab() -> &'static Mutex<Slab<Session>> {
    static S: OnceLock<Mutex<Slab<Session>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn insert_session(session: Session) -> u32 {
    session_slab().lock().unwrap().insert(session) as u32
}

fn get_conn(handle: u32) -> Result<Connection, String> {
    let slab = session_slab().lock().unwrap();
    slab.get(handle as usize)
        .map(|s| s.conn.clone())
        .ok_or_else(|| format!("invalid session handle: {handle}"))
}

/// Return the remote peer's public key for a session.
pub fn session_remote_id(handle: u32) -> Result<iroh::PublicKey, String> {
    get_conn(handle).map(|c| c.remote_id())
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Establish a session (QUIC connection) to a remote peer.
///
/// Uses the connection pool — if a live connection already exists it is reused.
/// Returns an opaque session handle.
pub async fn session_connect(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    direct_addrs: Option<&[std::net::SocketAddr]>,
) -> Result<u32, String> {
    let parsed = parse_node_addr(remote_node_id)?;
    let node_id = parsed.node_id;
    let mut addr = iroh::EndpointAddr::new(node_id);
    for a in &parsed.direct_addrs {
        addr = addr.with_ip_addr(*a);
    }
    if let Some(addrs) = direct_addrs {
        for a in addrs {
            addr = addr.with_ip_addr(*a);
        }
    }

    let ep_raw = endpoint.raw().clone();
    let addr_clone = addr.clone();

    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN_DUPLEX, || async move {
            ep_raw
                .connect(addr_clone, ALPN_DUPLEX)
                .await
                .map_err(|e| format!("connect session: {e}"))
        })
        .await?;

    let handle = insert_session(Session {
        conn: pooled.conn.clone(),
    });

    Ok(handle)
}

/// Open a new bidirectional stream on an existing session.
///
/// Returns `FfiDuplexStream` with `read_handle` / `write_handle` backed by
/// body channels — the same interface used by fetch and raw_connect.
pub async fn session_create_bidi_stream(
    session_handle: u32,
) -> Result<FfiDuplexStream, String> {
    let conn = get_conn(session_handle)?;

    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| format!("open_bi: {e}"))?;

    Ok(wrap_bidi_stream(send, recv))
}

/// Accept the next incoming bidirectional stream from the remote peer.
///
/// Blocks until the remote opens a stream, or returns `None` when the
/// connection is closed.
pub async fn session_next_bidi_stream(
    session_handle: u32,
) -> Result<Option<FfiDuplexStream>, String> {
    let conn = get_conn(session_handle)?;

    match conn.accept_bi().await {
        Ok((send, recv)) => Ok(Some(wrap_bidi_stream(send, recv))),
        Err(e) => {
            // ConnectionError means the connection is closed.
            let msg = e.to_string();
            if msg.contains("closed") || msg.contains("reset") || msg.contains("timed out") {
                Ok(None)
            } else {
                Err(format!("accept_bi: {e}"))
            }
        }
    }
}

/// Accept an incoming session (QUIC connection) from a remote peer.
///
/// Blocks until a peer connects.  Returns an opaque session handle, or
/// `None` if the endpoint is shutting down.
pub async fn session_accept(
    endpoint: &IrohEndpoint,
) -> Result<Option<u32>, String> {
    let incoming = match endpoint.raw().accept().await {
        Some(inc) => inc,
        None => return Ok(None),
    };

    let conn = incoming
        .await
        .map_err(|e| format!("accept session: {e}"))?;

    let handle = insert_session(Session {
        conn,
    });

    Ok(Some(handle))
}

/// Close a session and remove it from the slab.
pub fn session_close(session_handle: u32) -> Result<(), String> {
    let mut slab = session_slab().lock().unwrap();
    if !slab.contains(session_handle as usize) {
        return Err(format!("invalid session handle: {session_handle}"));
    }
    let session = slab.remove(session_handle as usize);
    session.conn.close(0u32.into(), b"closed");
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Wrap raw QUIC send/recv streams into body-channel–backed `FfiDuplexStream`.
fn wrap_bidi_stream(
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
) -> FfiDuplexStream {
    // Receive side: pump from QUIC recv → BodyWriter → BodyReader (JS reads via nextChunk).
    let (recv_writer, recv_reader) = make_body_channel();
    let read_handle = insert_reader(recv_reader);
    tokio::spawn(pump_recv(recv, recv_writer));

    // Send side: pump from BodyReader (JS writes via sendChunk) → QUIC send.
    let (send_writer, send_reader) = make_body_channel();
    let write_handle = insert_writer(send_writer);
    tokio::spawn(pump_send(send_reader, send));

    FfiDuplexStream {
        read_handle,
        write_handle,
    }
}

/// Pump raw bytes from a QUIC `RecvStream` into a `BodyWriter`.
async fn pump_recv(mut recv: iroh::endpoint::RecvStream, writer: BodyWriter) {
    loop {
        match recv.read_chunk(READ_BUF).await {
            Ok(Some(chunk)) => {
                let bytes = bytes::Bytes::copy_from_slice(&chunk.bytes);
                if writer.send_chunk(bytes).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
    // writer drops → BodyReader sees EOF.
}

/// Pump raw bytes from a `BodyReader` into a QUIC `SendStream`.
async fn pump_send(reader: BodyReader, mut send: iroh::endpoint::SendStream) {
    loop {
        match reader.next_chunk().await {
            None => break,
            Some(data) => {
                if send.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = send.finish();
}
