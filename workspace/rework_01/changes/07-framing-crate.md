# Change 07 — iroh-http-framing: new role and cleanup

## Risk: Low — the crate is kept; its role changes

## Problem

`iroh-http-framing` was the host-side HTTP framing implementation for the old
wire format. After change 01 (hyper), the host path no longer uses it for I/O.

The crate currently has:
- `encode_chunk` / `parse_chunk_header` — custom chunked encoding
- `push_hex_usize` / `usize_to_hex` — manual hex nibble loop
- `serialize_trailers` / `parse_trailers` — hand-rolled byte scanner
- `terminal_chunk` / `terminal_chunk_start` — sentinel bytes
- Zero dependencies (everything is hand-rolled)
- `#![no_std]` + `extern crate alloc`

## Solution

The crate is kept but repurposed. It becomes:

1. **The reference wire-format implementation** — the canonical source of
   truth for how the protocol wire format works, intended for embedded
   implementations to conform against
2. **The conformance test vector source** — byte-exact encode/decode pairs
   that any reimplementation must match
3. **A future embedded building block** — once embedded QUIC/Iroh support
   matures, this crate is the starting point for an embedded HTTP layer

### What changes in the crate

**Add `httparse` for the hand-rolled byte scanner:**

```toml
# iroh-http-framing/Cargo.toml
[dependencies]
httparse = { version = "1", default-features = false }
```

`default-features = false` is what enables `no_std` in httparse. Keep
`#![no_std]` and `extern crate alloc`.

Replace `parse_trailers` with httparse:

```rust
pub fn parse_trailers(bytes: &[u8]) -> Result<(Vec<(String, String)>, usize), FramingError> {
    if bytes.starts_with(b"\r\n") {
        return Ok((Vec::new(), 2));
    }
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(FramingError::Incomplete)?;

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let status = httparse::parse_headers(&bytes[..end + 4], &mut headers)
        .map_err(|e| FramingError::Parse(e.to_string()))?;
    let consumed = match status {
        httparse::Status::Complete((n, _)) => n,
        httparse::Status::Partial => return Err(FramingError::Incomplete),
    };
    let pairs = headers
        .iter()
        .take_while(|h| !h.name.is_empty())
        .map(|h| {
            let name = h.name.to_ascii_lowercase();
            let value = core::str::from_utf8(h.value)
                .map_err(|_| FramingError::Parse("trailer value not UTF-8".into()))?
                .trim()
                .to_string();
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, FramingError>>()?;
    Ok((pairs, consumed))
}
```

**Replace the nibble loop with `core::fmt::Write`:**

```rust
fn push_hex_usize(buf: &mut alloc::vec::Vec<u8>, n: usize) {
    use core::fmt::Write as _;
    let mut s = alloc::string::String::new();
    core::fmt::write(&mut s, format_args!("{:x}", n)).ok();
    buf.extend_from_slice(s.as_bytes());
}
```

Note: use `core::fmt::Write`, **not** `std::io::Write` — the former is
available in `no_std` via `alloc::string::String`'s `Write` impl.

**Add conformance tests:**

```rust
#[cfg(test)]
mod tests {
    // Existing round-trip tests remain unchanged.

    // Add golden test vectors — byte-exact expected outputs:
    #[test]
    fn chunk_encoding_golden() {
        assert_eq!(
            encode_chunk(b"hello"),
            b"5\r\nhello\r\n"
        );
    }

    #[test]
    fn terminal_chunk_golden() {
        assert_eq!(terminal_chunk(), b"0\r\n\r\n");
    }

    #[test]
    fn trailer_serialization_golden() {
        let out = serialize_trailers(&[("x-foo", "bar"), ("x-baz", "qux")]);
        assert_eq!(out, b"x-foo: bar\r\nx-baz: qux\r\n\r\n");
    }

    #[test]
    fn trailer_parse_golden() {
        let input = b"x-foo: bar\r\nx-baz: qux\r\n\r\n";
        let (pairs, consumed) = parse_trailers(input).unwrap();
        assert_eq!(consumed, input.len());
        assert_eq!(pairs, vec![
            ("x-foo".into(), "bar".into()),
            ("x-baz".into(), "qux".into()),
        ]);
    }
}
```

**Add a fuzz target:**

```rust
// crates/iroh-http-framing/fuzz/fuzz_targets/parse_trailers.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = iroh_http_framing::parse_trailers(data);
    let _ = iroh_http_framing::parse_chunk_header(data);
});
```

Run: `cargo +nightly fuzz run parse_trailers -- -max_total_time=60`

### What does NOT change

- `#![no_std]` stays
- All public function signatures stay
- `encode_chunk`, `parse_chunk_header`, `serialize_trailers`,
  `terminal_chunk`, `terminal_chunk_start` — all stay
- The crate name and version stay

### What the crate description should say

Update `Cargo.toml`:
```toml
description = "iroh-http wire-format reference implementation (no_std). \
               Implements HTTP/1.1 chunked body encoding and HTTP trailer \
               serialization. Used as the conformance specification for \
               embedded backend implementations."
```

## Files changed

| File | Change |
|---|---|
| `iroh-http-framing/Cargo.toml` | Add `httparse = { version = "1", default-features = false }`; update description |
| `iroh-http-framing/src/lib.rs` | Replace `parse_trailers` with httparse; replace nibble loop with `core::fmt::Write`; add golden tests |
| `iroh-http-framing/fuzz/` | New fuzz target directory |

## Validation

```
cargo test -p iroh-http-framing
cargo +nightly fuzz run parse_trailers -- -max_total_time=60
```

All existing tests must pass. Golden tests must pass. Fuzz must not crash.
