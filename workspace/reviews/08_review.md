---
status: reported
source: static code scan — crates/ and packages/
date: 2026-04-11
---

# Dead Code & Unused Dependency Scan

Static scan of all Rust crates and packages looking for unreachable code,
unused public symbols, redundant constants, and orphaned files.  No code was
run; findings are based on cross-reference of call sites across the workspace.
Backwards-compatibility is not a concern — nothing has been released.

---

## Tracker

| ID | Finding (short) | Priority | Status |
|----|-----------------|----------|--------|
| R8-01 | `serialize_request_head` / `parse_request_head` never called outside framing | P1 | UNRESOLVED |
| R8-02 | `serialize_response_head` / `parse_response_head` never called outside framing | P1 | UNRESOLVED |
| R8-03 | `httparse` dependency only needed by items R8-01/R8-02 | P1 | UNRESOLVED |
| R8-04 | ALPN constants duplicated between `iroh-http-framing` and `iroh-http-core` | P2 | UNRESOLVED |
| R8-05 | `reason_phrase()` never imported by any crate outside framing | P2 | UNRESOLVED |
| R8-06 | `FfiRequest` struct defined but never constructed in any binding | P2 | UNRESOLVED |
| R8-07 | `session_ready()` is a no-op never called by any binding | P2 | UNRESOLVED |
| R8-08 | `ConnectionPool::remove()` suppressed with `#[allow(dead_code)]` | P2 | UNRESOLVED |
| R8-09 | Stateless QPACK helpers are `pub` inside a `pub(crate)` module but only called within the same file | P3 | UNRESOLVED |
| R8-10 | `add_mdns()` in `iroh-http-discovery` is not called by any binding | P2 | UNRESOLVED |
| R8-11 | `mobile_mdns.rs` is an orphaned stub file — not declared with `mod` in `lib.rs` | P1 | UNRESOLVED |
| R8-12 | `parse_direct_addrs()` copy-pasted identically in three binding crates | P3 | UNRESOLVED |

Status conventions: `RESOLVED` / `PARTIAL` / `UNRESOLVED`

---

## Findings

### 1) P1 — `serialize_request_head` / `parse_request_head` never called outside framing

**Files:**
- `crates/iroh-http-framing/src/lib.rs:44` — `serialize_request_head`
- `crates/iroh-http-framing/src/lib.rs:75` — `parse_request_head`

**Detail:**
`iroh-http-core` uses QPACK (via `qpack_bridge.rs`) for all request/response
head serialisation.  The HTTP/1.1 text-serialisation functions in
`iroh-http-framing` are only referenced in the crate's own doc-example and
`#[cfg(test)]` blocks.  No external crate imports them.

```
grep "serialize_request_head\|parse_request_head" crates/ packages/
→ only matches inside iroh-http-framing/src/lib.rs (doc comment + test)
```

**Recommendation:** Delete `serialize_request_head` and `parse_request_head`.

---

### 2) P1 — `serialize_response_head` / `parse_response_head` never called outside framing

**Files:**
- `crates/iroh-http-framing/src/lib.rs:109` — `serialize_response_head`
- `crates/iroh-http-framing/src/lib.rs:140` — `parse_response_head`

**Detail:**
Same situation as R8-01.  Response heads are also QPACK-encoded; these HTTP/1.1
serialisers are never invoked outside the framing crate itself.

**Recommendation:** Delete `serialize_response_head` and `parse_response_head`.

---

### 3) P1 — `httparse` dependency only needed by dead serialisers

**Files:**
- `crates/iroh-http-framing/Cargo.toml:8`

**Detail:**
`httparse` is the only external dependency in `iroh-http-framing`.  It is used
exclusively by `parse_request_head` and `parse_response_head` (R8-01/R8-02).
If those functions are removed, the crate has zero dependencies and becomes a
thin helper module containing `encode_chunk`, `terminal_chunk*`,
`serialize_trailers`, `parse_trailers`, and `parse_chunk_header` — all of which
are actively used by `iroh-http-core`.

**Recommendation:** Remove `httparse` after deleting R8-01/R8-02.

---

### 4) P2 — ALPN constants duplicated between `iroh-http-framing` and `iroh-http-core`

**Files:**
- `crates/iroh-http-framing/src/lib.rs:262–272` — `ALPN_BASE`, `ALPN_DUPLEX`, `ALPN_TRAILERS`, `ALPN_FULL`
- `crates/iroh-http-core/src/lib.rs` — same constants defined again as `ALPN`, `ALPN_DUPLEX`, `ALPN_TRAILERS`, `ALPN_FULL`

**Detail:**
Both crates define all four ALPN byte-string constants.  The framing versions
are never imported by any other crate — only the core versions are used.

**Recommendation:** Delete the four ALPN constants from `iroh-http-framing`.

---

### 5) P2 — `reason_phrase()` never imported outside framing

