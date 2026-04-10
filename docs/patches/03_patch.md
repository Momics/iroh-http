---
status: integrated
---

# iroh-http — Patch 03: Python Bindings

This document specifies `iroh-http-py`, the Python platform target.

---

## Background — why a separate patch?

The JS platform targets (Node, Deno, Tauri) share a single TypeScript bridge
layer (`iroh-http-shared`) and an almost-identical pattern: JS holds opaque
handles, Rust owns all state in global slabs, and the two sides communicate
through a thin FFI or Tauri-invoke boundary.

Python cannot participate in that pattern for two reasons:

1. **No shared bridge layer.** The body-streaming model (`nextChunk`,
   `sendChunk`, `finishBody`) is a JS-specific design. Python has its own
   idiomatic I/O model (`asyncio`, `async for`, `async with`). Mapping the
   handle-based slab protocol into Python would feel foreign. Instead,
   `iroh-http-py` is a direct PyO3 binding — the Python objects hold the
   Rust state directly (no slab indirection needed).

2. **GIL and event-loop considerations.** Python has a GIL; napi and Deno FFI
   do not. Async Rust futures must be bridged to Python `asyncio` coroutines,
   not run on a separate Tokio thread pool in a way that is invisible to
   Python's event loop. `pyo3-async-runtimes` handles this but requires a
   deliberate design choice at every `async` boundary.

The reshuffling: `iroh-http-py` binds **directly to `iroh-http-core`** and
does not go through `iroh-http-shared`. The JS bridge layer is not touched.

---

## Design

### Package layout

```
packages/iroh-http-py/
├── Cargo.toml             # crate-type = ["cdylib"], pyo3 + pyo3-async-runtimes
├── pyproject.toml         # maturin build backend, package metadata
├── src/
│   └── lib.rs             # #[pymodule] — IrohNode, IrohRequest, IrohResponse
└── iroh_http/
    ├── __init__.py        # re-exports + `create_node` convenience alias
    └── py.typed           # PEP 561 marker — enables type checking in editors
```

### Python API

The API follows the guidelines in `guidelines.md`: idiomatic Python first.

```python
import asyncio
from iroh_http import create_node

async def main():
    # Context-manager usage — node is closed automatically on exit
    async with await create_node() as node:
        print(node.node_id)     # str — base32 public key
        print(node.keypair)     # bytes — 32-byte Ed25519 secret key

        # Persist identity across restarts
        # node2 = await create_node(key=node.keypair)

        # ── fetch ──────────────────────────────────────────────────────────
        resp = await node.fetch(peer_id, "httpi://peer/api")
        body: bytes = await resp.bytes()
        text: str   = await resp.text()
        data: Any   = await resp.json()

        # ── serve ──────────────────────────────────────────────────────────
        async def handler(request: IrohRequest) -> IrohResponse:
            body = await request.body()
            return IrohResponse(status=200, body=b"hello " + body)

        node.serve(handler)  # returns immediately; runs in background
        await asyncio.sleep(10)

asyncio.run(main())
```

### Class design

```
create_node(
    key: bytes | None = None,           # 32-byte Ed25519 secret key
    idle_timeout: int | None = None,    # milliseconds
    relays: list[str] | None = None,
    dns_discovery: str | None = None,
) -> IrohNode

class IrohNode:
    node_id: str                         # read-only property
    keypair: bytes                       # read-only property (32 bytes)

    async def fetch(
        self,
        peer_id: str,
        url: str,
        *,
        method: str = "GET",
        headers: list[tuple[str, str]] | None = None,
        body: bytes | None = None,
    ) -> IrohResponse

    def serve(self, handler: Callable[[IrohRequest], Awaitable[IrohResponse]]) -> None

    async def close(self) -> None

    # Context manager support
    async def __aenter__(self) -> "IrohNode": ...
    async def __aexit__(self, *exc: Any) -> None: ...

class IrohRequest:
    method: str
    url: str
    headers: list[tuple[str, str]]
    peer_id: str                         # authenticated remote identity

    async def body(self) -> bytes        # reads and buffers the full request body

class IrohResponse:
    def __init__(
        self,
        status: int = 200,
        headers: list[tuple[str, str]] | None = None,
        body: bytes = b"",
    ): ...

    status: int
    headers: list[tuple[str, str]]
    url: str                             # only populated on responses from fetch()

    async def bytes(self) -> bytes
    async def text(self, encoding: str = "utf-8") -> str
    async def json(self) -> Any
```

