---
status: integrated
---

# iroh-http — Patch 02: Deno FFI

This document specifies the Deno native platform target. It is self-contained
and independent of the changes in patch 01. Python bindings are covered
separately in patch 03.

---

## 1. Deno FFI (`iroh-http-deno`)

### Problem

`iroh-http-node` uses napi-rs and therefore only runs inside Node.js. Deno has
its own native-module system — **Deno FFI** (`Deno.dlopen`) — that loads
arbitrary C-ABI shared libraries. There is no napi layer. The same core Rust
logic can be exposed as a `cdylib` with a minimal C entry point and loaded
directly inside Deno.

### Design

#### Package layout

```
packages/iroh-http-deno/
├── Cargo.toml             # crate-type = ["cdylib"], deps on iroh-http-core
├── src/
│   ├── lib.rs             # iroh_http_call C entry point + global Tokio runtime
│   ├── dispatch.rs        # method name → iroh-http-core call
│   └── serve_registry.rs  # per-endpoint request queue for the polling serve model
├── deno.jsonc             # deno tasks: build, build:all, build:debug
├── scripts/
│   ├── build-native.mts   # build for the current platform, output to lib/
│   └── build-all.mts      # cross-compile to all 5 platforms (zigbuild + mingw)
├── lib/                   # (git-ignored) compiled platform libraries
└── guest-ts/
    ├── adapter.ts         # DenoAdapter — wraps Deno.dlopen, implements Bridge
    └── mod.ts             # createNode() public export
```

#### C ABI

A single dispatcher function, matching the pattern established by the
`iroh-deno` reference implementation (`.old_references/iroh-deno`):

```
i32 iroh_http_call(
    method_ptr: *const u8,   // UTF-8 method name
    method_len: usize,
    payload_ptr: *const u8,  // JSON payload bytes
    payload_len: usize,
    out_ptr: *mut u8,        // caller-allocated output buffer
    out_cap: usize
)
```

Return value:
- `>= 0` — bytes written to `out_ptr`.
- `< 0` — `-required_size`; caller must retry with a larger buffer.

All inputs and outputs are JSON-encoded. The output is always of the shape
`{ ok: T } | { err: string }`. All dispatch methods are `async` on the Rust
side; the FFI symbol is declared `nonblocking: true` in Deno, meaning the
call returns a `Promise<i32>`.

#### Dispatch methods

| Method | Payload | Response |
|---|---|---|
| `createEndpoint` | `{ key?: number[], idleTimeout?: number, relays?: string[] }` | `{ endpointHandle: u32, nodeId: string, keypair: number[] }` |
| `closeEndpoint` | `{ endpointHandle: u32 }` | `{}` |
| `allocBodyWriter` | `{}` | `{ handle: u32 }` |
| `nextChunk` | `{ handle: u32 }` | `{ chunk: number[] \| null }` |
| `sendChunk` | `{ handle: u32, chunk: number[] }` | `{}` |
| `finishBody` | `{ handle: u32 }` | `{}` |
| `cancelRequest` | `{ handle: u32 }` | `{}` |
| `nextTrailer` | `{ handle: u32 }` | `{ trailers: [string, string][] \| null }` |
| `sendTrailers` | `{ handle: u32, trailers: [string, string][] }` | `{}` |
| `rawFetch` | `{ endpointHandle, nodeId, url, method, headers, reqBodyHandle? }` | `{ status, headers, bodyHandle, url, trailersHandle }` |
| `serveStart` | `{ endpointHandle: u32 }` | `{}` |
| `nextRequest` | `{ endpointHandle: u32 }` | `RequestPayload \| null` |
| `respond` | `{ reqHandle: u32, status: u16, headers: [string, string][] }` | `{}` |

#### Serve polling model

napi-rs supports threadsafe callbacks from Rust back into JS. Deno FFI does
not. Instead, `rawServe` is implemented as a polling loop on the TypeScript
side:

1. `serveStart(endpointHandle)` — Rust starts the accept loop via
   `iroh_http_core::serve`. Each incoming request is serialised to JSON and
   pushed into a per-endpoint `mpsc` channel stored in a global slab.
