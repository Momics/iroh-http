# Body Compression

iroh-http compresses request and response bodies using zstd. Compression is
negotiated with standard HTTP headers (`Accept-Encoding` / `Content-Encoding`)
and handled entirely at the Rust layer — the JS handler always sees plain bytes.

Enabled via the `compression` Cargo feature flag (off by default).

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
- **Inbound requests** (`node.serve`): if the request carries
  `Content-Encoding: zstd`, the body is decompressed before the handler runs.
  If the request advertises `Accept-Encoding: zstd`, the response body is
  compressed before sending.

The JS handler never sees compressed bytes in either direction.

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
