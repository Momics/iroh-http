# iroh-http — Protocol Extensions

This document specifies four additions to the base architecture described in `brief.md`. Each is self-contained and can be implemented independently.

---

## 1. Protocol Naming

### Problem

The base spec calls the framing "HTTP/1.1". This is technically incorrect — the connection is not over TCP, the URL scheme is custom, the peer identity header is injected by the library, and (once bidirectional mode is added) the sequencing rules of HTTP/1.1 no longer hold.

### Decision

The protocol is **"Iroh-HTTP"**. Internally this means:

- HTTP/1.1 wire format for headers — identical syntax, identical `httparse` parsing.
- HTTP/1.1 status codes and header field semantics — unchanged.
- Framing divergences are versioned via ALPN (see §4), not by abusing the HTTP version field.

The phrase "HTTP/1.1 framing" in `brief.md` should be understood as "Iroh-HTTP framing that borrows the HTTP/1.1 header wire format". No HTTP version is sent on the wire — the request line is `METHOD /path Iroh-HTTP/1` and the status line is `Iroh-HTTP/1 200 OK`. This removes any claim of compliance with RFC 9112 while keeping the format readable and parseable by any HTTP/1.1 parser that does not validate the version string strictly. `httparse` does not validate the version string.

**Impact on `iroh-http-framing`:** change the serialised version token. No other change required.

---

## 2. Bidirectional Streaming

### Problem

HTTP/1.1 requires the request body to precede the response. The QUIC bidi stream is already full-duplex — `SendStream` and `RecvStream` are independent. Enforcing sequential order is an artificial constraint. Real-time use cases (live collaboration, audio/video, interactive RPC) benefit from the ability to stream data to the server while the server simultaneously streams data back, all within a single logical request.

### Design

Bidirectional mode is **explicit and opt-in**. A regular `fetch`/`serve` exchange continues to work as documented in `brief.md`. Bidirectional mode is activated via a dedicated API on both sides.

#### Client side

```ts
interface DuplexStream {
  writable: WritableStream<Uint8Array>;  // send data to the server
  readable: ReadableStream<Uint8Array>;  // receive data from the server
}

// New method on IrohNode
node.connect(nodeId: string, path: string, init?: RequestInit): Promise<DuplexStream>
```

`connect` opens a bidi QUIC stream, sends an Iroh-HTTP request with an `Upgrade: iroh-duplex` header, and returns immediately with both streams open. The caller can read and write concurrently without waiting for either side to finish.

#### Server side

The handler receives the `Upgrade` header. It signals acceptance by returning a `101 Switching Protocols` response along with a `DuplexHandler`:

```ts
interface DuplexHandler {
  readable: ReadableStream<Uint8Array>;  // data arriving from the client
  writable: WritableStream<Uint8Array>;  // data to send to the client
}

node.serve({}, async (req) => {
  if (req.headers.get('upgrade') === 'iroh-duplex') {
    const { readable, writable } = req.duplex();  // new method on Request
    // readable and writable are both open immediately
    const reader = readable.getReader();
    const writer = writable.getWriter();
    // ... read and write concurrently
    return new Response(null, { status: 101 });
  }
  return new Response('not a duplex request', { status: 400 });
});
```

`req.duplex()` is added by `iroh-http-shared` when constructing the web `Request` from a raw `FfiRequest` that carried an `Upgrade: iroh-duplex` header. Calling `req.duplex()` on a non-upgraded request throws.

#### Wire protocol

After both sides have sent their initial headers (client request headers, server 101 response headers), both `SendStream` and `RecvStream` are treated as raw byte channels. No further HTTP framing is applied. Body chunks flow freely in both directions until either side finishes their `SendStream`.

#### Framing summary

```
Client -> Server:
  METHOD /path Iroh-HTTP/1\r\n
  Upgrade: iroh-duplex\r\n
  [other request headers]\r\n
  \r\n
  [free-form binary body chunks...]
  [SendStream.finish()]

Server -> Client:
  Iroh-HTTP/1 101 Switching Protocols\r\n
  [optional response headers]\r\n
  \r\n
  [free-form binary body chunks...]
  [SendStream.finish()]
```

