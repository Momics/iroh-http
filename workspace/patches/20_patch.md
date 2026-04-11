---
status: pending
---

# iroh-http — Patch 20: `serve` API Ergonomics

## Problem

`node.serve(options, handler)` has three issues:

1. **`options` is a lie.** The parameter is typed `Record<string, unknown>` but
   every platform adapter ignores it (`_options` in `lib.ts`, never forwarded
   to Rust). Callers have to pass `{}` as a placeholder. An untyped record that
   is silently discarded is worse than no parameter.

2. **No overloaded signatures.** `Deno.serve` lets callers write
   `serve(handler)` for the common case. Our API forces the empty-object
   placeholder on every call site — pure ceremony with no benefit.

3. **Returns `void`.** There is no handle to observe or stop the server after
   it starts, so the only way to know when it has finished is `node.closed`.
   That information is already there; it just is not surfaced by `serve`.

---

## What Deno.serve options actually apply here

Deno's `ServeOptions` is the reference, but most fields are TCP/TLS concerns
that have no meaning in iroh-http:

| Deno option | Applies? | Reason |
|---|---|---|
| `port` / `hostname` | No | iroh nodes are addressed by public key, not ip:port |
| `cert` / `key` / `alpnProtocols` | No | QUIC handles encryption end-to-end |
| `signal` | **Yes** | The Rust `ServeHandle::shutdown()` already exists (Patch 15). A thin `stopServe` FFI entry point is all that is needed |
| `onError` | **Yes** | Currently errors from handlers are silently swallowed with a generic 500 |
| `onListen` | **Yes** | Useful for startup logging; iroh equivalent carries `nodeId` not `hostname:port` |
| `handler` (inside options) | **Yes** | Enables `serve({ handler, onListen })` single-argument form |

The right set of options for the current implementation is therefore:

```ts
export interface ServeOptions {
  /**
   * Called once when the serve loop is ready to accept connections.
   *
   * Iroh-HTTP equivalent of Deno's `onListen({ hostname, port })`.
   * The server starts immediately after `serve()` returns; this hook fires
   * synchronously after `rawServe` returns (Iroh binds during `createNode`,
   * not during `serve`, so the loop is immediately live).
   */
  onListen?: (info: { nodeId: string }) => void;

  /**
   * Called when a request handler throws or rejects.
   *
   * The returned `Response` is sent to the client. If this callback also
   * throws, the request receives a bare `500 Internal Server Error`.
   *
   * @default Returns `500 Internal Server Error` with no body.
   */
  onError?: (error: unknown) => Response | Promise<Response>;

  /**
   * When the signal is aborted, the serve loop stops accepting new connections
   * and drains in-flight requests (graceful shutdown), then resolves
   * `ServeHandle.finished`.
   *
   * This only stops the serve loop — the node itself stays alive and can still
   * call `fetch` or start a new `serve`. To shut down the node entirely, call
   * `node.close()`.
   *
   * Uses the existing `ServeHandle::shutdown()` primitive from Patch 15.
   */
  signal?: AbortSignal;

  /**
   * Inline handler — allows the single-argument `serve({ handler })` form.
   * Mutually exclusive with passing `handler` as the second argument.
   */
  handler?: ServeHandler;
}
```

The `signal` option stops the serve loop without closing the endpoint. The
Rust-side mechanism already exists: `ServeHandle::shutdown()` (added in
Patch 15) notifies the accept loop to drain and exit. All that is needed on top
is a thin platform FFI entry point so JS can reach it.

---

## Design

### Multiple call signatures

Mirror the three most useful `Deno.serve` variants:

```ts
// 1. Handler only — the most common case
node.serve(handler: ServeHandler): ServeHandle;

// 2. Options + handler
node.serve(options: ServeOptions, handler: ServeHandler): ServeHandle;

// 3. Handler inside options (single-arg, named params)
node.serve(options: ServeOptions & { handler: ServeHandler }): ServeHandle;
```

All three resolve to the same implementation. The overloads live on `IrohNode`
(in `bridge.ts`) and on the `ServeFn` type (in `serve.ts`).

### Return type: `ServeHandle`

```ts
export interface ServeHandle {
  /**
   * Resolves when the serve loop terminates — either because `node.close()`
   * was called or because a fatal error occurred.
   *
   * Mirrors `Deno.HttpServer.finished` and `WebTransportSession.closed`.
   */
  readonly finished: Promise<void>;
}
```

`makeServe` already receives `endpointHandle` but not the `closed` promise.
`buildNode` (in `index.ts`) does have access to it via the `closeEndpoint`
closure; the simplest fix is to thread a `finished: Promise<void>` parameter
into `makeServe`, or to construct and return a `ServeHandle` inline in
`buildNode` after calling `makeServe`.

The inline approach is cleaner — `buildNode` already assembles the node object
and can attach `finished: node.closed` itself:

```ts
// index.ts — buildNode
serve: (...args) => {
  makeServe(bridge, info.endpointHandle, rawServe)(...args);
  return { finished: node.closed };
},
```

`node.closed` has to be defined before `serve` is called; since it is set in
the same `IrohNode` literal, this works as long as `node.closed` is evaluated
lazily (it is — it is a property access on `node`, not a captured value).

### `onError` wiring in `makeServe`

```ts
// serve.ts — inside the rawServe callback
try {
  res = await Promise.resolve(handler(req));
} catch (err) {
  res = options.onError
    ? await Promise.resolve(options.onError(err))
    : new Response(null, { status: 500 });
}
```

Currently the `try/catch` is only around the pipe, not the handler call itself.
A thrown handler is currently an unhandled rejection. This is worth fixing
independently of the options discussion.

