---
status: pending
---

# iroh-http — Patch 19: Body Compression

## Problem

Bodies are transmitted uncompressed. For the traffic patterns iroh-http is
optimised for — JSON APIs, structured data, event streams — this wastes
bandwidth and increases latency, particularly over high-latency P2P relay
paths.

---

## Design

### Approach

Use the **standard HTTP compression mechanism** defined by
[RFC 9110 §8.4](https://datatracker.ietf.org/doc/html/rfc9110#section-8.4):
`Accept-Encoding` to advertise capability, `Content-Encoding` to signal that
a message body is compressed.

This is the same mechanism used by every HTTP client and server in existence.
It requires no custom protocol, no ALPN changes, and no new concepts for the
developer to learn. It is transparent to JS and Python handlers — the Rust
layer decompresses incoming bodies and compresses outgoing ones before any
bytes cross the FFI boundary.

### Where compression lives: Rust

Compression is implemented once in `iroh-http-core`, behind a feature flag,
and applies to all platform adapters (Node, Deno, Tauri, Python) without any
adapter-specific code.

Doing it per-platform would mean implementing the same codec four times.
`CompressionStream` / `DecompressionStream` (WHATWG Streams) are inconsistently
available across runtimes, do not cover the server-side decompression path
cleanly, and would require buffering in JS — the wrong layer for a streaming
body pump.

The Rust body pump already sees the raw bytes, the parsed headers, and the
framing boundaries. It is the right place.

### Algorithm

`zstd` is the only supported encoding. It offers the best ratio/speed tradeoff for structured data, has an excellent pure-streaming Rust crate, and is supported by all modern HTTP clients and servers. gzip and brotli are out of scope — iroh-http targets peer-to-peer traffic between nodes that all run this library, so interop with legacy gzip-only clients is not a requirement.

### Negotiation

**Client → Server** (outbound `fetch`):

1. Rust automatically appends `Accept-Encoding: zstd` to every request
   when the `compression` feature is enabled.
2. The remote server replies with the body compressed and a
   `Content-Encoding: zstd` header.
3. Rust detects `Content-Encoding: zstd`, decompresses the response body, and strips
   the header before constructing the `Response` delivered to JS. JS sees a
   plain, uncompressed body — identical to what a browser delivers from
   `globalThis.fetch`.

**Server ← Client** (inbound request to `serve`):

1. A remote peer sends a compressed request body with `Content-Encoding: zstd`.
2. Rust detects the header, decompresses the request body, and strips the
   header before delivering the `Request` to the JS handler.
3. The JS handler sees a plain, uncompressed body. `req.headers.get('content-encoding')`
   returns `null`.

**Response compression** (outbound response from `serve`):

1. If the incoming request includes `Accept-Encoding: zstd`, Rust compresses
   the response body the handler returns and appends `Content-Encoding: zstd`
   to the response headers before writing to the stream.
2. The JS handler does not need to set `Content-Encoding` itself — the library
   handles it. If the handler explicitly sets `Content-Encoding` (e.g. because
   the body is already compressed), the library respects it and skips automatic
   compression for that response.

### Threshold

Bodies smaller than a configurable `minBodyBytes` threshold (default: 512 B)
are sent uncompressed regardless — small bodies frequently expand under
compression, and the algorithm overhead is not justified. This is applied
silently; no header change, no error.

### Portability

Because the wire format is standard HTTP (`Content-Encoding: zstd`), a
compressed iroh-http node can exchange compressed bodies with any HTTP client
or server that supports zstd — not just other iroh-http nodes.

### Embedded / no_std targets

The `compression` feature has `default = []` — it is **opt-in and never
compiled into a binary unless explicitly requested**. Embedded and ESP targets
build `iroh-http-core` without the feature and pay zero code-size or runtime
overhead: `zstd` is not pulled into the dependency graph, and the `compress.rs`
module does not exist in the build.

When a node without the feature receives a `Content-Encoding: zstd` response
from a peer that has it enabled, it passes the compressed bytes through as-is.
The body bytes delivered to the handler will be raw compressed data. This is
the same behaviour as any HTTP client that does not advertise the encoding —
the peer should not have compressed the response in the first place because the
node will not send `Accept-Encoding`. In practice: two iroh-http nodes of
mismatched feature sets degrade gracefully to uncompressed transfer.

---

## Changes

### `crates/iroh-http-core/Cargo.toml`

```toml
[features]
default = []
qpack = ["dep:qpack"]
compression = ["dep:zstd"]

[dependencies]
zstd = { version = "0.13", optional = true, default-features = false }
```

### `crates/iroh-http-core/src/compress.rs` (new file)

```rust
// Transparently compress / decompress async byte streams.
// Only compiled when the `compression` feature is enabled.

/// Wrap `reader` in a zstd-decompressing reader.
pub fn decompress(reader: impl AsyncBufRead + Send + 'static)
    -> Pin<Box<dyn AsyncRead + Send>>;

/// Wrap `reader` in a zstd-compressing reader at the given level.
pub fn compress(reader: impl AsyncRead + Send + 'static, level: i32)
    -> Pin<Box<dyn AsyncRead + Send>>;

/// Return true if the `Content-Encoding` header value is `"zstd"`.
pub fn is_zstd(value: &str) -> bool;

/// The `Accept-Encoding` header value this node will advertise.
pub const ACCEPT_ENCODING: &str = "zstd";
```

### `crates/iroh-http-core/src/endpoint.rs`

New field on `NodeOptions`:

```rust
/// Body compression options. `None` disables compression (default).
pub compression: Option<CompressionOptions>,
```

```rust
pub struct CompressionOptions {
    /// zstd compression level (1–22). Default: 3.
    pub level: Option<i32>,
    /// Do not compress bodies smaller than this many bytes. Default: 512.
    pub min_body_bytes: Option<usize>,
}
```

`CompressionOptions` is stored in `EndpointInner` so both `client.rs` and
`server.rs` can read it without threading it through every call.

### `crates/iroh-http-core/src/client.rs`

On the **send** path, when `endpoint.compression()` is `Some(_)`:

1. Inject `Accept-Encoding: zstd` into the outgoing request headers
   (unless the caller already set one).

On the **receive** path, after parsing the response head:

1. Check response headers for `Content-Encoding`.
2. If present, wrap the receive byte stream in the matching `decompress()`
   reader and remove `Content-Encoding` from the headers delivered to JS.

No changes to the slab, `BodyReader`, or JS-facing types.

### `crates/iroh-http-core/src/server.rs`

On the **receive** path (incoming request), after parsing the request head:

1. Check for `Content-Encoding`. If present, wrap the incoming body stream
   in the matching `decompress()` reader and remove the header before
   building the `RequestPayload` delivered to JS.

On the **send** path (outgoing response), after `respond()` is called:

1. If the original request had `Accept-Encoding` containing a supported
   encoding and the response body is not already `Content-Encoding`-labelled
   and the body exceeds `min_body_bytes`:
   - Wrap the response body stream in `compress()`.
   - Append `Content-Encoding: <encoding>` to the response headers.

### `iroh-http-shared/src/bridge.ts`

Add `compression` to `NodeOptions`:

```ts
compression?: boolean | {
    /** zstd compression level 1–22. Default: 3. */
    level?: number;
    /**
     * Skip compression for bodies smaller than this many bytes.
     * Default: 512.
     */
    minBodyBytes?: number;
};
```

`true` is shorthand for default options. `false` or omitted disables
compression.

No other JS changes. The `fetch` and `serve` signatures are untouched.

---

## Behaviour Matrix

| Client | Server | Request body | Response body |
|---|---|---|---|
| compression on | compression on | client compresses if above threshold; server decompresses | server compresses if above threshold; client decompresses |
| compression on | no compression | client compresses; server **cannot decompress** → wire error (handled; connection reset gracefully) | server sends uncompressed; client receives plain body |
| no compression | compression on | client sends plain body; server receives plain body | server checks `Accept-Encoding` (absent) → sends plain body |
| neither | neither | plain | plain |

The "client on, server off" case is the only sharp edge. When the client sends
`Content-Encoding: zstd` to a server that does not understand it, the server
will treat the body as opaque bytes. For iroh-http nodes, where both ends run
the same library version, the feature flag state will match in practice. For
interop with non-iroh-http nodes, the client must not set `Content-Encoding`
on the request unless the server is known to support it — this is standard
HTTP behaviour. The `compression` option controls only the client's willingness
to decompress responses and compress responses server-side; it does not force
`Content-Encoding` on outbound requests to unknown peers.

---

## Out of Scope

- Brotli (`br`) — the pure-Rust brotli crate is large; add in a follow-up
  if demand exists.
- Compression for bidirectional (`createBidirectionalStream`) streams — body
  framing is different there; add once the basic request/response path is solid.
- Dictionary compression (`zstd` trained dictionaries for repeated JSON keys).
- Compression for request bodies sent by the client. The patch covers:
  transparently decompressing inbound `Content-Encoding` request bodies;
  advertising `Accept-Encoding`; compressing outbound response bodies.
  Compressing outbound request bodies is possible but requires the caller to
  know the server supports the encoding — omit for now.
