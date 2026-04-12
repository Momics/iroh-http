# Release Plan

## Distribution channels

| Package | Platform | Current status |
|---|---|---|
| `@momics/iroh-http-shared` | npm + JSR | Config looks correct |
| `@momics/iroh-http-node` | npm | **Broken for multi-platform** |
| `@momics/iroh-http-deno` | JSR | Config looks correct |
| `@momics/iroh-http-tauri` | npm | Config looks correct |
| `iroh-http` | PyPI | Missing metadata |
| `iroh-http-{core,discovery}` | crates.io | Missing metadata |

---

## Issues to fix before any release

### 1. Node.js — napi-rs platform packages not configured

napi-rs multi-platform distribution requires one small package per platform (e.g. `@momics/iroh-http-node-darwin-arm64`) listed as `optionalDependencies` in the main package. The main `package.json` currently has none of that — just `"files": ["*.node"]`. A user installing from npm on Linux would get nothing.

`index.darwin-arm64.node` is also committed to git and needs to be untracked.

### 2. crates.io — all three crates missing publish metadata

`iroh-http-core` and `iroh-http-discovery` have no `repository`, `documentation`, `keywords`, or `categories` fields. These affect search ranking and whether crates.io renders the README.

Add to each `[package]` section:
```toml
repository   = "https://github.com/momics/iroh-http"
documentation = "https://docs.rs/iroh-http-core"
keywords     = ["p2p", "http", "quic", "iroh"]
categories   = ["network-programming", "web-programming"]
```

### 3. PyPI — `pyproject.toml` missing `[project.urls]`

```toml
[project.urls]
Repository = "https://github.com/momics/iroh-http"
```

### 4. No release/publish CI workflow

`ci.yml` only runs check + typecheck. There is no `release.yml` that fires on `git tag v*` to build cross-platform binaries and publish to all five platforms. napi-rs and maturin both have turnkey GitHub Actions templates for this.

### 5. `workspace/` and `.obsidian/` are not gitignored

Both directories contain internal notes and editor config that must not be public. Add to `.gitignore`:
```
workspace/
.obsidian/
```

---

## Files to add

| File | Why |
|---|---|
| `.github/workflows/release.yml` | Automated cross-platform build + publish on tag |
| `CHANGELOG.md` | Standard for any published package; users expect it |
| `SECURITY.md` | Required if open-sourcing; good practice either way |
| `.github/ISSUE_TEMPLATE/bug_report.md` | Bug report template |
| `.github/ISSUE_TEMPLATE/feature_request.md` | Feature request template |

## Files/entries to remove or gitignore

- `workspace/` is removed from the repo (deleted)
- Add `.obsidian/` to `.gitignore`
- `git rm --cached packages/iroh-http-node/index.darwin-arm64.node`
- `git rm --cached packages/iroh-http-node/lib.js lib.d.ts` and maps (already in `.gitignore` but were committed)

---

## Private repo vs. open source

**Recommendation: stay private now, open source after the next two milestones.**

### Why not open source today

- Core broke during testing; code quality needs to be in a defensible state before public scrutiny.
- Patch 17 is not yet integrated. Opening mid-patch creates a confusing first impression.
- The release infrastructure (no `release.yml`, broken napi-rs platform setup) isn't ready.

### Why open source eventually

- iroh-http is fundamentally a trust-based protocol. Users connecting nodes by public key will want to audit the library that manages those keys. Closed source is a real barrier here.
- The whole project (docs, recipes, `CONTRIBUTING.md`) is written for a public audience already.
- p2p networking libraries that aren't open source rarely get adoption. The network effect depends on community visibility.
- Publishing packages from a private repo sidesteps the trust problem short-term but kills contributor momentum.

### The path to open source

1. Fix napi-rs platform package structure and add `release.yml`
2. Untrack committed binaries; add `workspace/` and `.obsidian/` to `.gitignore`
3. Add `repository`/`keywords`/`categories` to all Cargo manifests and `pyproject.toml`
4. Add `CHANGELOG.md` and `SECURITY.md`
5. Finish patch 17, get tests green
6. Open source the repository

At that point the codebase, infrastructure, and documentation are all in a state that can hold up to public eyes.
