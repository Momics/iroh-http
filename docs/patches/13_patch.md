---
status: pending
depends: [12]
---

# iroh-http — Patch 13: QPACK Header Compression

## Problem

Every request and response sends headers as raw ASCII text — no compression,
no indexing, no deduplication. For API-style traffic with repetitive headers
(`Content-Type: application/json`, `Authorization: Bearer ...`,
`iroh-node-id: ...`), this is wasteful:

- A typical JSON API request carries ~200-400 bytes of headers per round trip.
- After QPACK's dynamic table warms up, repeated headers compress to 1-2 byte
  index references.
- For a chat app making hundreds of small requests, header overhead can exceed
  payload size.

The brief already identifies this in **Future (v2+)**:

> QPACK (the HTTP/3 header compression scheme) is a separate spec from HTTP/3
> itself and can be implemented as a layer between `iroh-http-framing` and the
> QUIC stream. [...] Both peers negotiate support via an ALPN identifier during
> the Iroh handshake. Nodes that do not support compression fall back to
> uncompressed headers.

This patch specifies that design.

---

## Dependency: Patch 12 (Connection Pool)

QPACK maintains a **per-connection dynamic table** — header entries are indexed
and referenced across multiple requests on the same connection. This only works
when connections are reused across requests.

Without the connection pool from Patch 12, every `fetch()` opens a new QUIC
connection, the dynamic table starts empty, and QPACK provides almost no
benefit (only static table hits and Huffman coding). The pool must land first.

---

## Design

### ALPN negotiation

Introduce a new ALPN protocol identifier for QPACK-capable nodes:

| ALPN | Meaning |
|------|---------|
| `iroh-http/1` | Base protocol — plaintext HTTP/1.1 headers (current) |
| `iroh-http/1-duplex` | Base + bidirectional streaming |
| `iroh-http/1-trailers` | Base + trailers |
| `iroh-http/1-full` | Base + duplex + trailers |
| **`iroh-http/2`** | QPACK-compressed headers, same body framing |
| **`iroh-http/2-duplex`** | QPACK + duplex |
| **`iroh-http/2-trailers`** | QPACK + trailers |
| **`iroh-http/2-full`** | QPACK + duplex + trailers |

Nodes advertise their supported protocols in preference order. A desktop node
might advertise `[iroh-http/2-full, iroh-http/1-full]`. An ESP node advertises
only `[iroh-http/1]`. QUIC's ALPN negotiation picks the best match
automatically — no application-level negotiation logic needed.

**Key rule**: `iroh-http/2` and `iroh-http/1` are **wire-compatible on the
body level**. The only difference is how request/response headers are encoded.
An `iroh-http/2` stream uses QPACK-compressed header blocks; an `iroh-http/1`
stream uses plaintext ASCII. Body framing (chunked encoding, trailers) is
identical.

### Wire format change

For `iroh-http/2` streams, the request/response head is replaced:

**Before (iroh-http/1):**
```
POST /api/data HTTP/1.1\r\n
Content-Type: application/json\r\n
Authorization: Bearer tok_abc\r\n
Transfer-Encoding: chunked\r\n
\r\n
<chunked body>
```

**After (iroh-http/2):**
```
[2-byte encoded header block length]
[QPACK-encoded header block]
<chunked body — unchanged>
```

The header block is a standard QPACK-encoded field section (RFC 9204 §4.5).
A 2-byte big-endian length prefix allows the parser to know exactly how many
bytes to read before decoding.

Pseudo-headers encode the method, path, and status:
- `:method` → `POST`
- `:path` → `/api/data`
- `:status` → `200`

This follows HTTP/3 conventions (RFC 9114 §4.3) even though the transport
is not HTTP/3.

### QPACK tables

**Static table**: The 99 pre-defined entries from RFC 9204 Appendix A. These
cover common headers like `:method: GET`, `content-type: application/json`,
`:status: 200`, etc. Available immediately on every connection with no setup.

**Dynamic table**: Per-connection, built up as headers are sent/received.
Repeated headers (like `authorization`, custom app headers) are added to the
dynamic table and referenced by index on subsequent requests.

**Table size**: Default dynamic table capacity of 4096 bytes (same as HTTP/3
default). Configurable via `NodeOptions.qpack_max_table_capacity`.

### Simplified QPACK (no decoder stream)

Full QPACK uses two unidirectional QUIC streams (encoder stream and decoder
stream) for dynamic table synchronisation between concurrent requests. This
is complex and primarily needed for HTTP/3's highly concurrent request model.

For iroh-http, a **simplified approach** is sufficient:

1. Use only the **static table** and **literal representations** initially.
   This alone provides Huffman coding and static index references — a
   meaningful compression win with zero connection-level state.
2. Phase in the dynamic table with a **blocking model**: each stream's header
   block is self-contained (uses only static refs and literals, or references
   dynamic entries that are guaranteed to exist). No decoder stream required.
   This is QPACK's "zero required insert count" mode.
3. The full encoder/decoder stream model can be added later if profiling shows
   the dynamic table miss rate justifies it.

This phased approach keeps the implementation small while still delivering the
primary compression wins.

### Where the code lives