Both sides read from `RecvStream` and write to `SendStream` concurrently after headers are exchanged. The stream closes when both sides have called `finish()` on their respective `SendStream`.

#### Changes required

| Layer | Change |
| --- | --- |
| `iroh-http-framing` | Serialise/parse `Upgrade: iroh-duplex` header; no other change |
| `iroh-http-core` | Server: detect `Upgrade` header, skip response-before-body constraint; Client: new `connect()` function that opens a stream and returns handle pair without waiting |
| `FfiRequest` | Add `is_duplex: bool` flag |
| `iroh-http-shared` | Add `req.duplex()` method; add `makeConnect()` wrapping the raw connect |
| `Bridge` | No new methods — the same `nextChunk`/`sendChunk`/`finishBody` primitives work for both directions |
| `IrohNode` | Expose `node.connect()` |
| ALPN | Bidirectional mode requires ALPN `iroh-http/1-duplex` (see §4) |

---

## 3. Request Cancellation (`AbortSignal`)

### Problem

There is currently no way for the caller to cancel an in-flight `fetch`. This is expected web-standard behaviour — `fetch` accepts an `AbortSignal` in `RequestInit`. Without it, a long-running or stalled request cannot be abandoned without shutting down the entire node.

At the QUIC level, Iroh exposes `STOP_SENDING` (tell the remote to stop sending on a stream) and `RESET_STREAM` (abruptly terminate your own send side). Both are the correct primitives for cancellation.

### Design

#### JS side (no API change)

`AbortSignal` is already part of `RequestInit`. The caller uses it exactly as they would with the platform `fetch`:

```ts
const controller = new AbortController();
setTimeout(() => controller.abort(), 5000);

try {
  const res = await node.fetch(peerId, '/slow-endpoint', {
    signal: controller.signal,
  });
} catch (e) {
  if (e.name === 'AbortError') {
    console.log('request cancelled');
  }
}
```

#### Bridge changes

One new method is added to the `Bridge` interface:

```ts
interface Bridge {
  nextChunk(handle: number): Promise<Uint8Array | null>;
  sendChunk(handle: number, chunk: Uint8Array): Promise<void>;
  finishBody(handle: number): Promise<void>;
  cancelRequest(handle: number): Promise<void>;  // new
}
```

`cancelRequest` closes the request's QUIC stream immediately from the Rust side.

#### `iroh-http-shared` changes

`makeFetch` reads the `signal` from `init`. If the signal is already aborted, it rejects immediately. Otherwise it registers an abort listener:

```ts
signal.addEventListener('abort', () => {
  bridge.cancelRequest(reqHandle);
});
```

When `cancelRequest` resolves, any pending `nextChunk` on the response body resolves to `null` (EOF), and the `Promise<Response>` rejects with an `AbortError`.

#### `iroh-http-core` changes

`cancelRequest(handle)` does two things:

1. Calls `send_stream.reset(0)` — tells the server to stop receiving.
2. Calls `recv_stream.stop(0)` — tells the server to stop sending.
3. Removes the handle from both slabs and drops the associated `BodyReader`/`BodyWriter`.

Any in-flight `next_chunk` awaiting on the dropped `BodyReader` returns `None` immediately.

#### Server-side behaviour

When the client cancels, the server's `RecvStream` will return an error on the next read (QUIC `STREAM_RESET` frame). `iroh-http-core` catches this in the body-pump task and closes the `BodyReader` channel. The handler's `req.body` readable stream closes with an error. The handler should treat this as a connection drop and return early.

No changes are required to the server-side JS API to support this. It is already a consequence of the QUIC stream behaviour.

#### Changes required

| Layer | Change |
| --- | --- |
| `iroh-http-core` | New `cancel_fetch(handle)` function; reset both stream sides and drop slab entries |
| `Bridge` | Add `cancelRequest(handle)` |
| `iroh-http-node` | Expose `cancel_request` as napi async fn |
| `iroh-http-tauri` | Expose `cancel_request` as Tauri command |
| `iroh-http-shared` | `makeFetch` wires `AbortSignal` to `bridge.cancelRequest` |