2. `nextRequest(endpointHandle)` — Rust awaits the next item from the channel.
   The call is `nonblocking: true`, so Deno awaits a `Promise`. Returns `null`
   when the endpoint is closed.
3. The TypeScript adapter runs a `while (true)` loop calling `nextRequest`,
   dispatching each request to the user handler in the background (without
   `await`), then calling `respond` when the handler's promise resolves.

The body streaming handles (read/write) work identically to the napi version
since they are global slabs in `iroh-http-core`.

#### Platform library naming

Filename convention: `lib/libiroh_http_deno.{os}-{arch}.{ext}`

| Platform | Filename |
|---|---|
| macOS arm64 | `libiroh_http_deno.darwin-aarch64.dylib` |
| macOS x86_64 | `libiroh_http_deno.darwin-x86_64.dylib` |
| Linux x86_64 | `libiroh_http_deno.linux-x86_64.so` |
| Linux arm64 | `libiroh_http_deno.linux-aarch64.so` |
| Windows x64 | `libiroh_http_deno.windows-x86_64.dll` |

`adapter.ts` resolves the correct file at load time from `Deno.build.os` /
`Deno.build.arch`. The path is resolved relative to `import.meta.url` so the
library is found regardless of working directory.

#### Cross-compilation

`build-all.mts` targets all five platforms from a single macOS host:

- macOS targets → plain `cargo`
- Linux targets → `cargo-zigbuild` (Zig provides a C linker for glibc targets
  without Docker)
- Windows x64 → `cargo` with the `x86_64-pc-windows-gnu` toolchain and
  `mingw-w64`

Prerequisites (macOS host):

```sh
rustup target add \
  aarch64-apple-darwin x86_64-apple-darwin \
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
  x86_64-pc-windows-gnu
cargo install cargo-zigbuild
brew install zig mingw-w64
```

#### Changes required

| Layer | Change |
|---|---|
| Rust workspace | Add `packages/iroh-http-deno` to `workspace.members` |
| `iroh-http-deno` (Rust) | New `cdylib` crate: `lib.rs` + `dispatch.rs` + `serve_registry.rs` |
| `iroh-http-deno` (TS) | `guest-ts/adapter.ts` (`DenoAdapter`), `guest-ts/mod.ts` (`createNode`) |
| Build scripts | `deno.jsonc`, `scripts/build-native.mts`, `scripts/build-all.mts` |
| `iroh-http-shared` | No changes — the Bridge interface and all helpers are reused unchanged |

### Problem

Python has no equivalent of napi or Deno FFI that bridges to an async Rust
runtime with low ceremony. The standard solution is **PyO3**, which generates
a Python extension module (a `.so` / `.pyd`) directly from Rust. The
**maturin** build tool handles packaging: it compiles the Rust crate, wraps
it as a wheel, and publishes to PyPI.

