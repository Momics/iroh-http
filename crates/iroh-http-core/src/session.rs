//! Session — a QUIC connection to a single remote peer.
//!
//! `session_connect` establishes (or reuses a pooled) connection and returns
//! an opaque handle.  Bidirectional streams, unidirectional streams, and
//! datagrams are all accessible through the session handle.

use iroh::endpoint::Connection;
use serde::Serialize;

use crate::{
    parse_node_addr,
    stream::{pump_body_to_quic_send, pump_quic_recv_to_body, HandleStore, SessionEntry},
    CoreError, FfiDuplexStream, IrohEndpoint, ALPN_DUPLEX,
};

/// Returns `true` if the connection error means "connection ended" rather
/// than a protocol-level bug.  Used to return `None` instead of `Err`.
fn is_connection_closed(err: &iroh::endpoint::ConnectionError) -> bool {
    use iroh::endpoint::ConnectionError::*;
    matches!(
        err,
        ApplicationClosed(_) | ConnectionClosed(_) | Reset | TimedOut | LocallyClosed
    )
}

/// Close information returned when a session ends.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseInfo {
    pub close_code: u64,
    pub reason: String,
}

// ── Session registry ──────────────────────────────────────────────────────────

fn get_conn(endpoint: &IrohEndpoint, handle: u64) -> Result<Connection, CoreError> {
    endpoint
        .handles()
        .lookup_session(handle)
        .map(|s| s.conn.clone())
        .ok_or_else(|| CoreError::invalid_handle(handle))
}

/// Return the remote peer's public key for a session.
pub fn session_remote_id(
    endpoint: &IrohEndpoint,
    handle: u64,
) -> Result<iroh::PublicKey, CoreError> {
    get_conn(endpoint, handle).map(|c| c.remote_id())
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Establish a session (QUIC connection) to a remote peer.
///
/// Each call creates a **dedicated** QUIC connection — sessions are not pooled.
/// This ensures that closing one session handle cannot affect other sessions
/// to the same peer.  (Fetch operations continue to use the shared pool for
/// efficiency; sessions opt out because `session_close` closes the underlying
/// connection.)
///
/// Returns an opaque session handle.
pub async fn session_connect(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    direct_addrs: Option<&[std::net::SocketAddr]>,
) -> Result<u64, CoreError> {
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

    let conn = endpoint
        .raw()
        .connect(addr, ALPN_DUPLEX)
        .await
        .map_err(|e| CoreError::connection_failed(format!("connect session: {e}")))?;

    let handle = endpoint.handles().insert_session(SessionEntry { conn })?;

    Ok(handle)
}

/// Open a new bidirectional stream on an existing session.
///
/// Returns `FfiDuplexStream` with `read_handle` / `write_handle` backed by
/// body channels — the same interface used by fetch and raw_connect.
pub async fn session_create_bidi_stream(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<FfiDuplexStream, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;

    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| CoreError::connection_failed(format!("open_bi: {e}")))?;

    wrap_bidi_stream(endpoint.handles(), send, recv)
}

/// Accept the next incoming bidirectional stream from the remote side.
///
/// Blocks until the remote opens a stream, or returns `None` when the
/// connection is closed.
pub async fn session_next_bidi_stream(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<Option<FfiDuplexStream>, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;

    match conn.accept_bi().await {
        Ok((send, recv)) => Ok(Some(wrap_bidi_stream(endpoint.handles(), send, recv)?)),
        Err(e) if is_connection_closed(&e) => Ok(None),
        Err(e) => Err(CoreError::connection_failed(format!("accept_bi: {e}"))),
    }
}

/// Accept an incoming session (QUIC connection) from a remote peer.
///
/// Blocks until a peer connects.  Returns an opaque session handle, or
/// `None` if the endpoint is shutting down.
pub async fn session_accept(endpoint: &IrohEndpoint) -> Result<Option<u64>, CoreError> {
    let incoming = match endpoint.raw().accept().await {
        Some(inc) => inc,
        None => return Ok(None),
    };

    let conn = incoming
        .await
        .map_err(|e| CoreError::connection_failed(format!("accept session: {e}")))?;

    let handle = endpoint.handles().insert_session(SessionEntry { conn })?;

    Ok(Some(handle))
}

/// Close a session and remove it from the registry.
///
/// `close_code` is an application-level error code (maps to QUIC VarInt).
/// `reason` is a human-readable string sent to the peer.
pub fn session_close(
    endpoint: &IrohEndpoint,
    session_handle: u64,
    close_code: u64,
    reason: &str,
) -> Result<(), CoreError> {
    let entry = endpoint
        .handles()
        .remove_session(session_handle)
        .ok_or_else(|| CoreError::invalid_handle(session_handle))?;
    let code = iroh::endpoint::VarInt::from_u64(close_code).map_err(|_| {
        CoreError::invalid_input(format!(
            "close_code {close_code} exceeds QUIC VarInt max (2^62 - 1)"
        ))
    })?;
    entry.conn.close(code, reason.as_bytes());
    Ok(())
}

