# iroh-http-deno review findings (2026-04-13)

## Finding 1 (packages/iroh-http-deno/src/adapter.ts:121-122) [added]
[P1] Shared buffer in nonblocking `nextChunk` can corrupt concurrent body reads

`iroh_http_next_chunk` is registered as `nonblocking: true`, but `bridge.nextChunk` reuses a single module-global `chunkBuf`. Two concurrent calls can write into the same memory region at the same time, so one stream can receive bytes from another stream. This is the same class of race previously fixed for `call()` output buffers.

## Finding 2 (packages/iroh-http-deno/src/serve_registry.rs:20-23) [added]
[P1] Serve polling queue cannot naturally terminate, leaving stuck `nextRequest` waits

`ServeQueue` stores both a `Sender` and `Receiver` in the same shared object. `next_request` awaits `recv()`, but because that same queue always owns a live sender, the channel never closes and `recv()` cannot return `None`. `stopServe` also does not remove/close the queue. This can leave the Rust future parked indefinitely after shutdown/close paths.

## Finding 3 (packages/iroh-http-deno/src/dispatch.rs:212-218) [added]
[P2] Compression options are effectively ignored unless `minBodyBytes` is set

The adapter sends both `compressionLevel` and `compressionMinBodyBytes`, but dispatch enables compression only when `compression_min_body_bytes.is_some()`, and `compression_level` is never used. As a result, `compression: true` and `compression: { level: ... }` are no-ops, which diverges from the API contract.

## Finding 4 (packages/iroh-http-deno/README.md:38-42) [added]
[P3] README example uses stale `drainTimeout` shape

The README shows `drainTimeout` as a top-level `createNode` option, but the actual API places it under `advanced.drainTimeout`. Copy-pasting the documented snippet silently does nothing for timeout tuning.
