---
status: pending
---

# iroh-http — Patch 10: DX & Packaging

Folder structure cleanup, npm/JSR/crates.io/PyPI naming, and the publishing
pipeline for all packages.

---

## 1. Folder structure changes

### Deno: `guest-ts/` → standard Deno layout

Move:
- `packages/iroh-http-deno/guest-ts/mod.ts` → `packages/iroh-http-deno/mod.ts`
- `packages/iroh-http-deno/guest-ts/adapter.ts` → `packages/iroh-http-deno/src/adapter.ts`
- Remove empty `guest-ts/` directory

Update `deno.jsonc`:
```json
{
  "exports": "./mod.ts"
}
```

Update `mod.ts` import:
```ts
import { ... } from "./src/adapter.ts";
```

**Rationale:** Deno's convention is `mod.ts` at package root, internal code
in `src/`. The `guest-ts` name was borrowed from Tauri's convention but
doesn't apply to Deno.

### Node: keep as-is

`index.ts` at root next to the generated `.node` binary is standard for
napi-rs packages. No change.

### Tauri: keep as-is

`guest-js/` is the canonical Tauri v2 plugin convention. No change.

### Shared: keep as-is

`src/` for a pure TypeScript library is standard. No change.

---

## 2. Package naming

### npm scope: `@momics`

All JS packages that publish to npm use the `@momics` scope.

| Package dir | npm name | Registry |
|------------|----------|----------|
| `packages/iroh-http-shared` | `@momics/iroh-http-shared` | npm |
| `packages/iroh-http-node` | `@momics/iroh-http-node` | npm |
| `packages/iroh-http-tauri` | `@momics/iroh-http-tauri` | npm |

### JSR: `@momics`

| Package dir | JSR name | Registry |
|------------|----------|----------|
| `packages/iroh-http-deno` | `@momics/iroh-http-deno` | JSR |

`@momics/iroh-http-shared` also publishes to JSR so the Deno package can import
it without npm compatibility layers.

### crates.io

| Crate dir | crates.io name |
|-----------|---------------|
| `crates/iroh-http-framing` | `iroh-http-framing` |
| `crates/iroh-http-core` | `iroh-http-core` |
| `crates/iroh-http-discovery` | `iroh-http-discovery` |
| `packages/iroh-http-tauri` (Rust side) | `tauri-plugin-iroh-http` |
| `packages/iroh-http-node` (Rust side) | `iroh-http-node` |
| `packages/iroh-http-deno` (Rust side) | `iroh-http-deno` |

Note: Tauri Rust plugins follow the `tauri-plugin-*` naming convention on
crates.io.

### PyPI

| Package dir | PyPI name |
|------------|-----------|
| `packages/iroh-http-py` | `iroh-http` |

---

## 3. `package.json` updates

### `@momics/iroh-http-shared`

```json
{
  "name": "@momics/iroh-http-shared",
  "version": "0.1.0",
  "description": "Shared TypeScript layer for iroh-http",
  "exports": {
    ".": {
      "import": "./dist/index.js",
      "require": "./dist/index.cjs",
      "types": "./dist/index.d.ts"
    }
  },
  "files": ["dist"],
  "publishConfig": { "access": "public" }
}
```

### `@momics/iroh-http-node`

```json
{
  "name": "@momics/iroh-http-node",
  "version": "0.1.0",
  "description": "Peer-to-peer HTTP for Node.js — powered by Iroh",
  "main": "./index.js",
  "types": "./index.d.ts",
  "files": ["index.js", "index.d.ts", "*.node"],
  "dependencies": { "@momics/iroh-http-shared": "workspace:*" },
  "publishConfig": { "access": "public" }
}
```

### `@momics/iroh-http-tauri`

```json
{
  "name": "@momics/iroh-http-tauri",
  "version": "0.1.0",
  "description": "Tauri plugin for iroh-http peer-to-peer HTTP",
  "exports": {
    ".": {
      "import": "./dist/index.js",
      "require": "./dist/index.cjs",
      "types": "./dist/index.d.ts"
    }
  },
  "files": ["dist"],
  "dependencies": { "@momics/iroh-http-shared": "workspace:*" },
  "peerDependencies": { "@tauri-apps/api": "^2" },
  "publishConfig": { "access": "public" }
}
```

### `@momics/iroh-http-deno` — `deno.jsonc`

```json
{
  "name": "@momics/iroh-http-deno",
  "version": "0.1.0",
  "exports": "./mod.ts",
  "publish": {
    "include": ["mod.ts", "src/", "lib/"]
  },
  "imports": {
    "@momics/iroh-http-shared": "jsr:@momics/iroh-http-shared@^0.1"
  }
}
```

The Deno package imports `@momics/iroh-http-shared` from JSR rather than a local
path. For local development, an import map override can point to the local
checkout.

---

## 4. Cargo.toml updates

### Tauri crate name

Rename the Tauri crate to follow the `tauri-plugin-*` convention:

```toml
# packages/iroh-http-tauri/Cargo.toml
[package]
name = "tauri-plugin-iroh-http"
```

### Workspace members

```toml
[workspace]
members = [
    "crates/iroh-http-framing",
    "crates/iroh-http-core",
    "crates/iroh-http-discovery",
    "packages/iroh-http-node",
    "packages/iroh-http-deno",
    "packages/iroh-http-tauri",
    "packages/iroh-http-py",
]
```

Note: `iroh-http-tauri` is added back (it's currently missing from the
workspace members list).

---

## 5. Shared package on JSR

For the Deno package to import `@momics/iroh-http-shared` from JSR, the shared
package needs a JSR publish config. Add a `jsr.jsonc` to
`packages/iroh-http-shared/`:

```json
{
  "name": "@momics/iroh-http-shared",
  "version": "0.1.0",
  "exports": "./src/index.ts",
  "publish": {
    "include": ["src/"]
  }
}
```

JSR accepts TypeScript source directly — no build step needed.

---

## 6. Workspace-level package manager

Add a root `package.json` for workspace-level npm scripts and dependency
management:

```json
{
  "private": true,
  "workspaces": [
    "packages/iroh-http-shared",
    "packages/iroh-http-node",
    "packages/iroh-http-tauri"
  ],
  "scripts": {
    "typecheck": "npm run typecheck --workspaces",
    "build:ts": "npm run build --workspaces",
    "build:rust": "cargo build --release",
    "build": "npm run build:rust && npm run build:ts"
  }
}
```

Deno is not included in npm workspaces since it uses its own toolchain.

---

## 7. Import experience — one import per platform

After all naming changes, the end-user experience is:

```bash
# Node
npm install @momics/iroh-http-node
```
```ts
import { createNode } from "@momics/iroh-http-node"
```

```bash
# Deno
deno add @momics/iroh-http-deno
```
```ts
import { createNode } from "@momics/iroh-http-deno"
```

```bash
# Tauri (JS side)
npm install @momics/iroh-http-tauri
# Tauri (Rust side — Cargo.toml)
# tauri-plugin-iroh-http = "0.1"
```
```ts
import { createNode } from "@momics/iroh-http-tauri"
```

```bash
# Python
pip install iroh-http
```
```python
from iroh_http import create_node
```

Users never need to import `@momics/iroh-http-shared` — it's a transitive
dependency, handled automatically by the package manager.
