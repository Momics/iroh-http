---
status: integrated
---

# iroh-http — Patch 11: Open-Source Readiness

Everything needed before the repository goes public: license, documentation,
CI, example apps, and a final pre-launch checklist.

---

## 1. License

Dual MIT / Apache-2.0 — standard for Rust ecosystem projects, compatible
with npm/JSR/PyPI, and maximally permissive for adoption.

Add to repo root:

- `LICENSE-MIT`
- `LICENSE-APACHE`

Add to each `Cargo.toml`:
```toml
license = "MIT OR Apache-2.0"
```

Add to each `package.json`:
```json
"license": "MIT OR Apache-2.0"
```

---

## 2. Root README.md

Structure:

```markdown
# iroh-http

Peer-to-peer HTTP — fetch and serve between devices using
[Iroh](https://iroh.computer) QUIC transport. No servers, no DNS, no TLS
certificates. Nodes are addressed by public key.

## How it works

[2-sentence architecture summary + simple ASCII diagram]

  ┌──────────┐   QUIC (Iroh)   ┌──────────┐
  │  Node A  │◄────────────────►│  Node B  │
  └──────────┘                  └──────────┘
  fetch("/api")                 serve(handler)

## Quick start

### Node.js
\`\`\`bash
npm install @momics/iroh-http-node
\`\`\`
\`\`\`ts
import { createNode } from "@momics/iroh-http-node"
const node = await createNode()
node.serve({}, () => new Response("hello"))
\`\`\`

### Deno
\`\`\`bash
deno add @momics/iroh-http-deno
\`\`\`
\`\`\`ts
import { createNode } from "@momics/iroh-http-deno"
\`\`\`

### Tauri
\`\`\`bash
npm install @momics/iroh-http-tauri
cargo add tauri-plugin-iroh-http
\`\`\`

### Python
\`\`\`bash
pip install iroh-http
\`\`\`

## Packages

| Package | Registry | Description |
|---------|----------|-------------|
| [@momics/iroh-http-node](packages/iroh-http-node) | npm | Node.js adapter |
| [@momics/iroh-http-deno](packages/iroh-http-deno) | JSR | Deno adapter |
| [@momics/iroh-http-tauri](packages/iroh-http-tauri) | npm + crates.io | Tauri plugin |
| [iroh-http](packages/iroh-http-py) | PyPI | Python bindings |

## Architecture

[Link to docs/patches/00_brief.md or a condensed version]

## License

MIT OR Apache-2.0
```

---

## 3. Per-package READMEs

Each package gets a focused README with:

1. One-liner description
2. Platform-specific install command
3. Minimal example (3–5 lines)
4. Configuration options (table of `NodeOptions`)
5. Link to root README for full docs

### `packages/iroh-http-node/README.md`

```markdown
# @momics/iroh-http-node

Peer-to-peer HTTP for Node.js.

## Install
\`npm install @momics/iroh-http-node\`

## Usage
\`\`\`ts
import { createNode } from "@momics/iroh-http-node"

const alice = await createNode()
const bob = await createNode()

bob.serve({}, () => new Response("hello from bob"))
const res = await alice.fetch(bob.publicKey.toString(), "/")
console.log(await res.text()) // "hello from bob"

await alice.close()
await bob.close()
\`\`\`

## Options
| Option | Type | Default | Description |
|--------|------|---------|-------------|
| key | SecretKey \| Uint8Array | generated | Identity key |
| idleTimeout | number | 30000 | Connection idle timeout (ms) |
| discovery | DiscoveryOptions | undefined | mDNS local discovery |
| drainTimeout | number | 30000 | Body stream drain timeout (ms) |

[Full documentation →](../../README.md)
```

### `packages/iroh-http-tauri/README.md`

Includes both Rust and JS setup instructions:

```markdown
# @momics/iroh-http-tauri

Tauri plugin for peer-to-peer HTTP.

## Install

### Rust (src-tauri)
\`\`\`bash
cargo add tauri-plugin-iroh-http
\`\`\`

\`\`\`rust
// src-tauri/lib.rs
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_iroh_http::init())
        .run(tauri::generate_context!())
        .expect("error running app");
}
\`\`\`

### JavaScript
\`\`\`bash
npm install @momics/iroh-http-tauri
\`\`\`

## Usage
\`\`\`ts
import { createNode } from "@momics/iroh-http-tauri"

const node = await createNode()
node.serve({}, (req) => new Response("hello"))
\`\`\`

## Permissions
Add to \`src-tauri/capabilities/default.json\`:
\`\`\`json
{ "permissions": ["iroh-http:default"] }
\`\`\`

## Mobile
Requires Tauri v2 mobile support. mDNS discovery on iOS/Android uses
native service discovery (NWBrowser / NsdManager). See patch 06 for details.
```

