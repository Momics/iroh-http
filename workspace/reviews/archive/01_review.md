---
status: integrated
---

# iroh-http — Code Review (Patch 01 Follow-up)

## Finding 1
**File:** `crates/iroh-http-framing/src/lib.rs:54-145`  
**Severity:** P0  
**Title:** Framing parser rejects its own serialized protocol version

`serialize_request_head`/`serialize_response_head` emit `Iroh-HTTP/1`, but parsing is delegated to `httparse`, which expects HTTP versions and rejects this token. This is confirmed by failing unit tests (`invalid HTTP version`). As-is, request/response head round-trips are broken.

## Finding 2
**File:** `crates/iroh-http-core/src/server.rs:226-242`  
**Severity:** P1  
**Title:** Body framing can contradict headers when Content-Length is set

Both client and server always pump body as chunked in non-duplex mode, but header serialization suppresses `Transfer-Encoding: chunked` when `Content-Length` is present. That creates header/body mismatch and protocol corruption for callers that set `Content-Length`.

## Finding 3
**File:** `packages/iroh-http-tauri/build.rs:2-12`  
**Severity:** P1  
**Title:** Tauri permissions not updated for new commands

The plugin now exposes `cancel_request`, `next_trailer`, `send_trailers`, and `raw_connect`, but the permission generation list/default permissions only include older commands. In default setups these new commands can be denied at runtime.

## Finding 4
**File:** `packages/iroh-http-shared/src/fetch.ts:109-114`  
**Severity:** P1  
**Title:** `Response.trailers` is re-executed on each property access

`trailers` is defined via a getter that calls `nextTrailer(handle)` every access. Rust `next_trailer` is single-use (removes handle), so second access can error with invalid-handle behavior. It should cache one promise/value per response.

## Finding 5
**File:** `packages/iroh-http-shared/src/fetch.ts:61-80`  
**Severity:** P1  
**Title:** Early AbortSignal does not cancel underlying fetch

Abort before `rawFetch` resolves only wins a `Promise.race`; it rejects JS-side but does not propagate cancellation to transport. `cancelRequest` is only wired after response head arrives, so in-flight request work can continue and leak/straggle.

## Finding 6
**File:** `packages/iroh-http-py/src/lib.rs:187-191`  
**Severity:** P1  
**Title:** Python `fetch` ignores provided request body

Python binding accepts a `body` parameter but intentionally discards it and always calls core fetch with `None` body reader. This causes silent behavioral mismatch: callers think they sent a body, but they did not.
