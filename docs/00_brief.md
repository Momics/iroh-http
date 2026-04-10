---
status: integrated
---

# iroh-http — Architecture Overview

## What This Is

A transport layer that replaces TCP/DNS with Iroh QUIC connections. Instead of a domain name, every node is addressed by its **public key**. Two nodes can establish a direct, encrypted, NAT-traversing connection using that key alone — no server, no DNS, no IP address required.

The library exposes three things to JavaScript:

- `createNode(options?)` — creates a node, returns an object with `fetch`, `serve`, `createBidirectionalStream`, `closed`, `nodeId`, `keypair`, and `close`
- `node.fetch(nodeId, url, init?)` — send an HTTP request to a remote node
- `node.serve(options, handler)` — receive HTTP requests, Deno-style
- `node.createBidirectionalStream(nodeId, path, init?)` — open a bidirectional streaming connection
- `node.closed` — `Promise<void>` that resolves when the node closes (mirrors WebTransport)

Both `fetch` and `serve` are fully web-standard (`Request`, `Response`, streaming bodies). The JS API is identical regardless of whether you are in Node.js or Tauri. Only the internal bridge changes.

-----

## Repository Layout

```
iroh-http/
├── crates/
│   ├── iroh-http-framing/        # no_std — pure HTTP/1.1 parse + serialize (httparse)
│   │   └── src/
│   │       └── lib.rs             # Request/response wire format, no async, no I/O
│   │
│   ├── iroh-http-core/           # Rust — Iroh endpoint, streaming, fetch/serve
│   │   └── src/
│   │       ├── lib.rs             # Public API surface, FFI-friendly types
│   │       ├── endpoint.rs        # Iroh endpoint lifecycle
│   │       ├── transport.rs       # Iroh bidi stream <-> async I/O
│   │       ├── client.rs          # fetch() implementation
│   │       ├── server.rs          # serve() implementation
│   │       └── stream.rs          # BodyReader / BodyWriter channel types
│   │
│   └── iroh-http-discovery/      # Optional — local discovery (mDNS on desktop)
│       └── src/
│           └── lib.rs             # Implements Iroh's Discovery trait via mDNS
│
├── packages/
│   ├── iroh-http-shared/         # TypeScript — shared JS layer, no native deps
│   │   └── src/
│   │       ├── bridge.ts          # Bridge interface definition
│   │       ├── streams.ts         # ReadableStream construction from handles
│   │       ├── serve.ts           # makeServe() — wraps raw callback in web Request/Response
│   │       └── fetch.ts           # makeFetch() — wraps raw fetch in web-standard signature
│   │
│   ├── iroh-http-node/           # Node.js package (napi-rs native addon)
│   │   ├── src/lib.rs             # napi-rs bindings
│   │   └── index.ts               # Wires bridge -> iroh-http-shared, exports createNode
│   │
│   └── iroh-http-tauri/          # Tauri plugin package
│       ├── src/lib.rs             # Tauri commands
│       └── guest-js/
│           └── index.ts           # Wires invoke bridge -> iroh-http-shared, exports createNode
```

-----

## Layer Diagram

