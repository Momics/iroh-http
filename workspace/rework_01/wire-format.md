# Wire Format Change and ALPN Versioning

## Current wire format

```
Request:
  [2-byte BE length][QPACK-encoded block: :method, :path, headers]
  [HTTP/1.1 chunked body: <hex>\r\n<data>\r\n ... 0\r\n[trailers]\r\n]

Response:
  [2-byte BE length][QPACK-encoded block: :status, headers]
  [HTTP/1.1 chunked body: <hex>\r\n<data>\r\n ... 0\r\n[trailers]\r\n]
```

Header compression is stateless QPACK (`qpack = "0.1"`). The dynamic table
was never enabled — the codebase comment explicitly states:
> "The `qpack` crate v0.1.0 does not publicly export Encoder/Decoder, so true
> dynamic-table compression is not yet available."

This means the current QPACK usage provides no compression benefit over
plain text headers. It only adds a custom encoding layer.

## New wire format (after rework)

Standard HTTP/1.1 over a raw QUIC bidirectional stream:

```
Request:
  GET /path HTTP/1.1\r\n
  Host: <node-id>\r\n
  <headers>\r\n
  \r\n
  [HTTP/1.1 chunked body with standard chunked encoding and trailers]

Response:
  HTTP/1.1 200 OK\r\n
  <headers>\r\n
  \r\n
  [HTTP/1.1 chunked body with standard chunked encoding and trailers]
```

This is byte-for-byte standard HTTP/1.1. Any HTTP/1.1 parser can decode it.

## Why this is a clean break, not a problem

The package has not been released. No external code depends on the wire
format. Version compatibility is a non-issue.

The ALPN strings are the version identifier on the wire. Old and new builds
will refuse to connect to each other because the ALPN won't match. This is
the correct behaviour.

## ALPN versioning

Current ALPN strings (must be retired):
```
b"iroh-http/1"
b"iroh-http/1-duplex"
b"iroh-http/1-trailers"
b"iroh-http/1-full"
```

New ALPN strings (proposed):
```
b"iroh-http/2"
b"iroh-http/2-duplex"
```

Notes:
- The `-trailers` and `-full` variants collapse into the base ALPN because
  hyper supports trailers natively — they are no longer optional feature
  negotiation points.
- The `-duplex` variant is preserved because `raw_connect` (`Upgrade: iroh-duplex`)
  uses a separate ALPN for capability advertisement. hyper's `upgrade::on`
  handles the 101 handshake identically.
- Version `2` reflects the wire format change, not an HTTP version.

## Duplex / raw_connect wire format

The duplex mode uses standard HTTP Upgrade semantics:

```
Client sends:
  CONNECT /path HTTP/1.1\r\n
  Upgrade: iroh-duplex\r\n
  <extra headers>\r\n
  \r\n

Server replies:
  HTTP/1.1 101 Switching Protocols\r\n
  \r\n

After 101: raw bidirectional byte stream (no HTTP framing)
```

This matches exactly what the current implementation does. hyper's
`hyper::upgrade` module handles the 101 handshake; after upgrade,
`hyper::upgrade::Upgraded` gives back the raw IO, which is then
split into the existing `BodyReader`/`BodyWriter` channel handles.

## Conformance and embedded future

`iroh-http-framing` is kept as a crate. It now serves as:
- The reference implementation of the wire format for embedded targets
- The source of golden test vectors (byte-exact encode/decode round-trips)
- Documentation of the wire protocol invariants

The host-side no longer uses it for I/O. An embedded implementation would
implement the same protocol independently and validate against the same
test vectors.
