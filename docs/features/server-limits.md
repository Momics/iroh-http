# Server Limits

All resource limits are configured at **`createNode(options)`** and enforced
in Rust before any JavaScript handler runs. They protect the serve loop
against misbehaving or hostile peers at the transport level.

## Options

```ts
const node = await createNode({
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
});
```

All limits are optional. Omitting a limit uses the default shown above.

## Why at the Rust layer

These limits intercept bytes or connections before they reach the FFI
boundary. A JS handler never runs for a rejected request, so no user code
needs to handle the overflow cases.

For example, without `maxRequestBodyBytes` a peer could stream an unbounded
body, accumulating data in the channel until memory is exhausted. The limit
is checked as bytes arrive in the Rust body reader — no full-body buffering
occurs.

## What each limit protects against

| Option | Attack vector | Behavior |
|---|---|---|
| `maxConcurrency` | Request flood from many peers | Excess requests queue until a slot is free; if none frees before `requestTimeout`, they receive a `408 Request Timeout` |
| `maxConnectionsPerPeer` | Connection flood from one peer | Excess connections are closed at the QUIC level (transport close, not an HTTP response) |
| `requestTimeout` | Slow request / stalled handler | 408 Request Timeout |
| `maxRequestBodyBytes` | Oversized body exhausting memory | 413 Content Too Large |
| `maxHeaderBytes` | Header flood exhausting memory | 431 Request Header Fields Too Large |