```
┌─────────────────────────────────────────────────────┐
│                   User Code (JS)                    │
│  const node = await createNode({ key: savedKey })   │
│  node.serve({}, req => res)                         │
│  node.fetch(peerId, '/api')                         │
│  node.createBidirectionalStream(peerId, '/ws')      │
│  node.close()  /  await node.closed                 │
└─────────────────┬───────────────────────────────────┘
                  │  web-standard Request / Response
┌─────────────────▼───────────────────────────────────┐
│             iroh-http-shared (TS)                   │
│  makeReadable()  pipeToWriter()                     │
│  makeServe()     makeFetch()                        │
│  — depends only on the Bridge interface             │
└────────────┬────────────────────┬───────────────────┘
             │                    │
    ┌────────▼──────┐   ┌────────▼──────────┐
    │  iroh-http-   │   │  iroh-http-tauri  │
    │  node (napi)  │   │  (Tauri plugin)   │
    │  .node addon  │   │  invoke() bridge  │
    └────────┬──────┘   └────────┬──────────┘
             │                   │
    ┌────────▼──────┐            │
    │  iroh-http-   │            │
    │  deno (FFI)   │            │
    │  .dylib/.so   │            │
    └────────┬──────┘            │
             └─────────┬─────────┘
                       │  FFI / Tauri commands
          ┌────────────▼────────────────────────┐
          │       iroh-http-core (Rust)          │
          │  BodyReader  BodyWriter  FfiRequest  │
          │  serve()  fetch()  IrohEndpoint      │
          └──────┬──────────────┬───────────────┘
                 │              │
    ┌────────────▼──┐  ┌───────▼────────────────┐
    │ iroh-http-    │  │ iroh-http-discovery    │
    │ framing       │  │ (optional, pluggable)  │
    │ (no_std)      │  └───────┬────────────────┘
    └───────────────┘          │
                       ┌───────▼────────────────┐
                       │    Iroh QUIC (UDP)      │
                       │  NodeId = public key    │
                       └────────────────────────┘
```

-----

## Package Descriptions

### `iroh-http-framing` (Rust crate, `no_std`)

Pure HTTP/1.1 serialisation and parsing. No async runtime, no I/O, no Iroh dependency. Uses `httparse` internally.

This crate exists so that embedded targets (ESP32, bare-metal) and future language bindings can reuse the wire format without pulling in Tokio or Iroh. It is also used by `iroh-http-core`.

**Responsibilities:**

- Serialize an HTTP/1.1 request line + headers into bytes.
- Parse an HTTP/1.1 response status line + headers from bytes.
- Serialize an HTTP/1.1 response status line + headers into bytes.
- Parse an HTTP/1.1 request line + headers from bytes.
- No body handling — bodies are streamed separately by the caller.

-----

### `iroh-http-core` (Rust crate)

The package that owns the Iroh endpoint and wires HTTP framing to QUIC streams. Nothing in here knows about JavaScript.

**Key types:**

| Type           | Purpose                                                                         |
| -------------- | ------------------------------------------------------------------------------- |
| `Keypair`      | Ed25519 keypair. Determines the node's public key (its network address).        |
| `IrohEndpoint` | Wraps the Iroh endpoint. Created once per node, shared between fetch and serve. |
| `FfiRequest`   | Flat struct: method, url, headers, body handle, `remote_node_id`                |
| `FfiResponse`  | Flat struct: status, headers, body handle                                       |
| `BodyReader`   | Async channel consumer — exposes `next_chunk() -> Option<Bytes>`                |
| `BodyWriter`   | Async channel producer — exposes `send_chunk(Bytes)` + `finish()`               |
| `ServeOptions` | Optional keypair, concurrency limit                                             |

**Key behaviours:**

- The `IrohEndpoint` is created from a keypair and holds the UDP socket. It is shared between `fetch` and `serve` on the same node, meaning a node has a single stable identity regardless of whether it is sending, receiving, or both.
- One bidi QUIC stream = one HTTP request/response. HTTP/1.1 framing (from `iroh-http-framing`) is used because it maps directly onto a single stream.
- Each bidi stream lives for exactly one request/response cycle. Once the response body is fully drained and `send.finish()` is called, the stream closes. Connection reuse between streams to the same peer is managed transparently by Iroh.
- Request and response bodies are pumped through `tokio::sync::mpsc` channels. This decouples the Iroh I/O loop from the JS pull cadence — JS can call `nextChunk` at its own pace without stalling the acceptor.
- Concurrency on the server side is bounded with a `tokio::sync::Semaphore`.
- QUIC connections have an idle timeout. Unused connections are cleaned up automatically by Iroh. The timeout is configurable via `NodeOptions.idleTimeout`.
- On incoming requests, the authenticated remote peer's `NodeId` is read from the QUIC connection metadata and attached to `FfiRequest.remote_node_id`. This is a cryptographic fact — it is never parsed from headers.
- Any incoming header named `iroh-node-id` supplied by the remote peer is **stripped** before the headers reach the FFI boundary. The library injects the authentic value itself.
- Discovery is pluggable. `iroh-http-core` accepts an optional `Box<dyn Discovery>` via `NodeOptions`. If none is supplied, no local discovery runs. Relay and DNS discovery URLs are also configurable via options.

