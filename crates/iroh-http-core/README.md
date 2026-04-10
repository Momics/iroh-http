# iroh-http-core

Rust core for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over [Iroh](https://iroh.computer) QUIC transport.

This crate provides the transport layer: an Iroh endpoint, `fetch()` for outgoing requests, and `serve()` for incoming requests. It speaks HTTP/1.1 framing over QUIC bidirectional streams. Nodes are addressed by Ed25519 public key — no DNS, no TLS certificates.

Platform adapters (Node.js, Tauri, Deno, Python) build on top of this crate via FFI or native bindings.

## Usage

```rust
use iroh_http_core::{IrohEndpoint, NodeOptions, fetch, serve};

// Create an endpoint
let endpoint = IrohEndpoint::bind(NodeOptions::default()).await?;
println!("Node ID: {}", endpoint.node_id());

// Fetch from a remote peer
let response = fetch(&endpoint, remote_node_id, "/api", "GET", &[], None, None).await?;

// Serve incoming requests
serve(endpoint, ServeOptions::default(), |req| {
    respond(req.req_handle, 200, vec![]);
});
```

## Features

- **Connection reuse** — QUIC connections to the same peer are pooled and multiplexed
- **Streaming bodies** — request and response bodies stream through `mpsc` channels with configurable backpressure
- **Fetch cancellation** — abort in-flight requests via cancellation tokens
- **Bidirectional streams** — full-duplex streaming via QUIC bidi streams
- **Trailer support** — HTTP/1.1 chunked trailers for streaming metadata
- **Configurable** — idle timeout, concurrency limits, channel capacity, chunk sizes

## License

MIT OR Apache-2.0
