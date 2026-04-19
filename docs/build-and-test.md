# Build & Test

Commands for building and testing iroh-http locally and in CI.

---

## Rust

```sh
cargo check --workspace              # typecheck all crates
cargo test --workspace               # run all unit + integration tests
cargo clippy --workspace -- -D warnings   # lint (warnings are errors)
cargo fmt --all -- --check           # format check
```

The Tauri plugin lives outside the workspace and is checked separately:

```sh
cd packages/iroh-http-tauri && cargo check
```

No-default-features checks verify the code compiles without optional
dependencies (e.g. discovery):

```sh
cargo check -p iroh-http-node --no-default-features
cargo check -p iroh-http-deno --no-default-features
```

---

## TypeScript

```sh
npm install                          # install workspace dependencies
npm run typecheck                    # tsc --noEmit across shared + adapters
```

---

## Integration Tests

Rust integration tests in `crates/iroh-http-core/tests/` exercise two real
Iroh nodes over real QUIC connections (no mocks, no stubs). These cover:

- Basic fetch/serve round-trips
- Request/response bodies and trailers
- Streaming
- Bidirectional streams and WebTransport sessions
- Timeouts and diagnostics
- Cryptographic sign/verify
- Node ticket parsing

---

## Property Tests

Property-based tests live in `crates/iroh-http-core/tests/property.rs` and
run as part of `cargo test --workspace`. They use
[proptest](https://crates.io/crates/proptest) to generate thousands of random
inputs and verify that every public API boundary either succeeds or returns an
error — never panics.

Coverage by module:

| Module | Invariants |
|--------|------------|
| `lib.rs` — parsers | `parse_node_addr` never panics on arbitrary strings and JSON tickets |
| `lib.rs` — base32 | `base32_encode` never panics, empty input → empty output |
| `lib.rs` — crypto | sign→verify roundtrip, `secret_key_sign` / `public_key_verify` never panic on arbitrary keys |
| `lib.rs` — errors | `core_error_to_json` / `format_error_json` always produce valid JSON |
| `endpoint.rs` | `parse_direct_addrs` never panics |
| `server.rs` | `respond` never panics on arbitrary status codes + headers |
| `stream.rs` | HandleStore capacity bounds, invalid-handle safety, reader/writer insert→cancel roundtrip, pending-reader store→claim roundtrip |

When adding a new `pub fn` to the crate, add a corresponding
`_never_panics` proptest so every entry point has at least a basic
contract test.

---

## Fuzz Testing

Fuzz targets live in `crates/iroh-http-core/fuzz/` and use
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer).
They require a **nightly** Rust toolchain.

### Targets

| Target | Entry point | What it exercises |
|--------|-------------|-------------------|
| `fuzz_parse_node_addr` | `parse_node_addr()` | Arbitrary strings through JSON/base32/socket-addr parsing |
| `fuzz_handle_store` | `HandleStore` lookups | Arbitrary `u64` handles on every take/cancel/finish/lookup path |
| `fuzz_respond` | `respond()` | Arbitrary status codes + header name/value pairs through validation |

### Running locally

```sh
# Install cargo-fuzz (once):
cargo install cargo-fuzz

# Run a target for 5 minutes:
cd crates/iroh-http-core
cargo +nightly fuzz run fuzz_respond -- -max_total_time=300 -max_len=1024

# List available targets:
cargo +nightly fuzz list
```

### Seed corpus

Seed inputs for the `respond` target are in
`crates/iroh-http-core/fuzz/corpus/fuzz_respond/`. The fuzzer reads these
as starting points, mutates them, and saves any new inputs that reach new
code paths back into the corpus directory.

### Nightly CI

A separate GitHub Actions workflow
([`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml)) runs all
three fuzz targets nightly (5 minutes each, in parallel). The corpus is
cached between runs so discoveries accumulate over time.

---

## End-to-End Tests

E2E tests for platform adapters require building the native library first:

### Node.js

```sh
cargo build --release -p iroh-http-node
cd packages/iroh-http-node
npx napi build --platform --release
npx tsc
node test/e2e.mjs
```

### Deno

```sh
cargo build --release -p iroh-http-deno
mkdir -p packages/iroh-http-deno/lib
# Copy the native lib for your platform (example: Linux x86_64)
cp target/release/libiroh_http_deno.so packages/iroh-http-deno/lib/libiroh_http_deno.linux-x86_64.so
deno test --allow-read --allow-ffi --allow-env --allow-net packages/iroh-http-deno/test/smoke.test.ts
```

---

## CI

CI runs on every push to `main` and every pull request. All of the following
must pass:

1. `cargo check --workspace` + tauri plugin
2. `cargo test --workspace`
3. `cargo fmt --all -- --check`
4. `cargo clippy --workspace -- -D warnings`
5. Security-focused clippy lints (`unwrap_used`, `panic`, `arithmetic_side_effects`)
6. No-default-features check (Node + Deno)
7. TypeScript typecheck (`npm run typecheck`)
8. Dependency auditing (`cargo audit`, `cargo deny`, `npm audit --audit-level=high`)
9. Node.js E2E tests + compliance tests (12 cases)
10. Deno E2E tests
11. Cross-runtime compliance (node↔deno via `tests/http-compliance/run.sh`)
12. PR dependency review (`actions/dependency-review-action`)

See [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) for the full
pipeline. Fuzz + sanitizer/miri hardening runs on a separate nightly schedule
— see [`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml).

### Running compliance tests locally

```sh
# Node compliance (cases.json fixture):
node packages/iroh-http-node/test/compliance.mjs

# Cross-runtime — requires both Node and Deno native libs built:
bash tests/http-compliance/run.sh
```
