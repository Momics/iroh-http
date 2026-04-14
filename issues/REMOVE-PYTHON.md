# Remove Python Adapter — Execution Plan

Python support is removed for v1.0. The Rust core remains ready for future
FFI adapters (the architecture is platform-agnostic by design). Python can
be re-added once the API stabilises.

---

## 1. Delete files and directories

| Path | What |
|------|------|
| `packages/iroh-http-py/` | Entire Python adapter (Rust PyO3 bindings, Python wrapper, tests, config) |
| `examples/python/` | Python example code + requirements.txt |
| `scripts/build-python.sh` | Python build script |
| `docs/guidelines/python.md` | Python coding guidelines |

## 2. Update scripts

### `scripts/version.sh`
- Remove `packages/iroh-http-py/Cargo.toml` from `CARGO_FILES` array
- Remove the `pyproject.toml` version-bump block
- Update comment: "7 Cargo.toml" → "6 Cargo.toml", remove "1 pyproject.toml"

### `scripts/release.sh`
- Remove `python3`, `maturin` from required tool checks
- Remove `twine` auth check
- Remove Python wheel build section (step 2e)
- Remove Python test section (step 3h)
- Remove PyPI publish section (step 5g)
- Update header comments

## 3. Update CI

### `.github/workflows/ci.yml`
- Remove the `python-check` job entirely

### `.github/workflows/release.yml`
- Remove the Python / PyPI publish section

## 4. Update `package.json` (root)

- Remove `"build:python": "bash scripts/build-python.sh"` script
- Remove `npm run build:python` from the `build` chain

## 5. Update `Cargo.toml` (root workspace)

- Remove the comment about iroh-http-py being excluded from the workspace

## 6. Update `.gitignore`

- Remove or keep the Python section (`__pycache__/`, `*.egg-info/`, etc.)
  — keep it (harmless, and useful if Python is re-added later)

## 7. Update documentation

### `README.md`
- Remove the Python quick-start example section
- Update "Multi-platform" feature line: remove "Python"
- Remove `iroh-http-py (PyO3)` from the package tree diagram

### `docs/README.md`
- Remove the Python guidelines link

### `docs/architecture.md`
- Remove Python from the layer diagram
- Remove `iroh-http-py | PyO3 | Python` from the adapter table
- Remove Python row from the concurrency model table
- Update any prose mentioning Python alongside other adapters

### `docs/build-and-test.md`
- Remove the entire "Python" build section
- Remove Python from the CI gates list (item 10)
- Remove "Running Python tests locally" section

### `docs/principles.md`
- Remove the Python guidelines link from the header
- Update prose about "JS or Python context" to just say "JS context"

### `docs/specification.md`
- Remove the "Python API Differences" section
- Remove Python quick-start example
- Remove Python cross-references throughout

### `docs/roadmap.md`
- Remove PyPI blocker
- Remove `iroh-http | PyPI` from distribution channels table
- Remove maturin reference
- Add a note that Python is a future-horizon goal

### `docs/guidelines/README.md`
- Remove the Python row from the table

### `docs/features/sign-verify.md`
- Remove Python column from feature table
- Remove "Python note" callout

### `docs/features/discovery.md`
- Remove Python column from feature table
- Remove "Python API differences" note

### `.github/copilot-instructions.md`
- Remove Python guidelines link
- Remove `test_node.py` from FFI boundary bugs regression line
- Remove `pyright` from type/export bugs line

### `TEST_PLAN.md`
- Remove Python integration/unit test rows from "Current State" table
- Remove "Python tests are not in CI" from "What is missing"
- Remove "No static type checking for Python" from missing list
- Remove section 4.3 (Python integration tests)
- Remove section 6.1 (Add pyright to CI) and 6.2 (Add Python tests to CI)
- Remove Python references from principles and strategy

### `scripts/README.md`
- Remove Python from the prerequisites table
- Remove maturin from prerequisites
- Remove Python from the "build.sh" order description
- Update release script docs to remove Python/PyPI/twine references

## 8. Update issue files

### `issues/TEST-003.md`
- Add a note that this issue is now superseded (Python adapter removed)

### `issues/TEST-005.md`
- Remove references to Python job in CI

## 9. Exploration docs (light-touch edits only)

These are historical design thinking documents. Change mentions from
"Node, Deno, Tauri, and Python" to "Node, Deno, and Tauri" where the
statement would be factually wrong if left as-is. Leave historical context
intact where it's clearly discussing past decisions.

- `docs/explorations/005-ffi-versioning-compatibility.md`
- `docs/explorations/006-project-nature-in-ecosystem.md`
- `docs/explorations/007-cross-runtime-test-strategy.md`

## 10. Verify

After all changes:
1. `cargo check --workspace` — no broken references
2. `npm run typecheck` — still passes
3. `cargo test --workspace` — all tests pass
4. `node packages/iroh-http-node/test/e2e.mjs` — 14 pass
5. `deno test ... smoke.test.ts` — 23 pass
6. Grep for orphaned references: `grep -r 'python\|Python\|PyO3\|maturin\|pyright\|iroh-http-py\|pyproject\|PyPI\|twine' --include='*.md' --include='*.sh' --include='*.yml' --include='*.json' --include='*.toml' .`

---

## Design note

The Rust core (`iroh-http-core`) exposes a platform-agnostic C-like API via
slotmap handles and JSON-serialised errors. This design is deliberately FFI-
friendly and does not depend on any adapter. Adding a new platform adapter
(Python, Swift, Kotlin, C#) is a matter of writing a new FFI bridge crate
that calls the same core functions. Nothing in this removal changes the core
architecture.
