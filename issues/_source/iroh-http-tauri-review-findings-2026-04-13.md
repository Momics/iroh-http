# iroh-http-tauri line-by-line review findings

Date: 2026-04-13  
Package: `packages/iroh-http-tauri`

## Findings

1. **P1: Broken CommonJS export path**
   - `require("@momics/iroh-http-tauri")` resolves to `dist/index.cjs`, but that file is not produced/present.
   - This causes CommonJS consumers to fail at runtime with `MODULE_NOT_FOUND`.
   - References:
     - `packages/iroh-http-tauri/package.json:10`
     - `packages/iroh-http-tauri/dist/` (missing `index.cjs`)

2. **P1: `maxPooledConnections` / `poolIdleTimeoutMs` are silently ignored in Tauri**
   - Guest JS sends both fields, but Rust command args do not define them and `NodeOptions` hardcodes both to `None`.
   - This makes pool tuning knobs appear supported while having no effect.
   - References:
     - `packages/iroh-http-tauri/guest-js/index.ts:485`
     - `packages/iroh-http-tauri/guest-js/index.ts:486`
     - `packages/iroh-http-tauri/src/commands.rs:23`
     - `packages/iroh-http-tauri/src/commands.rs:88`
     - `packages/iroh-http-tauri/src/commands.rs:89`

3. **P2: `session_connect` drops invalid `direct_addrs` instead of rejecting**
   - Invalid socket strings are silently dropped with `filter_map(...ok())` rather than surfaced as input errors.
   - This can mask misconfiguration and cause confusing fallback behavior.
   - Reference:
     - `packages/iroh-http-tauri/src/commands.rs:473`

4. **P2: Default permission set omits exposed command groups (session/mDNS/crypto)**
   - `iroh-http:default` only grants base fetch/serve commands, but plugin command registration includes session, mDNS, and crypto commands.
   - README tells users to apply only `iroh-http:default`, leading to permission failures on those APIs.
   - References:
     - `packages/iroh-http-tauri/permissions/default.toml:5`
     - `packages/iroh-http-tauri/build.rs:32`
     - `packages/iroh-http-tauri/README.md:50`

5. **P3: Lifecycle listener cleanup is never used**
   - `installLifecycleListener` returns an unsubscribe function, but `createNode` does not store or call it.
   - This risks stale listeners and redundant ping attempts after node shutdown.
   - References:
     - `packages/iroh-http-tauri/guest-js/index.ts:317`
     - `packages/iroh-http-tauri/guest-js/index.ts:345`
     - `packages/iroh-http-tauri/guest-js/index.ts:544`

## Validation notes

- `cargo check` passed in `packages/iroh-http-tauri`.
- `npm run typecheck --workspace=@momics/iroh-http-tauri` passed.
- `node -e "require('@momics/iroh-http-tauri')"` failed due to missing `dist/index.cjs`.