-----

### `iroh-http-discovery` (Rust crate, optional)

Implements Iroh's `Discovery` trait using mDNS for local network peer discovery on desktop platforms. This crate depends on `iroh` for the trait definition but contains no HTTP or framing logic.

**When to use it:**

- Desktop applications that need to find peers on the local network.
- Passed into `createNode` options at the Rust level by the Node.js or Tauri bridge.

**When NOT to use it:**

- Tauri on iOS/Android — these platforms require native OS APIs (`NSDNetService` on iOS, `NsdManager` on Android) accessed via Tauri's mobile plugin system. A platform-specific implementation satisfies the same `Discovery` trait.
- Environments where only relay-based or DNS-based discovery is needed.

-----

### `iroh-http-shared` (TypeScript package)

Pure TypeScript, no native dependencies. Both the Node.js and Tauri packages import from here. Contains all logic that reconstructs web-standard objects from raw FFI data.

**`Bridge` interface** — the only thing that differs between Node and Tauri:

```ts
interface Bridge {
  nextChunk(handle: number): Promise<Uint8Array | null>;
  sendChunk(handle: number, chunk: Uint8Array): Promise<void>;
  finishBody(handle: number): Promise<void>;
}
```

**`makeReadable(bridge, handle)`** — wraps a `BodyReader` handle in a `ReadableStream`. Calls `nextChunk` on each pull, closes the stream on `null`.

**`pipeToWriter(bridge, stream, handle)`** — drains a `ReadableStream` into a `BodyWriter` handle by repeatedly calling `sendChunk`, then `finishBody`.

**`makeServe(bridge, rawServe)`** — wraps the raw Rust callback-based serve in the Deno-compatible signature:

```ts
(options, (req: Request) => Response | Promise<Response>) => void
```

**`makeFetch(bridge, rawFetch)`** — wraps the raw Rust fetch in:

```ts
(nodeId: string, input: string | URL, init?: RequestInit) => Promise<Response>
```

-----

### `iroh-http-node` (Node.js package)

A `napi-rs` native addon. Compiles the Rust core to a `.node` file that loads directly into Node without a subprocess or IPC.

**What it does:**

- Spawns a Tokio runtime internally.
- Maintains a global slab (`handle -> BodyReader` and `handle -> BodyWriter`) so JS can reference open streams by integer handle.
- Exposes `nextChunk`, `sendChunk`, `finishBody`, `serve`, `fetch`, and `createEndpoint` as async napi functions.
- The `index.ts` entry point wires these into `iroh-http-shared` and exports `createNode`.

**Why napi-rs:** It is async-native and Tokio-aware. Promises returned from Rust resolve naturally in the Node event loop without extra bridging.

-----

### `iroh-http-tauri` (Tauri plugin)

A standard Tauri plugin. Because Tauri applications already run a Rust binary, the core crate is linked directly — no separate runtime, no subprocess.

**What it does:**

- Registers `next_chunk`, `send_chunk`, `finish_body`, `serve`, `fetch`, and `create_endpoint` as Tauri commands.
- Maintains the same handle slab pattern as the Node package.
- `guest-js/index.ts` implements the `Bridge` interface using `invoke()` and wires it into `iroh-http-shared`, exporting `createNode`.

**Binary transfer note:** Tauri's `invoke` serialises through JSON by default, which has overhead for large binary chunks. For high-throughput streaming the plugin uses Tauri's `Channel<Vec<u8>>` binary path internally. The JS API does not change — this is an internal implementation detail of the bridge.

-----

## The `createNode` Factory

Both packages export a single `createNode` function. This is the entry point for all usage.

