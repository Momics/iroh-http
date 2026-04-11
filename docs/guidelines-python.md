# Python Platform Guidelines

Applies to: `iroh-http-py` (PyO3 bindings + pure-Python wrapper).

---

## Naming

| Scope       | Convention       | Example                          |
| ----------- | ---------------- | -------------------------------- |
| Functions   | `snake_case`     | `create_node`, `fetch`           |
| Classes     | `PascalCase`     | `IrohNode`, `IrohResponse`       |
| Constants   | `UPPER_SNAKE`    | *(none currently)*               |
| Properties  | `snake_case`     | `node_id`, `remote_node_id`      |
| Modules     | `snake_case`     | `iroh_http`                      |

The top-level import is `from iroh_http import create_node`.

---

## Types — value objects over dicts

| Concept       | Use                       | Never                                |
| ------------- | ------------------------- | ------------------------------------ |
| Response      | `IrohResponse`            | dict with `status`/`headers`/`body`  |
| Request       | `IrohRequest`             | dict or tuple unpacking              |
| Byte data     | `bytes`                   | `bytearray`, raw `memoryview`        |
| Headers       | `list[tuple[str, str]]`   | `dict[str, str]` (loses duplicates)  |
| Errors        | `RuntimeError` (PyO3)     | Custom exception hierarchy (for now) |

**Why `IrohResponse` and `IrohRequest`?** Python has no built-in
`Response` or `Request` type. Unlike JS where WHATWG types are standard,
Python needs its own value objects to expose properties and body-consumption
methods. These classes earn their existence because the alternative (raw
dicts) loses discoverability and type safety.

---

## Async patterns

- **All I/O is `async def`.** Body reads, fetch, connect — everything that
  touches the network is async.
- Entry point: `node = await create_node()`.
- Body consumption is async:
  ```python
  response = await node.fetch(peer_id, "httpi://peer/path")
  body = await response.bytes()
  text = await response.text()
  data = await response.json()
  ```
- Uses `pyo3-async-runtimes` to bridge Tokio futures into Python
  `asyncio` awaitables.

---

## Serve handler contract

The serve handler is a callable with signature:

```python
async def handler(request: IrohRequest) -> dict:
    ...
```

**Input:** `IrohRequest` with properties:
- `method: str` — HTTP method
- `url: str` — full `httpi://` URL
- `headers: list[tuple[str, str]]` — request headers
- `remote_node_id: str` — authenticated peer identity

And async methods:
- `await request.body()` — read full body as `bytes`
- `await request.text()` — read full body as `str`

**Output:** the handler currently returns a dict:
```python
{"status": 200, "headers": [("content-type", "text/plain")], "body": b"hello"}
```

The body value can be `bytes`, `str` (encoded as UTF-8), or `None`.

---

## `IrohResponse` API

`IrohResponse` is a value object returned by `node.fetch()`. It exposes:

| Property / Method     | Type                        | Description                   |
| --------------------- | --------------------------- | ----------------------------- |
| `response.status`     | `int`                       | HTTP status code              |
| `response.headers`    | `list[tuple[str, str]]`     | Response headers              |
| `response.url`        | `str`                       | Final URL of responding peer  |
| `await response.bytes()` | `bytes`                  | Full body as bytes            |
| `await response.text()`  | `str`                    | Full body as UTF-8 string     |
| `await response.json()`  | `Any`                    | Body parsed as JSON           |

Body methods consume the body stream. Call only one of them, once.

---

## Error handling

- Rust errors cross the FFI boundary as `PyRuntimeError` via PyO3's
  `PyErr::new::<PyRuntimeError, _>(msg)`.
- Error messages are plain strings (not yet using the structured
  `classify_error_json` codes on the Python side).
- Future work: introduce typed exceptions (`IrohTimeoutError`,
  `IrohConnectionError`) mapped from Rust error codes.

---

## Type annotations

- Ship a `py.typed` marker file for PEP 561 compliance.
- All public functions and classes should have type stubs or inline
  annotations sufficient for `mypy` / `pyright` to understand.
- PyO3-generated classes get their types from the `#[pyclass]` /
  `#[pymethods]` definitions.

---

## Packaging

- Build system: `maturin` (PEP 517 backend).
- The native extension is `iroh_http_py` (built by PyO3).
- The pure-Python wrapper `iroh_http/__init__.py` re-exports everything
  under the clean `iroh_http` namespace.
- `__all__` must list every public symbol.

---

## Testing

- Tests use `pytest` with `pytest-asyncio`.
- Test through the public `iroh_http` namespace, not the raw
  `iroh_http_py` extension.
- Two-node integration tests: create two nodes in the same process, fetch
  between them.
- Body consumption tests cover `bytes()`, `text()`, and `json()`.
