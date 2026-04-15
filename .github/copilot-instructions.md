# iroh-http

Peer-to-peer HTTP over Iroh QUIC transport. Rust core + FFI adapters for Node.js, Deno, and Tauri. Nodes addressed by Ed25519 public key, not DNS.

## Reference

- [Principles](../docs/principles.md) — engineering invariants, hierarchy of values. Read before any non-trivial change.
- [Architecture](../docs/architecture.md) — layer diagram, component responsibilities, concurrency model. Read before modifying core.
- [Specification](../docs/specification.md) — normative interface contract for all adapters. Read when adding or changing APIs.
- [Protocol](../docs/protocol.md) — `httpi://` URL scheme, HTTP/1.1-over-QUIC wire format, ALPN versioning.
- [Design decisions](../docs/internals/design-decisions.md) — the *why* behind hyper, slotmap, moka, wire format, compression policy.
- [Roadmap](../docs/roadmap.md) — v1.0 release checklist, open source path.
- [Build & test](../docs/build-and-test.md) — Rust, TypeScript, and E2E test commands. CI pipeline gates.
- [Documentation index](../docs/README.md) — entry point to all docs, features, internals, and recipes.

## Guidelines

- [Rust](../docs/guidelines/rust.md) — naming, visibility, error handling, async, testing for `iroh-http-core` and `iroh-http-discovery`.
- [JavaScript / TypeScript](../docs/guidelines/javascript.md) — platform types, error classes, streaming, serve/fetch contracts.
- [Tauri](../docs/guidelines/tauri.md) — invoke commands, channels, plugin structure for `iroh-http-tauri`.

## Skills

- [manage-issues](.github/skills/manage-issues/SKILL.md) — create, close, and label GitHub issues. Includes regression test policy and commit-linking format.
- [git-conventions](.github/skills/git-conventions/SKILL.md) — commit messages, branch names, PR titles. Follow Conventional Commits for every commit.
