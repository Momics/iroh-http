# Change 03 — Replace compress.rs with tower-http layers

## Risk: Low — additive replacement, feature-gated

## Problem

`compress.rs` (255 lines) implements streaming zstd compression and
decompression using `async-compression`. It manually:

- Wraps `BodyReader` in a custom `BodyAsyncRead` adapter (~50 lines) to bridge
  the mpsc channel into `tokio::io::AsyncRead`
- Spawns a task that pipes the `AsyncRead` through `async-compression`'s
  `ZstdEncoder` / `ZstdDecoder`
- Handles minimum-size thresholds (don't compress small bodies)
- Negotiates based on `Accept-Encoding: zstd` / `Content-Encoding: zstd` headers

The code is correct, but it's 255 lines that replicate what `tower-http` does
as a one-line middleware layer — with broader algorithm support and better
integration with hyper's body types.

## Solution

`tower-http` provides `CompressionLayer` and `RequestDecompressionLayer`.
After change 01 (hyper), request and response bodies flow through hyper's
`Incoming` and `BoxBody` types, which are the exact types these layers expect.

### Server-side response compression

In the `ServiceBuilder` chain (change 02), add:

```rust
use tower_http::compression::{CompressionLayer, predicate::SizeAbove};

let svc = tower::ServiceBuilder::new()
    .concurrency_limit(max_concurrency)
    .timeout(request_timeout)
    .layer(
        CompressionLayer::new()
            .zstd(true)
            .gzip(true)     // free, adds no meaningful overhead
            .br(true)       // free
            .compress_when(SizeAbove::new(512))  // replaces min_size_bytes
    )
    .service(RequestService { ... });
```

`CompressionLayer` reads `Accept-Encoding` from the request and sets
`Content-Encoding` on the response automatically. The application code never
touches compression — it just returns a `Response<BoxBody>` with uncompressed
bytes.

### Client-side request decompression

For decompressing server responses received by the client, use
`tower_http::decompression::DecompressionLayer` in the client's service stack:

```rust
// client.rs — wrap the hyper sender in a tower service with decompression
let svc = tower::ServiceBuilder::new()
    .layer(tower_http::decompression::DecompressionLayer::new())
    .service(HyperClientService::new(sender));
```

The client receives the compressed bytes from the server, tower-http
decompresses them transparently before returning to the application layer.

### Feature flag

Keep the `compression` feature flag in `Cargo.toml` so compression remains
opt-in:

```toml
[features]
compression = ["tower-http/compression-zstd", "tower-http/compression-gzip", "tower-http/compression-br", "tower-http/decompression-full"]

[dependencies]
tower-http = { version = "0.6", features = ["timeout", "trace"] }
# compression features added conditionally via the feature flag above
```

### Compression level

`tower-http`'s `CompressionLayer` currently does not expose a per-level zstd
API (it uses the default level). If configurable compression level is needed,
open a follow-up: either contribute to tower-http or wrap the body in a custom
`Body` impl after the layer. For the initial rework, default level is
acceptable — the existing default was already 3 (fast).

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `tower-http` with compression features; remove `async-compression` |
| `iroh-http-core/src/compress.rs` | **Deleted** |
| `iroh-http-core/src/server.rs` | Add `CompressionLayer` to ServiceBuilder (change 02 already sets this up) |
| `iroh-http-core/src/client.rs` | Add `DecompressionLayer` to client service chain |
| `iroh-http-core/src/lib.rs` | Remove `compress` module export |

## Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

The existing compression integration tests (`test_compression_zstd`,
`test_no_compression_below_threshold`) must pass with the new implementation.
Add:
- `test_compression_gzip` — verifies gzip negotiation now works
- `test_compression_brotli` — verifies brotli negotiation now works

## Notes

- Compression negotiation is now fully standards-compliant — the server
  responds to `Accept-Encoding` correctly for any algorithm in tower-http's
  supported set.
- The `CompressionOptions` struct (currently `{ level: u8, min_size_bytes: usize }`)
  is simplified. The `min_size_bytes` is expressed as `SizeAbove::new(n)`.
  The `level` field is removed pending tower-http API support.
- The `BodyAsyncRead` adapter in `compress.rs` is gone. This also removes
  one of the few places in the codebase where `tokio::io::AsyncRead` was
  manually implemented for a custom type.