| Layer | Role |
|---|---|
| `iroh-http-framing` | Add QPACK encode/decode functions alongside existing HTTP/1.1 functions. Behind a `qpack` Cargo feature (default: on for `std`, off for minimal `no_std` builds). The static table and Huffman table are `const` data — no runtime allocation needed. |
| `iroh-http-core` | Select the encode/decode path based on the negotiated ALPN. If `iroh-http/2*` was negotiated, use QPACK functions; otherwise fall back to plaintext. The connection pool (Patch 12) holds the per-connection QPACK encoder/decoder state. |
| Bridge / JS layers | **No changes.** Headers arrive as `Vec<(String, String)>` regardless of wire encoding. Compression is invisible above the FFI boundary. |

### `iroh-http-framing` API additions

```rust
// New functions in iroh-http-framing (behind `qpack` feature):

/// Encode a request head using QPACK.
/// Returns the length-prefixed encoded header block.
pub fn qpack_encode_request_head(
    encoder: &mut QpackEncoder,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    chunked: bool,
) -> Vec<u8>;

/// Decode a QPACK-encoded request head.
/// Returns (method, path, headers, bytes_consumed).
pub fn qpack_decode_request_head(
    decoder: &mut QpackDecoder,
    bytes: &[u8],
) -> Result<(String, String, Vec<(String, String)>, usize), FramingError>;

/// Encode a response head using QPACK.
pub fn qpack_encode_response_head(
    encoder: &mut QpackEncoder,
    status: u16,
    headers: &[(&str, &str)],
    chunked: bool,
) -> Vec<u8>;

/// Decode a QPACK-encoded response head.
pub fn qpack_decode_response_head(
    decoder: &mut QpackDecoder,
    bytes: &[u8],
) -> Result<(u16, String, Vec<(String, String)>, usize), FramingError>;

/// QPACK encoder state (per-connection on the sending side).
pub struct QpackEncoder { /* static table ref, optional dynamic table */ }

/// QPACK decoder state (per-connection on the receiving side).
pub struct QpackDecoder { /* static table ref, optional dynamic table */ }
```

Body encoding (chunked framing, trailers) is **unchanged** — the existing
`encode_chunk`, `terminal_chunk`, `serialize_trailers`, and `parse_trailers`
functions continue to work identically for both `iroh-http/1` and
`iroh-http/2`.

---

## ESP / no_std interoperability

ESP and other constrained devices continue to use `iroh-http/1` (plaintext
headers). They do not need to implement QPACK. Interoperability is maintained
through ALPN negotiation:

- ESP advertises `[iroh-http/1]`
- Desktop advertises `[iroh-http/2-full, iroh-http/1-full, iroh-http/1]`
- QUIC ALPN negotiation picks `iroh-http/1` as the common protocol
- Both sides use plaintext headers — no degradation, no error

When an ESP connects to another ESP, they use `iroh-http/1`. When two desktop
nodes connect, they use `iroh-http/2-full`. The wire format difference is
confined to header encoding — bodies, trailers, and duplex streams work
identically regardless.

The `qpack` feature in `iroh-http-framing` can be disabled for no_std builds
to eliminate the static table and Huffman data from the binary (~4 KB).

---

## Configuration

Add to `NodeOptions`:

```rust
pub struct NodeOptions {
    // ... existing fields ...

    /// Maximum QPACK dynamic table capacity in bytes.
    /// Set to 0 to disable the dynamic table (static + Huffman only).
    /// Default: 4096.
    pub qpack_max_table_capacity: Option<usize>,
}
```

---

## Scope of changes

| Layer | Change |
|---|---|
| `iroh-http-framing/Cargo.toml` | Add `qpack` feature (default: on). Add `huffman` static data. |
| `iroh-http-framing/src/qpack.rs` (new) | Static table, Huffman encode/decode, `QpackEncoder`, `QpackDecoder`, header block encode/decode. |
| `iroh-http-framing/src/lib.rs` | Re-export QPACK types behind feature gate. Add `iroh-http/2*` ALPN constants. |
| `iroh-http-core/src/client.rs` | After ALPN negotiation, check negotiated proto. Use QPACK encode for `iroh-http/2*`, plaintext for `iroh-http/1*`. |
| `iroh-http-core/src/server.rs` | Same: detect negotiated ALPN on accepted connection, select decode path. |
| `iroh-http-core/src/pool.rs` (from Patch 12) | Store per-connection `QpackEncoder`/`QpackDecoder` alongside the cached `Connection`. |
| `iroh-http-core/src/endpoint.rs` | Add `iroh-http/2*` ALPNs to the advertised list. Add `qpack_max_table_capacity` to `NodeOptions`. |
| Bridge / JS layers | **No changes.** |

---

## Verification

1. **ALPN fallback test**: Connect a QPACK-capable node to a base-only node.
   Verify they negotiate `iroh-http/1` and communicate correctly.
2. **Compression test**: Send 100 requests with identical headers. Measure
   total header bytes on the wire. Expect >80% reduction after warmup
   (static table hits + Huffman).
3. **Round-trip test**: Encode headers with `QpackEncoder`, decode with
   `QpackDecoder`, assert equality.
4. **ESP interop test**: Run an `iroh-http/1`-only node alongside an
   `iroh-http/2` node. Verify bidirectional communication works.
5. **no_std build test**: Build `iroh-http-framing` with `default-features =
   false` and confirm it compiles without `qpack`.