```ts
interface NodeOptions {
  key?: Uint8Array;        // 32-byte Ed25519 secret key — omit to generate a new one
  idleTimeout?: number;    // milliseconds before an unused connection is closed (default: Iroh's default)
  relays?: string[];       // relay server URLs for NAT traversal
  dnsDiscovery?: string;   // DNS discovery server URL
}

interface IrohNode {
  nodeId: string;         // the public key — this node's network address
  keypair: Uint8Array;    // the secret key — persist this to restore the same identity
  fetch: typeof fetch;    // web-standard fetch, bound to this node's endpoint
  serve: typeof serve;    // Deno-compatible serve, bound to this node's endpoint
  close(): Promise<void>; // drop the endpoint, close all connections, release the UDP socket
}

createNode(options?: NodeOptions): Promise<IrohNode>
```

`fetch` and `serve` share the same underlying `IrohEndpoint`. A node that only calls `fetch` never opens a listener. A node that only calls `serve` never makes outgoing connections. Both can be used together.

-----

## Node and Connection Lifecycle

When `createNode` is called, an Iroh endpoint is created from the keypair. The keypair determines the node's public key, which is its permanent network address for the lifetime of that key.

If no key is supplied, one is generated. The returned `keypair` field should be persisted by the caller if identity continuity matters across restarts:

```ts
// First run — generate and persist
const node = await createNode();
await fs.writeFile('node.key', node.keypair);

// Subsequent runs — restore
const savedKey = await fs.readFile('node.key');
const node = await createNode({ key: savedKey });

console.log(node.nodeId); // same public key as before
```

The library has no opinion on how or where the keypair is stored.

**Connection lifecycle:**

| Event                        | Handled by                                             |
| ---------------------------- | ------------------------------------------------------ |
| New connection to a peer     | Iroh, on first `fetch` to that peer                    |
| Reuse of existing connection | Iroh, transparent — avoids repeated handshakes         |
| Idle connection cleanup      | Iroh, configurable via `idleTimeout` in `NodeOptions`  |
| Silent peer / network drop   | Iroh, idle timeout + QUIC keepalive                    |
| Intentional node shutdown    | `node.close()` — drops endpoint, sends CONNECTION_CLOSE to all peers |

Users have no direct connection management API. Each `fetch` call is stateless and self-contained from the caller's perspective. Each `serve` handler invocation handles one request and ends. The underlying QUIC connections are transparent.

-----

## Peer Identity

### On incoming requests (server side)

When a request arrives, the remote peer's public key is a cryptographic fact established during the QUIC handshake. `iroh-http-core` reads the `NodeId` from the authenticated connection metadata and passes it to JS via `FfiRequest.remote_node_id`.

`iroh-http-shared` injects it as a header when constructing the web-standard `Request`:

```ts
const req = new Request(url, {
  headers: [
    ...raw.headers,
    ['iroh-node-id', raw.remoteNodeId],  // injected by the library, not the peer
  ],
  // ...
});
```

JS developers access it like any other header:

```ts
node.serve({}, async (req) => {
  const peerId = req.headers.get('iroh-node-id');
  console.log('request from:', peerId);
  return new Response('ok');
});
```

**Security:** Any incoming `iroh-node-id` header sent by the remote peer is stripped by `iroh-http-core` before the headers cross the FFI boundary. The value seen by the handler is always the authenticated identity from the QUIC connection.

### URL scheme

URLs use the `http+iroh://` scheme with the node's public key as the host:

```
http+iroh://b5ea...f3c2/api/data?q=hello
```

- On the **server side**, incoming requests arrive with `http+iroh://<own-public-key>/path` as `req.url`. Routing by pathname works naturally with `new URL(req.url).pathname`.
- On the **client side**, after a `fetch`, the response URL reflects the remote peer: `http+iroh://<remote-public-key>/path`.
- If a user supplies a plain `http://` or `https://` URL to `fetch`, the library can forward it over standard TCP as a fallback — enabling seamless backwards compatibility with the regular web.

The header name (`iroh-node-id`) and URL scheme (`http+iroh://`) may be refined before v1 ships.

-----

## Data Flow: Incoming Request (server side)