**Files:**
- `crates/iroh-http-framing/src/lib.rs:286`

**Detail:**
`reason_phrase` converts a status code to a text string (e.g. `200 → "OK"`).
It is only called inside the framing crate's own `#[cfg(test)]` block.  No
binding or core code imports it.

**Recommendation:** Delete `reason_phrase` (and its tests).

---

### 6) P2 — `FfiRequest` struct defined but never constructed in any binding

**Files:**
- `crates/iroh-http-core/src/lib.rs:117`

**Detail:**
`FfiRequest` is a public struct with `method`, `url`, `headers`, and
`remote_node_id` fields.  All four language bindings (Node, Deno, Python, Tauri)
work with `RequestPayload` instead, which is a superset.  `FfiRequest` is never
instantiated: the single search hit is its own definition.

**Recommendation:** Delete `FfiRequest`.

---

### 7) P2 — `session_ready()` is an unconditional no-op never called by any binding

**Files:**
- `crates/iroh-http-core/src/session.rs:179`
- `crates/iroh-http-core/src/lib.rs:25` (re-exports it)

**Detail:**
`session_ready` immediately returns `Ok(())` — iroh connections are considered
ready as soon as `session_connect` returns.  The comment says it is "Kept for
WebTransport API compatibility", but no binding exposes it: neither Node, Deno,
Tauri, nor Python calls it.  The TypeScript session layer hardcodes
`ready: Promise.resolve(undefined)` without going through the FFI.

**Recommendation:** Delete `session_ready` from `session.rs` and remove it from
the `lib.rs` re-export list.

---

### 8) P2 — `ConnectionPool::remove()` annotated `#[allow(dead_code)]`

**Files:**
- `crates/iroh-http-core/src/pool.rs:165`

**Detail:**
The method exists with an explicit `#[allow(dead_code)]` annotation, meaning the
compiler itself flagged it.  A search of all call sites finds zero usages outside
`pool.rs`.

**Recommendation:** Delete `ConnectionPool::remove()` and the `#[allow(dead_code)]`
suppression.

---

### 9) P3 — Stateless QPACK helpers are `pub` with no external callers

**Files:**
- `crates/iroh-http-core/src/qpack_bridge.rs:25–91`

**Detail:**
The four stateless helpers — `encode_request_stateless`, `encode_response_stateless`,
`decode_request_stateless`, `decode_response_stateless` — are declared `pub`.
The module itself is `pub(crate)`, so they cannot escape the crate, but they are
only ever called from within `QpackCodec`'s own methods in the same file.  Making
them `pub` inside a `pub(crate)` module is misleading and suggests they were
intended for external use that never materialised.

**Recommendation:** Change the four functions from `pub` to `fn` (private).

---

### 10) P2 — `add_mdns()` in `iroh-http-discovery` is not called by any binding

**Files:**
- `crates/iroh-http-discovery/src/lib.rs:38` (`#[cfg(feature = "mdns")]`)
- `crates/iroh-http-discovery/src/lib.rs:56` (`#[cfg(not(feature = "mdns"))]` stub)

**Detail:**
All four language bindings use `start_browse()` and `start_advertise()` for
mDNS.  `add_mdns()` is the older "attach and forget" API; it appears only in the
crate's own doc-comment example and in workspace patch documentation, not in any
binding.

**Recommendation:** Delete `add_mdns()`.

---

### 11) P1 — `mobile_mdns.rs` is a phantom file: never declared in `lib.rs`

**Files:**
- `packages/iroh-http-tauri/src/mobile_mdns.rs`
- `packages/iroh-http-tauri/src/lib.rs` (no `mod mobile_mdns;` line)

**Detail:**
`mobile_mdns.rs` contains only a comment stub describing a future iOS/Android
implementation.  There is no `mod mobile_mdns;` declaration in `lib.rs`, so the
Rust compiler never compiles or links this file — it is completely invisible to
the build.  It cannot contain any logic that is currently active.

**Recommendation:** Delete `mobile_mdns.rs`.

---

### 12) P3 — `parse_direct_addrs()` copy-pasted identically across three binding crates

**Files:**
- `packages/iroh-http-node/src/lib.rs:35`
- `packages/iroh-http-deno/src/dispatch.rs:30`
- `packages/iroh-http-tauri/src/commands.rs:17`

**Detail:**
All three files contain byte-for-byte the same private helper:

```rust
fn parse_direct_addrs(addrs: &Option<Vec<String>>) -> Option<Vec<std::net::SocketAddr>> {
    addrs.as_ref().map(|v| {
        v.iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .collect()
    })
}
```

This is not dead code, but the duplication is worth noting.  A single instance
in `iroh-http-core` (as a `pub(crate)` or `pub` helper) would eliminate the
three copies.

**Recommendation:** Move to `iroh-http-core` and import from there.
