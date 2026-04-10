# Protocol

iroh-http implements HTTP/1.1 semantics over Iroh's QUIC transport. This document covers the wire format, URL scheme, and protocol details that are common across all platform packages.

## URL scheme: `httpi://`

iroh-http uses the `httpi://` URL scheme. The host part is the peer's public key encoded in base32:

```
httpi://<public-key>/path?query=value
```

For example:

```
httpi://b5ea4f3c2a7b9d1e8f6c0a3b5d7e9f1a2c4b6d8e0f1a3c5b7d9e1f3a5c7d9e/api/data?format=json
```

### How it works

| Context | URL format | Example |
|---------|-----------|---------|
| **Server side** (`req.url`) | `httpi://<own-public-key>/path` | `httpi://b5ea.../api/data` |
| **Client side** (`res.url`) | `httpi://<remote-public-key>/path` | `httpi://b5ea.../api/data` |
| **fetch() input** | Just the path | `"/api/data"` |

When you call `node.fetch(peer, "/api/data")`, you pass just the path — the library handles connecting to the peer by public key. The response's `.url` property contains the full `httpi://` URL reflecting the remote peer's identity.

On the server side, `req.url` is a full `httpi://` URL with the server's own public key. You can route by pathname using `new URL(req.url).pathname`.

### Why not `http://` or `http+iroh://`?

- **`http://`** would be misleading — this isn't TCP-based HTTP and there's no DNS lookup.
- **`http+iroh://`** caused problems — the `+` character is not accepted by some URL parsers (including Node.js and browser WHATWG parsers).
- **`httpi://`** is clean, parseable everywhere, and immediately signals "HTTP-like protocol."

### Compatibility with `Request`/`Response`

The web-standard `Request` constructor only accepts `http:` and `https:` URLs. iroh-http handles this internally: when constructing a `Request` object for your serve handler, the scheme is transparently normalized to `http:`. The original `httpi://` URL is always available via `payload.url` if needed. The `Response.url` on the client side preserves the `httpi://` form.

## Wire format

iroh-http uses standard HTTP/1.1 framing (request line, headers, body) over QUIC bidirectional streams. Each HTTP request opens a new QUIC stream within a single connection to the peer.

```
Client                              Server
  │                                    │
  ├──── QUIC bidi stream ─────────────►│
  │  GET /api/data HTTP/1.1\r\n       │
  │  Host: <peer-key>\r\n             │
  │  \r\n                              │
  │                                    │
  │◄──────────────────────────────────┤
  │  HTTP/1.1 200 OK\r\n              │
  │  Content-Type: application/json\r\n│
  │  Transfer-Encoding: chunked\r\n   │
  │  \r\n                              │
  │  <chunked body>                    │
  │  <trailers>                        │
  │                                    │
```

### ALPN identifiers

The QUIC connection uses ALPN (Application-Layer Protocol Negotiation) to agree on capabilities:

| ALPN | Meaning |
|------|---------|
| `iroh-http/1` | Basic request/response |
| `iroh-http/1+duplex` | Bidirectional streaming |
| `iroh-http/1+trailers` | Response trailers |
| `iroh-http/1+full` | All features |

### Request identity

Every request carries the peer's authenticated identity from the QUIC connection. The header `iroh-node-id` is injected by the server-side framing layer (and any client-supplied `iroh-node-id` header is stripped for security). Handlers can trust `req.headers.get("iroh-node-id")` as the authenticated caller identity.

### Bidirectional streaming

Requests with `Upgrade: iroh-duplex` initiate a full-duplex stream. The server responds with `101 Switching Protocols`, and both sides can read and write independently. This is exposed as `node.createBidirectionalStream()` on the client and `req.acceptWebTransport()` on the server.

### Trailers

Response trailers are sent after the chunked body using standard HTTP/1.1 trailer encoding. The client accesses them via the non-standard `res.trailers` promise. On the server side, attach a `trailers()` function to the `Response` object.
