---
date: 2026-04-13
status: open
package: iroh-http-py
---

# iroh-http-py Line-by-Line Review Findings

Date: 2026-04-13  
Scope: `packages/iroh-http-py` (`src/lib.rs`, Python package files, and tests)  
Validation run: `uv run --extra dev pytest -q` (7 failed, 20 passed, 3 skipped, 7 errors)

## Severity legend

- `P0` Critical correctness/safety issue with immediate runtime impact.
- `P1` High-impact API/runtime defect.
- `P2` Medium-impact behavior/API mismatch.
- `P3` Low-impact typing/tests/quality gap.

---

## Findings

### ISS-PY-001 (`P0`) `serve()` can panic due to missing Tokio reactor context

**Evidence**
- `serve` is synchronous and immediately enters async/tokio-dependent serving path:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:637`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:645`
- Test failures show runtime panic (`there is no reactor running`):
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/tests/test_node.py:79`

**Impact**
- User code calling `node.serve(handler)` may raise `pyo3_runtime.PanicException`.

**Remediation**
1. Ensure `serve` executes from a Tokio runtime context (or convert API to async and enter runtime explicitly).
2. Add a regression test that calls `serve` in current pytest-asyncio execution mode.

---

### ISS-PY-002 (`P1`) Unsafe raw-pointer lifetime in `IrohBrowseSession.__anext__`

**Evidence**
- Converts `self` to `usize` pointer and dereferences inside async future:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:324`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:327`

**Impact**
- Potential use-after-free / undefined behavior if Python object is dropped before future completion.

**Remediation**
1. Replace raw-pointer capture with an owned `Py<...>`/Arc-backed state that guarantees lifetime across await.
2. Add cancellation/drop tests for browse iteration.

---

### ISS-PY-003 (`P1`) `HandlerResponse` exists natively but is not re-exported from package root

**Evidence**
- Native class is registered:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:1028`
- Top-level package omits it from import/re-export list:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/iroh_http/__init__.py:29`
- Runtime import fails in test:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/tests/test_node.py:185`

**Impact**
- Public API is broken for documented `HandlerResponse` usage.

**Remediation**
1. Re-export `HandlerResponse` in `iroh_http/__init__.py`.
2. Add/adjust stub entry for `HandlerResponse`.

---

### ISS-PY-004 (`P1`) `__all__` exports `IrohBrowseSession` even when symbol is absent

**Evidence**
- Conditional import swallows absence:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/iroh_http/__init__.py:42`
- `__all__` always includes `IrohBrowseSession`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/iroh_http/__init__.py:58`
- Runtime check: `from iroh_http import *` fails with `AttributeError` when `mdns` feature is off.

**Impact**
- Import-time failure for wildcard imports in non-`mdns` builds.

**Remediation**
1. Only append `IrohBrowseSession` to `__all__` when import succeeds.
2. Add a non-`mdns` import test (`import *`).

---

### ISS-PY-005 (`P2`) `compression_level` option is documented/public but ignored

**Evidence**
- Option exposed in docs/signature:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:901`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/iroh_http/__init__.pyi:16`
- Value is explicitly unused:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:922`

**Impact**
- Caller-provided compression level has no effect.

**Remediation**
1. Either wire `compression_level` into core options or remove/deprecate the parameter.
2. Add behavior test for non-default compression configuration.

---

### ISS-PY-006 (`P2`) `next_unidirectional_stream` returns `IrohBidiStream` with invalid write handle

**Evidence**
- Returns `IrohBidiStream` with `write_handle: 0`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:484`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:486`

**Impact**
- Exposes writable API surface on receive-only stream object; write/close behavior is undefined or error-prone.

**Remediation**
1. Return a dedicated receive-only stream type.
2. At minimum, guard write/close on invalid handles with clear Python exceptions.

---

### ISS-PY-007 (`P2`) Invalid `direct_addrs` are silently dropped

**Evidence**
- Parsing uses `filter_map(...ok())` in `connect` and `fetch`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:557`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:605`

**Impact**
- Address typos are ignored instead of raising actionable errors.

**Remediation**
1. Fail fast when any provided address is invalid.
2. Include offending address in error text.

---

### ISS-PY-008 (`P3`) Type stubs do not match runtime behavior

**Evidence**
- Stub says sync close:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/iroh_http/__init__.pyi:93`
- Runtime exposes async-returning close:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/src/lib.rs:410`
- Runtime introspection confirms non-coroutine declaration at Python layer but awaitable behavior (`inspect.iscoroutinefunction(...) == False`).
- `HandlerResponse` missing from stubs.

**Impact**
- Incorrect IDE/type-checker guidance and potential misuse by consumers.

**Remediation**
1. Update `.pyi` to reflect awaitable return types from PyO3 async methods.
2. Add `HandlerResponse` and conditional mDNS typing notes.

---

### ISS-PY-009 (`P3`) Crypto test cases contain no-op assertions (false confidence)

**Evidence**
- Tests create signatures but do not call/assert `public_key_verify`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/tests/test_crypto.py:55`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/tests/test_crypto.py:75`

**Impact**
- Reported pass does not validate intended verify behavior.

**Remediation**
1. Replace with explicit positive/negative verify assertions.
2. Remove dead setup that does not contribute to assertions.

---

## Test-failure snapshot (supporting evidence)

From `uv run --extra dev pytest -q`:
- `tests/test_node.py::{test_serve_fetch_basic,test_serve_fetch_with_body,test_response_text,test_response_json,test_handler_500_on_exception}` failed with `pyo3_runtime.PanicException` (`no reactor running`).
- `tests/test_node.py::test_serve_with_handler_response` failed with `ImportError` for `HandlerResponse`.
- `tests/test_session.py` had 7 setup/runtime failures due to session connection timeout in this environment.

These failures corroborate ISS-PY-001 and ISS-PY-003 directly.
