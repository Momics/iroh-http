# Change 06 — FFI input validation via the http crate

## Risk: Low — additive only, validates before existing logic

## Problem

Every public FFI function accepts raw strings for HTTP method, header names,
header values, and a bare `u16` for status. There is no validation at the
entry point. Invalid inputs propagate silently through hyper and fail deep
inside the connection:

- `method = "GETT"` — not a valid HTTP method token
- `name = "Content Length"` — space in header name, invalid per RFC 7230
- `status = 0` — not a valid HTTP status code (valid range: 100–599)

## Solution

Add `http = "1"` to `iroh-http-core/Cargo.toml` (also needed for change 01
— these can be combined into one Cargo.toml edit).

At the entry point of each FFI-facing function, parse raw inputs into `http`
crate types and return a descriptive `Err(String)` if invalid. The validated
types are used immediately and not surfaced across the FFI boundary.

### client.rs — fetch()

```rust
// After the scheme check and before building the Request:
http::Method::from_bytes(method.as_bytes())
    .map_err(|_| format!("invalid HTTP method {:?}", method))?;

for (name, value) in &headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("invalid header name {:?}", name))?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| format!("invalid header value for {:?}", name))?;
}
```

### server.rs — respond()

```rust
http::StatusCode::from_u16(status)
    .map_err(|_| format!("invalid HTTP status code: {status}"))?;

for (name, value) in &headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("invalid response header name {:?}", name))?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| format!("invalid response header value for {:?}", name))?;
}
```

### Error code taxonomy

Add `INVALID_INPUT` to `classify_error_code` in `lib.rs`:

```rust
// In classify_error_code():
if msg.contains("invalid HTTP method")
    || msg.contains("invalid header")
    || msg.contains("invalid HTTP status")
{
    return "INVALID_INPUT";
}
```

This makes validation failures machine-readable at the platform adapter layer,
consistent with the existing error taxonomy.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `http = "1"` (shared with change 01) |
| `iroh-http-core/src/client.rs` | Validate method and headers in `fetch()` |
| `iroh-http-core/src/server.rs` | Validate status and headers in `respond()` |
| `iroh-http-core/src/lib.rs` | Add `INVALID_INPUT` to `classify_error_code` |

## Tests to add

```rust
// client.rs
#[tokio::test]
async fn fetch_rejects_invalid_method() {
    // GETT is not a valid token — extra T
    let result = fetch(..., "GETT", ...).await;
    assert!(result.unwrap_err().contains("invalid HTTP method"));
}

#[tokio::test]
async fn fetch_rejects_header_name_with_space() {
    // Space in header name — invalid per RFC 7230
    let result = fetch(..., &[("Content Length".into(), "0".into())]).await;
    assert!(result.unwrap_err().contains("invalid header name"));
}

// server.rs
#[tokio::test]
fn respond_rejects_status_zero() {
    let result = respond(handle, 0, vec![]);
    assert!(result.unwrap_err().contains("invalid HTTP status code"));
}

#[tokio::test]
fn respond_rejects_status_600() {
    let result = respond(handle, 600, vec![]);
    assert!(result.unwrap_err().contains("invalid HTTP status code"));
}
```

## Validation

```
cargo test -p iroh-http-core
```

New tests pass. No existing tests break — the validation only rejects inputs
that were already erroneous (would have failed downstream anyway).
