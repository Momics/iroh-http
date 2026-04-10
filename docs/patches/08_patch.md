---
status: integrated
---

# iroh-http — Patch 08: Structured Rust Errors

Replace `Result<T, String>` throughout `iroh-http-core` and all platform
adapters with structured `{ code, message }` error payloads, eliminating
the regex-based error classification on the JS side.

---

## Problem

All Rust functions currently return `Result<T, String>`. The JS
`classifyError` function parses these strings with regexes to determine
which `IrohError` subclass to throw:

```ts
if (/connect|connection|refused|unreachable/i.test(msg))
  return new IrohConnectError(msg);
```

This is fragile — any change to a Rust error message, a new upstream
library version, or a differently-worded OS error can break classification.
It also makes it impossible to distinguish errors that happen to contain the
word "connect" from actual connection errors.

---

## Solution

### Rust error enum

Define a structured error type in `iroh-http-core`:

```rust
// crates/iroh-http-core/src/error.rs

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct IrohError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Endpoint creation / bind failure
    Bind,
    /// Connection to remote peer failed
    Connect,
    /// Stream read/write error during body transfer
    Stream,
    /// Protocol-level error (framing, ALPN mismatch, malformed head)
    Protocol,
    /// Request was aborted by the caller
    Aborted,
    /// Invalid argument passed by the caller
    InvalidArgument,
    /// Handle is invalid or expired (slab cleanup, TTL, double-free)
    InvalidHandle,
    /// Internal error (catch-all for unexpected failures)
    Internal,
}

impl std::fmt::Display for IrohError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for IrohError {}
```

All public functions in `iroh-http-core` change from `Result<T, String>` to
`Result<T, IrohError>`. Migration is mechanical: wrap existing
`.map_err(|e| format!(...))` calls with the appropriate `ErrorCode`.

### JSON serialisation

When errors cross the FFI boundary (all three adapters), they serialize as:

```json
{ "err": { "code": "CONNECT", "message": "connect: connection refused" } }
```

Instead of the current:

```json
{ "err": "connect: connection refused" }
```

### JS-side changes

#### `iroh-http-shared/src/errors.ts`

`classifyError` becomes a simple code-to-class map:

```ts
export function classifyError(raw: { code: string; message: string }): IrohError {
  switch (raw.code) {
    case "BIND":             return new IrohBindError(raw.message);
    case "CONNECT":          return new IrohConnectError(raw.message);
    case "STREAM":           return new IrohStreamError(raw.message);
    case "PROTOCOL":         return new IrohProtocolError(raw.message);
    case "ABORTED":          return new IrohAbortError(raw.message);
    case "INVALID_ARGUMENT": return new IrohArgumentError(raw.message);
    case "INVALID_HANDLE":   return new IrohHandleError(raw.message);
    default:                 return new IrohError(raw.message);
  }
}
```

The regex-based classification is removed entirely. The old function
signature is kept as a deprecated overload for backward compatibility during
the transition:

```ts
/** @deprecated Pass structured `{ code, message }` instead. */
export function classifyError(raw: string): IrohError;
export function classifyError(raw: { code: string; message: string }): IrohError;
export function classifyError(
  raw: string | { code: string; message: string },
): IrohError {
  if (typeof raw === "string") return classifyErrorLegacy(raw);
  // ... switch on raw.code ...
}
```

#### New error class: `IrohAbortError`

Currently fetch abort returns a generic `IrohError("aborted")`. With
structured codes this becomes `IrohAbortError`, which extends `DOMException`
with `name: "AbortError"` so it's compatible with standard `AbortSignal`
error handling:

```ts
export class IrohAbortError extends DOMException {
  constructor(message = "The operation was aborted") {
    super(message, "AbortError");
  }
}
```

#### New error classes: `IrohArgumentError`, `IrohHandleError`

Small additions for the two new codes, both extending `IrohError`.

### Adapter changes

Each adapter's FFI error path changes from passing a raw string to passing
the structured `{ code, message }` object.

**Node (napi-rs):** The `#[napi]` functions currently return `napi::Result`
with string errors. Change to return a JSON-serialized `IrohError` in the
error message field, or use napi's `Error::from_reason` with a prefixed
code. The TS bridge extracts the code from the napi error.

**Tauri:** `commands.rs` currently uses `Result<T, String>`. Change to
`Result<T, tauri::Error>` where the error payload is a serialized
`IrohError`. The `err()` helper becomes:

```rust
fn iroh_err(e: IrohError) -> tauri::Error {
    tauri::Error::PluginSerialization(serde_json::to_string(&e).unwrap())
}
```

**Deno:** The `call<T>` dispatcher already parses `{ err: ... }`. Change
the Rust side to write `{ "err": { "code": "...", "message": "..." } }`
and the TS side to pass the object to `classifyError`.

---

## Migration strategy

1. Add `error.rs` to `iroh-http-core` with `IrohError` + `ErrorCode`
2. Convert `iroh-http-core` functions from `Result<T, String>` to
   `Result<T, IrohError>` (mechanical — ~30 call sites)
3. Update adapters to serialize the structured error
4. Update `classifyError` with the overloaded signature
5. Add `IrohAbortError`, `IrohArgumentError`, `IrohHandleError`
6. Remove regex patterns from `classifyErrorLegacy` (can keep as fallback
   for one release, then remove)

---

## Error code reference

| Code | When | Current error string (before) |
|------|------|-------------------------------|
| `BIND` | Endpoint creation fails | `"bind: ..."` |
| `CONNECT` | Connection to peer fails | `"connect: ..."` |
| `STREAM` | Body read/write fails | `"body reader dropped"` |
| `PROTOCOL` | Malformed head, ALPN mismatch | `"failed to parse ..."` |
| `ABORTED` | Fetch cancelled via AbortSignal | `"aborted"` |
| `INVALID_ARGUMENT` | Bad input from caller | various |
| `INVALID_HANDLE` | Slab handle expired or invalid | `"invalid reader handle: N"` |
| `INTERNAL` | Unexpected / catch-all | various |
