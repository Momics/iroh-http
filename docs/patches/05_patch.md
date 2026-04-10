---
status: pending
---

# iroh-http — Patch 05: Key Classes + Structured Errors

Two related changes that improve the JS public API surface. Both are about
wrapping raw primitives in proper classes so user code reads naturally and
type-checkers have something to work with.

---

## 1. Key Classes — `PublicKey` and `SecretKey`

### Problem

The current `IrohNode` interface exposes identity as raw primitives:

```ts
interface IrohNode {
  nodeId: string;          // base32
  keypair: Uint8Array;     // raw 32-byte secret key
}
```

And `fetch` / `createBidirectionalStream` accept a plain `string` for the
peer's node ID:

```ts
node.fetch(peerId: string, url, init?)
```

This has three problems:

1. **No type safety.** A plain `string` cannot distinguish a node ID from any
   other string. Passing a URL where a node ID is expected silently produces a
   confusing Rust-side error (`"invalid base32 char: :"`).
2. **No operations.** Users who want to persist, compare, or display keys
   need to write their own base32 encode/decode. The library already has that
   code (in Rust and in the old JS reference) but doesn't expose it.
3. **No Web Crypto integration.** Ed25519 signing and verification are useful
   for authenticating off-band messages (e.g. proving a peer identity in an
   HTTP header to a third party). The key classes can wrap `crypto.subtle`
   cleanly.

### Design

Two classes, exported from `iroh-http-shared` and re-exported by every
platform package:

```ts
class PublicKey {
  /** Raw 32-byte key material (copy). */
  get bytes(): Uint8Array;

  /** Lowercase base32 string (the "node ID"). */
  toString(): string;

  /** Compare two keys for equality. */
  equals(other: PublicKey): boolean;

  /** Parse from base32 string (case-insensitive). */
  static fromString(s: string): PublicKey;

  /** Construct from 32 raw bytes. */
  static fromBytes(bytes: Uint8Array): PublicKey;

  /** Verify an Ed25519 signature (Web Crypto). */
  verify(data: Uint8Array, signature: Uint8Array): Promise<boolean>;
}
```

```ts
class SecretKey {
  /** Derived public key (available after binding, or after derivePublicKey()). */
  get publicKey(): PublicKey;

  /** Copy of the raw 32-byte secret key material. */
  toBytes(): Uint8Array;

  /** Base32 representation. */
  toString(): string;

  /** Generate a fresh random key using crypto.getRandomValues. */
  static generate(): SecretKey;

  /** Construct from 32 raw bytes. */
  static fromBytes(bytes: Uint8Array): SecretKey;

  /** Parse from base32. */
  static fromString(s: string): SecretKey;

  /** Derive the public key using Web Crypto (async, Ed25519). */
  derivePublicKey(): Promise<PublicKey>;

  /** Sign a message with Ed25519 (Web Crypto). */
  sign(data: Uint8Array): Promise<Uint8Array>;
}
```

#### API changes

The `IrohNode` interface adds typed key properties alongside the existing raw
ones (for backwards compatibility the raw `nodeId` string and `keypair`
`Uint8Array` remain, but are marked `@deprecated`):

```ts
interface IrohNode {
  /** @deprecated Use `publicKey.toString()` instead. */
  nodeId: string;
  /** @deprecated Use `secretKey.toBytes()` instead. */
  keypair: Uint8Array;

  /** The node's public identity. */
  publicKey: PublicKey;
  /** The node's secret key (persist this to restore identity across restarts). */
  secretKey: SecretKey;

  fetch(peer: PublicKey | string, input: string | URL, init?: RequestInit): Promise<Response>;
  // ... rest unchanged
}
```

`fetch`, `createBidirectionalStream`, and `serve` all accept both
`PublicKey` and `string` (base32). Internally, `PublicKey.toString()` is
called before passing to the FFI layer. This is a non-breaking widening of
the parameter type.

The `NodeOptions` interface accepts `SecretKey | Uint8Array` for the `key`
field:

```ts
interface NodeOptions {
  key?: SecretKey | Uint8Array;
  // ...
}
```

#### `buildNode` changes

`buildNode` constructs the `PublicKey` and `SecretKey` from the
`EndpointInfo` that the platform adapter returns:

```ts
const publicKey = PublicKey.fromString(info.nodeId);
const secretKey = SecretKey._fromBytesWithPublicKey(info.keypair, publicKey);
```

#### File location

The key classes and base32 utilities live in `iroh-http-shared/src/keys.ts`.
They have **zero platform dependencies** — only `crypto.subtle` (available in
Node 18+, Deno, Tauri webview, and all modern browsers).

---

## 2. Structured Error Classes

### Problem

Currently all errors from the Rust layer reach JS as plain `Error` objects
with a message string containing the Rust error text. There is no way for
user code to `catch` specific failure categories:

```ts
try {
  await node.fetch(peerId, "/api");
} catch (e) {
  // e is Error { message: "connect: connection timed out" }
  // No .code, no .name other than "Error", no instanceof check possible
}
```

The Rust side uses `Result<T, String>` everywhere — all type information is
lost at the FFI boundary. The napi layer maps every error to
`napi::Error::new(Status::GenericFailure, msg)`. The Tauri side wraps them in
a Tauri `Error`. In both cases, JS receives a generic `Error`.

The guidelines say: _"use `DOMException` names (`AbortError`, `NetworkError`,
`TypeError`) where applicable"_. The review (`00_review.md`) flagged that
errors other than `AbortError` have no structure.

### Design

#### Error taxonomy

The errors that iroh-http can produce fall into a small, finite set:

