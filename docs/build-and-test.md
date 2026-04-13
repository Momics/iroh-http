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
5. No-default-features check (Node + Deno)
6. TypeScript typecheck (`npm run typecheck`)
7. Node.js E2E tests
8. Deno E2E tests

See [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) for the full
pipeline.
