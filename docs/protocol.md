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
| **fetch() input** | Full `httpi://` URL | `"httpi://b5ea.../api/data"` |

When you call `node.fetch("httpi://<peer-public-key>/api/data")`, the peer identity and path are encoded together in one URL — just like regular `fetch`. The response's `.url` property contains the same `httpi://` URL reflecting the remote peer's identity.

A legacy two-argument form `node.fetch(peer, "/path")` is also accepted for backwards compatibility, but the URL form is preferred.

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
  │                                    │
```

### ALPN identifiers

The QUIC connection uses ALPN (Application-Layer Protocol Negotiation) to identify the protocol version:

| ALPN | Meaning |
|------|---------|
| `iroh-http/2` | Standard request/response (current) |
| `iroh-http/2-duplex` | Raw bidirectional stream via HTTP Upgrade (`raw_connect`) |

The version number changed from 1 to 2 when the wire format migrated from custom framing to standard HTTP/1.1 over QUIC. Old and new builds refuse to connect to each other — the ALPN mismatch is intentional and is the correct way to signal breaking wire-format changes. See [internals/wire-format.md](internals/wire-format.md) for details.

### Request identity

Every request carries the peer's authenticated identity from the QUIC connection. The header `Peer-Id` is injected by the server-side framing layer (and any client-supplied `Peer-Id` header is stripped for security). Handlers can trust `req.headers.get("Peer-Id")` as the authenticated caller identity.

### Bidirectional streaming

Requests with `Upgrade: iroh-duplex` initiate a full-duplex stream. The server responds with `101 Switching Protocols`, and both sides can read and write independently. This is exposed as `session.createBidirectionalStream()` on the client (where `session` is the return value of `node.connect(peer)`) and `req.upgrade()` on the server.

---

## Standards compliance

iroh-http is built on standards wherever they provide the right abstraction. Where it deviates, it does so deliberately and for documented reasons. The goal is that the *developer-facing API* feels entirely standard; the *transport* is where the differences live.

### Where we comply

| Standard | Where it applies | Notes |
|----------|-----------------|-------|
| HTTP/1.1 semantics (RFC 7230–7235) | All requests and responses | Methods, status codes, headers, chunked encoding — delegated entirely to hyper v1 |
| WHATWG Fetch API | `node.fetch()` | The `fetch()` contract is the API contract; deviations are bugs |
| Deno.serve contract | `node.serve()` | Handler signature, `onListen`, `signal` shutdown, `onError` — all follow Deno.serve exactly |
| WHATWG `Request` / `Response` / `Headers` / `ReadableStream` | All platform adapters | Native platform types are used throughout; no custom wrappers |
| WHATWG WebTransport API | `IrohSession` | `IrohSession` satisfies the full `WebTransport` interface: `ready`, `closed`, `datagrams`, `createBidirectionalStream()`, `incomingBidirectionalStreams`, etc. |
| HTTP Upgrade (RFC 7230 §6.7) | `raw_connect` / duplex streams | Standard `Upgrade: iroh-duplex` + `101 Switching Protocols` handshake via hyper |
| HTTP compression negotiation | Compression | `Accept-Encoding` / `Content-Encoding` / `Vary` headers follow standard HTTP negotiation rules, including quality value (`q=`) preference ordering |

### Where we deviate, and why

| Area | Standard | Our approach | Reason |
|------|----------|-------------|--------|
| Transport | TCP | QUIC via Iroh | QUIC provides multiplexing, 0-RTT, flow control, and NAT traversal that TCP cannot. The deviation is the whole point of the library. |
| TLS / identity | Certificate-based TLS (CA-signed) | Keypair-based encryption built into QUIC | No certificate infrastructure needed. Identity is a permanent Ed25519 keypair, not a certificate with an expiry and a CA chain. Iroh handles the encryption. |
| Addressing | DNS hostname | Ed25519 public key (base32) | Nodes are addressed by cryptographic identity, not by DNS name. No servers required. |
| URL scheme | `http://` / `https://` | `httpi://` | `http://` would be misleading (no TCP, no DNS). `http+iroh://` breaks some URL parsers (the `+` is not universally accepted). `httpi://` is clean, parseable everywhere, and signals the distinction. |
| ALPN | `http/1.1`, `h2` | `iroh-http/2`, `iroh-http/2-duplex` | The protocol runs over iroh's QUIC, not standard TLS. Custom ALPN strings correctly identify the iroh-http wire format and prevent accidental connection to unrelated services. |
| Connection model | Persistent TCP connection reused across requests (keep-alive) | One HTTP/1.1 exchange per QUIC stream | QUIC provides multiplexing at the connection level; HTTP keep-alive is unnecessary and disabled. Multiple concurrent requests share one QUIC connection via separate streams. |
| WebTransport wire negotiation | WHATWG WebTransport spec: HTTP/3 QUIC streams or HTTP/2 extended CONNECT (RFC 8441) | HTTP/1.1 `Upgrade: iroh-duplex` → `101 Switching Protocols` | iroh-http currently runs HTTP/1.1 over QUIC streams, not HTTP/3. The `IrohSession` API is spec-compliant; the wire negotiation differs. When HTTP/3 support arrives (`h3-noq`), this will converge with the spec. See [roadmap.md](roadmap.md#horizon-3----embedded-and-http3). |
| Compression algorithm policy | Web convention: gzip, brotli, zstd | zstd only | Both sides of an iroh-http connection run this library, so there is no legacy browser or CDN to support. zstd consistently outperforms gzip and brotli in compression ratio and speed. Starting without legacy baggage, we chose the best available algorithm. Negotiation mechanics (headers, quality values) remain standard HTTP. |
