---
status: pending
---

# iroh-http — Patch 18: Documentation & JSDoc Audit

## Goal

Every developer-facing symbol — TypeScript, Rust, and Python — must have
documentation that is indistinguishable in quality from the Web Platform API
docs (MDN). A developer should be able to use this library entirely from IDE
tooltips without ever opening a browser.

The old `iroh` and `http-tauri` packages set the benchmark. Their doc style
should be the direct model.

---

## The Benchmark

Read the old packages before starting. The documentation style to replicate:

- **`iroh/src/`** — especially `IrohConnection.mts`, `IrohEndpoint.mts`,
  `IrohAdapter.mts`, `types.d.ts`, `IrohReceiveStream.mts`, `IrohSendStream.mts`
- **`http-tauri/guest-js/`** — especially `serve.ts`, `types.ts`, `tls.ts`,
  `ServerRequest.ts`, `ServerHandle.ts`, `health.ts`, `fetch.ts`

These files demonstrate:
- A crisp one-sentence summary that stands alone in a tooltip.
- A prose paragraph when the behaviour or contract is non-obvious.
- `@param` with a description for every non-trivial argument.
- `@returns` describing what the resolved value means, not just its type.
- `@throws` for every typed error the function can raise.
- `@default` on every optional field that has a meaningful default.
- `@example` blocks that show real, runnable usage — not pseudocode.
- Inline `/** ... */` on every struct/interface field, however short.
- `@deprecated` with a migration instruction on any symbol being phased out.
- `@internal` to suppress IDE suggestions for symbols that are exported for
  technical reasons but are not part of the public contract.

---

## Quality Bar

A symbol's documentation passes the bar when:

1. **A developer who has never read the source** can understand what it does,
   what it expects, what it returns, and what can go wrong — from the tooltip
   alone.
2. **Every optional field or parameter** has a `@default` value noted.
3. **Every function that can throw** lists at least one `@throws` tag.
4. **Every public function** has at least one `@example` showing realistic,
   copy-paste-ready usage.
5. **Non-obvious behaviour is explained in prose** — not left as an exercise
   for the reader. If a developer reading the docs for the first time would
   have a question, the answer belongs in the doc.
6. **The summary line is precise**, not generic. `"Fetch data."` fails.
   `"Send an HTTP request to a remote Iroh node and return its response."` passes.

---

## Scope

### TypeScript / JavaScript (iroh-http-shared, iroh-http-node, iroh-http-tauri, iroh-http-deno)

Every `export`ed symbol: classes, interfaces, type aliases, enum members,
standalone functions, and every field/method within them.

Doc format: `/** ... */` JSDoc. Single-line `//` comments on exported symbols
do not satisfy the requirement.

Special note on **napi bindings** (`iroh-http-node/src/lib.rs`): napi-rs
copies `///` Rust doc comments verbatim into the generated `index.d.ts` that
is shipped to npm. Every `#[napi]` function and every field on a
`#[napi(object)]` struct must have a `///` doc comment so that npm consumers
see documentation in their IDE without reading Rust source.

### Rust (iroh-http-core, iroh-http-node/src, iroh-http-tauri/src)

Every `pub` item: functions, structs, enums, their fields and variants.

Doc format: `///` line comments. Structs must have `///` on every field, not
just on the struct itself.

### Python (iroh-http-py)

Every public class, method, and module-level function.

Doc format: Google-style docstrings with `Args:`, `Returns:`, `Raises:`, and
`Example:` sections.

---

## Process

The audit must be performed **file by file** rather than symbol by symbol, to
catch context that spans multiple symbols in the same file (e.g. a type that
is only meaningful when understood alongside the function that uses it). For
each file:

1. Read the entire file.
2. Enumerate every exported / public symbol.
3. For each symbol: does it meet the quality bar above? If not, write the doc.
4. Cross-check against the old reference packages — if something similar exists
   in `iroh/src/` or `http-tauri/guest-js/`, the style and depth should match.
5. After writing, re-read the file as a developer encountering it for the first
   time. If a question arises, the answer belongs in the docs.

Start with the highest developer-traffic files:

1. The `createNode` entry point in each platform package.
2. The `IrohNode` interface and `NodeOptions` — the two types every developer
   will read first.
3. Error classes — critical for `catch` block ergonomics.
4. Key types (`PublicKey`, `SecretKey`) — identity is a primary concept.
5. Remaining shared types (`BidirectionalStream`, `DuplexStream`, serve/fetch
   types, stream helpers).
6. Rust `pub` structs and functions in `iroh-http-core`.
7. napi `#[napi]` functions (generates the npm `.d.ts`).
8. Python public API.

---

## Out of Scope

- Private / internal symbols (`pub(crate)`, unexported TS, `_`-prefixed Python)
- Generated files (`*.d.ts` from napi, `index.js`, build artifacts)
- Test files
- `iroh-http-framing` (no developer-facing surface)
- Comments inside function bodies