---

## 4. Trailer Headers

### Problem

There is no way to send metadata after the body. This is useful for:

- **Checksums** — send a hash of the body after it is fully written, so the receiver can verify integrity without buffering.
- **Final status** — a server streaming a long computation can append a final `result: ok` or `result: error` trailer after the body finishes, without needing a separate request.
- **Timing** — send `server-timing` or similar diagnostics after the response completes.

HTTP/1.1 technically supports chunked trailers but they are poorly supported and awkward. Since we own the framing, we can define clean trailer support.

### Design

Trailers are a second set of headers sent after the body's `SendStream` reach end-of-data, but before `finish()` is called. They use the same wire format as request/response headers (one `Name: Value\r\n` per trailer, terminated by `\r\n`).

#### Wire format

```
[body chunks]
\r\n                       <- end of body signal (zero-length chunk sentinel)
Trailer-Name: value\r\n
Another-Trailer: value\r\n
\r\n                       <- end of trailers
[SendStream.finish()]
```

The zero-length sentinel distinguishes "body ended, trailers follow" from `SendStream.finish()` which means "stream closed entirely". This adds one round of framing logic to `iroh-http-framing`.

#### JS API

On the **response** side, `Response` already has a `trailers` property defined in the Fetch spec (it returns a `Promise<Headers>`). This is not implemented in most runtimes but the interface exists. `iroh-http-shared` populates it:

```ts
// Reading trailers on the client after receiving a response
const res = await node.fetch(peerId, '/checksummed-file');
const body = await res.arrayBuffer();
const trailers = await res.trailers;  // resolves after body is fully consumed
const checksum = trailers.get('x-checksum');
```

On the **request** side, a `trailers` option is added to `RequestInit`-style usage via a new field on the `FfiRequest`:

```ts
// Sending trailers with a request body
const res = await node.fetch(peerId, '/upload', {
  method: 'POST',
  body: fileStream,
  trailers: async () => new Headers({ 'x-checksum': await computeHash() }),
});
```

The `trailers` callback is called after the body stream is drained and before the QUIC stream is finished.

On the **serve** side, the handler can attach trailers to a `Response`:

```ts
node.serve({}, async (req) => {
  const data = await generateLargeDataStream();
  const hash = await computeHash(data);
  return new Response(data, {
    trailers: () => new Headers({ 'x-checksum': hash }),
  });
});
```

#### Bridge changes

Two new methods:

```ts
interface Bridge {
  // ... existing methods ...
  nextTrailer(handle: number): Promise<[string, string][] | null>;  // null = no trailers
  sendTrailers(handle: number, trailers: [string, string][]): Promise<void>;
}
```

`nextTrailer` is called once after `nextChunk` returns `null`. It returns the trailer name-value pairs, or `null` if no trailers were sent.

#### Changes required

| Layer | Change |
| --- | --- |
| `iroh-http-framing` | Add zero-length sentinel after body; parse/serialize trailer block |
| `iroh-http-core` | Pump trailers from stream after body; expose via `FfiResponse.trailers_handle` and `FfiRequest.trailers_handle` |
| `Bridge` | Add `nextTrailer` and `sendTrailers` |
| `iroh-http-node` | Expose both as napi async fn |
| `iroh-http-tauri` | Expose both as Tauri commands |
| `iroh-http-shared` | Populate `res.trailers` promise; read `init.trailers` callback; read `Response` trailers option |

---

## 5. Capabilities Negotiation (ALPN)

### Problem

Multiple extensions will diverge from the base Iroh-HTTP framing. Without a negotiation mechanism, a node with a newer implementation cannot safely use an extension with a node that does not support it. Failing silently or with a framing error is worse than refusing to connect.

QUIC already performs ALPN negotiation during the handshake — the client proposes a list of protocol identifiers, the server picks one. If no common identifier exists, the handshake fails before any application data is exchanged.

### ALPN strings

