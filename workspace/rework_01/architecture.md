# Architecture — Before and After

## Before (current)

```
┌─────────────────────────────────────────────────────────┐
│  Platform adapters  (napi-rs · PyO3 · Deno FFI)         │
│  unchanged — consume pub FFI functions only              │
└───────────────────────────┬─────────────────────────────┘
                            │ pub fn fetch / respond / next_chunk / …
┌───────────────────────────▼─────────────────────────────┐
│  iroh-http-core                                          │
│                                                          │
│  client.rs      — custom request framing, pump loops     │
│  server.rs      — custom accept loop, response framing   │
│  qpack_bridge.rs— custom QPACK header encode/decode      │
│  compress.rs    — custom streaming zstd (255 lines)      │
│  pool.rs        — custom Slot enum + watch channel pool  │
│  stream.rs      — HashMap<u32,T> + AtomicU32 slabs       │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  iroh-http-framing   (#![no_std])                        │
│  custom chunked encoding, custom trailer byte scanner    │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  Iroh 0.96  (iroh-quinn → Quinn 0.11)                    │
│  SendStream / RecvStream                                 │
└─────────────────────────────────────────────────────────┘
```

---

## After (rework_01)

```
┌─────────────────────────────────────────────────────────┐
│  Platform adapters  (napi-rs · PyO3 · Deno FFI)         │
│  unchanged                                              │
└───────────────────────────┬─────────────────────────────┘
                            │ pub fn fetch / respond / next_chunk / …
┌───────────────────────────▼─────────────────────────────┐
│  iroh-http-core  (thin integration layer)                │
│                                                          │
│  client.rs    — connect, build hyper Request, FFI glue   │
│  server.rs    — accept, dispatch hyper Request, FFI glue │
│  pool.rs      — cache-backed single-flight strategy       │
│  stream.rs    — existing handle model + hardening         │
│  endpoint.rs  — IrohEndpoint, ServeOptions (unchanged)   │
└──────┬──────────────────────────────┬────────────────────┘
       │                              │
┌──────▼───────┐          ┌──────────▼───────────────────┐
│  tower-http  │          │  hyper v1                     │
│              │          │                               │
│  Compression │          │  HTTP/1.1 framing             │
│  Layer       │          │  Header parsing               │
│  Decompression          │  Chunked encoding             │
│  Layer       │          │  Trailer support              │
│  (zstd only) │          │  Upgrade / duplex handshake   │
│              │          │  Body streaming               │
└──────────────┘          └──────────┬────────────────────┘
                                     │
┌────────────────────────────────────▼────────────────────┐
│  Iroh 0.96  SendStream / RecvStream                      │
│  (implements AsyncWrite / AsyncRead — hyper drives       │
│   them directly via hyper's IO trait)                   │
└─────────────────────────────────────────────────────────┘
```

---

## Lines of custom code eliminated

| File | Current | After |
|---|---|---|
| `iroh-http-framing/src/lib.rs` | ~300 lines custom | Removed/deprecated from active host runtime path |
| `qpack_bridge.rs` | ~150 lines custom | **Deleted** — hyper handles headers as standard HTTP/1.1 |
| `compress.rs` | ~255 lines custom | **Deleted** — replaced by `tower-http` CompressionLayer |
| `client.rs` (pump loops, framing) | ~500 lines | ~150 lines (connect + FFI glue only) |
| `server.rs` (accept loop, framing) | ~310 lines | ~150 lines (accept + dispatch only) |
| `stream.rs` (handles/channels) | ~450 lines | Preserved initially, hardened with explicit guardrails |
| `pool.rs` (Slot + watch) | ~240 lines | Replaced by cache-backed single-flight implementation |

Total reduction: roughly **1,400 lines of custom Rust replaced by well-maintained crates**.

---

## Dependency additions

| Crate | Version | Purpose |
|---|---|---|
| `hyper` | `1` | HTTP/1.1 engine |
| `hyper-util` | `0.1` | `TokioIo` adapter (wraps AsyncRead/AsyncWrite for hyper's IO trait) |
| `http` | `1` | Type-safe method, header, status validation |
| `http-body-util` | `0.1` | Body combinators (`StreamBody`, `BodyExt`) |
| `tower` | `0.5` | Service trait + `ServiceBuilder` |
| `tower-http` | `0.6` | `CompressionLayer`, `DecompressionLayer` |
| `moka` | `0.12` | Async cache + single-flight primitive for pool |

Already in workspace (no new additions):
| Crate | Already present |
|---|---|
| `tokio` | workspace dep |
| `bytes` | workspace dep |

Removed:
| Crate | Removed because |
|---|---|
| `qpack` | Header encoding replaced by hyper |
| `async-compression` | Compression replaced by tower-http |

---

## Future: h3 upgrade path

When Iroh exposes its underlying `quinn::Connection` directly (currently
wrapped but not exported), plugging in `h3` + `h3-quinn` would mean:

1. Swapping `hyper_util::server::conn::auto` for `h3::server::Connection`
2. The `tower::Service` layer and all application logic are unchanged

Nothing in this rework closes that door.