```
Remote peer
    |  Iroh QUIC bidi stream (UDP)
    v
iroh-http-core: read HTTP headers (iroh-http-framing)
    |
    +-- read remote NodeId from authenticated connection
    |
    +-- spawn task: pump RecvStream -> BodyReader channel (16 KB chunks)
    |
    v
FfiRequest { method, url, headers, remote_node_id } + reqBodyHandle
    |  FFI boundary
    v
iroh-http-shared: construct web Request with ReadableStream body
    |  inject iroh-node-id header from remote_node_id
    v
User handler: (req: Request) => Promise<Response>
    |
    v
iroh-http-shared: drain Response body via pipeToWriter -> resBodyHandle
    |  FFI boundary
    v
iroh-http-core: write HTTP response headers, pump BodyWriter -> SendStream
    |
    v
Remote peer
```

-----

## Data Flow: Outgoing Request (client side)

```
User code: node.fetch(peerId, '/file')
    |
iroh-http-shared: flatten Request -> FfiRequest + optional bodyHandle
    |  FFI boundary
    v
iroh-http-core:
    connect IrohEndpoint to peerId (handles NAT traversal)
    open bidi stream
    write HTTP/1.1 request headers + body chunks (iroh-http-framing)
    read HTTP/1.1 response headers (iroh-http-framing)
    pump RecvStream -> BodyReader channel
    |
    v
FfiResponse { status, headers } + resBodyHandle
    |  FFI boundary
    v
iroh-http-shared: construct web Response with ReadableStream body
    |  set response URL to http+iroh://<peerId>/path
    v
User code: await res.json() / res.body.pipeTo(...)
```

-----

## The Bridge Interface in Practice

The only code that differs between Node and Tauri is how the three bridge methods are implemented:

| Method                     | Node.js                                | Tauri                                    |
| -------------------------- | -------------------------------------- | ---------------------------------------- |
| `nextChunk(handle)`        | napi async fn, zero-copy into `Buffer` | `invoke('plugin:iroh-http\|next_chunk')` |
| `sendChunk(handle, chunk)` | napi async fn, reads from `Buffer`     | `Channel<Vec<u8>>` binary path           |
| `finishBody(handle)`       | napi fn, drops handle from slab        | `invoke('plugin:iroh-http\|finish_body')`|

Everything above these three methods is shared.

-----

## User-Facing API Examples

All examples work identically in Node.js and Tauri.

**Create a node and persist its identity:**

```ts
import { createNode } from 'iroh-http';

const savedKey = await loadKeyFromDisk(); // undefined on first run
const node = await createNode({ key: savedKey });

if (!savedKey) await saveKeyToDisk(node.keypair);

console.log('my address:', node.nodeId);
```

**Serve a large file with streaming:**

```ts
node.serve({}, async (req) => {
  const url = new URL(req.url);
  const file = await openAsReadableStream(url.pathname);
  return new Response(file, {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
});
```

**Serve a realtime event stream:**

```ts
node.serve({}, async (req) => {
  const stream = new ReadableStream({
    async start(controller) {
      const enc = new TextEncoder();
      for await (const event of subscribeToEvents()) {
        controller.enqueue(enc.encode(`data: ${JSON.stringify(event)}\n\n`));
      }
      controller.close();
    },
  });
  return new Response(stream, {
    headers: { 'Content-Type': 'text/event-stream' },
  });
});
```

**Fetch from a remote node:**

```ts
const res = await node.fetch(remotePeerNodeId, '/api/data');
const json = await res.json();
```

**Read the authenticated peer identity on the server:**

```ts
node.serve({}, async (req) => {
  const peerId = req.headers.get('iroh-node-id');
  console.log(`request from ${peerId} to ${req.url}`);
  return Response.json({ peer: peerId });
});
```

**Shut down a node:**

```ts
await node.close();
```

-----

## Discovery

Discovery controls how nodes find each other's addresses. There are three mechanisms, each independently configurable:

**Relay servers** — Iroh's relay infrastructure for NAT traversal. Configured via `NodeOptions.relays`. If omitted, Iroh's default public relays are used.

