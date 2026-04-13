# iroh-http

Peer-to-peer HTTP over Iroh QUIC transport. Rust core + FFI adapters for Node.js, Deno, Tauri, and Python. Nodes addressed by Ed25519 public key, not DNS.

## Context

- [Principles](../docs/principles.md) — engineering invariants, hierarchy of values, self-evaluation checklist. Read before any non-trivial change.
- [Architecture](../docs/architecture.md) — layer diagram, component responsibilities, concurrency model, scope boundaries. Read before modifying core.
- [Design decisions](../docs/internals/design-decisions.md) — the *why* behind hyper, slotmap, moka, wire format, compression policy. Read when touching internals.
- [Documentation index](../docs/README.md) — entry point to all documentation, features, internals, and recipes.
- [Roadmap](../docs/roadmap.md) — v1.0 release checklist, open source path, embedded and HTTP/3 horizons.

## Coding Guidelines

- [Rust](../docs/guidelines/rust.md) — naming, visibility, error handling, async, testing for `iroh-http-core` and `iroh-http-discovery`.
- [JavaScript / TypeScript](../docs/guidelines/javascript.md) — platform types, error classes, streaming, serve/fetch contracts for Node, Deno, Tauri adapters.
- [Python](../docs/guidelines/python.md) — PyO3 conventions for `iroh-http-py`.
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
