# JavaScript / TypeScript Platform Guidelines

Applies to: `iroh-http-node`, `iroh-http-deno`, `iroh-http-tauri` (guest JS),
and the shared layer `iroh-http-shared`.

---

## Naming

| Scope       | Convention       | Example                        |
| ----------- | ---------------- | ------------------------------ |
| Functions   | `camelCase`      | `createNode`, `makeFetch`      |
| Types       | `PascalCase`     | `IrohNode`, `PublicKey`        |
| Constants   | `UPPER_SNAKE`    | `METHODS_WITH_BODY`            |
| Properties  | `camelCase`      | `nodeId`, `bodyHandle`         |
| Events      | `lowercase-dash` | `"abort"`, `"close"`           |

Prefix internal/FFI-only types with `Ffi` (`FfiRequest`, `FfiResponseHead`).
Never export `Ffi`-prefixed types from user-facing modules.

---

## Types — use what the platform provides

| Concept               | Use                         | Never                                  |
| --------------------- | --------------------------- | -------------------------------------- |
| Request               | `Request` (WHATWG)          | Custom request class                   |
| Response              | `Response` (WHATWG)         | Custom response class                  |
| Headers               | `Headers` (WHATWG)          | `Record<string, string>`, bare tuples  |
| Readable body         | `ReadableStream<Uint8Array>` | `AsyncIterator`, Node `stream`        |
| Byte data             | `Uint8Array`                | `ArrayBuffer`, Node `Buffer` in APIs   |
| Cancellation          | `AbortSignal`               | Boolean flags, custom cancel tokens    |
| URL                   | `URL` or `string`           | Custom URL class                       |
| Errors                | `DOMException` subtypes     | Generic `Error`, string throws         |
| Async results         | `Promise<T>`                | Callbacks, EventEmitter                |
| Cleanup               | `Symbol.asyncDispose`       | Manual `.destroy()` / `.free()`        |

Internal code may use `[string, string][]` for header pairs across the FFI
boundary, but these must be converted to `Headers` before reaching user code.

---

## Error handling

All errors thrown to user code must be subclasses of `DOMException` or typed
error classes from `@momics/iroh-http-shared/errors`.

Use the structured error codes from the Rust core (`classify_error_json`).
The JS layer maps these codes to specific error classes:

| Rust code          | JS class              | `name` property        |
| ------------------ | --------------------- | ---------------------- |
| `TIMEOUT`          | `IrohConnectError`    | `"TimeoutError"`       |
| `ABORT`            | `IrohAbortError`      | `"AbortError"`         |
| `INVALID_HANDLE`   | `IrohHandleError`     | `"InvalidHandle"`      |
| `STREAM_RESET`     | `IrohStreamError`     | `"StreamReset"`        |
| *(catch-all)*      | `IrohError`           | `"IrohError"`          |

Never throw plain strings. Never expose Rust error messages without
classification.

---

## Async patterns

- **All I/O is `async`/`await`.** No synchronous FFI calls that block the
  event loop.
- **Fire-and-forget** calls (e.g., `cancelFetch`) are synchronous and
  intentionally do not return a `Promise`.
- **`AbortSignal` integration:** every long-running operation (`fetch`,
  `createBidirectionalStream`) accepts `signal` in its options. If already
  aborted, reject immediately with `AbortError` before touching the
  transport.
- **Cleanup:** `IrohNode` implements `Symbol.asyncDispose` so it works with
  `await using`. The `close()` method is also available for explicit
  cleanup. `node.closed` is a `Promise<void>` that resolves when shutdown
  completes.

---

## Streaming

Body streams are `ReadableStream<Uint8Array>`, created via `makeReadable()`.

**Rules:**

- One `ReadableStream` per body handle. Never create two streams from the
  same handle.
- Request bodies that support streaming set `duplex: "half"` in
  `RequestInit`.
- Writer-side streaming uses `pipeToWriter()` which calls `sendChunk` for
  each chunk, then `finishBody`.
- `cancel()` on a `ReadableStream` calls `bridge.cancelRequest(handle)`.

---

## Serve handler contract

The serve handler has the signature:

```ts
type ServeHandler = (req: Request) => Response | Promise<Response>;
```

- The handler receives a standard `Request`. The authenticated peer identity
  is injected as the `iroh-node-id` header.
- The handler returns a standard `Response` (or a `Promise<Response>`).
- Request trailers are exposed as `(req as any).trailers: Promise<Headers>`.
- Response trailers are sent via a handle provided in the payload. Call
  `bridge.sendTrailers()` exactly once per response.
- The `httpi:` scheme in request URLs is replaced with `http:` before
  constructing the `Request` so that the WHATWG URL parser accepts them.

---

## Fetch signature

```ts
type FetchFn = (
  peer: PublicKey | string,
  input: string | URL,
  init?: IrohFetchInit
) => Promise<Response>;
```

`IrohFetchInit` extends standard `RequestInit` with:
- `signal?: AbortSignal` — cancellation
- `directAddrs?: string[]` — direct address hints

The returned `Response` has a non-standard `trailers: Promise<Headers>`
property for reading response trailers.

---

## Bridge interface

The `Bridge` interface is the **only** platform-varying abstraction. Each
adapter (Node, Tauri, Deno) implements it. It handles:

- Body I/O: `nextChunk`, `sendChunk`, `finishBody`
- Cancellation: `cancelRequest`, `allocFetchToken`, `cancelFetch`
- Trailers: `nextTrailer`, `sendTrailers`

Bridge methods use integer handles (`u32` slab indices). These handles are
an internal detail and must never appear in user-facing APIs.

---

## Platform-specific notes

### Node.js (`iroh-http-node`)

- Uses **napi-rs** for FFI. Body chunks cross the boundary as `Buffer`.
- Serve callback uses `ThreadsafeFunction` in fire-and-forget mode.
- The main export is `createNode(options?)` which returns `IrohNode`.

### Deno (`iroh-http-deno`)

- Uses **C-ABI FFI** via `Deno.dlopen` + JSON-based dispatch.
- Chunks cross the boundary as raw byte pointers.
- Same `createNode(options?)` → `IrohNode` API shape as Node.

### Tauri (`iroh-http-tauri`)

- Uses **Tauri invoke** for commands, **Channel** for serve callback.
- Body chunks are base64-encoded across the invoke boundary.
- Guest JS uses the shared `buildNode` factory — same `IrohNode` shape.
- The Tauri plugin registers `create_endpoint`, `fetch`, `serve`, etc. as
  Tauri commands.

---

## Testing

- Test through the public `IrohNode` surface, not through `Bridge` methods.
- Serve handler tests should use real QUIC connections (two nodes, same
  process).
- Cancellation and timeout tests must exercise `AbortSignal` integration.
- Streaming tests must verify both streaming reads and writes, including
  early cancellation.
