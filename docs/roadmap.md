# Roadmap

## Horizon 1 — v1.0 Release

The primary near-term goal. The library works; this is about making it
releasable and trustworthy for public consumption.

### Done

- [x] crates.io publish metadata (`repository`, `documentation`, `keywords`, `categories`)
- [x] Release CI workflow (`release.yml` with cross-platform matrix build)
- [x] CI workflow (`ci.yml` — Rust check, TypeScript typecheck, E2E tests)
- [x] Clean repository state (`.gitignore` for workspace/, .obsidian/, generated files)
- [x] Issue templates (`bug.yml`, `feature.yml`)
- [x] Cross-platform builds: Node (5 targets), Deno (5 targets)
- [x] Build logic lives in each package (not root shell scripts)
- [x] All GitHub issues (#1–#6) resolved

### Remaining before v1.0

**Node.js — napi-rs platform packages**

napi-rs multi-platform distribution requires one small package per platform
(e.g. `@momics/iroh-http-node-darwin-arm64`) listed as
`optionalDependencies` in the main package. Without this, a user installing
on a different platform gets no `.node` binary. napi-rs has a
[`@napi-rs/cli artifacts`](https://napi.rs/docs/cross-build/summary)
command that generates these packages.

**`CHANGELOG.md`**

Standard expectation for published packages. Options:
- **`git-cliff`** — generates a changelog from conventional commit messages.
  Run `git cliff -o CHANGELOG.md` before each release. Zero config if
  commits follow `feat:`, `fix:`, `refactor:` prefixes.
- **GitHub release notes** — auto-generated from PR titles when creating a
  release. Good for GitHub-native consumers, but doesn't produce a file in
  the repo.
- **`release-please`** — Google's bot that watches commits, opens a PR with
  version bump + changelog, and publishes on merge. Heavier but fully
  automated.

Recommendation: use `git-cliff` in `release.sh` so the changelog is always
in the repo and reproducible from git history.

**`SECURITY.md`**

Required for responsible open source, especially for a library handling
Ed25519 keys and peer identity. Should describe:
- How to report vulnerabilities privately (use GitHub Security Advisories)
- Supported versions
- Disclosure timeline

### Distribution channels

| Package | Platform | Status |
|---------|----------|--------|
| `@momics/iroh-http-shared` | npm + JSR | ✅ Published |
| `@momics/iroh-http-node` | npm | ⚠️ Platform packages not configured |
| `@momics/iroh-http-deno` | JSR + GitHub releases | ✅ Published, runtime binary download |
| `@momics/iroh-http-tauri` | npm | ✅ Config correct |
| `iroh-http-core` | crates.io | ✅ Metadata complete |
| `iroh-http-discovery` | crates.io | ✅ Metadata complete |

---

## Horizon 2 — Open Source

### Pre-open-source checklist

Tracked in [#43 — Automate release pipeline](https://github.com/Momics/iroh-http/issues/43):

1. [ ] Fix Node.js napi-rs platform package split
2. [ ] Add `CHANGELOG.md` (via `git-cliff` or equivalent)
3. [x] Add `SECURITY.md`
4. [ ] Re-enable CI workflows (`push: [main]`, `pull_request: [main]`)
5. [ ] Move release flow from `scripts/release.sh` to GitHub Actions
       (`release.yml` triggered by `v*` tags)
6. [ ] Run `release.sh --dry-run` end-to-end to validate publish flow
7. [ ] Decide on npm scope: keep `@momics/` or use `iroh-http`
8. [ ] Final sweep: no internal references, credentials, or private URLs
9. [ ] Make repository public on GitHub

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

**Python support is a future goal** once the core and JS adapters are stable
and the release pipeline is solid.
