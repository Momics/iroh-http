//! Iroh QUIC transport primitives shared by the HTTP client and server.
//!
//! `IrohStream` adapts a single bidirectional QUIC stream to
//! `tokio::io::{AsyncRead, AsyncWrite}` so hyper's HTTP/1.1 driver can
//! sit on top of it. `ConnectionPool` keeps idle dials warm so repeated
//! `fetch` calls to the same peer don't pay the QUIC handshake cost
//! every time.

pub(crate) mod io;
pub(crate) mod pool;
