# Change 06 — FFI input validation via the http crate

## Risk: Low — additive only, validates before existing logic

## Problem

Every public FFI function accepts raw strings for HTTP method, header names,
header values, and a bare `u16` for status. There is no validation at the
entry point. Invalid inputs propagate silently through hyper and fail deep
inside the connection:

- `method = "BAD METHOD"` — invalid token syntax (space)
- `name = "Content Length"` — space in header name, invalid per RFC 7230
- `status = 0` — not a valid HTTP status code (valid range: 100–599)

## Solution

Two changes:

1. Add `http = "1"` to `iroh-http-core/Cargo.toml` (also needed for change 01
   — these can be combined into one Cargo.toml edit).
2. Introduce a typed `CoreError` with an `ErrorCode` enum. Since the package
   is unreleased, there is no reason to keep string-matching error classification.

### ErrorCode enum

```rust
/// Machine-readable error codes for the FFI boundary.
///
/// Platform adapters match on this directly — no string parsing needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidInput,
    ConnectionFailed,
    Timeout,
    BodyTooLarge,
    HeaderTooLarge,
    PeerRejected,
    Cancelled,
    Internal,
}

/// Structured error returned by core functions.
///
/// `code` is machine-readable. `message` carries human-readable detail.
pub struct CoreError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for CoreError {}
```

The existing `classify_error_code` function and all its string-matching
branches are **deleted**. Errors are created with the correct code at the
point of origin.

### FFI input validation

At the entry point of each FFI-facing function, parse raw inputs into `http`
crate types and return a `CoreError` with `ErrorCode::InvalidInput` if
invalid. The validated types are used immediately and not surfaced across the
FFI boundary.

Notes:

- Extension methods are allowed if token syntax is valid.
- This is complementary to hyper parsing; we validate early at FFI boundaries
  so callers get stable, immediate errors.
- Protocol-level scheme policy (`httpi://` vs `http://`/`https://`) remains an
  explicit check in our API layer and is not delegated to hyper.

### client.rs — fetch()

```rust
// After the scheme check and before building the Request:
let method = http::Method::from_bytes(method.as_bytes())
    .map_err(|_| CoreError {
        code: ErrorCode::InvalidInput,
        message: format!("invalid HTTP method {:?}", method),
    })?;

for (name, value) in &headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| CoreError {
            code: ErrorCode::InvalidInput,
            message: format!("invalid header name {:?}", name),
        })?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| CoreError {
            code: ErrorCode::InvalidInput,
            message: format!("invalid header value for {:?}", name),
        })?;
}
```

### server.rs — respond()

```rust
let status = http::StatusCode::from_u16(status)
    .map_err(|_| CoreError {
        code: ErrorCode::InvalidInput,
        message: format!("invalid HTTP status code: {status}"),
    })?;

for (name, value) in &headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| CoreError {
            code: ErrorCode::InvalidInput,
            message: format!("invalid response header name {:?}", name),
        })?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| CoreError {
            code: ErrorCode::InvalidInput,
            message: format!("invalid response header value for {:?}", name),
        })?;
}
```

### Adapter mapping

Platform adapters convert `CoreError` to their native error type:

```rust
// Node.js (napi-rs)
impl From<CoreError> for napi::Error {
    fn from(e: CoreError) -> Self {
        napi::Error::new(napi::Status::GenericFailure, e.to_string())
    }
}

// Python (PyO3)
impl From<CoreError> for PyErr {
    fn from(e: CoreError) -> Self {
        match e.code {
            ErrorCode::InvalidInput => PyValueError::new_err(e.message),
            ErrorCode::Timeout => PyTimeoutError::new_err(e.message),
            _ => PyRuntimeError::new_err(e.message),
        }
    }
}
```

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `http = "1"` (shared with change 01) |
| `iroh-http-core/src/lib.rs` | Add `CoreError`, `ErrorCode` enum; **delete** `classify_error_code` |
| `iroh-http-core/src/client.rs` | Validate method and headers in `fetch()`, return `CoreError` |
| `iroh-http-core/src/server.rs` | Validate status and headers in `respond()`, return `CoreError` |

## Tests to add

```rust
// client.rs
#[tokio::test]
async fn fetch_rejects_invalid_method() {
    let result = fetch(..., "BAD METHOD", ...).await;
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("invalid HTTP method"));
}

#[tokio::test]
async fn fetch_rejects_header_name_with_space() {
    let result = fetch(..., &[("Content Length".into(), "0".into())]).await;
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("invalid header name"));
}

// server.rs
#[tokio::test]
fn respond_rejects_status_zero() {
    let err = respond(handle, 0, vec![]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("invalid HTTP status code"));
}

#[tokio::test]
fn respond_rejects_status_600() {
    let err = respond(handle, 600, vec![]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("invalid HTTP status code"));
}
```

## Validation

```
cargo test -p iroh-http-core
```

New tests pass. No existing tests break — the validation only rejects inputs
that were already erroneous (would have failed downstream anyway).
