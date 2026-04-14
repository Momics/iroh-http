# Python Guidelines

Applies to: `iroh-http-py` (PyO3 bindings + pure-Python wrapper).

For engineering values and invariants, see [principles.md](../principles.md).

---

## Naming

| Scope | Convention | Example |
|-------|------------|---------|
| Functions | `snake_case` | `create_node`, `fetch` |
| Classes | `PascalCase` | `IrohNode`, `IrohResponse` |
| Constants | `UPPER_SNAKE` | *(none currently)* |
| Properties | `snake_case` | `node_id`, `remote_node_id` |
| Modules | `snake_case` | `iroh_http` |

Top-level import: `from iroh_http import create_node`.

---

## Use Python-Native Types

| Concept | Use | Never |
|---------|-----|-------|
| Response | `IrohResponse` | dict with `status`/`headers`/`body` |
| Request | `IrohRequest` | dict or tuple unpacking |
| Byte data | `bytes` | `bytearray`, raw `memoryview` |
| Headers | `list[tuple[str, str]]` | `dict[str, str]` (loses duplicates) |

Python has no built-in `Request`/`Response` types, so `IrohRequest` and `IrohResponse` are value objects that provide discoverability and body-consumption methods. A dict alternative is not acceptable.

---

## Async Patterns

All I/O is `async def`. Entry point: `node = await create_node()`. Body consumption is async:

```python
response = await node.fetch(peer_id, "/path")
body = await response.bytes()      # full body
text = await response.text()       # UTF-8 decoded
data = await response.json()       # parsed JSON
```

Body methods consume the body stream. Call only one, exactly once.

---

## Serve Handler Contract

See [ServeHandler in the specification](../specification.md#servehandler) for the canonical handler contract (including the Python form).

`IrohRequest` properties: `method`, `url`, `headers`, `remote_node_id`. Async methods: `await request.body()`, `await request.text()`.

Handler returns a dict: `{"status": 200, "headers": [...], "body": b"..."}`. Body value may be `bytes`, `str` (encoded as UTF-8), or `None`.

---

## Error Handling

Rust errors cross the FFI boundary as `RuntimeError` via PyO3. Use the Rust error code in the message to allow callers to distinguish error types. Future: introduce typed exceptions (`IrohTimeoutError`, `IrohConnectionError`) mapped from Rust error codes — follow the error classification pattern from the [JavaScript guidelines](javascript.md).

---

## Type Annotations

- Ship a `py.typed` marker file (PEP 561).
- All public functions and classes have type stubs or inline annotations sufficient for `mypy` / `pyright`.
- `__all__` must list every public symbol.

---

## Packaging

Build system: `maturin` (PEP 517). The pure-Python wrapper `iroh_http/__init__.py` re-exports everything from the native extension under the clean `iroh_http` namespace.

---

## Feature Flags

The Python wheel is compiled with optional Cargo features that gate certain
APIs at the Rust level:

| Feature | Default | APIs gated |
|---------|:-------:|------------|
| `mdns` | off | `node.browse()`, `node.advertise()` — raise `RuntimeError` if missing |
| `compression` | off | `compression_level` / `compression_min_body_bytes` in `create_node()` — silently ignored if missing |

Document feature availability when shipping pre-built wheels.

---

## API Differences from JS/TS

See [Python API differences in the specification](../specification.md#python-api-differences) for the full mapping table.

Python exposes Rust functions directly via PyO3 as module-level functions
rather than class methods. Cancellation uses `async for … break` instead of
`AbortSignal`.

---

## Testing

- Use `pytest` with `pytest-asyncio`.
- Test through the public `iroh_http` namespace, not the raw native extension.
- Integration tests: two nodes in the same process, fetch between them.
- Cover all body consumption methods (`bytes()`, `text()`, `json()`).