| Identifier | Meaning |
| --- | --- |
| `iroh-http/1` | Base protocol. HTTP/1.1-framed Iroh-HTTP, half-duplex, no trailers. |
| `iroh-http/1-duplex` | Base + bidirectional streaming (§2). |
| `iroh-http/1-trailers` | Base + trailer headers (§4). |
| `iroh-http/1-full` | Base + bidirectional + trailers + cancellation signals. |

Identifiers are additive. A node always includes `iroh-http/1` in its proposal list so it can communicate with base-only peers. A node that supports trailers proposes `['iroh-http/1-trailers', 'iroh-http/1']` in preference order. The server picks the highest it supports.

Cancellation (§3) requires no framing changes and no new ALPN — it is a QUIC-level primitive available on all versions.

### Negotiation at the `createNode` level

The negotiated protocol is transparent to the JS caller. `iroh-http-core` records which ALPN was agreed and gates extension behaviour accordingly:

- If the peer only agreed `iroh-http/1`, calling `node.connect()` (duplex) throws with a clear error: `"peer does not support duplex mode"`.
- If the peer only agreed `iroh-http/1`, trailers sent by the caller are silently dropped on the wire (they are framed as body data in base mode). Callers that require trailers should check capabilities explicitly.

### Explicit capability check (optional)

A future convenience method on `IrohNode`:

```ts
node.capabilities(nodeId: string): Promise<Set<'duplex' | 'trailers'>>
```

This opens a connection to the peer and reads back the negotiated ALPN. No data is exchanged — it is a handshake-only probe. Can be added in v2 if demand arises.

### Changes required

| Layer | Change |
| --- | --- |
| `iroh-http-core` | Pass ALPN list to Iroh endpoint builder; record negotiated ALPN per connection; gate duplex/trailer behaviour on negotiated ALPN |
| `iroh-http-framing` | No change — the ALPN identifiers are defined here as constants |
| `NodeOptions` | Add optional `capabilities: ('duplex' \| 'trailers')[]` — defaults to advertising all supported capabilities |
| JS packages | Expose negotiation error as a typed error class (`CapabilityError`) |

---

## Summary of New Bridge Methods

The full `Bridge` interface after all extensions:

```ts
interface Bridge {
  // Existing (brief.md)
  nextChunk(handle: number): Promise<Uint8Array | null>;
  sendChunk(handle: number, chunk: Uint8Array): Promise<void>;
  finishBody(handle: number): Promise<void>;

  // §3 — Cancellation
  cancelRequest(handle: number): Promise<void>;

  // §4 — Trailers
  nextTrailer(handle: number): Promise<[string, string][] | null>;
  sendTrailers(handle: number, trailers: [string, string][]): Promise<void>;
}
```

Bidirectional mode (§2) reuses the existing `nextChunk`/`sendChunk`/`finishBody` methods — no new bridge methods needed.

---

## Summary of New `IrohNode` Members

```ts
interface IrohNode {
  // Existing (brief.md)
  nodeId: string;
  keypair: Uint8Array;
  fetch(nodeId: string, input: string | URL, init?: RequestInit): Promise<Response>;
  serve(options: ServeOptions, handler: (req: Request) => Response | Promise<Response>): void;
  close(): Promise<void>;

  // §2 — Bidirectional streaming
  connect(nodeId: string, path: string, init?: RequestInit): Promise<DuplexStream>;
}
```

`AbortSignal` (§3) is passed through the existing `fetch` `init` parameter — no new member needed.

Trailers (§4) are read/written via `Response.trailers` and `RequestInit.trailers` — no new member needed.

---

## Implementation Order

These are independent but have a natural ordering by complexity:

1. **Protocol rename** (§1) — one-line change in `iroh-http-framing`. Do this first so all subsequent work uses the correct version string.
2. **`AbortSignal`** (§3) — small, self-contained, high user value. No framing changes.
3. **Capabilities negotiation** (§5) — needed before shipping extensions. Define the ALPN constants and wire up the Iroh endpoint builder. The gating logic can be a stub initially.
4. **Trailers** (§4) — framing addition. Implement behind `iroh-http/1-trailers` ALPN.
5. **Bidirectional streaming** (§2) — largest change. Implement behind `iroh-http/1-duplex` ALPN.
