# Body Compression

iroh-http compresses request and response bodies using zstd. Compression is
negotiated with standard HTTP headers (`Accept-Encoding` / `Content-Encoding`)
and handled entirely at the Rust layer — the JS handler always sees plain bytes.

Enabled by default. To disable (e.g. for minimal binary size or environments
without a C toolchain), compile with `default-features = false`.

## Supported algorithms

**zstd only.** No `gzip`, no `br`, no `deflate`.

This is intentional. iroh-http is a new P2P protocol with no legacy-client
compatibility surface — every client is an iroh-http peer linked against the
same Rust core. We therefore standardise on a single modern algorithm rather
than negotiating across a menagerie. zstd was chosen because:

- Better compression ratio than gzip at equivalent CPU.
- Faster decoding than brotli at equivalent ratio.
- Pure-Rust implementation already in the dependency tree (`zstd` crate).
- The `tower-http` `compression-zstd` and `decompression-zstd` features
  give us streaming compression with no custom code (see
  [ADR-013 — Lean on the ecosystem](../adr/013-lean-on-the-ecosystem.md)).

### Behaviour for unsupported `Accept-Encoding`

A peer advertising **only** `gzip` or `br` (e.g. a non-iroh-http client
bridged in, or a hand-crafted request) receives the response **uncompressed**.
There is no negotiation failure — `tower-http`'s compression layer treats an
unsupported `Accept-Encoding` as "send identity" rather than `406 Not Acceptable`.

If you need to verify the wire encoding, inspect `Content-Encoding` on the
response: it is present iff the body was compressed.

### Runtime notes

| Runtime | Compression behaviour |
|---|---|
| Node.js (`@momics/iroh-http-node`) | Full support. Zstd built into the NAPI binary. |
| Deno (`@momics/iroh-http-deno`) | Full support. Zstd built into the dynamic library. |
| Tauri (`@momics/iroh-http-tauri`) | Full support. Zstd built into the plugin. |
| Browser fetch over `httpi://` | N/A. iroh-http does not run in browsers — the WebTransport prototype in `nebulous` is a separate code path. |

## Configuration

```ts
await createNode({
  compression: true,            // zstd at default level
  // or:
  compression: {
    level: 3,                   // zstd compression level (1–22, default 3)
    minBodyBytes: 1024,         // skip compression below this size (default 1 KB)
  },
});
```

When `compression` is enabled:

- **Outbound requests** (`node.fetch`): `Accept-Encoding: zstd` is injected
  automatically. If the server responds with `Content-Encoding: zstd`, the
  body is decompressed before being delivered to JS.
- **Inbound requests** (`node.serve`): if the request advertises
  `Accept-Encoding: zstd`, the response body is compressed before sending.
  If the request itself carries `Content-Encoding: zstd`, the request body
  is decompressed before being delivered to the handler — the
  `Content-Encoding` header is also stripped so the handler sees the request
  as if it had arrived uncompressed.

The JS handler never sees compressed bytes in either direction:
**fetch** decompresses inbound responses, and **serve** decompresses inbound
request bodies and compresses outbound response bodies.

## Transferring pre-compressed content

If you are serving a file that is already compressed (e.g. a `.zst` archive,
an image, an encrypted blob), the Rust layer will not re-compress it.
Compression is skipped automatically when any of the following are true:

| Condition | Mechanism |
|---|---|
| Response already has `Content-Encoding` set | Body is pre-encoded; re-compressing would corrupt it |
| `Content-Type` is `image/*`, `audio/*`, `video/*`, `application/zstd`, or `application/octet-stream` | Content is already opaque or compressed |
| Response carries `Cache-Control: no-transform` | Proxy / transform opt-out per RFC 9111 §5.2.2.7 |
| Body is smaller than `minBodyBytes` | Not worth the CPU cost |

To serve a `.zst` file and have the peer receive the raw compressed bytes:

```ts
// Server side — return the file with the correct content type.
// The Rust layer sees Content-Type: application/zstd and skips compression.
node.serve((_req) =>
  new Response(await Deno.readFile("archive.tar.zst"), {
    headers: { "content-type": "application/zstd" },
  })
);
```

To opt out of compression on a per-request basis from the fetch side:

```ts
// Pass Accept-Encoding: identity — the Rust layer will not inject its own
// Accept-Encoding header when the caller has already provided one.
const res = await node.fetch(peerId.toURL("/file"), {
  headers: { "accept-encoding": "identity" },
});
```

## Why at the Rust layer

Compression must intercept the byte stream before it crosses the FFI boundary.
Doing it in JS would require buffering the full body on both sides of the
boundary to compress/decompress, which defeats streaming. At the Rust level,
compression happens inline in the body channel with a fixed-size ring buffer —
no full-body buffering required.

## Wire protocol

Uses standard RFC 9110 content negotiation:

```
→ Accept-Encoding: zstd
← Content-Encoding: zstd
```

Only `zstd` is supported. `gzip` and `br` are not — iroh-http is a new
protocol with no legacy client compatibility requirement.

## Notes

- Bodies smaller than `minBodyBytes` are sent uncompressed even when
  compression is enabled. Compressing small bodies adds CPU with no net gain.
- The `compression` feature flag adds the `zstd` crate to the dependency tree.
  When the flag is off, the binary has zero compression overhead.
- Streaming bodies are compressed incrementally — the QUIC send buffer is not
  stalled waiting for the full body.
