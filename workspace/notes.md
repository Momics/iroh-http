
  - We are currently writing integration and unit tests for the core and all platforms.
  - Patch 17 is not yet integrated and also needs to be evaluated for 'use'.
  - We should invest heavily in developer UX and great JSDoc like the previous packages. We should have an agent check all developer facing (and perhaps even our own internal code) for exceptional IDE documentation.
  - Much of the core broke when we tried testing which indicates that the developers who built it didn't really write good code. It's vital that an agent 'aggressively' checks all the core code and makes sure it satisfies robust 'Rust-standard' code requirements.
  - Verify that all patches were applied correctly.

## Integration test timeout on macOS — open investigation

**Symptom:**
`cargo test --test integration` consistently times out after ~30 s on macOS Apple Silicon.
Every test that calls `fetch()` fails with `"connect: timed out"`.
`cargo test --test bidi_stream` passes (4/4, ~3 s).
`cargo test --test integration --features compression` passes (49/49, ~92 s).

**Root cause: not yet identified.**
The failure is reproducible and deterministic (not flaky), but the exact reason why
the `compression` feature gate changes connectivity behaviour is unknown.

**Key findings from investigation:**

1. **Same IP addresses in both cases.**
   Both test binaries bind to `192.168.50.16:<random-port>` (the LAN interface, not
   loopback), so "QUIC loopback to 127.0.0.1" is not the cause here — unlike the
   Deno test issue which was fixed by switching to relay/ticket.

2. **The pool is not the cause.**
   Bypassing the `ConnectionPool` and calling `ep.raw().connect(addr, ALPN)` directly
   also times out, ruling out any pool-layer bug.

3. **ALPN protocol is not the cause.**
   Both `ALPN` (`iroh-http/1`) and `ALPN_DUPLEX` (`iroh-http/1-duplex`) time out in
   the `integration` binary. `session_connect` (which uses `ALPN_DUPLEX`) also times
   out in this binary.

4. **The same `session_connect` call succeeds in the `bidi_stream` binary.**
   The only difference between the two test binaries is the set of symbols compiled
   in. Adding the integration tests' `use` imports to `bidi_stream.rs` does not
   break it. Compiling (but not calling) `serve()` in `bidi_stream` still works.

5. **`--features compression` makes all 49 integration tests pass.**
   This is the only known workaround. It adds `async-compression` as a dependency
   and enables the zstd compression code paths in `client.rs` and `server.rs`.
   Why this affects QUIC connection establishment is unknown.

**Hypotheses to explore (for a fresh pair of eyes):**

- The `async-compression` crate (or one of its transitive deps like `zstd-sys`)
  may initialise something (a thread pool, a global allocator setting, an OS-level
  socket option) as a side-effect of being linked in.
- There may be a subtle static initialisation ordering difference between the two
  feature sets that affects iroh's internal state machine for QUIC path selection.
- The `iroh` crate's address-resolution actor (`RemoteStateActor`) may behave
  differently depending on timing or thread count, and `async-compression` linker
  order may influence Tokio's thread pool startup.

**Recommended next step:**
Bisect what `async-compression` brings in. Check whether merely linking
`zstd-sys` (without any async code) reproduces the fix. Also try enabling
`tokio`'s `tracing` feature and capturing the iroh span output during a
failing run to see exactly where the connection attempt stalls.