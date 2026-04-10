# iroh-http-framing

`no_std` HTTP/1.1 header serialization and parsing for [iroh-http](https://github.com/momics/iroh-http).

This crate handles the wire format only — serialize and parse request/response heads, chunked body encoding, and trailers. No async, no I/O, no networking. Uses [`httparse`](https://crates.io/crates/httparse) internally.

Designed to be usable on embedded targets (ESP32, bare-metal) and in any language with a QUIC transport. If two peers agree on this wire format, they interoperate — regardless of platform.

## Usage

```rust
use iroh_http_framing::{serialize_request_head, parse_request_head};

// Serialize
let bytes = serialize_request_head("GET", "/api/data", &[("host", "peer1")], false);

// Parse
let (method, path, headers, consumed) = parse_request_head(&bytes).unwrap();
assert_eq!(method, "GET");
assert_eq!(path, "/api/data");
```

## Features

- **`no_std`** — only requires `alloc`
- **Request/response head** — serialize and parse HTTP/1.1 status lines + headers
- **Chunked encoding** — encode/decode chunked transfer bodies
- **Trailers** — serialize and parse trailer headers after chunked bodies
- **ALPN constants** — protocol identifiers for capability negotiation

## License

MIT OR Apache-2.0