| Category | When | Example message |
|---|---|---|
| **`IrohBindError`** | `createNode` fails | endpoint bind failure, invalid key bytes |
| **`IrohConnectError`** | `fetch` / `createBidirectionalStream` can't reach the peer | DNS resolution, QUIC handshake, timeout |
| **`IrohStreamError`** | body read/write fails mid-stream | stream reset, writer dropped |
| **`IrohProtocolError`** | HTTP framing parse failure | malformed head, too many headers |
| **`AbortError`** | `AbortSignal` fires | _(already a DOMException)_ |
| **`TypeError`** | invalid argument at the JS boundary | FormData body not supported |

#### Error class hierarchy

```ts
/** Base class for all iroh-http errors. */
class IrohError extends Error {
  /** Machine-readable error code string. */
  code: string;
  constructor(message: string, code: string);
}

class IrohBindError extends IrohError { }
class IrohConnectError extends IrohError { }
class IrohStreamError extends IrohError { }
class IrohProtocolError extends IrohError { }
```

All classes set `this.name` to the class name (`"IrohBindError"`, etc.) so
`err.name` and `err instanceof IrohBindError` both work.

`AbortError` stays as a `DOMException` (web standard). `TypeError` stays as
a native `TypeError`.

#### Error code strings

Each error class uses a small set of codes:

```
IrohBindError:
  INVALID_KEY       — key bytes were wrong length or failed validation
  ENDPOINT_FAILURE  — Iroh endpoint could not bind (port, relay, etc.)

IrohConnectError:
  DNS_FAILURE       — could not resolve the peer's address
  TIMEOUT           — QUIC handshake timed out
  REFUSED           — peer reset the connection
  ALPN_MISMATCH     — peer does not support required capabilities

IrohStreamError:
  STREAM_RESET      — QUIC stream was reset by the peer
  WRITER_DROPPED    — the body channel's consumer was dropped
  INVALID_HANDLE    — slab handle is no longer valid

IrohProtocolError:
  PARSE_FAILURE     — HTTP head bytes could not be parsed
  TOO_MANY_HEADERS  — header count exceeded the limit (64)
  UPGRADE_REJECTED  — server returned non-101 for duplex request
```

#### How strings become structured errors

The Rust layer already prefixes its error strings with contextual labels:
`"connect: …"`, `"open_bi: …"`, `"parse response head: …"`, etc. The JS
shared layer introduces a thin mapping function:

```ts
function classifyError(raw: string): IrohError {
  if (raw.startsWith("connect"))     return new IrohConnectError(raw, "REFUSED");
  if (raw.includes("timed out"))     return new IrohConnectError(raw, "TIMEOUT");
  if (raw.includes("parse"))         return new IrohProtocolError(raw, "PARSE_FAILURE");
  if (raw.includes("invalid") && raw.includes("handle"))
                                     return new IrohStreamError(raw, "INVALID_HANDLE");
  if (raw.includes("writer dropped") || raw.includes("reader dropped"))
                                     return new IrohStreamError(raw, "WRITER_DROPPED");
  // fallback
  return new IrohError(raw, "UNKNOWN");
}
```

This lives in `iroh-http-shared/src/errors.ts`. Each platform adapter wraps
FFI/invoke rejections through `classifyError` before rethrowing. The
fallback is always a generic `IrohError` with `code: "UNKNOWN"` so no error
is ever a raw untyped `Error`.

Long-term, the Rust side should return structured error codes (e.g. a JSON
`{ code: string, message: string }`) instead of flat strings, which would
make classification deterministic. That is a separate refactor and not
required for this patch — string-prefix matching is good enough for the
initial set because the Rust error messages are controlled by us and stable.

#### File location

`iroh-http-shared/src/errors.ts` — exports all classes + `classifyError`.

#### Integration points

| Layer | Change |
|---|---|
| `iroh-http-shared/src/errors.ts` | New file: error classes + `classifyError` |
| `iroh-http-shared/src/fetch.ts` | Wrap `rawFetch` rejection through `classifyError` |
| `iroh-http-shared/src/fetch.ts` | Wrap `rawConnect` rejection through `classifyError` |
| `iroh-http-shared/src/serve.ts` | Pipe errors classified before `console.error` |
| `iroh-http-shared/src/index.ts` | Export error classes |
| `iroh-http-node/index.ts` | `createEndpoint` rejection → `IrohBindError` |
| `iroh-http-tauri/guest-js/index.ts` | Same |
| `iroh-http-deno/guest-ts/adapter.ts` | Same |
| No Rust changes required | Strings are classified on the JS side |

---

## 3. Changes required (combined)

| Layer | Change |
|---|---|
| `iroh-http-shared/src/keys.ts` | New file: `PublicKey`, `SecretKey`, base32 |
| `iroh-http-shared/src/errors.ts` | New file: `IrohError`, `IrohBindError`, `IrohConnectError`, `IrohStreamError`, `IrohProtocolError`, `classifyError` |
| `iroh-http-shared/src/bridge.ts` | `IrohNode` adds `publicKey` / `secretKey`; `fetch` accepts `PublicKey \| string`; `NodeOptions.key` accepts `SecretKey \| Uint8Array` |
| `iroh-http-shared/src/index.ts` | Export keys + errors |
| `iroh-http-shared/src/fetch.ts` | Wrap rejections in `classifyError`; resolve `PublicKey` to string |
| `iroh-http-shared/src/serve.ts` | Classify pipe errors |
| Each platform adapter | `createNode` uses `buildNode` with new key classes; `createEndpoint` errors → `IrohBindError` |
| `iroh-http-core` | No changes |
| `iroh-http-framing` | No changes |
| `guidelines.md` | No changes needed — already specifies DOMException + class-based errors |