/// Wait for the QUIC handshake to complete on a session.
///
/// Resolves immediately if the handshake has already completed.
pub async fn session_ready(endpoint: &IrohEndpoint, session_handle: u64) -> Result<(), CoreError> {
    // Validate handle exists — keeps error behavior consistent with other session APIs.
    let _conn = get_conn(endpoint, session_handle)?;
    // iroh connections are fully established by the time session_connect returns,
    // so ready always resolves immediately. Kept for WebTransport API compatibility.
    Ok(())
}

/// Wait for the session to close and return the close information.
///
/// Blocks until the connection is closed by either side.  Removes the
/// session from the registry so resources are freed.
pub async fn session_closed(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<CloseInfo, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;
    let err = conn.closed().await;
    // Connection is dead — clean up the registry entry.
    endpoint.handles().remove_session(session_handle);
    let (close_code, reason) = parse_connection_error(&err);
    Ok(CloseInfo { close_code, reason })
}

// ── Unidirectional streams ────────────────────────────────────────────────────

/// Open a new unidirectional (send-only) stream on an existing session.
///
/// Returns a write handle backed by a body channel.
pub async fn session_create_uni_stream(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<u64, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;
    let send = conn
        .open_uni()
        .await
        .map_err(|e| CoreError::connection_failed(format!("open_uni: {e}")))?;

    let handles = endpoint.handles();
    let (send_writer, send_reader) = handles.make_body_channel();
    let write_handle = handles.insert_writer(send_writer)?;
    tokio::spawn(pump_body_to_quic_send(send_reader, send));

    Ok(write_handle)
}

/// Accept the next incoming unidirectional (receive-only) stream.
///
/// Returns a read handle, or `None` when the connection is closed.
pub async fn session_next_uni_stream(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<Option<u64>, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;

    match conn.accept_uni().await {
        Ok(recv) => {
            let handles = endpoint.handles();
            let (recv_writer, recv_reader) = handles.make_body_channel();
            let read_handle = handles.insert_reader(recv_reader)?;
            tokio::spawn(pump_quic_recv_to_body(recv, recv_writer));
            Ok(Some(read_handle))
        }
        Err(e) if is_connection_closed(&e) => Ok(None),
        Err(e) => Err(CoreError::connection_failed(format!("accept_uni: {e}"))),
    }
}

// ── Datagrams ─────────────────────────────────────────────────────────────────

/// Send a datagram on the session.
///
/// Fails if `data.len()` exceeds `session_max_datagram_size`.
pub fn session_send_datagram(
    endpoint: &IrohEndpoint,
    session_handle: u64,
    data: &[u8],
) -> Result<(), CoreError> {
    let conn = get_conn(endpoint, session_handle)?;
    conn.send_datagram(bytes::Bytes::copy_from_slice(data))
        .map_err(|e| CoreError::internal(format!("send_datagram: {e}")))
}

/// Receive the next datagram from the session.
///
/// Blocks until a datagram arrives, or returns `None` when the connection closes.
pub async fn session_recv_datagram(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<Option<Vec<u8>>, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;
    match conn.read_datagram().await {
        Ok(data) => Ok(Some(data.to_vec())),
        Err(e) if is_connection_closed(&e) => Ok(None),
        Err(e) => Err(CoreError::connection_failed(format!("recv_datagram: {e}"))),
    }
}

/// Return the current maximum datagram payload size for this session.
///
/// Returns `None` if datagrams are not supported on the current path.
pub fn session_max_datagram_size(
    endpoint: &IrohEndpoint,
    session_handle: u64,
) -> Result<Option<usize>, CoreError> {
    let conn = get_conn(endpoint, session_handle)?;
    Ok(conn.max_datagram_size())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Wrap raw QUIC send/recv streams into body-channel–backed `FfiDuplexStream`.
fn wrap_bidi_stream(
    handles: &HandleStore,
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
) -> Result<FfiDuplexStream, CoreError> {
    let mut guard = handles.insert_guard();

    // Receive side: pump from QUIC recv → BodyWriter → BodyReader (JS reads via nextChunk).
    let (recv_writer, recv_reader) = handles.make_body_channel();
    let read_handle = guard.insert_reader(recv_reader)?;
    tokio::spawn(pump_quic_recv_to_body(recv, recv_writer));

    // Send side: pump from BodyReader (JS writes via sendChunk) → QUIC send.
    let (send_writer, send_reader) = handles.make_body_channel();
    let write_handle = guard.insert_writer(send_writer)?;
    tokio::spawn(pump_body_to_quic_send(send_reader, send));

    guard.commit();
    Ok(FfiDuplexStream {
        read_handle,
        write_handle,
    })
}

/// Extract close code and reason from a QUIC `ConnectionError`.
fn parse_connection_error(err: &iroh::endpoint::ConnectionError) -> (u64, String) {
    match err {
        iroh::endpoint::ConnectionError::ApplicationClosed(info) => {
            let code: u64 = info.error_code.into();
            let reason = String::from_utf8_lossy(&info.reason).into_owned();
            (code, reason)
        }
        other => (0, other.to_string()),
    }
}
