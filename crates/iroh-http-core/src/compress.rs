//! Transparent zstd body compression / decompression.
//!
//! Only compiled when the `compression` feature is enabled.
//! Operates on body channels: spawns a background task that reads from one
//! channel, transforms the data, and writes to another.

use bytes::Bytes;
use crate::stream::{BodyReader, BodyWriter, make_body_channel};

/// Options for body compression.
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// zstd compression level (1–22). Default: 3.
    pub level: i32,
    /// Do not compress bodies smaller than this many bytes. Default: 512.
    /// Only applied when `Content-Length` is known; streaming bodies are always
    /// compressed.
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

// ── Decompression ────────────────────────────────────────────────────────────

/// Spawn a task that reads compressed chunks from `input`, decompresses them
/// with streaming zstd, and delivers decompressed chunks to the returned reader.
pub fn decompress_body(input: BodyReader) -> BodyReader {
    let (writer, reader) = make_body_channel();
    tokio::spawn(decompress_task(input, writer));
    reader
}

async fn decompress_task(input: BodyReader, writer: BodyWriter) {
    if let Err(e) = decompress_loop(input, &writer).await {
        tracing::warn!("iroh-http: zstd decompress error: {e}");
    }
    // writer drops here → reader sees EOF
}

async fn decompress_loop(input: BodyReader, writer: &BodyWriter) -> Result<(), String> {
    let mut decompressor = zstd::bulk::Decompressor::new()
        .map_err(|e| format!("zstd init: {e}"))?;
    // Accumulate all compressed bytes, then decompress in one shot per chunk.
    // For streaming, we buffer the entire compressed input (which is bounded
    // by the body size) and do a single bulk decompression.
    let mut compressed = Vec::new();

    while let Some(chunk) = input.next_chunk().await {
        compressed.extend_from_slice(&chunk);
    }

    if compressed.is_empty() {
        return Ok(());
    }

    // Bulk decompress — try increasingly larger output buffers.
    let mut cap = compressed.len() * 4;
    let decompressed = loop {
        match decompressor.decompress(&compressed, cap) {
            Ok(data) => break data,
            Err(e) => {
                // If the buffer was too small, double and retry.
                if cap < 256 * 1024 * 1024 {
                    cap *= 2;
                    continue;
                }
                return Err(format!("zstd decompress: {e}"));
            }
        }
    };

    if !decompressed.is_empty() {
        // Send in reasonably sized chunks.
        for chunk in decompressed.chunks(64 * 1024) {
            if writer.send_chunk(Bytes::copy_from_slice(chunk)).await.is_err() {
                return Ok(());
            }
        }
    }

    Ok(())
}

// ── Compression ──────────────────────────────────────────────────────────────

/// Spawn a task that reads plain chunks from `input`, compresses them with
/// zstd at the given level, and delivers compressed chunks to the returned reader.
pub fn compress_body(input: BodyReader, level: i32) -> BodyReader {
    let (writer, reader) = make_body_channel();
    tokio::spawn(compress_task(input, writer, level));
    reader
}

async fn compress_task(input: BodyReader, writer: BodyWriter, level: i32) {
    if let Err(e) = compress_loop(input, &writer, level).await {
        tracing::warn!("iroh-http: zstd compress error: {e}");
    }
    // writer drops here → reader sees EOF
}

async fn compress_loop(
    input: BodyReader,
    writer: &BodyWriter,
    level: i32,
) -> Result<(), String> {
    // Accumulate all plaintext, then compress in bulk.
    let mut plain = Vec::new();

    while let Some(chunk) = input.next_chunk().await {
        plain.extend_from_slice(&chunk);
    }

    if plain.is_empty() {
        return Ok(());
    }

    let compressed = zstd::bulk::compress(&plain, level)
        .map_err(|e| format!("zstd compress: {e}"))?;

    // Send in reasonably sized chunks.
    for chunk in compressed.chunks(64 * 1024) {
        if writer.send_chunk(Bytes::copy_from_slice(chunk)).await.is_err() {
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
