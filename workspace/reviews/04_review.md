---
status: reported
source: docs/guidelines.md
date: 2026-04-11
---

# API Guidelines Compliance Audit

Audit target: all first-party crates and packages under `crates/`, `packages/`,
and `examples/` against `docs/guidelines.md`.

---

## Top Findings

### 1. JS/TS public API leaks internal FFI types and handle shapes

Guideline conflict:
- JS/TS section: `FfiRequest` / `FfiResponse` / slab-handle types are internal and
  must not appear in public interface.

Current state:
- `packages/iroh-http-shared/src/index.ts` re-exports `FfiRequest`,
  `FfiResponse`, `RequestPayload`, `EndpointInfo`, `Raw*` function types, and
  `FfiDuplexStream`.
- `packages/iroh-http-shared/src/bridge.ts` defines these as exported types in
  the same module as public node types.

Impact:
- Public API exposes Rust/bridge internals and handle-level concepts that the
  guideline explicitly forbids.

---

### 2. JS/TS error model is custom hierarchy instead of platform-standard shape

Guideline conflict:
- Universal principle 1 (Errors): use DOMException names where applicable
  (`AbortError`, `NetworkError`, `TypeError`) over ad-hoc custom errors.

Current state:
- `packages/iroh-http-shared/src/errors.ts` exports a custom `IrohError`
  hierarchy (`IrohConnectError`, `IrohProtocolError`, etc.) and `classifyError`
  maps transport errors into those classes.

Impact:
- API feel diverges from platform-native web conventions.

---

### 3. Python API diverges from Python section requirements

Guideline conflicts:
- `node.serve()` handler should return an `IrohResponse` value object, not dict.
- Resource-holding classes should support `__aenter__` / `__aexit__`.
- Public package should be fully type-annotated for checkers (with `py.typed`).

Current state:
- `packages/iroh-http-py/src/lib.rs` enforces dict return keys
  (`status`, `headers`, `body`) in `serve`.
- No `__aenter__` / `__aexit__` implementations found.
- `py.typed` exists but there are no `.pyi` stubs and only one Python file
  (`__init__.py`) with no signature annotations for extension members.

Impact:
- Python developer ergonomics and type-checker compatibility do not match the
  documented contract.

---

### 4. Cross-platform API parity drift (Node / Deno / Tauri)

Guideline conflicts:
- Platform-native parity principle: same conceptual JS API surface across Node,
  Deno, and Tauri.

Current state:
- Deno adapter still sends `options.relays` field in `createEndpointInfo`, while
  shared API uses `relayMode`.
- Deno `rawFetch` signature omits `directAddrs`, while shared `RawFetchFn`
  requires it.
- Node and Tauri currently have broken type/build state around this drift.

Impact:
- Inconsistent semantics and compile-time breakage between platforms.

---

### 5. Tauri guest JS file has trailing broken block; package does not typecheck

Current state:
- `packages/iroh-http-tauri/guest-js/index.ts` ends with trailing stray text
  after `export type { NodeOptions, IrohNode };`.
- `npm run typecheck` reports parse errors from this file.

Impact:
- Package is not in a shippable state and blocks confidence in guideline
  conformance.

---

### 6. “Don’t reinvent the wheel” partially violated via duplicated base32 code

Guideline conflict:
- Prefer established standard/crate over custom algorithm implementations unless
  divergence is necessary.

Current state:
- Custom base32 codec exists in:
  - `crates/iroh-http-core/src/lib.rs`
  - `packages/iroh-http-shared/src/keys.ts`

Impact:
- Maintenance burden and duplicated logic across languages.

---

## Package-by-Package Status

- `PASS` `crates/iroh-http-framing`
- `PASS` `crates/iroh-http-discovery`
- `PARTIAL` `crates/iroh-http-core`
- `FAIL` `packages/iroh-http-shared`
- `FAIL` `packages/iroh-http-node`
- `FAIL` `packages/iroh-http-deno`
- `FAIL` `packages/iroh-http-tauri`
- `FAIL` `packages/iroh-http-py`
- `PARTIAL` examples (`examples/node`, `examples/deno`, `examples/tauri`,
  `examples/python`)

Notes on examples:
- Node and Deno examples are largely aligned with `Request/Response` usage.
- Tauri example reads `x-iroh-node-id`; guideline/doc expectation is
  `iroh-node-id`.
- Python example returns dict from handler, matching current implementation but
  diverging from the guideline’s `IrohResponse` return requirement.

---

## Verification Snapshot

Commands run during audit:

1. `npm run typecheck`
- Fails in `packages/iroh-http-node` (signature/type drift).
- Fails in `packages/iroh-http-tauri` (syntax errors in `guest-js/index.ts`).

2. `cargo check --workspace`
- Fails in `packages/iroh-http-node` and `packages/iroh-http-deno`:
  `NodeOptions` initializer missing new `max_header_size` field.

3. `cargo check -p iroh-http-py`
- Fails similarly: `NodeOptions` initializer missing `max_header_size`.

4. `cargo check` (inside `packages/iroh-http-tauri`)
- Fails similarly: `NodeOptions` initializer missing `max_header_size`.

5. `cargo check -p iroh-http-framing`
- Pass.

6. `cargo check -p iroh-http-core`
- Pass.

7. `cargo check -p iroh-http-discovery`
- Pass.

---

## Suggested Next Fix Order

1. Restore build/type health (`max_header_size` plumbing + Tauri TS syntax fix).
2. Remove FFI/internal type leakage from public JS exports.
3. Resolve cross-platform option drift (`relayMode`, `directAddrs`, naming).
4. Align JS error surface with guideline expectations (DOMException-first policy).
5. Rework Python serve/response model and add async context manager support.
6. Evaluate replacing custom base32 implementations with maintained equivalents.
