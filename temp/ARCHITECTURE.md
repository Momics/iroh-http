# Architecture Notes

This document captures current implementation decisions, their rationale, and open questions. Unlike `PRINCIPLES.md`, this document is **not stable** — it should evolve as the codebase evolves. Treat its contents as *current best-guesses that can be revisited with justification*, not as invariants.

When a decision here is revisited and changed, update this document. A wrong or outdated architecture doc is worse than no doc.

---

## What This Library Is

An HTTP implementation over [Iroh](https://iroh.computer/) sockets, exposed to Deno, Node, Tauri, and Python via an FFI bridge.

The **Rust core** is responsible for exactly one thing: correct, reliable HTTP transport over Iroh. Nothing else. It is not a framework. It does not contain middleware, retry logic, auth helpers, or request interceptors. Those belong in userland.

The **exposed API surface** is:
- `fetch` — implementing the [WHATWG Fetch specification](https://fetch.spec.whatwg.org/)
- `serve` — implementing the [Deno.serve](https://docs.deno.com/api/deno/~/Deno.serve) contract

Both APIs must behave exactly as a JS or Python developer expects based on those contracts. Any deviation from spec behavior is a bug unless explicitly documented as an intentional, justified exception.

---

## Stack

| Concern | Current Solution | Notes |
|---|---|---|
| HTTP | `hyper` | Core HTTP/1.1 and HTTP/2 |
| Async runtime | `tokio` | Standard for this stack |
| Transport | Iroh sockets | Custom `AsyncRead`/`AsyncWrite` impl required |
| TLS | TBD | Iroh likely handles encryption at the transport layer; verify before adding a separate TLS layer |
| Compression | In core (Rust) | `Accept-Encoding` negotiation and body encoding belong in Rust, not userland — see Compression section |
| WebTransport | In core via HTTP upgrade | Negotiated as HTTP upgrade; session lifecycle managed in Rust — see WebTransport section |
| Middleware composition | `tower` | Preferred model for timeouts at the transport level |
| FFI | TBD per target | See FFI section below |

---

## Transport: Iroh Integration

Iroh provides the underlying socket/connection primitive. To use it with Hyper, Iroh connections must implement the traits Hyper expects for async I/O.

**Current approach:** implement `hyper::rt::Read` and `hyper::rt::Write` (or `tokio::io::AsyncRead`/`AsyncWrite`) for the Iroh connection type, then pass it into Hyper's connection handling directly.

**Key question to revisit:** Can `hyper-util`'s built-in client and connection pooling be used with the Iroh transport by implementing the right traits? If yes, that is strongly preferred over any custom pooling solution. Custom pools are custom bugs.

---

## Compression

Compression negotiation belongs in the Rust core, not in userland. This is protocol-level behavior — the core reads `Accept-Encoding` from the request, decides whether the response body is compressible, applies encoding, and sets `Content-Encoding` and `Vary` headers accordingly. A JS caller receives an already-encoded response body and has no opportunity to do this correctly after the fact.

This is consistent with how Deno handles it: compression lives entirely inside the Rust layer.

**Compression must be applied when all of the following are true:**
- The request includes an `Accept-Encoding` header indicating support for `gzip` or `br` (brotli)
- The response `Content-Type` is considered compressible (text, JSON, etc. — not images or already-compressed formats)
- The response body exceeds a minimum size threshold (Deno uses 64 bytes; below this, compression overhead exceeds benefit)

**Compression must be skipped when:**
- The response already has a `Content-Encoding` header (userland has done its own encoding)
- The response has a `Content-Range` header (range requests have externally negotiated byte ranges)
- The response has `Cache-Control: no-transform`
- The response body is a stream (streaming compression requires different handling — treat as a separate feature)

**Quality value preference** in `Accept-Encoding` must be respected (e.g. `br;q=1.0, gzip;q=0.8` prefers brotli).

---

## WebTransport

WebTransport is exposed via the HTTP upgrade mechanism. The upgrade handshake and session lifecycle must be managed in Rust — the JS side receives a `WebTransport`-like interface after the upgrade is complete.

Since Iroh is QUIC-based, this is a natural fit. WebTransport over HTTP/2 uses extended CONNECT; over HTTP/3 it uses QUIC streams directly. The appropriate path depends on how Iroh sessions are negotiated.

**The core is responsible for:**
- Handling the HTTP upgrade handshake
- Managing the WebTransport session lifecycle (open, streams, close)
- Exposing bidirectional and unidirectional stream primitives across the FFI boundary

**Userland is responsible for:**
- Application-level protocol over the streams
- Stream multiplexing logic specific to the application

> ⚠️ Open question: does the Iroh transport support the extended CONNECT mechanism required for WebTransport over HTTP/2, or does this require HTTP/3? Resolve before implementing.

---

## Connection Limits

The core must enforce a configurable maximum number of concurrent connections. This is a safety guarantee — not application policy — because Rust accepts connections before JS/Python has any opportunity to reject them. An unbounded connection count is a latent OOM and DoS vector.

**Required behavior:**
- A hard maximum concurrent connection limit, configurable at construction time (with a documented, conservative default)
- Documented behavior when the limit is hit: new connections should be rejected with a 503 or similar, not silently dropped or queued indefinitely
- The limit must be tested under load, not just asserted

This is distinct from rate limiting, which is application-specific and belongs in userland.

---

## Connection Pooling

> ⚠️ This section describes a custom component. Custom infrastructure requires ongoing justification.

A custom connection pool exists. Before accepting this as permanent, it must answer the following questions. If it cannot answer all four, it should be replaced with an existing solution or fixed.

1. **Capacity** — what is the maximum pool size, and what happens when it is exhausted? Does the caller block, get an error, or get a new connection outside the pool?
2. **Stale connection eviction** — how are dead or half-closed connections detected and removed? Is this tested?
3. **Cancellation safety** — what happens when a checkout future is dropped mid-flight? Is the connection returned, closed, or leaked?
4. **Failure behavior** — is the pool tested under concurrent load and connection failure injection, not just the happy path?

**Justification required:** Document here why `hyper-util`'s pooling, or another existing solution, does not meet the requirements. If no justification exists, the custom pool should be considered a candidate for replacement.

---

## FFI Bridge

The library targets four host environments: Deno, Node, Tauri, and Python. The FFI strategy may differ per target (e.g. Napi for Node, `deno_bindgen` or `Deno.dlopen` for Deno, PyO3 for Python).

**Invariants regardless of FFI strategy:**

- Every FFI entry point must be panic-safe. Panics must be caught at the boundary and converted to an error value. A Rust panic reaching a JS or Python runtime is a hard crash.
- Errors must be represented in a form the calling language can inspect and handle meaningfully. Opaque integers are not acceptable. JS users expect errors that look like `TypeError` or similar native types.
- Memory ownership at every boundary must be documented: who allocates, who frees, and when. This must be documented per function, not assumed.
- The FFI surface should be as small and as stable as possible. Every exposed function is a contract.

**Current FFI surface:** *(document the actual exposed functions here as they are defined)*

---

## Scope Boundaries

These are examples of what belongs in the Rust core vs. what belongs in userland. The test for any ambiguous case: *"Could this be implemented correctly in JS/Python with the information the core already exposes?"* If yes, it stays in userland. If no — because the core has already consumed or acted on the relevant information before JS/Python sees it — it belongs in core.

**Belongs in the Rust core:**
- HTTP connection lifecycle (open, reuse, close)
- Request serialization and response deserialization
- Compression negotiation (`Accept-Encoding` / `Content-Encoding` / `Vary`) — JS sees the body after Rust has read it; compression must happen here
- Connection limits (max concurrent connections) — Rust accepts connections before JS can reject them
- Transport-level timeout enforcement — minimum time-to-headers, half-open connection cleanup
- WebTransport and WebSocket upgrade handshakes — connection state lives in Rust
- Panic safety and error translation at the FFI boundary
- Correct implementation of the Fetch spec and Deno.serve contract

**Belongs in userland (JS/Python):**
- Retry logic and backoff strategies
- Request/response interceptors and middleware
- Authentication and authorization helpers
- Tracing configuration and export (the core emits structured `tracing` spans; userland wires up the subscriber and exporter)
- Logging and metrics
- Caching
- Rate limiting (always application-specific: by IP, by user, by endpoint cost)
- Any behavior that requires knowledge of application semantics

If a feature request would add something from the second list to the Rust core, the correct response is to ensure the core exposes enough information for userland to implement it — not to implement it in the core.

---

## Open Questions

Track unresolved architectural decisions here rather than leaving them as implicit assumptions in the code.

- [ ] Can `hyper-util` pooling be used directly with the Iroh transport via trait impls? (Investigate before treating the custom pool as permanent)
- [ ] What is the TLS story? Does Iroh handle encryption at the transport level, making a separate TLS layer redundant?
- [ ] Does the Iroh transport support extended CONNECT (required for WebTransport over HTTP/2), or does WebTransport require HTTP/3 here?
- [ ] What is the FFI strategy per target — is it unified or per-runtime?
- [ ] How are streaming request/response bodies represented across the FFI boundary?
- [ ] How are WebTransport streams represented across the FFI boundary?
- [ ] What is the error type hierarchy exposed to each target language?
- [ ] What is the documented behavior when the connection limit is hit — 503, silent drop, or configurable?
