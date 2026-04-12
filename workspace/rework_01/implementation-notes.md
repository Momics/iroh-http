# Implementation Notes — Critical Path Details

These notes cover patterns and decisions that are not obvious from the change
documents alone. A developer working through changes 01-07 should read this
first.

---

## 1. Threading `remote_node_id` into `RequestService`

### The problem

The peer's identity is a QUIC connection-level fact, not an HTTP request-level
fact. `RequestService` is a `tower::Service<Request<Incoming>>` — it only
sees the HTTP request. But `RequestPayload` requires `remote_node_id`.

### Current code

In `server.rs`, the accept loop extracts `remote_node_id` at the connection
level:

```rust
// Line 260 — inside handle_connection(), after accepting the QUIC connection
let remote_id = base32_encode(conn.remote_id().as_bytes());
```

This is then threaded through `dispatch_request(... remote_node_id ...)` as a
plain function argument and placed directly into `RequestPayload`.

### After rework

`RequestService` must receive `remote_node_id` before `serve_connection` is
called. The simplest approach: **clone a fresh `RequestService` per-connection
with the peer identity baked in**. This is natural because `RequestService`
is `Clone` and `serve_connection` takes ownership.

```rust
// Accept loop — after extracting the connection
let remote_id = base32_encode(conn.remote_id().as_bytes());

// Clone the base service and inject the peer identity
let mut peer_svc = base_svc.clone();
peer_svc.remote_node_id = Some(remote_id);

tokio::spawn(async move {
    let _guard = guard;
    let io = IrohStream::new(send, recv);
    hyper::server::conn::http1::Builder::new()
        .keep_alive(false)
        .serve_connection(
            hyper_util::rt::TokioIo::new(io),
            peer_svc,  // <-- has remote_node_id
        )
        .with_upgrades()
        .await;
});
```

Update `RequestService`:

```rust
#[derive(Clone)]
struct RequestService {
    on_request: Arc<dyn Fn(RequestPayload) + Send + Sync>,
    ep_idx: u32,
    remote_node_id: Option<String>,  // set per-connection before serve_connection
    #[cfg(feature = "compression")]
    compression: Option<CompressionOptions>,
}
```

Inside `RequestService::call`, use `self.remote_node_id.clone().unwrap()` to
populate `RequestPayload.remote_node_id`.

**Alternative**: use `http::Request::extensions_mut()` to inject the peer ID
as a typed extension. This is a common tower pattern but adds a layer of
indirection. The per-connection clone is simpler for our use case.

---

## 2. `pump_hyper_body_to_channel` — bridging hyper body to existing channels

### The problem

Change 01 references `pump_hyper_body_to_channel(body, body_writer, trailer_tx)`
but doesn't define it. This function is the critical bridge between hyper's
`Incoming` body type and the existing `BodyWriter` / `TrailerTx` channels.

### Implementation

hyper v1 bodies produce `http_body::Frame<Bytes>` values. Each frame is
either `Frame::data(Bytes)` or `Frame::trailers(HeaderMap)`.

```rust
use http_body_util::BodyExt;

/// Drain a hyper body into the existing BodyWriter channel, delivering
/// trailers via the oneshot when the body ends.
async fn pump_hyper_body_to_channel(
    body: hyper::body::Incoming,
    writer: BodyWriter,
    trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
) {
    let mut body = body;
    let mut trailers = Vec::new();

    while let Some(frame_result) = body.frame().await {
        match frame_result {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    if writer.send_chunk(data.clone()).await.is_err() {
                        // Reader side dropped — stop pumping
                        return;
                    }
                } else if let Ok(hdrs) = frame.into_trailers() {
                    // Convert HeaderMap to Vec<(String, String)> for FFI
                    trailers = hdrs
                        .iter()
                        .map(|(k, v)| {
                            (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())
                        })
                        .collect();
                }
            }
            Err(_) => break,
        }
    }

    // Signal body completion by dropping the writer
    drop(writer);

    // Deliver trailers (empty vec if none received)
    let _ = trailer_tx.send(trailers);
}
```

### Inverse: channel to hyper body (for sending)

When sending a request or response body through hyper, the existing
`BodyReader` channel must be adapted into an `http_body::Body`. Use
`http_body_util::StreamBody` wrapping a `futures::Stream` of `Frame<Bytes>`:

```rust
use http_body_util::StreamBody;
use http_body::Frame;

/// Adapt a BodyReader + optional trailers into a hyper-compatible body.
fn body_from_channel(
    reader: BodyReader,
    trailer_rx: Option<oneshot::Receiver<Vec<(String, String)>>>,
) -> StreamBody<impl Stream<Item = Result<Frame<Bytes>, Infallible>>> {
    let stream = async_stream::stream! {
        // Yield data frames
        loop {
            match reader.next_chunk().await {
                Some(data) => yield Ok(Frame::data(data)),
                None => break,
            }
        }
        // Yield trailers if available
        if let Some(rx) = trailer_rx {
            if let Ok(trailers) = rx.await {
                let mut map = HeaderMap::new();
                for (k, v) in trailers {
                    if let (Ok(name), Ok(val)) = (
                        HeaderName::from_bytes(k.as_bytes()),
                        HeaderValue::from_str(&v),
                    ) {
                        map.insert(name, val);
                    }
                }
                if !map.is_empty() {
                    yield Ok(Frame::trailers(map));
                }
            }
        }
    };
    StreamBody::new(stream)
}
```

---

## 3. `CoreError` migration scope

### What changes

The `CoreError` / `ErrorCode` enum replaces the current pattern where every
function returns `Result<T, String>` and errors are classified after the fact
by `classify_error_code()` (string matching on error messages).

### Functions that change signature

All public FFI-facing functions in `iroh-http-core` change from
`Result<T, String>` to `Result<T, CoreError>`:

**`stream.rs`:**
- `next_chunk(handle: u32) -> Result<Option<Bytes>, CoreError>`
- `send_chunk(handle: u32, chunk: Bytes) -> Result<(), CoreError>`
- `finish_body(handle: u32) -> Result<(), CoreError>`
- `send_trailers(handle: u32, trailers: ...) -> Result<(), CoreError>`
- `next_trailer(handle: u32) -> Result<Option<...>, CoreError>`

**`client.rs`:**
- `fetch(endpoint, method, url, headers, ...) -> Result<FfiResponse, CoreError>`
- `raw_connect(endpoint, node_id, path, ...) -> Result<FfiDuplexStream, CoreError>`

**`server.rs`:**
- `respond(req_handle, status, headers) -> Result<(), CoreError>`
- `serve(endpoint, options, on_request) -> ServeHandle` (unchanged — errors
  are logged/counted, not returned)

**`lib.rs`:**
- `parse_node_addr(s: &str) -> Result<ParsedNodeAddr, CoreError>`
- **Delete** `classify_error_code()` and `classify_error_json()`

**`endpoint.rs`:**
- `IrohEndpoint::bind(opts) -> Result<IrohEndpoint, CoreError>`

### Adapter impact

Each adapter currently calls `classify_error_json()` or pattern-matches on
the `String` error. After the migration:

**Node.js (napi-rs)** — `packages/iroh-http-node/src/lib.rs`:
- Replace `classify_error_json(e)` calls with `CoreError::from(e)` or direct
  construction.
- Implement `From<CoreError> for napi::Error`.

**Deno FFI** — `packages/iroh-http-deno/src/lib.rs`:
- Same pattern — replace string classification with `CoreError` conversion.

**Python (PyO3)** — `packages/iroh-http-py/src/lib.rs`:
- Implement `From<CoreError> for PyErr` mapping `ErrorCode` variants to
  Python exception types.

**Tauri** — `packages/iroh-http-tauri/src/lib.rs`:
- Tauri commands can return any `Serialize` error. Either serialize `CoreError`
  directly or implement `Into<tauri::Error>`.

### Migration strategy

1. Add `CoreError` and `ErrorCode` to `lib.rs`.
2. Convert internal `Result<T, String>` returns in `stream.rs`, `client.rs`,
   `server.rs` to `Result<T, CoreError>` using helper constructor methods:

   ```rust
   impl CoreError {
       pub fn invalid_handle(handle: u32) -> Self {
           CoreError {
               code: ErrorCode::InvalidInput,
               message: format!("unknown handle: {handle}"),
           }
       }
       pub fn timeout(detail: impl std::fmt::Display) -> Self {
           CoreError {
               code: ErrorCode::Timeout,
               message: detail.to_string(),
           }
       }
       // ... one per ErrorCode variant
   }
   ```

3. Update each adapter to use `From<CoreError>`.
4. Delete `classify_error_code` and `classify_error_json`.

---

