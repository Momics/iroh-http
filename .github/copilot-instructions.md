# iroh-http

Peer-to-peer HTTP over Iroh QUIC transport. Rust core + FFI adapters for Node.js, Deno, and Tauri. Nodes addressed by Ed25519 public key, not DNS.

## Context

- [Principles](../docs/principles.md) — engineering invariants, hierarchy of values, self-evaluation checklist. Read before any non-trivial change.
- [Architecture](../docs/architecture.md) — layer diagram, component responsibilities, concurrency model, scope boundaries. Read before modifying core.
- [Design decisions](../docs/internals/design-decisions.md) — the *why* behind hyper, slotmap, moka, wire format, compression policy. Read when touching internals.
- [Documentation index](../docs/README.md) — entry point to all documentation, features, internals, and recipes.
- [Roadmap](../docs/roadmap.md) — v1.0 release checklist, open source path, embedded and HTTP/3 horizons.
- [Specification](../docs/specification.md) — normative interface contract for all adapters. Read when adding or changing adapter APIs.

## Coding Guidelines

- [Rust](../docs/guidelines/rust.md) — naming, visibility, error handling, async, testing for `iroh-http-core` and `iroh-http-discovery`.
- [JavaScript / TypeScript](../docs/guidelines/javascript.md) — platform types, error classes, streaming, serve/fetch contracts for Node, Deno, Tauri adapters.
- [Tauri](../docs/guidelines/tauri.md) — invoke commands, channels, plugin structure for `iroh-http-tauri`.

## Protocol & Wire Format

- [Protocol](../docs/protocol.md) — `httpi://` URL scheme, HTTP/1.1-over-QUIC wire format, ALPN versioning, `Request`/`Response` compatibility.

## Build & Test

- [Build & test](../docs/build-and-test.md) — Rust, TypeScript, and E2E test commands. CI pipeline gates.

## Features

- [Features index](../docs/features/README.md) — individual feature specs: compression, discovery, observability, rate limiting, server limits, sign/verify, streaming, tickets, trailer headers, WebTransport.

## Internals

- [Internals index](../docs/internals/README.md) — contributor deep dives: HTTP engine, resource handles, connection pool, wire format.

## Recipes

- [Recipes index](../docs/recipes/README.md) — 28 practical patterns built on iroh-http primitives (local-first sync, sealed messages, device handoff, capability tokens, etc.).

## Issue Resolution Policy

Every fixed issue must leave a regression test in the appropriate layer:

- **FFI boundary bugs** → per-adapter integration test (`e2e.mjs`, `smoke.test.ts`)
- **Rust core bugs** → `cargo test` (in `integration.rs` or a new test file)
- **Type/export bugs** → verified by `tsc` (no new test needed if CI gates it)
- **Protocol behavior** → `cases.json` entry in `tests/http-compliance/`
- **Docs/build/config** → N/A (document in the issue's `## Regression test` section)

When closing an issue, fill in the `## Regression test` section of the issue file with the layer, test name/path, and whether the test was verified failing before the fix.
