---
name: fix-issues
description: 'Systematically resolve open GitHub issues on Momics/iroh-http: fetch open issues, triage by priority, group combinable fixes, implement one group at a time, run local CI (npm run ci), commit with issue reference, post commit link and close the issue, then push all commits in one go. USE FOR: "fix open issues", "work through the backlog", "resolve issues", "tackle the issue list". DO NOT USE FOR: creating new issues (use manage-issues), one-off bug fixes without issue context, or PR creation.'
---

# Fix Issues — Momics/iroh-http

Resolve open GitHub issues systematically: triage → plan → fix → verify → commit → close → push.

## Phase 1 — Discover

Fetch all open issues:

```
mcp_github-mcp_list_issues → owner: Momics, repo: iroh-http, state: OPEN, perPage: 100
```

Read the full body of any issue that lacks sufficient detail before planning.

## Phase 2 — Triage

Sort by priority label: **P1 first**, then P2, then P3. Within a priority tier, prefer issues that:
- Touch fewer files (lower risk)
- Have clear acceptance criteria
- Are not blocked on another open issue

**Skip (defer to a future session):**
- Issues with no labels — triage them first using the `manage-issues` skill
- Issues that require significant architectural decisions without prior analysis
- Issues where the fix would touch the same files as another planned fix and conflict

Record the final ordered work plan before proceeding. Use session memory if the list is long.

## Phase 3 — Group

Decide which issues to combine into a single commit vs. keep separate.

**Combine when:**
- Changes touch the same file(s) and the diff would be reviewed as one unit
- Fixes are logically inseparable (fixing one makes no sense without the other)
- Same crate/package, same type of change (e.g., two CI config corrections, two clippy lints)

**Keep separate when:**
- Different concerns that would produce a muddled commit message
- One fix might cause the other's CI to fail (fix separately to keep bisectability)
- Different scopes — keep `git blame` clean for future diagnostics
- One is a `fix`, the other is a `refactor` or `ci`

Label each group in your plan before writing any code.

## Phase 4 — Fix loop

For each group, in plan order:

### 4a. Read before touching
Read all relevant files in the issue's Evidence section. Understand the existing code before changing anything.

### 4b. Implement
Make the minimal change that satisfies the acceptance criteria. Do not refactor adjacent code, add unrelated docs, or widen scope. If the fix reveals a deeper problem, file a new issue rather than expanding this one.

### 4c. Verify locally

```
npm run ci
```

This runs: full release build → `scripts/check.sh` (fmt + clippy strict + cargo test workspace + cargo test tauri + bench smoke + feature checks + typecheck) → Node e2e → Deno tests → interop suite.

**If CI fails:**
- Fix the failure before moving on — never commit broken code
- If the failure is pre-existing and unrelated to this issue, note it and decide: fix it in the same commit (if trivial), open a new issue, or skip this group if it blocks verification

### 4d. Commit

Follow the `git-conventions` skill for commit message format. Every commit that resolves one or more issues must include a `Closes #N` or `Fixes #N` footer for each issue resolved.

```
fix(scope): short description

Body explaining what and why.

Closes #42
Closes #43
```

After committing, record the commit hash.

### 4e. Close the issue

For each issue resolved in this commit:

1. Post a comment:
   ```
   Fixed in [<short-hash>](https://github.com/Momics/iroh-http/commit/<full-hash>) — `<commit subject>`.

   <One sentence describing what changed and why.>
   ```
2. Close with `mcp_github-mcp_issue_write` → `state: "closed"`, `state_reason: "completed"`.

Then move to the next group.

## Phase 5 — Push

Only after **all** planned groups are committed and their issues are closed:

```
git push origin main
```

Do not push after each individual commit. The single push keeps the remote history clean and avoids partial states if CI is interrupted.

## Guardrails

- Never push with failing CI
- Never close an issue before the commit that fixes it is made
- Never combine issues whose fixes conflict — this produces a commit that is hard to revert
- If a fix turns out to be larger than expected mid-implementation, stop, file a more detailed issue, and skip that group for this session
- Amend the commit (not a new commit) if CI catches something in the immediately preceding fix before moving on

## Related skills

- [manage-issues](./../manage-issues/SKILL.md) — create, label, and structure issues
- [git-conventions](./../git-conventions/SKILL.md) — commit message format and branch naming
