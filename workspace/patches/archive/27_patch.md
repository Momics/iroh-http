---
status: implemented
refs: features/webtransport.md
---

# Patch 27 — WebTransport Compatibility + Datagrams

Introduce `IrohSession` implementing the WebTransport API, add
`node.connect(peer): Promise<IrohSession>`, and expose datagrams, as described
in [webtransport.md](../features/webtransport.md).

This is the largest single patch in the series. It establishes sessions as
first-class objects and makes iroh-http structurally identical to the
WebTransport API in the browser.

## Problem

iroh-http exposes HTTP conveniences (`fetch`, `serve`) but has no way to
access the QUIC session that underlies them. Datagrams, incoming streams, and
per-session lifecycle control are all inaccessible. The `closed` promise has
the wrong shape (`Promise<void>` instead of `Promise<{closeCode, reason}>`).

## Changes

### 1. Rust — session handle

Introduce a `sessionHandle` concept parallel to the existing `nodeHandle`:

```rust
// bridge.rs

/// Open a new QUIC connection to `node_id` and return a session handle.
pub async fn connect(node_handle: u32, node_id: &str) -> Result<u32, String>

/// Returns a Promise that resolves when the QUIC handshake completes.
pub async fn session_ready(session_handle: u32) -> Result<(), String>

/// Returns a Promise that resolves when the session closes.
/// Resolves to { closeCode: number, reason: string }.
pub async fn session_closed(session_handle: u32) -> CloseInfo

/// Initiate a graceful close.
pub fn session_close(session_handle: u32, close_code: u32, reason: &str)

/// Open a new bidirectional QUIC stream within the session.
pub async fn session_create_bidi_stream(session_handle: u32) -> u32  // stream handle

/// Open a new unidirectional (send-only) QUIC stream.
pub async fn session_create_uni_stream(session_handle: u32) -> u32   // stream handle

/// Long-poll: wait for an incoming bidi stream. Returns a stream handle.
pub async fn session_next_bidi_stream(session_handle: u32) -> Option<u32>

/// Long-poll: wait for an incoming uni stream. Returns a stream handle.
pub async fn session_next_uni_stream(session_handle: u32) -> Option<u32>

/// Send a datagram. Fails if len > maxDatagramSize.
pub async fn session_send_datagram(session_handle: u32, data: &[u8]) -> Result<(), String>

/// Long-poll: receive the next datagram.
pub async fn session_recv_datagram(session_handle: u32) -> Vec<u8>

/// Current maximum datagram payload size for this session.
pub fn session_max_datagram_size(session_handle: u32) -> Option<u32>
```

```rust
#[derive(Serialize)]
pub struct CloseInfo {
    pub close_code: u32,
    pub reason: String,
}
```

### 2. `IrohSession` — TypeScript interface

Add to `packages/iroh-http-shared/src/index.ts`:

```ts
/**
 * A WebTransport-compatible session to a single remote peer.
 * Returned by node.connect(peer).
 */
interface IrohSession extends WebTransport {
  /** The peer's verified public key. Not on standard WebTransport. */
  readonly remoteId: PublicKey;
}
```

`IrohSession` must satisfy the full `WebTransport` interface:

```ts
// Standard WebTransport properties (all must be implemented):
readonly ready: Promise<undefined>;
readonly closed: Promise<WebTransportCloseInfo>;
readonly datagrams: WebTransportDatagramDuplexStream;
createBidirectionalStream(options?: WebTransportSendStreamOptions): Promise<WebTransportBidirectionalStream>;
createUnidirectionalStream(options?: WebTransportSendStreamOptions): Promise<WritableStream<Uint8Array>>;
readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;
readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array>>;
close(closeInfo?: WebTransportCloseInfo): void;
```

### 3. `node.connect` — TypeScript

Add to `IrohNode`:

```ts
/** Open a WebTransport-compatible session to a peer. */
connect(peer: string | NodeAddr): Promise<IrohSession>;
```

`node.fetch` and `node.serve` stay unchanged externally. Internally, `fetch`
calls `connect`, opens a bidi stream on the session, and sends the HTTP
request. The connection pool maps `peer → session` so repeated `fetch` calls
to the same peer reuse the session.

### 4. `node.closed` shape change

```ts
// Before:
readonly closed: Promise<void>;

// After:
readonly closed: Promise<{ closeCode: number; reason: string }>;
```

Update all four adapters and any code that awaits `node.closed`.

### 5. `IrohDatagramDuplexStream`

Implement `WebTransportDatagramDuplexStream`:

```ts
class IrohDatagramDuplexStream implements WebTransportDatagramDuplexStream {
  readonly readable: ReadableStream<Uint8Array>;  // backed by session_recv_datagram loop
  readonly writable: WritableStream<Uint8Array>;  // backed by session_send_datagram
  readonly maxDatagramSize: number | null;        // from session_max_datagram_size
  incomingHighWaterMark: number;
  outgoingHighWaterMark: number;
}
```

`readable` uses the same pull-based `makeReadable` pattern as body streams:
calls `session_recv_datagram` only when the consumer pulls.

### 6. Incoming stream iterables

```ts
// incomingBidirectionalStreams:
// ReadableStream backed by a loop calling session_next_bidi_stream

// incomingUnidirectionalStreams:
// ReadableStream backed by a loop calling session_next_uni_stream
```

Both are lazy: the loop only starts when the stream is consumed.

### 7. Platform adapters

All four adapters must implement `connect` and wire the session handle through
the new FFI functions. The session object is created in the shared adapter
layer where possible, calling the native functions via the same bridge
abstraction used for node operations.

### 8. Tests

```rust
#[tokio::test]
async fn session_connect_sends_datagram() {
    let (a, b) = two_test_nodes().await;
    let session = bridge::connect(a.handle, &b.public_key()).await.unwrap();
    bridge::session_send_datagram(session, b"ping").await.unwrap();
    let received = bridge::session_recv_datagram(b_session).await;
    assert_eq!(received, b"ping");
}

#[tokio::test]
async fn session_closed_resolves_with_close_code() {
    let (a, b) = two_test_nodes().await;
    let session = bridge::connect(a.handle, &b.public_key()).await.unwrap();
    bridge::session_close(session, 42, "done");
    let info = bridge::session_closed(session).await;
    assert_eq!(info.close_code, 42);
    assert_eq!(info.reason, "done");
}
```

## Files

- `crates/iroh-http-core/src/bridge.rs` — session handle + all session FFI functions
- `crates/iroh-http-core/src/session.rs` — new file: session state management
- `packages/iroh-http-shared/src/index.ts` — `IrohSession`, `node.connect`, `node.closed` shape
- `packages/iroh-http-shared/src/session.ts` — new: `IrohSession` class implementation
- `packages/iroh-http-shared/src/datagrams.ts` — new: `IrohDatagramDuplexStream`
- All four adapter packages — `connect` method, updated `closed` shape
- `crates/iroh-http-core/tests/session.rs` — new integration tests

## Notes

- Sessions are reference-counted. `node.connect(peer)` returns the existing
  session if one is open to that peer (connection pool); it does not open a
  new QUIC connection on every call.
- The `connect` function used internally by `fetch` must not expose the session
  to the caller — `fetch` continues to return `Promise<Response>`.
- This patch does not add browser WebTransport wire compatibility. The
  `IrohSession` API is source-compatible with `WebTransport` but operates over
  Iroh's QUIC identity layer, not HTTP/3.
