---
id: "011"
title: "API surface audit — lock the public interface for v1"
status: open
date: 2026-04-25
area: api
tags: [api, whatwg, fetch, serve, specification, stability, semver]
---

# [011] API surface audit — lock the public interface for v1

## Context

iroh-http targets three JS/TS runtimes with an API deliberately modelled on
web standards: `fetch()` follows WHATWG Fetch, `serve()` follows the
Deno.serve contract. A formal specification exists
([specification.md](../specification.md)) defining the normative interface
contract.

Before v1.0, the public interface must be audited: every exported symbol
checked against the specification, every deviation documented or fixed, and
the result frozen. Once the surface is locked, the underlying Rust core and
FFI bridges can evolve freely without breaking consumers.

This exploration asks what "locked" means precisely, and what work remains to
get there.

## Questions

1. Does the actual exported API of each adapter match the specification? Where
   does it diverge — missing methods, extra methods, different signatures?
2. Are there any non-standard extensions (beyond `httpi://` and `IrohSession`)
   that should be explicitly documented as extensions vs. removed?
3. Should the `IrohNode` interface be split into a stable core and an
   unstable/experimental surface (e.g. `node.peerStats()`, `node.pathChanges()`)
   so observability APIs can evolve without a major version bump?
4. What is the type-level contract? Do the TypeScript types in
   `iroh-http-shared` accurately describe the runtime behaviour, including
   error types, optional fields, and return types?
5. How should the audit be verified ongoing — a conformance test, a type-level
   snapshot, or a manual checklist?

## What we know

### The specification defines

- `createNode(options?)` → `IrohNode`
- `IrohNode.fetch()`, `.serve()`, `.connect()`, `.browse()`, `.advertise()`
- `IrohNode.close()`, `.closed`, `.addr()`, `.ticket()`
- `IrohNode.stats()`, `.peerStats()`, `.pathChanges()`
- `ServeHandle` with `.finished`, `.abort()`
- `IrohSession` (WebTransport-compatible)
- `NodeOptions`, `ServeOptions`, `IrohFetchInit`, `MdnsOptions`
- Error classes: `IrohError`, `IrohTimeoutError`, `IrohArgumentError`,
  `IrohHandleError`, `IrohConnectionError`

### What WHATWG alignment gives us

The fetch/serve contract is well-specified externally. Deviations from it are
bugs unless explicitly documented. Key areas where iroh-http intentionally
diverges:

- `fetch()` accepts `PublicKey` as first argument (not just URL/Request)
- `fetch()` accepts `directAddrs` in init (peer address hints)
- Response includes `Peer-Id` header (see [001](001-peer-identity-in-api.md))
- `serve()` returns `ServeHandle` (not `Deno.HttpServer`)
- No CORS, no cookies, no redirects (peer-to-peer, not browser)

### Gaps observed informally

- The roadmap notes that some feature docs were written ahead of
  implementation and now lag behind (e.g. `stats()` described as "planned"
  when it's implemented).
- `node.sessions` (inbound raw QUIC session acceptor) was proposed in #117 —
  unclear if it shipped or is spec-only.
- The duplex upgrade transport mode was proposed for removal in #117 — unclear
  if `req.acceptWebTransport()` still exists in the API.
- `PublicKey.fromPeerId()` and `.toURL()` were added in #118 — should be in
  the spec.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Manual audit: compare spec → exports → types for each adapter | Thorough; catches everything | Labour-intensive; one-time |
| Automated type-level snapshot (e.g. `api-extractor` or `dts-compare`) | Catches drift automatically in CI | Setup cost; doesn't catch semantic drift |
| Conformance test suite: one test per spec requirement | Living verification; catches regressions | Expensive to write; overlaps with existing tests |
| Stability tiers (`@stable`, `@experimental`) in JSDoc | Communicates intent; allows evolution | Adds maintenance burden; tooling support varies |

## Implications

- Affects all three adapters and `iroh-http-shared`.
- Interacts with [005 — FFI versioning](005-ffi-versioning-compatibility.md):
  the public JS API and the FFI surface are different contracts but coupled.
- Interacts with [007 — cross-runtime test strategy](007-cross-runtime-test-strategy.md):
  a conformance test suite would extend the existing compliance harness.
- This is a prerequisite for v1.0. Semver major stability means the public API
  contract is the thing being versioned.

## Next steps

- [ ] For each adapter (Node, Deno, Tauri): enumerate every public export and
      compare against `specification.md`. Produce a diff table.
- [ ] For `iroh-http-shared`: compare the TypeScript type definitions against
      runtime behaviour. Flag any `any` types, missing optionality, or
      undocumented error codes.
- [ ] Decide on stability tiers: which APIs are stable-for-v1 vs. experimental.
      Candidates for experimental: `peerStats()`, `pathChanges()`,
      `sessions`, mDNS `browse()`/`advertise()`.
- [ ] Update `specification.md` with any APIs added since it was last revised
      (e.g. `PublicKey.fromPeerId()`, `PublicKey.toURL()`).
- [ ] Decide on ongoing verification: CI type snapshot, conformance tests, or
      manual checklist per release.
