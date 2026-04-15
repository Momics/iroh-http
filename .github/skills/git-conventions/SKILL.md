---
name: git-conventions
description: 'Git commit message and branch conventions for Momics/iroh-http. USE FOR: every commit, branch creation, and PR title. Ensures Conventional Commits format for changelog generation with git-cliff. DO NOT USE FOR: general coding or issue management.'
---

# Git Conventions — Momics/iroh-http

## When to use

Always. Every commit message, branch name, and PR title must follow these conventions.

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/) v1.0.0.

### Format

```
<type>(<scope>): <short description>

<optional body>

<optional footer>
```

### Types

| Type | When |
|------|------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf` | Performance improvement |
| `docs` | Documentation only |
| `test` | Adding or updating tests |
| `ci` | CI/CD workflow changes |
| `build` | Build system, dependencies, scripts |
| `chore` | Maintenance that doesn't fit above (formatting, .gitignore, etc.) |

### Scope (optional but encouraged)

Use the package or area name: `core`, `node`, `deno`, `tauri`, `shared`, `discovery`.

Examples:
- `feat(core): add connection statistics to PeerStats`
- `fix(node): map all PeerStats fields in napi bridge`
- `ci: disable workflows for local-only development`

### Rules

1. **Subject line:** imperative mood, lowercase, no period, max 72 chars
2. **Body:** wrap at 80 chars, explain *what* and *why* (not *how*)
3. **Footer:** reference issues with `Closes #N` or `Fixes #N`
4. **Breaking changes:** add `!` after type/scope: `feat(core)!: rename Peer-Id header`
5. **Multi-scope changes:** use the most significant scope, or omit scope

### Examples

```
feat(core): add QUIC connection stats to PeerStats

Expose rttMs, bytesSent, bytesReceived, lostPackets, sentPackets,
and congestionWindow from the pooled QUIC connection. Fields are
null before the first fetch.

Closes #3
```

```
fix(node): replace generic error with INVALID_HANDLE code

Users saw "invalid endpoint handle" with no context. Now returns
structured { code: "INVALID_HANDLE", message: "node closed or
not found (handle 42)" }.

Closes #5
```

```
chore: remove local issues/ folder

All issues tracked on GitHub now. Removes 136 files.
```

## Branch names

Format: `<type>/<short-kebab-description>`

Examples:
- `feat/connection-stats`
- `fix/invalid-handle-error`
- `docs/open-source-checklist`

## PR titles

Same format as commit subject lines. GitHub uses the PR title as the merge commit message.
