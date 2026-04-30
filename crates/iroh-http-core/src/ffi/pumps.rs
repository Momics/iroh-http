//! Shared pump helpers bridging QUIC streams and body channels.
//!
//! Used by the FFI session API (uni/bidi streams).

use super::handles::{BodyReader, BodyWriter};

// ── Shared pump helpers ───────────────────────────────────────────────────────

/// Default read buffer size for QUIC stream reads.
pub(crate) const PUMP_READ_BUF: usize = 64 * 1024;

/// Pump raw bytes from a QUIC `RecvStream` into a `BodyWriter`.
///
/// Reads `PUMP_READ_BUF`-sized chunks and forwards them through the body
/// channel.  Stops when the stream ends or the writer is dropped.
pub(crate) async fn pump_quic_recv_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
) {
    while let Ok(Some(chunk)) = recv.read_chunk(PUMP_READ_BUF).await {
        if writer.send_chunk(chunk.bytes).await.is_err() {
            break;
        }
    }
    // writer drops → BodyReader sees EOF.
}

/// Pump raw bytes from a `BodyReader` into a QUIC `SendStream`.
///
/// Reads chunks from the body channel and writes them to the stream.
/// Finishes the stream when the reader reaches EOF.
pub(crate) async fn pump_body_to_quic_send(
    reader: BodyReader,
    mut send: iroh::endpoint::SendStream,
) {
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
