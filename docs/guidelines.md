# iroh-http — API Design Guidelines

These principles govern every public-facing API in this project, across all
supported platforms. Each platform target has its own idiom section, but the
overarching rule is the same everywhere:

> **Make the API feel like it belongs to the platform, not like a Rust crate
> wrapped in glue code.**

---

## Universal Principles

### 1. Web-standard first (JS/TS targets)

For JavaScript and TypeScript targets the gold standard is the **WHATWG Living
Standards** and the existing set of browser/Deno globals. Before introducing
any custom type or method, check whether a standard equivalent already exists.

- **Fetch / Request / Response** — use them as-is. Never invent `IrohRequest`
  or `IrohResponse` in the JS layer; the web types already exist and are
  universally understood.
- **ReadableStream / WritableStream** — use them for all streaming I/O. Never
  expose raw handles or callback-based APIs.
- **AbortSignal / AbortController** — the standard way to cancel async
  operations. Never invent a `cancel()` method alongside `fetch`.
- **Headers** — use the `Headers` class; not a `[string, string][]` array at
  the API boundary.
- **WebTransport naming** — for bidirectional streams use
  `BidirectionalStream`, `createBidirectionalStream`, `closed` promise, etc.
  These names are already established in the WebTransport spec and recognisable
  to any web developer.
- **Errors** — use `DOMException` names (`AbortError`, `NetworkError`,
  `TypeError`) where applicable, not ad-hoc string messages.

### 2. Platform-native feel

The user of this library should be able to read the API at a glance without
consulting internal documentation:

- **Deno**: APIs should be indistinguishable from built-in Deno APIs. The
  `serve()` signature mirrors `Deno.serve`. Stream types use the web globals
  already available in Deno.
- **Node.js**: Use the same ergonomics as the `fetch` global introduced in
  Node 18+. Return types are standard web types, not Node `Buffer` or
  `EventEmitter`.
- **Python**: See the Python section below.
- **Tauri**: The `invoke()` bridge is an implementation detail; the JS-side
  consumer sees the same `createNode` / `node.fetch` / `node.serve` surface
  as Node and Deno. Tauri-specific options (plugin permissions, etc.) are
  handled in platform config, not in the JS API.

### 3. Minimal surface, maximum composability

- Expose the smallest number of primitives needed. Users should be able to
  build higher-level patterns themselves using standard tools.
- Avoid duplicate APIs for the same concept. If `fetch` already handles
  `POST`, do not add a `post()` shorthand.
- Prefer options objects over positional parameters for anything beyond two
  arguments.
- Every async operation should be cancellable via an `AbortSignal` (JS) or
  equivalent platform primitive; build this in from the start, not as an
  afterthought.

### 4. Security by default

- The authenticated remote peer identity (`iroh-node-id`) is always injected
  as a header by the library. It is never spoofable by the remote. User code
  should be able to trust `req.headers.get('iroh-node-id')` without additional
  verification.
- The library never exposes raw QUIC handles or connection objects to user code;
  only web-standard types cross the API boundary.

### 5. Naming conventions

| Concept | JS name | Python name |
|---|---|---|
| Node / endpoint | `createNode()` → `IrohNode` | `create_node()` → `IrohNode` |
| Peer address | `nodeId: string` | `node_id: str` |
| Secret key | `keypair: Uint8Array` | `keypair: bytes` |
| HTTP fetch | `node.fetch(peerId, url, init?)` | `await node.fetch(peer_id, url, ...)` |
| HTTP serve | `node.serve(opts, handler)` | `node.serve(handler)` |
| Duplex stream | `BidirectionalStream` | (future; see patch 04 / 05) |
| Cancel | `AbortSignal` | `asyncio.CancelledError` / `anyio` token |

---

## JavaScript / TypeScript

Follow the WHATWG / Web Platform baseline exactly. Specific rules:

- Public function and method names: `camelCase`.
- Type and interface names: `PascalCase`.
- Never export a thing that requires knowledge of Rust internals to use
  (e.g., handle numbers, slab indices).
- The `FfiRequest` / `FfiResponse` / slab-handle types are **internal**. They
  must not appear in the public interface of any package.
- All async operations return a `Promise`; never mix callbacks and promises.
- `serve()` callbacks receive a standard `Request` and must return a standard
  `Response` (or a Promise thereof). This makes handlers fully portable
  between iroh-http, `Deno.serve`, `fetch` handlers, and any other spec-
  compliant framework.

---

## Python

The Python API should feel like idiomatic `asyncio` code written by a Python
developer — **not** like a JavaScript API translated word-for-word into Python.

- **Naming**: `snake_case` for all functions, methods, and parameters.
  `PascalCase` for classes.
- **Async**: everything that does I/O is `async def`. Users call it with
  `await`; they do not need to know about Tokio or any Rust runtime.
- **Context managers**: classes that hold resources implement
  `__aenter__` / `__aexit__` so they can be used with `async with`.
  `create_node()` should be usable as both `node = await create_node()` and
  `async with await create_node() as node: ...`.
- **Type hints**: all public functions and classes are fully annotated.
  The package ships a `py.typed` marker and inline annotations (not stub
  files) so type checkers work out of the box.
- **Errors**: use standard Python exceptions. `ConnectionError`,
  `TimeoutError`, `ValueError` rather than a custom exception hierarchy
  (unless a finer distinction is genuinely needed by callers).
- **Response body**: `await resp.bytes()`, `await resp.text()`, and
  `await resp.json()` mirror the `httpx` / Starlette convention that Python
  HTTP developers already know.
- **No callback-style serve**: the handler passed to `node.serve()` is always
  an `async def` coroutine function. Sync handlers are not supported; users
  who need sync I/O should use `asyncio.to_thread` inside their handler.
- **Serve return value**: the handler returns an `IrohResponse` value object,
  not a dict. This keeps types checkable and prevents silent key-name bugs.

---

## Embedded / ESP (future)

For resource-constrained targets (e.g. ESP32 via ESP-IDF `std` environment):

- The API must be sync-only — no `async`/`await`, no heap-allocated futures.
- Use the `no_std`-compatible framing crate (`iroh-http-framing`) as the base.
- Expose a blocking C-compatible API from a separate `iroh-http-esp` crate:
  `iroh_http_init()`, `iroh_http_fetch()`, `iroh_http_serve()`.
- Surface a thin Arduino/ESP-IDF wrapper (C++ class or plain C structs) on
  top of the C ABI so the embedded developer never writes Rust directly.
- Memory: no dynamic allocation in hot paths; caller-supplied buffers only.

---

## Evolving this document

These guidelines are a living document. When a new platform target is added,
add a section here describing the naming and idiom conventions for that
platform **before** writing any implementation code. Patches that introduce
new public API symbols should reference the relevant section of this document
in their design rationale.