### `onListen` wiring

`rawServe` is fire-and-forget (`void` return). There is no callback when the
Rust loop is ready. Iroh binds during `createNode`, not during `serve`, so the
loop is immediately live. Call `onListen` synchronously after `rawServe`
returns:

```ts
rawServe(endpointHandle, ...);
options.onListen?.({ nodeId });
```

This requires threading `nodeId` into `makeServe`. The node ID is available in
`buildNode` as `info.nodeId` (or derivable from the public key).

### `signal` wiring

Each platform needs a new FFI entry point to call `ServeHandle::shutdown()`
without closing the whole endpoint:

**Rust (iroh-http-node / iroh-http-deno dispatcher):**
```rust
// Node.js (napi)
#[napi]
pub fn stop_serve(endpoint_handle: u32) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.serve_handle.lock().unwrap()
      .as_ref()
      .map(|h| h.shutdown());
    Ok(())
}

// Deno (dispatch.rs)
"stopServe" => {
    let ep = get_endpoint(p["endpointHandle"])?;
    ep.serve_handle.lock().unwrap()
      .as_ref()
      .map(|h| h.shutdown());
    Ok(json!({}))
}
```

**JS (`makeServe` in `serve.ts`):**
```ts
if (options.signal) {
  if (options.signal.aborted) {
    stopServe(endpointHandle);
  } else {
    options.signal.addEventListener(
      "abort",
      () => stopServe(endpointHandle),
      { once: true }
    );
  }
}
```

`stopServe` must be threaded into `makeServe` as a new parameter (alongside
`nodeId` and `finished`). Each platform adapter provides it from the bridge.

---

## Files to change

### `packages/iroh-http-shared/src/serve.ts`

- Export `ServeOptions` and `ServeHandle` interfaces.
- Change `ServeFn` signature to the three overloads.
- Replace `Record<string, unknown>` with `ServeOptions` in `makeServe`.
- Wrap `handler(req)` in a try/catch that delegates to `options.onError`.
- Call `options.onListen?.(...)` after `rawServe`.
- Wire `options.signal` to the `stopServe` callback.
- Return `ServeHandle` from the returned function (the `finished` promise is
  threaded in as a new parameter to `makeServe`).

```ts
// Before
export function makeServe(
  bridge: Bridge,
  endpointHandle: number,
  rawServe: RawServeFn
): ServeFn { ... }

// After
export function makeServe(
  bridge: Bridge,
  endpointHandle: number,
  rawServe: RawServeFn,
  nodeId: string,
  finished: Promise<void>,
  stopServe: () => void   // calls the platform's stopServe FFI
): ServeFn { ... }
```

### `packages/iroh-http-shared/src/bridge.ts`

- Replace `serve(options: Record<string, unknown>, handler: ...)` on `IrohNode`
  with the three overloads.
- Change `RawServeFn` options parameter from `Record<string, unknown>` to
  `ServeOptions` (or `unknown` — the raw layer doesn't need to read options).
- Export `ServeOptions` and `ServeHandle` from this file or from `serve.ts`.

### `packages/iroh-http-shared/src/index.ts`

- Pass `info.nodeId` and `node.closed` (or a settled-on-close Promise) into
  `makeServe`.
- Wire the three-overload signature through `buildNode`.
- Export `ServeOptions` and `ServeHandle`.

### `packages/iroh-http-node/lib.ts`

- Update `rawServe` adapter: the `_options` parameter stays ignored (the Rust
  layer has no per-serve options today), but the type should narrow to `unknown`
  rather than `Record<string, unknown>` so future Rust options can be added
  without a breaking change at the adapter boundary.
- Add a `stopServe` wrapper that calls the new `napiStopServe(endpointHandle)`
  napi binding and pass it to `makeServe`.

### `packages/iroh-http-node/src/lib.rs`

- Add `#[napi] pub fn stop_serve(endpoint_handle: u32)` — calls
  `ep.serve_handle.lock().unwrap().as_ref().map(|h| h.shutdown())`.

### `packages/iroh-http-deno/src/adapter.ts` (and Tauri equivalent)

- Same: add a `stopServe` wrapper that calls `"stopServe"` via the JSON
  dispatch channel and pass it to `makeServe`.

### `packages/iroh-http-deno/src/dispatch.rs`

- Add `"stopServe"` arm to the dispatch match: lock the endpoint's
  `serve_handle` and call `h.shutdown()`.

---

## What we are NOT doing

- **`port` / `hostname`** — not applicable to iroh-http.
- **`reusePort`** — not applicable; the QUIC socket is bound once in
  `createNode`.
- **`ServeHandle.shutdown()`** — a method on the JS handle that mirrors `signal`
  abort is possible but redundant: callers can use
  `const ac = new AbortController(); serve({ signal: ac.signal }); ac.abort()`.
  Omit for now to keep the surface small.

---

## Call-site before / after

```ts
// Before — everyone must write this boilerplate
node.serve({}, async (req) => {
  return Response.json({ ok: true });
});

// After — all three are equivalent
node.serve(async (req) => Response.json({ ok: true }));

node.serve({}, async (req) => Response.json({ ok: true }));

node.serve({
  onListen: ({ nodeId }) => console.log(`listening on ${nodeId}`),
  onError: (err) => new Response(`internal error: ${err}`, { status: 500 }),
  signal: controller.signal,  // abort to stop the serve loop gracefully
  handler: async (req) => Response.json({ ok: true }),
});

// Return value
const server = node.serve(handler);
await server.finished; // resolves when node closes
```
