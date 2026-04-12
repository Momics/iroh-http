# Change 01 — Adopt hyper v1 as the HTTP engine

## Risk: High — this is the foundational change everything else builds on

## Problem

iroh-http implements HTTP over QUIC streams entirely from scratch:

- Request and response heads are encoded via a custom stateless QPACK wrapper
  (`qpack_bridge.rs`, ~150 lines) that prefixes a 2-byte length before a QPACK
  block. The dynamic table is never used, giving no compression benefit.
- Body framing uses custom HTTP/1.1 chunked encoding in `iroh-http-framing`.
- Reading a request head off the wire involves a manual read-accumulate loop
  that refills a buffer until the 2-byte header length is satisfied, then
  decodes with QPACK. This is ~80 lines of careful byte arithmetic done twice
  (client and server).
- Body pump functions (`pump_body_to_stream`, `pump_stream_to_body`,
  `pump_quic_recv_to_body`, `pump_body_to_quic_send`) total roughly 300 lines
  of channel-to-stream bridging logic.
- Trailer encoding and decoding is handled by `iroh-http-framing`'s
  `serialize_trailers` / `parse_trailers` — another 80 lines.

## Key discovery

`iroh::endpoint::SendStream` and `iroh::endpoint::RecvStream` are re-exports
of `iroh-quinn` types, which implement:
- `tokio::io::AsyncWrite` on `SendStream`
- `tokio::io::AsyncRead` on `RecvStream`

hyper v1 drives I/O through its own `hyper::rt::Read` / `hyper::rt::Write`
traits. `hyper-util` provides `TokioIo<T>` which adapts any `AsyncRead +
AsyncWrite` into what hyper needs. This means Iroh streams can be handed to
hyper with a small wrapper that combines send/recv halves.

## Solution

Add `hyper`, `hyper-util`, and `http-body-util` to
`iroh-http-core/Cargo.toml`. Wire hyper's HTTP/1.1 connection handling to
Iroh's QUIC bidirectional streams.

### Client path (`client.rs`)

**Before:**
```rust
let (mut send, mut recv) = conn.open_bi().await?;
// Encode request head manually with QPACK
let head = codec.encode_request(method, path, &headers)?;
send.write_all(&head).await?;
// Pump body chunks through custom chunked encoding
pump_body_to_stream(req_body, send, chunked, trailer_rx).await;
// Read response head manually: 2-byte len → QPACK decode
let (status, resp_headers, _) = read_head_qpack(&mut recv, &codec, max).await?;
// Pump response body through custom chunk decoder
tokio::spawn(pump_stream_to_body(recv, body_writer, trailer_tx));
```

**After:**
```rust
let (send, recv) = conn.open_bi().await?;
// Wrap Iroh streams for hyper
let io = hyper_util::rt::TokioIo::new(IrohStream { send, recv });
// Hand to hyper — all framing, encoding, chunking is hyper's problem
let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
    .keep_alive(false)  // each QUIC stream is one exchange
    .handshake(io)
    .await?;
tokio::spawn(conn);  // drive the connection
// Build a standard http::Request
let req = http::Request::builder()
    .method(method)
    .uri(path)
    .body(body)?;
let resp = sender.send_request(req).await?;
// Extract status, headers, body via standard http types
let status = resp.status().as_u16();
let headers = resp.headers()...
// Stream body frames through existing BodyWriter channel
let body = resp.into_body();
tokio::spawn(pump_hyper_body_to_channel(body, body_writer, trailer_tx));
```

The `IrohStream` adapter is a thin struct that pairs Iroh's split
`SendStream` + `RecvStream` into a single `AsyncRead + AsyncWrite` type,
which `TokioIo` then adapts for hyper:

```rust
struct IrohStream {
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
}

impl tokio::io::AsyncRead for IrohStream { /* delegate to recv */ }
impl tokio::io::AsyncWrite for IrohStream { /* delegate to send */ }
```

This is small but required glue. It must remain minimal and covered by
integration tests.

### Server path (`server.rs`)

**Before:**
```rust
// Manual head-reading loop, QPACK decode, pump spawning, chunked response
```

**After:**
```rust
let io = hyper_util::rt::TokioIo::new(IrohStream { send, recv });
hyper::server::conn::http1::Builder::new()
    .keep_alive(false)   // each QUIC stream is one exchange
    .serve_connection(io, service)
    .with_upgrades()     // enables the duplex Upgrade path
    .await?;
```

The `service` is a `tower::Service<http::Request<Incoming>>` that invokes
the user's `on_request` callback (see change 02).

### Trailer handling

hyper v1 supports HTTP/1.1 trailers via `http_body::Frame`. On the send side,
`StreamBody` accepts `Frame::trailers(HeaderMap)` interleaved with
`Frame::data(Bytes)`. On the receive side, `body.trailers().await` returns
the trailer `HeaderMap` after the body is consumed.

This replaces the custom `TrailerTx` / `TrailerRx` `oneshot` channel pattern
with hyper's native trailer API. The FFI functions `send_trailers` and
`next_trailer` become thin wrappers around this.

### Duplex / raw_connect

hyper's `with_upgrades()` call on the server enables HTTP Upgrade. On the
client, after receiving a `101` response, `hyper::upgrade::on(response)` yields
an `Upgraded` IO object — the raw stream after protocol switch. This maps
directly to the existing `pump_quic_recv_to_body` / `pump_body_to_quic_send`
pattern, using the upgraded IO as the source/sink instead of raw QUIC streams.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `hyper`, `hyper-util`, `http`, `http-body-util` |
| `iroh-http-core/src/client.rs` | Replace pump loops + QPACK with hyper client conn |
| `iroh-http-core/src/server.rs` | Replace manual head/body handling with hyper server conn |
| `iroh-http-core/src/qpack_bridge.rs` | **Deleted** |
| `iroh-http-core/src/stream.rs` | Remove `TrailerTx`/`TrailerRx` oneshot types; update `send_trailers`/`next_trailer` |

## Dependencies to add

```toml
# iroh-http-core/Cargo.toml
hyper = { version = "1", features = ["http1", "client", "server"] }
hyper-util = { version = "0.1", features = ["tokio"] }
http = "1"
http-body-util = "0.1"
```

## Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
cargo test --test e2e
deno test
```

All existing integration tests exercise the full client→server round-trip.
If they pass, the wire format and body streaming are correct.

Additional targeted tests to add:
- `round_trip_with_trailers` — confirm trailer send/receive through hyper
- `duplex_upgrade_handshake` — confirm 101 and post-upgrade raw streaming
- `large_body_backpressure` — confirm flow control under hyper
- `reject_oversized_header_block` — header size limit still enforced
- `reject_oversized_request_body` — body size limit still enforced
- `reject_oversized_trailers` — trailer size limit enforced

## Notes

- The `qpack` crate dependency is removed entirely.
- The `ALPN` constants change from `iroh-http/1` to `iroh-http/2` (see
  `wire-format.md`).
- `iroh-http-framing` is removed/deprecated as runtime code in this rework (see change 07).
- **Keep-alive must be disabled** on both `http1::Builder` (client) and
  `http1::Builder` (server). Each QUIC bidirectional stream carries exactly
  one request-response exchange. hyper must not attempt to read a second
  request or send Connection: keep-alive headers. See `architecture.md`
  "Per-stream-per-request model" section.
