---
status: done
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

### Implementation: the `qpack` crate

The [`qpack`](https://crates.io/crates/qpack) crate (v0.1.0, MIT) is an
extraction of the QPACK module from the hyper community's
[`h3`](https://github.com/hyperium/h3) crate. The code is identical
(confirmed by diff) — only import ordering and a `Clone` derive differ.
This means we get the hyper community's battle-tested, RFC 9204-compliant
implementation without pulling in the rest of h3.

**Dependencies:** `bytes` + `http` — both already in our tree.

**Key API:**

```rust
// Stateless mode (Phase 1): static table + Huffman, no per-connection state
qpack::encode_stateless(&mut buf, headers)?;   // -> Result<u64, EncoderError>
qpack::decode_stateless(&mut buf, max_size)?;  // -> Result<Decoded, DecoderError>

// Stateful mode (Phase 2): adds dynamic table for repeated-header compression
let mut encoder = qpack::Encoder::new();
encoder.encode(&mut block, &mut encoder_stream, stream_id, headers)?;

let mut decoder = qpack::Decoder::new();
let decoded = decoder.decode_header(&mut block)?;
```

### Phased rollout

**Phase 1 — stateless (ship with this patch):**

Use `encode_stateless()` / `decode_stateless()` only. This gives:
- Huffman coding of header values (~30% size reduction on ASCII)
- Static table index references for common headers (`:method: GET` = 1 byte)
- Zero per-connection state — works even without the connection pool

This is the right starting point. Two function calls, no state management.

**Phase 2 — stateful (future, after Patch 12 lands and we have usage data):**

Switch to `Encoder` / `Decoder` with dynamic tables. Requires:
- Per-connection encoder/decoder state stored in the connection pool
- Dynamic table capacity negotiation
- Encoder/decoder stream wiring (the `qpack` crate supports this)

Only pursue Phase 2 if profiling shows repeated custom headers dominate
traffic (e.g. large auth tokens sent on every request).

### Where the code lives

| Layer | Role |
|---|---|
| `iroh-http-framing` | **Unchanged.** Stays `no_std`, plaintext HTTP/1.1 only. No QPACK dependency. |
| `iroh-http-core` | Adds `qpack` as an optional dependency behind a `qpack` Cargo feature (default: on). Thin wrapper functions convert between `iroh-http-core`'s `(method, path, headers)` tuples and `qpack::HeaderField` slices. Selects encode/decode path based on negotiated ALPN. |
| Bridge / JS layers | **No changes.** Headers arrive as `Vec<(String, String)>` regardless of wire encoding. Compression is invisible above the FFI boundary. |

### Integration in `iroh-http-core`

New internal module `crates/iroh-http-core/src/qpack_bridge.rs`:

```rust
use qpack::{HeaderField, encode_stateless, decode_stateless};

/// Encode a request head as a QPACK header block with a 2-byte length prefix.
pub fn encode_request(method: &str, path: &str, headers: &[(&str, &str)], chunked: bool) -> Vec<u8> {
    let mut fields: Vec<HeaderField> = vec![
        HeaderField::new(":method", method),
        HeaderField::new(":path", path),
    ];
    for (name, value) in headers {
        fields.push(HeaderField::new(*name, *value));
    }
    if chunked {
        fields.push(HeaderField::new("transfer-encoding", "chunked"));
    }
    let mut block = bytes::BytesMut::new();
    let _ = encode_stateless(&mut block, fields.iter().map(|f| f.clone()));
    // Prepend 2-byte big-endian length
    let len = (block.len() as u16).to_be_bytes();
    let mut out = Vec::with_capacity(2 + block.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(&block);
    out
}

/// Decode a QPACK header block (after reading the 2-byte length prefix).
/// Returns (method, path, headers, total_bytes_consumed).
pub fn decode_request(bytes: &[u8]) -> Result<(String, String, Vec<(String, String)>, usize), String> {
    if bytes.len() < 2 { return Err("incomplete qpack header".into()); }
    let block_len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    if bytes.len() < 2 + block_len { return Err("incomplete qpack block".into()); }
    let mut buf = &bytes[2..2 + block_len];
    let decoded = decode_stateless(&mut buf, 65536)
        .map_err(|e| format!("qpack decode: {e:?}"))?;
    let mut method = String::from("GET");
    let mut path = String::from("/");
    let mut headers = Vec::new();
    for field in decoded.fields() {
        match field.name.as_ref() {
            b":method" => method = String::from_utf8_lossy(field.value.as_ref()).into(),
            b":path" => path = String::from_utf8_lossy(field.value.as_ref()).into(),
            name => {
                let n = String::from_utf8_lossy(name).into_owned();
                let v = String::from_utf8_lossy(field.value.as_ref()).into_owned();
                headers.push((n, v));
            }
        }
    }
    Ok((method, path, headers, 2 + block_len))
}
```

Response encode/decode follows the same pattern with `:status` instead of
`:method`/`:path`.

Body encoding (chunked framing, trailers) is **unchanged** — the existing
`encode_chunk`, `terminal_chunk`, `serialize_trailers`, and `parse_trailers`
functions continue to work identically for both `iroh-http/1` and
`iroh-http/2`.

### Fallback if the crate goes unmaintained

The `qpack` crate is ~4.5K lines (excluding tests). If it ever becomes
abandoned, the code can be vendored into `iroh-http-core/src/qpack/` under
MIT attribution in under 5 minutes. The API surface we use (`encode_stateless`,
`decode_stateless`, `HeaderField`) is small and stable.

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

`iroh-http-framing` is completely unaffected — it stays `no_std` and plaintext.
The `qpack` dependency only exists in `iroh-http-core` (which already requires
`std` + `tokio`). Disabling the `qpack` feature on `iroh-http-core` removes
the dependency entirely and the node advertises only `iroh-http/1*` ALPNs.

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
| `iroh-http-framing` | **No changes.** Stays `no_std`, plaintext HTTP/1.1 only. |
| `iroh-http-core/Cargo.toml` | Add `qpack = { version = "0.1", optional = true }`. Add `qpack` feature (default: on). |
| `iroh-http-core/src/qpack_bridge.rs` (new) | Thin wrapper: converts `(method, path, headers)` tuples to/from `qpack::HeaderField` slices. Handles length-prefix framing and pseudo-header mapping. ~100 lines. |
| `iroh-http-core/src/lib.rs` | Add `iroh-http/2*` ALPN constants. Conditionally export `qpack_bridge` behind feature gate. |
| `iroh-http-core/src/client.rs` | After ALPN negotiation, check negotiated proto. Use `qpack_bridge::encode_request` for `iroh-http/2*`, `serialize_request_head` for `iroh-http/1*`. |
| `iroh-http-core/src/server.rs` | Same: detect negotiated ALPN on accepted connection, select decode path. |
| `iroh-http-core/src/endpoint.rs` | Add `iroh-http/2*` ALPNs to the advertised list (only when `qpack` feature is enabled). Add `qpack_max_table_capacity` to `NodeOptions` (reserved for Phase 2). |
| Bridge / JS layers | **No changes.** |

---

## Verification

1. **ALPN fallback test**: Connect a QPACK-capable node to a base-only node.
   Verify they negotiate `iroh-http/1` and communicate correctly.
2. **Compression test**: Send 100 requests with identical headers. Measure
   total header bytes on the wire. Expect meaningful reduction from
   static table hits + Huffman coding.
3. **Round-trip test**: Encode headers with `qpack::encode_stateless`, decode
   with `qpack::decode_stateless`, assert equality for various header
   combinations.
4. **ESP interop test**: Run an `iroh-http/1`-only node alongside an
   `iroh-http/2` node. Verify bidirectional communication works.
5. **Feature gate test**: Build `iroh-http-core` with
   `default-features = false` and confirm it compiles without the `qpack`
   dependency and only advertises `iroh-http/1*` ALPNs.
6. **no_std unaffected test**: Confirm `iroh-http-framing` continues to build
   with `no_std` — it should have no awareness of QPACK at all.
