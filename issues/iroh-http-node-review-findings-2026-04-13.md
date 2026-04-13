# iroh-http-node line-by-line review findings

Date: 2026-04-13  
Package: `packages/iroh-http-node`

## Findings

1. **P1: `disableNetworking` is ignored in Node adapter unless `relayMode === "disabled"`**
   - `createNode()` computes `disableNetworking` from `relayMode` and does not merge `options.disableNetworking`.
   - This makes `createNode({ disableNetworking: true })` ineffective.
   - References:
     - `packages/iroh-http-node/lib.ts:325`
     - `packages/iroh-http-node/lib.ts:350`

2. **P1: Windows packaging path is broken**
   - `package.json` declares Windows targets, but this package snapshot does not ship Windows `.node` binaries and has no `optionalDependencies` fallback package declarations.
   - Generated loader still attempts `require('@momics/iroh-http-node-win32-x64-msvc')`.
   - References:
     - `packages/iroh-http-node/package.json:9`
     - `packages/iroh-http-node/package.json:34`
     - `packages/iroh-http-node/index.js:65`

3. **P2: Compression options are partially wired**
   - `compressionLevel` is accepted in the FFI options type but never used.
   - Compression is enabled only when `compressionMinBodyBytes` is set.
   - This means `compression: true` (or object with only `level`) does not enable compression as expected.
   - References:
     - `packages/iroh-http-node/src/lib.rs:93`
     - `packages/iroh-http-node/src/lib.rs:165`
     - `packages/iroh-http-node/lib.ts:354`

4. **P3: README options example is out of sync with actual API shape**
   - Docs show `relays` and `discovery: { mdns: true, serviceName: ... }`, which does not match current typed options (`relayMode` and `discovery.mdns` object form).
   - Reference:
     - `packages/iroh-http-node/README.md:45`

## Validation notes

- `npm run typecheck` passed in `packages/iroh-http-node`.
- `npm test` failed early in smoke setup due to bind error in this environment, so e2e/compliance were not reached.
