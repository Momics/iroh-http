---
date: 2026-04-13
status: open
---

# API Surface Parity Analysis

Comparison of exported developer-facing APIs across all four platform packages: Node, Deno, Tauri, and Python.

---

## Top-level exports

| Export | Node | Deno | Tauri | Python |
|--------|------|------|-------|--------|
| `createNode` / `create_node` | ✅ | ✅ | ✅ | ✅ |
| `generateSecretKey` / `generate_secret_key` | ✅ | ✅ | ❌ | ✅ |
| `secretKeySign` / `secret_key_sign` | ✅ | ✅ | ❌ | ✅ |
| `publicKeyVerify` / `public_key_verify` | ✅ | ✅ | ❌ | ✅ |
| `IrohNode` (type/class) | ✅ (type) | ✅ (type) | ✅ (type) | ✅ (class) |
| `NodeOptions` (type) | ✅ | ✅ | ✅ | — |

---

## `IrohNode` method/property surface

| Member | Node | Deno | Tauri | Python | Notes |
|--------|------|------|-------|--------|-------|
| `publicKey` | ✅ | ✅ | ✅ | ❌ | JS has `PublicKey` object; Python has `node_id: str` |
| `secretKey` | ✅ | ✅ | ✅ | ❌ | JS has `SecretKey` object; Python has `keypair: bytes` |
| `node_id` / `nodeId` | ✅ (deprecated) | ✅ (deprecated) | ✅ (deprecated) | ✅ | Flat string version |
| `keypair` | ✅ (deprecated) | ✅ (deprecated) | ✅ (deprecated) | ✅ | Raw bytes version |
| `fetch()` | ✅ | ✅ | ✅ | ✅ | JS uses `RequestInit`; Python has positional args |
| `serve()` | ✅ → `ServeHandle` | ✅ → `ServeHandle` | ✅ → `ServeHandle` | ✅ → `None` | Python returns nothing; use `stop_serve()` instead |
| `stop_serve()` | ❌ | ❌ | ❌ | ✅ | JS exposes this on `ServeHandle` |
| `connect()` | ✅ → `IrohSession` | ✅ → `IrohSession` | ✅ → `IrohSession` | ✅ → `IrohSession` | |
| `browse()` | ✅ → async iterable | ✅ → async iterable | ✅ → async iterable | ✅ → `IrohBrowseSession` | JS accepts `AbortSignal`; Python does not |
| `advertise()` | ✅ → `Promise<void>` | ✅ → `Promise<void>` | ✅ → `Promise<void>` | ✅ → `None` | JS is cancellable via signal |
| `addr()` | ✅ async | ✅ async | ✅ async | ✅ **sync** | Async/sync mismatch |
| `ticket()` | ✅ async | ✅ async | ✅ async | ✅ **sync** | Async/sync mismatch |
| `homeRelay()` / `home_relay()` | ✅ async | ✅ async | ✅ async | ✅ **sync** | Async/sync mismatch |
| `peerInfo()` / `peer_info()` | ✅ async | ✅ async | ✅ async | ✅ async | |
| `peerStats()` / `peer_stats()` | ✅ async | ✅ async | ✅ async | ✅ async | |
| `pathChanges()` | ✅ | ✅ | ✅ | ❌ | Not implemented in Python |
| `closed` (property) | ✅ | ✅ | ✅ | ❌ | Python has no "node died" signal |
| `close()` | ✅ async | ✅ async | ✅ async | ✅ async | |
| `[Symbol.asyncDispose]` | ✅ | ✅ | ✅ | ❌ | Python uses `__aenter__`/`__aexit__` instead |

---

## Summary of gaps

### Tauri missing vs. Node/Deno

- No `generateSecretKey`, `secretKeySign`, `publicKeyVerify` at top-level. Crypto utilities are available natively in the WebView but no iroh-http wrapper is exported.

### Python missing vs. JS

- `publicKey` / `secretKey` wrapper objects — only raw `node_id: str` and `keypair: bytes` are exposed.
- `pathChanges()` method is absent entirely.
- `closed` promise / lifecycle signal is absent.
- `[Symbol.asyncDispose]` → context manager exists (`async with node:`) but is a different mechanism.
- `serve()` returns `None`; there is no `ServeHandle` with `finished`, `onListen`, `onError`, or signal-based stop.

### Python async/sync inconsistencies

`addr()`, `ticket()`, and `home_relay()` are **synchronous** in Python but **async** in all JS platforms. This is the most surprising divergence and is likely to cause confusion when porting code between platforms.
