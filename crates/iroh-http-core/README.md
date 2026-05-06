# iroh-http-core

Rust core for [iroh-http](https://github.com/momics/iroh-http). Runs HTTP/1.1 over [Iroh](https://iroh.computer) QUIC bidirectional streams via [hyper](https://hyper.rs). Nodes are addressed by Ed25519 public key.

> **This is an FFI-bridge crate.** Application developers should use the higher-level adapters:
> - **Node.js** → [`@momics/iroh-http-node`](https://www.npmjs.com/package/@momics/iroh-http-node)
> - **Deno** → [`@momics/iroh-http-deno`](https://jsr.io/@momics/iroh-http-deno)
> - **Tauri** → [`@momics/iroh-http-tauri`](https://www.npmjs.com/package/@momics/iroh-http-tauri)

## Usage

```rust
use iroh_http_core::{
    IrohEndpoint, NodeOptions, ServeOptions,
    fetch, serve, respond,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Bind a local endpoint
    let endpoint = IrohEndpoint::bind(NodeOptions::default()).await?;
    println!("Node ID: {}", endpoint.node_id());

    // Serve incoming requests
    let _handle = serve(endpoint.clone(), ServeOptions::default(), |req| {
        respond(req.req_handle, 200, vec![], None);
    });

    // Fetch from a remote peer (raw FFI-level API)
    let resp = fetch(
        &endpoint,
        remote_node_id,
        "httpi://peer.local/api",
        "GET",
        &[],
        None, None, None, None,
    ).await?;

    Ok(())
}
```

## Features

- **Connection reuse**: QUIC connections to the same peer are pooled and multiplexed
- **Streaming bodies**: request and response bodies stream through `mpsc` channels with configurable backpressure
- **Fetch cancellation**: abort in-flight requests via cancellation tokens
- **Bidirectional streams**: full-duplex streaming via QUIC bidi streams
- **Trailer support**: HTTP/1.1 chunked trailers for streaming metadata
- **Configurable**: idle timeout, concurrency limits, channel capacity, chunk sizes
- **Optional compression**: zstd request/response compression via the `compression` feature (enabled by default)

## License

`MIT OR Apache-2.0`
