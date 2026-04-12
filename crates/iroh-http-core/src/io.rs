//! `IrohStream` — wraps Iroh's split QUIC stream pair into a single
//! `AsyncRead + AsyncWrite` type suitable for use with `hyper_util::rt::TokioIo`.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Combines an Iroh `SendStream` (write half) and `RecvStream` (read half)
/// into a single bidirectional IO object.
///
/// hyper v1 drives I/O through `hyper_util::rt::TokioIo<T>` which requires
/// a single `T: AsyncRead + AsyncWrite`.  Iroh provides split halves, so this
/// struct bridges the gap.
///
/// `poll_shutdown` calls `SendStream::poll_shutdown`, which in turn calls
/// `SendStream::finish()` — this sends a FIN on the QUIC stream and signals
/// end-of-stream to the remote peer.  hyper calls `poll_shutdown` when the
/// HTTP exchange is complete; the FIN is required for the peer to know the
/// response (or request) is done.
pub(crate) struct IrohStream {
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
}

impl IrohStream {
    pub(crate) fn new(
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) -> Self {
        Self { send, recv }
    }
}

impl AsyncRead for IrohStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for IrohStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map(|r| r.map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e)))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        // Calls SendStream::finish() — sends FIN on the QUIC stream.
        Pin::new(&mut self.send).poll_shutdown(cx)
    }
}
