---
name: manage-issues
description: 'Create, update, and close GitHub issues on Momics/iroh-http. USE FOR: filing bugs or feature requests, closing issues with commit links, triaging issues with labels and priority. Ensures consistent structure (Summary, Evidence, Impact, Remediation, Acceptance criteria) and correct label usage. DO NOT USE FOR: general coding, PR creation, or release management.'
---

# Manage GitHub Issues — Momics/iroh-http

## Repository

- **Owner:** Momics
- **Repo:** iroh-http
- **Issue tracker:** GitHub Issues (no local issue files)

## When to use

- User asks to file a bug or feature request
- You discover a problem during implementation that should be tracked
- User asks to close / update / triage issues
- After fixing a bug — to close the issue with a linked commit

## Labels

Every issue gets **exactly one priority** and **at least one area** label.

### Priority (pick one)

| Label | When |
|-------|------|
| `P1` | Blocks users, breaks protocol, data loss |
| `P2` | Important but has workaround |
| `P3` | Minor, cosmetic, nice-to-have |

### Area (pick one or more)

| Label | Scope |
|-------|-------|
| `api` | API design, ergonomics, type signatures |
| `protocol` | Wire format, ALPN, `httpi://` scheme, header spec |
| `connectivity` | Peer discovery, QUIC connections, relay, hole punching |
| `observability` | Stats, metrics, logging, `peerStats()` |
| `dx` | Developer experience, error messages, docs clarity |

### Type (pick one)

| Label | When |
|-------|------|
| `bug` | Something is broken |
| `enhancement` | New feature or improvement |
| `documentation` | Docs-only change |

## Creating an issue

Use the GitHub MCP tools. Structure the body as markdown with these sections:

```markdown
## Summary

[One paragraph: what's wrong or what's needed]

## Evidence

[File paths, error messages, test output, or user reports that demonstrate the problem]

## Impact

[Who is affected, how severely, any workarounds]

## Remediation

[Suggested fix steps, files to change, or open questions]

## Acceptance criteria

[Numbered list of concrete conditions that mean "done"]
```

### Example tool call

```
mcp_github-mcp_list_issues  → check for duplicates first
mcp_github-mcp_issue_write  → method: "create", title, body, labels: ["bug", "P2", "connectivity"]
```

## Closing an issue

When closing after a fix:

1. **Add a comment** linking the commit with a full URL so it renders as a clickable link:
   ```
   Fixed in [abc1234](https://github.com/Momics/iroh-http/commit/abc1234) — `commit message`.

   [Brief description of what changed]
   ```
2. **Close the issue** with `state: "closed"` and `state_reason: "completed"`.

### Regression test policy

Per the repo's Issue Resolution Policy (in `copilot-instructions.md`):

| Bug type | Where to add test |
|----------|-------------------|
| FFI boundary | `adapter.test.ts` (Deno) or `adapter.test.mjs` (Node) |
| Rust core | `cargo test` — `integration.rs` or new test file |
| Type/export | Verified by `tsc` (no new test if CI gates it) |
| Protocol | `cases.json` in `tests/http-compliance/` |
| Docs/config | N/A — note in closing comment |

## Updating an issue

Use `mcp_github-mcp_issue_write` with `method: "update"` to change title, body, labels, or state.

Use `mcp_github-mcp_add_issue_comment` to add progress notes or ask questions.

## Searching for issues

Before creating, always search first:
```
mcp_github-mcp_search_issues → query: "repo:Momics/iroh-http <keywords>"
```

## Rules

1. Never create duplicate issues — search first.
2. Every issue gets a priority label and at least one area label.
3. Titles are imperative and specific: "Add rttMs field to PeerStats" not "stats improvement".
4. Commit links in closing comments must be full URLs (clickable in GitHub UI).
5. Don't create issues for trivial changes that can be done inline.
