# Wire Format

iroh-http uses standard HTTP/1.1 framing over raw QUIC bidirectional streams. This document describes the wire encoding, ALPN versioning, and the duplex upgrade protocol.

---

## Wire encoding

Each QUIC bidirectional stream carries exactly one HTTP/1.1 request-response exchange:

```
Client → Server (request):
  GET /path HTTP/1.1\r\n
  Host: <node-id>\r\n
  <headers>\r\n
  \r\n
  [chunked body: <hex-len>\r\n<data>\r\n … 0\r\n\r\n]
  [optional trailers in final chunk header]

Server → Client (response):
  HTTP/1.1 200 OK\r\n
  <headers>\r\n
  \r\n
  [chunked body: <hex-len>\r\n<data>\r\n … 0\r\n\r\n]
  [optional trailers in final chunk header]
```

This is byte-for-byte standard HTTP/1.1. Any conforming HTTP/1.1 parser can decode it.

hyper v1 handles all framing, chunked encoding, trailer delivery, and header parsing. iroh-http-core contributes no custom framing code.

---

## ALPN strings

ALPN is the version identifier on the wire. Old and new builds that use different ALPN strings will refuse to connect to each other — this is the correct behaviour.

| ALPN | Used for |
|------|----------|
| `b"iroh-http/2"` | Regular HTTP requests (`fetch`) |
| `b"iroh-http/2-duplex"` | Duplex / raw_connect (`Upgrade: iroh-duplex`) |

The version number `2` reflects the wire format change from the original custom QPACK-prefixed encoding, not the HTTP version. The protocol is still HTTP/1.1.

### Retired ALPNs (version 1)

These ALPNs are no longer supported. Any endpoint still using version 1 wire format will fail to negotiate with a version 2 endpoint:

```
b"iroh-http/1"
b"iroh-http/1-duplex"
b"iroh-http/1-trailers"
b"iroh-http/1-full"
```

The `-trailers` and `-full` variants from version 1 no longer exist because hyper supports trailers natively — they are not optional negotiation points.

---

## Trailer support

Trailers are carried in the final chunk of a chunked-encoded HTTP/1.1 body.

For trailers to be delivered in HTTP/1.1, the client **must** include `TE: trailers` in the request. iroh-http-core adds this header automatically on all outgoing `fetch()` calls.

The server declares which trailers to expect by including a `Trailer: <header-name>` response header (added by the JS handler via `respond()`).

---

## Duplex wire format

The duplex mode (`raw_connect`) uses standard HTTP Upgrade:

```
Client → Server:
  CONNECT /path HTTP/1.1\r\n
  Upgrade: iroh-duplex\r\n
  <extra headers>\r\n
  \r\n

Server → Client:
  HTTP/1.1 101 Switching Protocols\r\n
  Upgrade: iroh-duplex\r\n
  \r\n

After 101: raw bidirectional byte stream (no HTTP framing)
```

After the 101 handshake, the QUIC stream becomes a raw pipe. Both sides can read and write freely. iroh-http-core wires the raw IO into the `BodyReader`/`BodyWriter` handle system so JS can use `nextChunk`/`sendChunk` to exchange data.

---

## Previous wire format (version 1)

For historical reference, the version 1 format used custom framing:

```
Request:
  [2-byte big-endian length][QPACK-encoded block: :method, :path, headers]
  [HTTP/1.1 chunked body]

Response:
  [2-byte big-endian length][QPACK-encoded block: :status, headers]
  [HTTP/1.1 chunked body]
```

The QPACK header encoding was stateless (dynamic table never enabled). Without dynamic table support there is no cross-request compression and the overhead of the custom encoding layer outweighs any static-table gain from QPACK's built-in static table. Version 2 replaces this with plain HTTP/1.1 headers.

---

## Conformance

Protocol conformance for version 2 is defined by:

1. This document
2. Integration tests in `crates/iroh-http-core/tests/integration.rs`
3. The security tests (`response_header_bomb_rejected`, `header_bomb_rejected`, `body_exceeds_limit_resets_stream`)

There is no separate framing crate in the active host path. `iroh-http-framing` is not used at runtime.