## 4. Hyper resource limit configuration

### The problem

The current code enforces limits via custom read loops with byte counting.
After moving to hyper, those custom loops are deleted. The limits must be
re-expressed using hyper's configuration.

### Header size limits

Currently: `endpoint.max_header_size()` (default 64 KB), enforced in the
custom `read_head_qpack` / `read_request_head_qpack` functions.

After: configure on hyper's builder:

```rust
// Server
hyper::server::conn::http1::Builder::new()
    .keep_alive(false)
    .max_buf_size(max_header_size)  // max header block bytes
    .max_headers(128)               // max number of header lines
    .serve_connection(io, service)
    .with_upgrades()
    .await?;

// Client
hyper::client::conn::http1::Builder::new()
    .keep_alive(false)
    .max_buf_size(max_header_size)
    .max_headers(128)
    .handshake(io)
    .await?;
```

`max_buf_size` controls the maximum bytes hyper will buffer while parsing
headers. This is the direct replacement for the custom `max_header_size`
check.

### Body size limits

Currently: `max_request_body_bytes` in `ServeOptions`, enforced by
`pump_recv_to_body` / `pump_recv_raw_to_body_limited` counting bytes.

After: wrap the body with `http_body_util::Limited`:

```rust
// Server side — limit the incoming request body
use http_body_util::Limited;

let limited_body = Limited::new(body, max_request_body_bytes);
// Pass limited_body to pump_hyper_body_to_channel instead of raw body.
// Limited returns an error when the limit is exceeded, which the pump
// function converts to BodyTooLarge.
```

On the client side, response body limiting follows the same pattern if
needed:

```rust
let limited_body = Limited::new(resp.into_body(), max_response_body_bytes);
```

### Trailer size limits

hyper does not have a built-in trailer size limit. The trailer `HeaderMap`
arrives as an in-memory map after body completion. Two options:

1. **Check after receipt**: in `pump_hyper_body_to_channel`, after receiving
   `Frame::trailers(hdrs)`, check `hdrs.len()` and total byte size. Return
   an error if exceeded.
2. **Rely on header size limit**: since trailers are parsed as part of the
   HTTP message by hyper, `max_buf_size` may already bound them. Verify this
   experimentally and add a test.

Option 1 is the safe choice. Add a `max_trailer_bytes` parameter (default:
same as `max_header_size`).

### Request timeout

Currently: `tokio::time::timeout` wrapping the handler task.

After: `tower::timeout::TimeoutLayer` in the `ServiceBuilder` chain (already
in change 02). No additional work needed.

### Concurrency limit

Currently: `tokio::sync::Semaphore`.

After: `tower::limit::ConcurrencyLimitLayer` in the `ServiceBuilder` chain
(already in change 02). No additional work needed.

---

## 5. Client-side decompression approach

### The problem

Change 03 references `DecompressionLayer` wrapping a `HyperClientService`,
but hyper's client path doesn't naturally use `tower::Service`. The client
uses `hyper::client::conn::http1::SendRequest`, which is a sender-style API,
not a service.

### Recommended approach

**Don't use tower on the client path for decompression**. Instead, decompress
inline after receiving the response:

```rust
let resp = sender.send_request(req).await?;

// Check Content-Encoding and decompress if zstd
let is_zstd = resp.headers()
    .get(http::header::CONTENT_ENCODING)
    .map_or(false, |v| v == "zstd");

if is_zstd {
    // Wrap the body in a decompressing adapter
    let body = resp.into_body();
    let decompressed = ZstdDecompressBody::new(body);
    // Pump decompressed body to BodyWriter channel
    pump_hyper_body_to_channel(decompressed, body_writer, trailer_tx).await;
} else {
    pump_hyper_body_to_channel(resp.into_body(), body_writer, trailer_tx).await;
}
```

`ZstdDecompressBody` can be implemented as a `Body` wrapper using
`async-compression`'s `ZstdDecoder`, or by using `tower-http`'s internal
decompression logic directly.

**Alternative**: if you want to use `DecompressionLayer`, wrap `SendRequest`
in a manual `Service` impl:

```rust
struct HyperClientService {
    sender: hyper::client::conn::http1::SendRequest<BoxBody>,
}

impl Service<Request<BoxBody>> for HyperClientService {
    type Response = Response<Incoming>;
    type Error = hyper::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sender.poll_ready(cx)
    }

    fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
        self.sender.send_request(req)
    }
}
```

