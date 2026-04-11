# Server Limits

`node.serve` accepts resource limit options that are enforced in Rust before
any JavaScript runs. These protect the serve loop against misbehaving or
hostile peers at the transport level.

## Options

```ts
node.serve({
  /** Maximum simultaneous in-flight requests across all peers. Default: 64. */
  maxConcurrency: 64,

  /** Maximum simultaneous connections from a single peer. Default: 8. */
  maxConnectionsPerPeer: 8,

  /** Per-request timeout in milliseconds. Default: 60 000. */
  requestTimeout: 60_000,

  /** Maximum request body size in bytes. Requests with larger bodies are
   *  rejected with 413 before the body is read. Default: none. */
  maxRequestBodyBytes: 10 * 1024 * 1024,  // 10 MB example

  /** Maximum request header block size in bytes. Requests with larger headers
   *  are rejected with 431. Default: 64 KB. */
  maxHeaderBytes: 64 * 1024,
}, handler);
```

All limits are optional. Omitting a limit uses the default shown above.
Set to `0` or `null` to disable a limit entirely.

## Why at the Rust layer

These limits intercept bytes or connections before they reach the FFI
boundary. A JS handler never runs for a rejected request, so no user code
needs to handle the overflow cases.

For example, without `maxRequestBodyBytes` a peer could stream an unbounded
body, accumulating data in the channel until memory is exhausted. The limit
is checked as bytes arrive in the Rust body reader — no full-body buffering
occurs.

## What each limit protects against

| Option | Attack vector | Response |
|---|---|---|
| `maxConcurrency` | Request flood from many peers | 503 Service Unavailable |
| `maxConnectionsPerPeer` | Connection flood from one peer | 429 Too Many Requests |
| `requestTimeout` | Slow request / stalled handler | 408 Request Timeout |
| `maxRequestBodyBytes` | Oversized body exhausting memory | 413 Content Too Large |
| `maxHeaderBytes` | Header flood exhausting memory | 431 Request Header Fields Too Large |

## Status

The Rust `ServeOptions` struct implements all five limits. The TypeScript
`serve()` call does not yet pass these options through to Rust — they all
fall through to their defaults.

→ [Patch 28](../patches/28_patch.md)