### Async bridge

`pyo3-async-runtimes` (the maintained successor to `pyo3-asyncio`) with the
`tokio-runtime` feature provides two primitives used throughout:

- `pyo3_async_runtimes::tokio::future_into_py(py, future)` — wraps a Rust
  `Future` as a Python coroutine. Used for `create_node`, `fetch`, `close`,
  `body()`, `bytes()`, `text()`, `json()`.
- `pyo3_async_runtimes::tokio::into_future(coroutine)` — converts a Python
  coroutine into a Rust `Future`. Used inside the serve callback to drive the
  Python handler to completion.

A shared Tokio runtime is initialised once via `OnceLock` and is reused for
the lifetime of the Python process.

### Serve callback model

`node.serve(handler)` starts `iroh_http_core::serve` in the background.
For each incoming request:

1. The Rust task acquires the Python GIL.
2. The `RequestPayload` is converted into an `IrohRequest` pyobject.
3. The Python `handler(request)` coroutine is called and immediately converted
   to a Rust `Future` via `into_future`.
4. The Rust task awaits the future; when it resolves it extracts `status` and
   `headers` from the returned `IrohResponse` and calls `respond()`.
5. The `IrohResponse.body` bytes are pumped into the response body channel
   using the standard `make_body_channel` primitives.

The Python handler coroutine is therefore driven by the Tokio runtime, **not**
by the caller's `asyncio` event loop. This is intentional: the serve loop is
not tied to any single Python event loop instance, so `iroh_http` can be used
from scripts that mix `asyncio.run()` calls with multi-threaded servers.
CPU-bound work inside a handler should use `asyncio.to_thread` as usual.

### Build and distribution

The standard toolchain for PyO3 extensions is **maturin**:

```sh
# Development — installs an editable .so into the active virtualenv
pip install maturin
maturin develop

# Release wheel for the current platform
maturin build --release

# Cross-compile (requires `cargo-zigbuild` for Linux; works from macOS)
maturin build --release --target x86_64-unknown-linux-gnu
```

The `pyproject.toml` specifies maturin as the build backend, the extension
module name (`iroh_http_native`), Python ≥ 3.9 (stable ABI via `abi3-py39`),
and package classifiers for PyPI.

`iroh_http/__init__.py` imports from `iroh_http_native` and re-exports
everything under the `iroh_http` namespace so users write
`from iroh_http import create_node`, never `from iroh_http_native import …`.

### Rust dependencies

```toml
iroh-http-core      = { path = "../../crates/iroh-http-core" }
pyo3                = { version = "0.22", features = ["extension-module", "abi3-py39"] }
pyo3-async-runtimes = { version = "0.22", features = ["tokio-runtime"] }
tokio               = { version = "1", features = ["rt-multi-thread"] }
bytes               = "1"
serde_json          = "1"
```

### Changes required

| Layer | Change |
|---|---|
| Rust workspace | Add `packages/iroh-http-py` to `workspace.members` |
| `iroh-http-py` (Rust) | New `cdylib` crate: `src/lib.rs` with PyO3 module |
| `iroh-http-py` (Python) | `iroh_http/__init__.py`, `py.typed`, `pyproject.toml` |
| `iroh-http-core` | No changes required |
| `iroh-http-shared` | Not used — Python binds directly to core |