### `packages/iroh-http-deno/README.md`

```markdown
# @momics/iroh-http-deno

Peer-to-peer HTTP for Deno.

## Install
\`deno add @momics/iroh-http-deno\`

## Usage
\`\`\`ts
import { createNode } from "@momics/iroh-http-deno"

const node = await createNode()
node.serve({}, () => new Response("hello"))
\`\`\`

## Permissions
Requires \`--allow-ffi\` and \`--unstable-ffi\` flags.

[Full documentation →](../../README.md)
```

---

## 4. CONTRIBUTING.md

```markdown
# Contributing

## Development setup

### Prerequisites
- Rust (stable)
- Node.js 18+
- Deno 2+
- pnpm (for workspace management)

### Build
\`\`\`bash
cargo build                    # Rust crates
npm run build --workspaces     # TypeScript packages
deno task build                # Deno native library
\`\`\`

### Test
\`\`\`bash
cargo test                     # Rust tests
npm test --workspaces          # JS tests
deno test                      # Deno tests
\`\`\`

### Code style
- Rust: `cargo fmt` + `cargo clippy`
- TypeScript: project tsconfig strict mode
- Commits: conventional commits preferred

## Filing issues
[standard guidance]

## Pull requests
[standard guidance]
```

---

## 5. GitHub Actions CI

`.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]

jobs:
  rust:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --workspace
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check

  node:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - uses: dtolnay/rust-toolchain@stable
      - run: npm ci --workspaces
      - run: npm run typecheck --workspaces
      - run: npm run build --workspaces

  deno:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: denoland/setup-deno@v2
      - uses: dtolnay/rust-toolchain@stable
      - run: deno task build
      - run: deno check mod.ts
        working-directory: packages/iroh-http-deno

  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-audit
      - run: cargo audit
```

---

## 6. Example apps

### `examples/node/`

```
examples/node/
├── package.json
└── index.ts          # Two nodes, fetch + serve, console output
```

Minimal self-contained demo: create two nodes, one serves, the other
fetches. Print the response. ~20 lines.

### `examples/deno/`

```
examples/deno/
├── deno.jsonc
└── main.ts           # Same as node example, Deno-style
```

### `examples/tauri/`

```
examples/tauri/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/main.rs
├── src/
│   └── App.tsx       # Simple UI with connect + send message
├── package.json
└── index.html
```

A minimal Tauri app: text input for peer ID, button to send a message,
display received messages. Uses `@momics/iroh-http-tauri`.

### `examples/python/`

```
examples/python/
├── requirements.txt
└── main.py           # Two nodes, fetch + serve
```

---

## 7. .gitignore

Ensure coverage:

```gitignore
# Rust
/target/
**/*.rs.bk

# Node
node_modules/
dist/
*.node

# Deno
lib/*.dylib
lib/*.so
lib/*.dll

# Python
__pycache__/
*.egg-info/
*.whl

# IDE
.vscode/settings.json
.idea/

# OS
.DS_Store
Thumbs.db
```

---

## 8. Pre-launch checklist

- [ ] LICENSE-MIT and LICENSE-APACHE in repo root
- [ ] `license` field in all `Cargo.toml` and `package.json` files
- [ ] Root README.md
- [ ] README.md in every publishable package
- [ ] CONTRIBUTING.md
- [ ] `.github/workflows/ci.yml` passing on all platforms
- [ ] `.gitignore` covers all artifacts
- [ ] No secrets, keys, or credentials in git history (`git log --all -p | grep -i secret`)
- [ ] `.old_references/` excluded via `.gitignore` (or removed from repo)
- [ ] `target/` not committed (add to `.gitignore`, remove if present)
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` is clean
- [ ] `npm run typecheck --workspaces` passes
- [ ] `deno check packages/iroh-http-deno/mod.ts` passes
- [ ] All `version` fields aligned at `0.1.0`
- [ ] Package names available: check npm (`@momics/iroh-http-node` etc.), JSR, crates.io, PyPI
- [ ] Example apps build and run for each platform
- [ ] `cargo audit` clean
- [ ] Changelog or RELEASES.md (even if just "0.1.0 — initial release")
- [ ] Repository description and topics set on GitHub
- [ ] Branch protection on `main` (require CI pass + review)
