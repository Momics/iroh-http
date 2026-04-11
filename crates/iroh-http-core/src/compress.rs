//! Transparent zstd body compression / decompression.
//!
//! Only compiled when the `compression` feature is enabled.
//! Operates on body channels: spawns a background task that reads from one
//! channel, transforms the data incrementally (streaming), and writes to
//! another.  No full-body buffering — each chunk is processed as it arrives.
//!
//! Uses `async-compression` for true async streaming — data flows through the
//! compressor/decompressor as individual chunks arrive, with bounded memory.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::stream::{make_body_channel, BodyReader, BodyWriter};
use bytes::{Buf, Bytes};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// Options for body compression.
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// zstd compression level (1–22). Default: 3.
    pub level: i32,
    /// Do not compress bodies smaller than this many bytes. Default: 512.
    pub min_body_bytes: usize,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            level: 3,
            min_body_bytes: 512,
        }
    }
}

/// The `Accept-Encoding` header value advertised when compression is enabled.
pub const ACCEPT_ENCODING: &str = "zstd";

/// Returns `true` if `value` signals zstd content encoding.
pub fn is_zstd(value: &str) -> bool {
    value.eq_ignore_ascii_case("zstd")
}

// ── AsyncRead adapter ────────────────────────────────────────────────────────

/// Wraps the mpsc receiver from a `BodyReader` as a `tokio::io::AsyncRead`.
/// Uses `poll_recv` directly — no stream conversion crate needed.
struct BodyAsyncRead {
    rx: mpsc::Receiver<Bytes>,
    remaining: Bytes,
}

impl BodyAsyncRead {
    /// Consume a `BodyReader` and extract the underlying receiver.
    /// Panics if the `BodyReader`'s `Arc` has been cloned elsewhere (it should
    /// not be when handed to compression).
    fn new(reader: BodyReader) -> Self {
        let mutex = Arc::try_unwrap(reader.rx)
            .expect("BodyReader passed to compression must not be shared");
        Self {
            rx: mutex.into_inner(),
            remaining: Bytes::new(),
        }
    }
}

impl tokio::io::AsyncRead for BodyAsyncRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Drain leftover bytes from a previous partial read.
        if !self.remaining.is_empty() {
            let n = std::cmp::min(buf.remaining(), self.remaining.len());
            buf.put_slice(&self.remaining[..n]);
            self.remaining.advance(n);
            return Poll::Ready(Ok(()));
        }

        // Poll the channel for the next chunk.
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(chunk)) => {
                let n = std::cmp::min(buf.remaining(), chunk.len());
                buf.put_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.remaining = chunk.slice(n..);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => Poll::Ready(Ok(())), // EOF
            Poll::Pending => Poll::Pending,
        }
    }
}

// ── Decompression ────────────────────────────────────────────────────────────

/// Spawn a task that reads compressed chunks from `input`, decompresses them
/// incrementally with streaming zstd, and delivers decompressed chunks to the
/// returned reader.  No full-body buffering.
pub fn decompress_body(input: BodyReader) -> BodyReader {
    let (writer, reader) = make_body_channel();
    tokio::spawn(decompress_task(input, writer));
    reader
}

async fn decompress_task(input: BodyReader, writer: BodyWriter) {
    if let Err(e) = decompress_loop(input, &writer).await {
        tracing::warn!("iroh-http: zstd decompress error: {e}");
    }
}

async fn decompress_loop(input: BodyReader, writer: &BodyWriter) -> Result<(), String> {
    let async_read = BodyAsyncRead::new(input);
    let buf_read = tokio::io::BufReader::new(async_read);
    let mut decoder = async_compression::tokio::bufread::ZstdDecoder::new(buf_read);

    let mut out_buf = vec![0u8; 64 * 1024];
    loop {
        let n = decoder
            .read(&mut out_buf)
            .await
            .map_err(|e| format!("zstd decompress: {e}"))?;
        if n == 0 {
            break;
        }
        if writer
            .send_chunk(Bytes::copy_from_slice(&out_buf[..n]))
            .await
            .is_err()
        {
            return Ok(());
        }
    }

    Ok(())
}

// ── Compression ──────────────────────────────────────────────────────────────

/// Spawn a task that reads plain chunks from `input`, compresses them
/// incrementally with streaming zstd, and delivers compressed chunks to the
/// returned reader.  No full-body buffering.
pub fn compress_body(input: BodyReader, level: i32) -> BodyReader {
    let (writer, reader) = make_body_channel();
    tokio::spawn(compress_task(input, writer, level));
    reader
}

async fn compress_task(input: BodyReader, writer: BodyWriter, level: i32) {
    if let Err(e) = compress_loop(input, &writer, level).await {
        tracing::warn!("iroh-http: zstd compress error: {e}");
    }
}

async fn compress_loop(input: BodyReader, writer: &BodyWriter, level: i32) -> Result<(), String> {
    let async_read = BodyAsyncRead::new(input);
    let buf_read = tokio::io::BufReader::new(async_read);
    let quality = async_compression::Level::Precise(level);
    let mut encoder =
        async_compression::tokio::bufread::ZstdEncoder::with_quality(buf_read, quality);

    let mut out_buf = vec![0u8; 64 * 1024];
    loop {
        let n = encoder
            .read(&mut out_buf)
            .await
            .map_err(|e| format!("zstd compress: {e}"))?;
        if n == 0 {
            break;
        }
        if writer
            .send_chunk(Bytes::copy_from_slice(&out_buf[..n]))
            .await
            .is_err()
        {
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trip_small() {
        let data = b"Hello, world!";
        let (w, r) = make_body_channel();
        tokio::spawn({
            let data = Bytes::from_static(data);
            async move {
                w.send_chunk(data).await.unwrap();
                // w drops here → EOF
            }
        });

        let compressed = compress_body(r, 3);
        let decompressed = decompress_body(compressed);

        let mut out = Vec::new();
        while let Some(chunk) = decompressed.next_chunk().await {
            out.extend_from_slice(&chunk);
        }
        assert_eq!(out, data);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trip_large() {
        // 256 KB of patterned data.
        let data: Vec<u8> = (0u8..=255).cycle().take(256 * 1024).collect();
        let data_clone = data.clone();
        let (w, r) = make_body_channel();
        // Send in a background task to avoid blocking on channel backpressure.
        tokio::spawn(async move {
            for chunk in data_clone.chunks(4096) {
                w.send_chunk(Bytes::copy_from_slice(chunk)).await.unwrap();
            }
        });

        let compressed = compress_body(r, 3);
        let decompressed = decompress_body(compressed);

        let mut out = Vec::new();
        while let Some(chunk) = decompressed.next_chunk().await {
            out.extend_from_slice(&chunk);
        }
        assert_eq!(out, data);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trip_empty() {
        let (w, r) = make_body_channel();
        drop(w);

        let compressed = compress_body(r, 3);
        let decompressed = decompress_body(compressed);

        let chunk = decompressed.next_chunk().await;
        assert!(chunk.is_none());
    }

    #[test]
    fn is_zstd_case_insensitive() {
        assert!(is_zstd("zstd"));
        assert!(is_zstd("ZSTD"));
        assert!(is_zstd("Zstd"));
        assert!(!is_zstd("gzip"));
    }
}