Then:
```rust
let svc = tower::ServiceBuilder::new()
    .layer(DecompressionLayer::new().zstd(true).gzip(false).br(false))
    .service(HyperClientService { sender });

let resp = svc.call(req).await?;
```

The inline approach is simpler and avoids introducing tower on the client
path. Use whichever feels cleaner during implementation — both are correct.

---

## 6. `IrohStream` shutdown semantics

The `AsyncWrite::poll_shutdown` implementation on `IrohStream` **must** call
`SendStream::finish()` to signal end-of-stream to the remote peer. Without
this, hyper's response completion will not properly close the QUIC stream:

```rust
impl tokio::io::AsyncWrite for IrohStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().send).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().send).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // This calls SendStream::finish() which sends a FIN on the QUIC stream
        Pin::new(&mut self.get_mut().send).poll_shutdown(cx)
    }
}
```

Verify that `iroh::endpoint::SendStream`'s `AsyncWrite::poll_shutdown`
implementation calls `finish()`. If it doesn't, add an explicit `finish()`
call before shutdown. This is critical — hyper calls `poll_shutdown` when the
response is complete, and the remote side depends on the FIN to know the
response is done.

---

## 7. Response-head rendezvous pattern

### Current pattern

The serve handler and the `respond()` FFI function communicate through a
oneshot channel stored in the slab:

```
1. serve handler receives QUIC request
2. Creates oneshot::channel() → (tx, rx)
3. Stores tx in SlabSet.response_head[req_id]
4. Builds RequestPayload with req_handle = compose_handle(ep_idx, req_id)
5. Calls on_request(payload) — fires the JS callback
6. Awaits rx.await — blocks until JS calls respond()
      ↓
   JS calls respond(req_handle, status, headers)
      ↓
7. respond() decomposes req_handle, removes tx from slab, sends ResponseHeadEntry
      ↓
8. rx.await resolves — serve handler writes headers + body to QUIC stream
```

### After rework

The same pattern lives inside `RequestService::call`. The `call` method
returns a `Future` that resolves to `Response<BoxBody>` — this is where the
rendezvous happens:

```rust
fn call(&mut self, req: Request<Incoming>) -> Self::Future {
    let on_request = self.on_request.clone();
    let ep_idx = self.ep_idx;
    let remote_node_id = self.remote_node_id.clone().unwrap();

    Box::pin(async move {
        // 1. Create response-head oneshot
        let (tx, rx) = oneshot::channel::<ResponseHeadEntry>();

        // 2. Store tx in slab → get req_handle
        let req_handle = allocate_req_handle(ep_idx, tx);

        // 3. Create body channels for request and response
        let (req_body_writer, req_body_reader) = make_body_channel();
        let (res_body_writer, res_body_reader) = make_body_channel();
        let (req_trailer_tx, req_trailer_rx) = oneshot::channel();
        let (res_trailer_tx, res_trailer_rx) = oneshot::channel();

        // 4. Spawn pump: hyper Incoming → req body channel
        let req_body = req.into_body();
        tokio::spawn(pump_hyper_body_to_channel(
            req_body, req_body_writer, req_trailer_tx,
        ));

        // 5. Store handles in slab, build payload
        let req_body_handle = store_reader(ep_idx, req_body_reader);
        let res_body_handle = store_writer(ep_idx, res_body_writer);
        // ... trailer handles ...

        let payload = RequestPayload {
            req_handle,
            req_body_handle,
            res_body_handle,
            // ... trailer handles ...
            method: /* from req */,
            url: /* from req */,
            headers: /* from req */,
            remote_node_id,
            is_bidi: /* from Upgrade header */,
        };

        // 6. Fire callback — JS will eventually call respond()
        on_request(payload);

        // 7. Await response head from JS
        let head = rx.await.map_err(|_| "serve task dropped".to_string())?;

        // 8. Build hyper Response with streaming body from channel
        let body = body_from_channel(res_body_reader, Some(res_trailer_rx));
        let mut response = Response::builder()
            .status(head.status)
            .body(BoxBody::new(body))
            .unwrap();
        for (k, v) in &head.headers {
            response.headers_mut().insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }

        Ok(response)
    })
}
```

The key insight: hyper's `serve_connection` drives the `Service::call` future.
The future parks on `rx.await` until JS calls `respond()`. hyper is unaware
of this — it just sees an async service that takes time to produce a response.
