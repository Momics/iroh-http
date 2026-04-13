# Roadmap

## Horizon 1 — v1.0 Release

The primary near-term goal. The library works; this is about making it
releasable and trustworthy for public consumption.

### Blockers before any release

**Node.js — napi-rs platform packages not configured**

napi-rs multi-platform distribution requires one small package per platform
(e.g. `@momics/iroh-http-node-darwin-arm64`) listed as
`optionalDependencies` in the main package. The main `package.json` only has
`"files": ["*.node"]` — a user installing on Linux gets nothing.

`index.darwin-arm64.node` is also tracked in git and needs to be untracked.

**crates.io — publish metadata missing**

`iroh-http-core` and `iroh-http-discovery` are missing `repository`,
`documentation`, `keywords`, and `categories` in their `[package]` sections.
Add to each:

```toml
repository    = "https://github.com/momics/iroh-http"
documentation = "https://docs.rs/iroh-http-core"
keywords      = ["p2p", "http", "quic", "iroh"]
categories    = ["network-programming", "web-programming"]
```

**PyPI — `[project.urls]` missing from `pyproject.toml`**

```toml
[project.urls]
Repository = "https://github.com/momics/iroh-http"
```

**No release CI workflow**

`ci.yml` runs check + test. There is no `release.yml` that fires on `git tag
v*` to build cross-platform binaries and publish to npm, JSR, PyPI, and
crates.io. napi-rs and maturin both have turnkey GitHub Actions templates.

**Unclean repository state**

- `workspace/` and `.obsidian/` are not gitignored. Add to `.gitignore`.
- `git rm --cached packages/iroh-http-node/index.darwin-arm64.node`
- Untrack any committed generated files (`lib.js`, `lib.d.ts`, source maps)
  that are already in `.gitignore`.

### Files to add before v1.0

| File | Why |
|------|-----|
| `.github/workflows/release.yml` | Automated cross-platform build + publish on tag |
| `CHANGELOG.md` | Standard for any published package; users expect it |
| `SECURITY.md` | Required if open-sourcing; good practice either way |
| `.github/ISSUE_TEMPLATE/bug_report.md` | Bug report template |
| `.github/ISSUE_TEMPLATE/feature_request.md` | Feature request template |

### Distribution channels

| Package | Platform | Status |
|---------|----------|--------|
| `@momics/iroh-http-shared` | npm + JSR | Config looks correct |
| `@momics/iroh-http-node` | npm | **Broken** — platform packages not configured |
| `@momics/iroh-http-deno` | JSR | Config looks correct |
| `@momics/iroh-http-tauri` | npm | Config looks correct |
| `iroh-http` | PyPI | Missing `[project.urls]` |
| `iroh-http-core` | crates.io | Missing publish metadata |
| `iroh-http-discovery` | crates.io | Missing publish metadata |

---

## Horizon 2 — Open Source

**Recommendation: stay private through Horizon 1, open source after.**

### Why not open source today

- Code quality needs to be defensible before public scrutiny.
- Release infrastructure (no `release.yml`, broken napi-rs setup) isn't ready.
- Opening mid-patch creates a confusing first impression.

### Why open source eventually

- iroh-http is a trust-based protocol. Users connecting nodes by public key
  will want to audit the library that manages those keys.
- The whole project (docs, recipes, `CONTRIBUTING.md`) is already written for
  a public audience.
- p2p networking libraries without open source rarely get adoption. The
  network effect depends on community visibility.

### Path to open source

1. Complete the Horizon 1 checklist
2. Untrack committed binaries; gitignore `workspace/` and `.obsidian/`
3. Add `CHANGELOG.md` and `SECURITY.md`
4. Get tests green end-to-end
5. Open source the repository

---

## Horizon 3 — Embedded and HTTP/3

These are long-term goals that influence architectural decisions today but are
not on the near-term roadmap.

### Embedded / `no_std`

Iroh's embedded QUIC support is still evolving. The current host-platform
implementation uses hyper v1 (not `no_std`-compatible). The path to embedded
requires a transport-agnostic `iroh-http-framing` crate — pure
parse/serialize logic with no tokio, no socket concerns, no Iroh coupling.

**Architectural constraints to preserve today:**

- Wire-level parsing/serialization must remain isolated from
  runtime/transport concerns. Never couple protocol logic to tokio or Iroh
  internals.
- Protocol semantics must be specified by conformance tests and test vectors,
  not by implicit host runtime behaviour.
- Platform adapters must not define protocol behaviour; they only map APIs.
- Error codes and failure semantics must be canonical and cross-platform.

A host-only dependency is acceptable today when:
1. It significantly improves correctness, safety, or maintainability now.
2. It does not erase protocol boundaries needed for embedded.
3. Protocol behaviour remains expressible through conformance tests.

### HTTP/3

Nothing in the current architecture closes the door to HTTP/3. The
`tower::Service` application layer and all business logic would be unchanged
— only the transport wiring needs to swap.

The blocker is upstream: there is no `h3-noq` crate yet (analogous to
`h3-quinn` but for Iroh's noq fork). Once that exists and Iroh exposes
`noq::Connection` publicly, the swap is straightforward. Track this via
the open question in [architecture.md](architecture.md).