**DNS discovery** — Resolve a node's address via DNS. Configured via `NodeOptions.dnsDiscovery`. Useful for well-known nodes with published DNS records.

**Local discovery (mDNS)** — Find peers on the same local network. This is the only mechanism that requires a separate crate (`iroh-http-discovery`) because platform requirements vary:

| Platform          | Implementation                                                              |
| ----------------- | --------------------------------------------------------------------------- |
| Desktop (macOS, Linux, Windows) | `iroh-http-discovery` crate — mDNS via Iroh's `Discovery` trait  |
| iOS (Tauri)       | Native `NSDNetService` via Tauri mobile plugin, same `Discovery` trait      |
| Android (Tauri)   | Native `NsdManager` via Tauri mobile plugin, same `Discovery` trait         |
| Node.js           | `iroh-http-discovery` crate, wired in at the napi level                     |

The `Discovery` trait is defined by Iroh. All implementations satisfy the same interface. `iroh-http-core` accepts an optional `Box<dyn Discovery>` — the bridge layer is responsible for wiring in the correct implementation for the platform. The JS API is unaware of discovery entirely; it is configured at the Rust level.

-----

## Key Technical Decisions

| Decision                                                 | Rationale                                                                                                                                                       |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `createNode` factory over module-level `fetch`/`serve`   | Both need a shared `IrohEndpoint`. The factory makes keypair ownership and endpoint lifecycle explicit. A pure-client node (fetch only) never opens a listener. |
| Keypair supplied to `createNode`, returned on the result | The library has no opinion on persistence. Returning the keypair makes it the caller's explicit responsibility.                                                 |
| HTTP/1.1, not HTTP/2                                     | One bidi QUIC stream per request maps directly. Multiplexing is redundant — QUIC already multiplexes streams at the transport layer.                            |
| `httparse` for HTTP framing                              | Only header serialisation and parsing are needed. No connection management, pooling, or keep-alive — those are TCP-era concerns.                                |
| `mpsc` channels for body streaming                       | Decouples the Iroh I/O loop from the JS pull cadence. JS can read chunks at its own pace without stalling the QUIC acceptor.                                    |
| Integer handles across FFI                               | Rust owns all stream state. JS holds only an opaque integer. No unsafe memory sharing across the FFI boundary.                                                  |
| Shared JS layer                                          | All stream construction and web-standard object reconstruction lives in `iroh-http-shared`. Each bridge implements exactly three methods.                       |
| Separate `iroh-http-framing` crate                       | Keeps the wire format reusable for embedded targets and future language bindings without pulling in Tokio or Iroh.                                              |
| Pluggable discovery via trait                            | Local discovery varies by platform. Injecting it via Iroh's `Discovery` trait keeps `iroh-http-core` platform-agnostic.                                         |
| `http+iroh://` URL scheme                                | Preserves HTTP semantics in the scheme name. Plain `http://` URLs can fall back to standard TCP for backwards compatibility.                                    |

-----

## Dependencies

| Package                | Dependencies                                          |
| ---------------------- | ----------------------------------------------------- |
| `iroh-http-framing`    | `httparse` (no_std compatible)                        |
| `iroh-http-core`       | `iroh-http-framing`, `iroh`, `tokio`, `bytes`, `slab` |
| `iroh-http-discovery`  | `iroh` (Discovery trait only)                         |
| `iroh-http-node`       | `iroh-http-core`, `napi`, `napi-derive`               |
| `iroh-http-tauri`      | `iroh-http-core`, `tauri`                             |
| `iroh-http-shared`     | none (pure TypeScript)                                |

-----

## Compile Optimisation

### Iroh Feature Flags

Iroh is modular. Only the features actually used should be enabled. Disable:

- Built-in local relay server (not needed — nodes connect via external relays)
- Metrics collection (adds binary size and a dependency on metrics crates)
- Discovery backends not in use (e.g. if only DNS discovery is needed, do not compile mDNS)

Enable only: endpoint, QUIC transport, and the discovery trait.

### Rust Release Profile

The following `Cargo.toml` profile settings minimise binary size:

```toml
[profile.release]
opt-level = "z"       # optimise for size over speed
lto = true            # link-time optimisation — removes unused code across crates
codegen-units = 1     # single codegen unit — slower compile, smaller binary
strip = true          # strip debug symbols from the final binary
panic = "abort"       # smaller than unwinding
```

### Per-Target Notes

| Target               | Notes                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Node.js (`.node`)    | The Tokio runtime is included and has a fixed baseline cost (~1-2 MB). Not avoidable.      |
| Tauri                | The Rust core is linked into the existing Tauri binary — no additional runtime overhead.    |
| ESP32 / embedded     | Use `iroh-http-framing` alone if only the wire format is needed. If running full Iroh, compile with `opt-level = "z"` and verify heap usage fits the target. |

### General Advice

- Run `cargo bloat` to identify which crates and functions contribute most to binary size.
- Use `cargo tree` to audit transitive dependencies and ensure nothing unnecessary is pulled in.
- Consider `cargo-udeps` to detect unused dependencies.

-----

## Embedded and Cross-Platform

### ESP32

Iroh runs on ESP32 (see Iroh's published guide). If Iroh itself fits on the target, `iroh-http-core` likely works too — its only additional dependency beyond Iroh is Tokio, and the async runtime can be swapped for `embassy` on embedded. The wire format is identical, so an ESP32 node and a desktop Node.js node interoperate seamlessly.

For constrained devices where full Iroh cannot run, `iroh-http-framing` (no_std, no async, no allocator requirement beyond `httparse`) can be paired with any QUIC implementation available for the target. The framing is the interoperability layer — as long as both ends agree on the HTTP/1.1 wire format, they communicate.

### Python

`iroh-http-core` can be bound to Python via PyO3. PyO3 supports async via `pyo3-asyncio`, so the existing `fetch` and `serve` implementations map to Python with relatively little glue:

```python
from iroh_http import create_node

node = await create_node()
res = await node.fetch(peer_id, "/api/data")
data = await res.json()
```

A future `iroh-http-python` package would depend on `iroh-http-core` via PyO3 — no changes to core are needed.

### Other Languages

The crate separation (`iroh-http-framing` for the wire format, `iroh-http-core` for the full runtime) means any language with a QUIC library can implement a compatible node. The protocol is plain HTTP/1.1 over a bidi QUIC stream — no custom framing to reverse-engineer.

-----

## What Is Not in Scope (v1)

- **TLS** — Iroh connections are already encrypted with the keypair. No additional TLS layer is needed.
- **HTTP/2 or HTTP/3** — not needed; see rationale in Key Technical Decisions.
- **Routing** — `serve` accepts a single handler. Routing is the application's responsibility.
- **Browser target** — QUIC from a browser requires WebTransport, which is a separate effort.
- **Header compression** — not needed at current scale; see Future section.

-----

## Future (v2+)

### QPACK Header Compression

QPACK (the HTTP/3 header compression scheme) is a separate spec from HTTP/3 itself and can be implemented as a layer between `iroh-http-framing` and the QUIC stream. It compresses repeated headers (auth tokens, content-type, cache headers) across requests on the same connection.

**When it matters:** Many small API-style requests with large or repetitive headers. For large file transfers and streaming, the overhead of uncompressed headers is negligible.

**How to retrofit:** Both peers negotiate support via an ALPN identifier during the Iroh handshake. Nodes that do not support compression fall back to uncompressed headers. The change is contained within `iroh-http-framing` and `iroh-http-core` — nothing above the FFI boundary changes. No breaking API change.

### Stream Prioritisation

HTTP/3 defines a formal prioritisation scheme for concurrent streams. Since we open one stream per request, this only matters when making many concurrent fetches to the same peer simultaneously. Can be added at the `iroh-http-core` level if demand arises.

### Browser Target (WebTransport)

Running in a browser requires replacing Iroh's UDP-based QUIC with WebTransport. The `iroh-http-shared` layer and the `Bridge` interface would remain unchanged — only a new bridge implementation (`iroh-http-browser`) would be needed.
