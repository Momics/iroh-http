//! Shared pump helpers bridging QUIC streams and body channels.
//!
//! Used by the FFI session API (uni/bidi streams) and by the duplex
//! upgrade path. After Slice E (#187) some of these may be deletable
//! once the body type unification reaches every call site.

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

/// Bidirectional pump between a byte-level I/O object and a pair of body channels.
///
/// Reads from `io` → sends to `writer` (incoming data).
/// Reads from `reader` → writes to `io` (outgoing data).
///
/// Used for both client-side and server-side duplex upgrade pumps.
pub(crate) async fn pump_duplex<IO>(io: IO, writer: BodyWriter, reader: BodyReader)
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut recv, mut send) = tokio::io::split(io);

    tokio::join!(
        async {
            use bytes::BytesMut;
            use tokio::io::AsyncReadExt;
            let mut buf = BytesMut::with_capacity(PUMP_READ_BUF);
            loop {
                buf.clear();
                match recv.read_buf(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if writer.send_chunk(buf.split().freeze()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        },
        async {
            use tokio::io::AsyncWriteExt;
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
            let _ = send.shutdown().await;
        },
    );
}