The Python API mirrors the JS surface as closely as Python idiom allows:
`asyncio`-native for `create_node` and `fetch`; sync-registered callback for
`serve` (the handler is an `async def` Python coroutine that runs on the
caller's event loop).

### Design

#### Package layout

```
packages/iroh-http-py/
├── Cargo.toml             # crate-type = ["cdylib"], pyo3 + pyo3-async-runtimes
├── pyproject.toml         # maturin build backend, package metadata
├── src/
│   └── lib.rs             # #[pymodule] with create_node, IrohNode, IrohResponse
└── iroh_http/
    ├── __init__.py        # re-exports + typed stubs for IDE support
    └── py.typed           # PEP 561 marker
```

#### Python API

```python
import asyncio
from iroh_http import create_node

async def main():
    node = await create_node()           # or create_node(key=bytes, idle_timeout=5000)
    print(node.node_id)                  # str — base32 public key
    print(node.keypair)                  # bytes — 32-byte secret key

    # HTTP fetch to a remote peer
    response = await node.fetch(peer_id, "httpi://peer/api/data")
    body = await response.bytes()        # bytes
    text = await response.text()         # str (UTF-8)

    # HTTP server — handler is an async def coroutine
    async def handler(request):
        body = await request.body()
        return {"status": 200, "headers": [], "body": b"hello"}

    node.serve(handler)                  # non-blocking; starts background task
    await node.close()

asyncio.run(main())
```

#### Rust class design

```
#[pyclass]  IrohNode
  node_id: str
  keypair: bytes (32)
  async fn fetch(peer_id, url, method, headers, body) -> IrohResponse
  fn serve(handler: PyObject)           # registers async callback
  async fn close()

#[pyclass]  IrohRequest                 # passed to serve handler
  method: str
  url: str
  headers: list[tuple[str,str]]
  remote_node_id: str
  async fn body() -> bytes              # reads full body

#[pyclass]  IrohResponse                # returned by fetch
  status: int
  headers: list[tuple[str,str]]
  url: str
  async fn bytes() -> bytes
  async fn text() -> str
  async fn json() -> Any
```

#### Async bridge

`pyo3-async-runtimes` (the maintained successor to `pyo3-asyncio`) with the
`tokio-runtime` feature provides two primitives:

- `pyo3_async_runtimes::tokio::future_into_py(py, future)` — wraps a Rust
  `Future` as a Python coroutine.
- `pyo3_async_runtimes::tokio::into_future(coroutine)` — converts a Python
  coroutine into a Rust `Future` so the Tokio runtime can await it.

`create_node` and `IrohNode.fetch` use `future_into_py`. The Tokio runtime is
initialised once via `OnceLock` on first call.

#### Serve callback model

`IrohNode.serve(handler)`:

1. Calls `iroh_http_core::serve(ep, opts, rust_callback)`.
2. For each incoming request, `rust_callback` is invoked from a Tokio task.
3. Inside `rust_callback`, the Python GIL is acquired; the Rust `RequestPayload`
   is converted to an `IrohRequest` pyobject; the Python handler coroutine is
   obtained by calling `handler(request)`.
4. The coroutine is then driven to completion via
   `pyo3_async_runtimes::tokio::into_future`, yielding a Python dict
   `{ status, headers, body }` that is mapped back to `respond()`.

This means the Python handler coroutine runs on the Tokio thread pool, not on
the Python event loop. For CPU-heavy handlers, `asyncio.to_thread` inside the
handler is still available. I/O-bound handlers work without any changes.

#### Build and distribution

`maturin` is the standard tool for compiling and packaging PyO3 extensions:

```sh
# Development: build an importable .so in the current virtualenv
pip install maturin
maturin develop

# Build a wheel for the current platform
maturin build --release

# Cross-compile with maturin (requires zig for Linux cross)
maturin build --release --target x86_64-unknown-linux-gnu
```

`pyproject.toml` specifies `maturin` as the build backend, the module name
`iroh_http_py` (the `.so` produced by Rust), and package metadata.
`iroh_http/__init__.py` imports `from .iroh_http_py import *` to present a
clean top-level namespace.

#### Dependencies

```toml
[dependencies]
iroh-http-core = { path = "../../crates/iroh-http-core" }
pyo3            = { version = "0.22", features = ["extension-module", "abi3-py39"] }
pyo3-async-runtimes = { version = "0.22", features = ["tokio-runtime"] }
tokio           = { version = "1", features = ["rt-multi-thread"] }
serde_json      = "1"
bytes           = "1"
```

`abi3-py39` enables the stable ABI so a single wheel works across Python 3.9+.

#### Changes required

| Layer | Change |
|---|---|
| Rust workspace | Add `packages/iroh-http-py` to `workspace.members` |
| `iroh-http-py` (Rust) | New `cdylib` crate: `src/lib.rs` with PyO3 module |
| `iroh-http-py` (Python) | `iroh_http/__init__.py` + `py.typed` |
| Build | `pyproject.toml` with maturin backend |
| `iroh-http-core` | No changes required |
| `iroh-http-shared` | Not used — Python has its own thin wrapper layer in Rust |
