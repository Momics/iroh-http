# Specification

> **Normative.** This document defines the public interface contract for all
> iroh-http adapters. It is the single source of truth for what a conformant
> adapter must expose. Coding-style guidance lives in [guidelines/](guidelines/README.md);
> behavioural details live in [features/](features/README.md).

---

## Overview

iroh-http provides HTTP/1.1-over-QUIC networking addressed by Ed25519 public
keys. Three JavaScript/TypeScript adapters (Node, Deno, Tauri) expose
identical semantics through platform-appropriate FFI mechanisms.

Every adapter must export the **core interfaces** below. **Feature
interfaces** are required only when the adapter claims to support that feature.

---

## Core Interfaces

### `createNode`

The sole entry point. Creates and returns an `IrohNode`.

```ts
function createNode(options?: NodeOptions): Promise<IrohNode>;
```

See [NodeOptions](#nodeoptions) for the full option set.

---

### `IrohNode`

The primary API surface. All interaction with the network flows through a
node instance.

```ts
interface IrohNode {
  /** The node's Ed25519 public key (its stable network address). */
  readonly publicKey: PublicKey;
  /** The node's Ed25519 secret key. */
  readonly secretKey: SecretKey;

  /** Send an HTTP request to a peer. */
  fetch(
    peer: PublicKey | string,
    input: string | URL,
    init?: IrohFetchInit,
  ): Promise<Response>;

  /** Start serving HTTP requests from peers. */
  serve(handler: ServeHandler): ServeHandle;
  serve(options: ServeOptions, handler: ServeHandler): ServeHandle;
  serve(options: ServeOptions & { handler: ServeHandler }): ServeHandle;

  /** Open a WebTransport session to a peer. */
  connect(
    peer: PublicKey | string,
    init?: { directAddrs?: string[] },
  ): Promise<IrohSession>;

  /** Discover peers on the local network via mDNS. */
  browse(
    options?: MdnsOptions,
    signal?: AbortSignal,
  ): AsyncIterable<PeerDiscoveryEvent>;

  /** Advertise this node on the local network via mDNS. */
  advertise(options?: MdnsOptions, signal?: AbortSignal): Promise<void>;

  /** Resolves when the node's endpoint has closed. */
  readonly closed: Promise<WebTransportCloseInfo>;

  /** Get this node's address information (node ID + addresses). */
  addr(): Promise<NodeAddrInfo>;
  /** Get a ticket string for this node (serialised address info). */
  ticket(): Promise<string>;
  /** Get the home relay URL, or null if not connected. */
  homeRelay(): Promise<string | null>;
  /** Get address info for a connected peer, or null. */
  peerInfo(peer: PublicKey | string): Promise<NodeAddrInfo | null>;
  /** Get connection statistics for a peer, or null. */
  peerStats(peer: PublicKey | string): Promise<PeerStats | null>;
  /** Stream path changes for a peer. */
  pathChanges(
    peer: PublicKey | string,
    pollIntervalMs?: number,
  ): AsyncIterable<PathInfo>;

  /** Close the node. */
  close(options?: CloseOptions): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}
```

---

### `NodeOptions`

Configuration for `createNode`. All fields are optional.

```ts
interface NodeOptions {
  // ── Identity ──────────────────────────────────────────────────────
  /** Pre-existing secret key (restores identity across restarts). */
  key?: SecretKey | Uint8Array;

  // ── Connectivity ──────────────────────────────────────────────────
  /** Relay mode: "default" | "staging" | "disabled" | relay URL(s). */
  relayMode?: RelayMode;
  /** Local bind address(es). */
  bindAddr?: string | string[];
  /** QUIC idle timeout in milliseconds. */
  idleTimeout?: number;

  // ── Discovery ─────────────────────────────────────────────────────
  discovery?: {
    dns?: boolean | { serverUrl?: string };
    mdns?: boolean | { serviceName?: string };
  };

  // ── Proxy ─────────────────────────────────────────────────────────
  proxyUrl?: string;
  proxyFromEnv?: boolean;

  // ── Debug ─────────────────────────────────────────────────────────
  keylog?: boolean;

  // ── Connection pool ───────────────────────────────────────────────
  maxPooledConnections?: number;
  poolIdleTimeoutMs?: number;

  // ── Compression ───────────────────────────────────────────────────
  compression?: boolean | { level?: number; minBodyBytes?: number };

  // ── Server limits ─────────────────────────────────────────────────
  /** Max concurrent in-flight requests. Default: 64. */
  maxConcurrency?: number;
  /** Max QUIC connections from one peer. Default: 8. */
  maxConnectionsPerPeer?: number;
  /** Per-request timeout in ms. Default: 60 000. */
  requestTimeout?: number;
  /** Max request body size in bytes. Unlimited by default. */
  maxRequestBodyBytes?: number;
  /** Max header block size in bytes. Default: 65 536. */
  maxHeaderBytes?: number;

  // ── Reconnect ─────────────────────────────────────────────────────
  reconnect?: { auto?: boolean; maxRetries?: number };

  // ── Advanced ──────────────────────────────────────────────────────
  advanced?: {
    channelCapacity?: number;
    maxChunkSizeBytes?: number;
    drainTimeout?: number;
    handleTtl?: number;
    maxConsecutiveErrors?: number;
  };

  // ── Testing ───────────────────────────────────────────────────────
  disableNetworking?: boolean;
}
```

---

### `IrohFetchInit`

Extends the standard `RequestInit` with iroh-specific fields.

```ts
interface IrohFetchInit extends RequestInit {
  /** Direct socket addresses to try before relay. */
  directAddrs?: string[];
}
```

---

### `ServeHandler`

The handler function passed to `node.serve()`.

```ts
type ServeHandler = (req: Request) => Response | Promise<Response>;
```

The incoming `Request` is augmented with:

| Property | Type | Description |
|---|---|---|
| `req.headers.get('Peer-Id')` | `string` | Authenticated peer's public key (base32) |
| `req.trailers` | `Promise<Headers>` | Trailer headers (see [Trailer headers](#trailer-headers)) |

---

### `ServeHandle`

Returned by `node.serve()`. Controls the running server.

```ts
interface ServeHandle {
  close(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}
```

---

### `ServeOptions`

Options for `node.serve()`. Same shape as the server-limit fields from
`NodeOptions` — allows overriding per-serve-call.

```ts
interface ServeOptions {
  maxConcurrency?: number;
  maxConnectionsPerPeer?: number;
  requestTimeout?: number;
  maxRequestBodyBytes?: number;
  drainTimeout?: number;
  maxConsecutiveErrors?: number;
}
```

---

## Key Classes

### `PublicKey`

Immutable Ed25519 public key. The node's stable network address.

```ts
class PublicKey {
  /** Copy of the raw 32-byte key material. */
  readonly bytes: Uint8Array;

  /** Lowercase base32 string (the "node ID"). */
  toString(): string;

  /** Constant-time equality check. */
  equals(other: PublicKey): boolean;

  /** Verify an Ed25519 signature. Returns false (never throws) on invalid sig. */
  async verify(data: Uint8Array, signature: Uint8Array): Promise<boolean>;

  /** Parse from base32 string (case-insensitive). */
  static fromString(s: string): PublicKey;

  /** Construct from 32 raw bytes. Copies the input. */
  static fromBytes(bytes: Uint8Array): PublicKey;
}
```

### `SecretKey`

Ed25519 secret key. Persist `toBytes()` to restore identity across restarts.

```ts
class SecretKey {
  /** Copy of the raw 32-byte secret key material. */
  toBytes(): Uint8Array;

  /** Base32 representation. */
  toString(): string;

  /** The derived public key. Throws if derivePublicKey() has not been called. */
  readonly publicKey: PublicKey;

  /** Generate a fresh random key. */
  static generate(): SecretKey;

  /** Construct from 32 raw bytes. */
  static fromBytes(bytes: Uint8Array): SecretKey;

  /** Parse from base32 string. */
  static fromString(s: string): SecretKey;

  /** Derive the Ed25519 public key via Web Crypto. Caches result. */
  async derivePublicKey(): Promise<PublicKey>;

  /** Sign data. Returns a 64-byte Ed25519 signature. */
  async sign(data: Uint8Array): Promise<Uint8Array>;
}
```

---

## Error Contract

All errors extend `IrohError`. Adapters must use these exact class names and
`name` property values.

| Rust error code | JS class | `name` property | When |
|---|---|---|---|
| `TIMEOUT` | `IrohConnectError` | `"NetworkError"` | Connection timeout |
| `REFUSED` | `IrohConnectError` | `"NetworkError"` | Connection refused |
| `PEER_REJECTED` | `IrohConnectError` | `"NetworkError"` | Peer rejected connection |
| `BODY_TOO_LARGE` | `IrohProtocolError` | `"IrohProtocolError"` | Body exceeds limit |
| `HEADER_TOO_LARGE` | `IrohProtocolError` | `"IrohProtocolError"` | Headers exceed limit |
| `INVALID_HANDLE` | `IrohHandleError` | `"IrohHandleError"` | Stale or invalid handle |
| `ABORTED` / `CANCELLED` | `IrohAbortError` | `"AbortError"` | Request cancelled |
| `INVALID_INPUT` | `IrohArgumentError` | `"TypeError"` | Invalid argument |
| `ENDPOINT_FAILURE` | `IrohBindError` | `"NetworkError"` | Endpoint bind failure |
| *(catch-all)* | `IrohError` | `"IrohError"` | Unclassified error |

```ts
class IrohError extends Error { name = "IrohError"; }
class IrohConnectError extends IrohError { name = "NetworkError"; }
class IrohProtocolError extends IrohError { name = "IrohProtocolError"; }
class IrohHandleError extends IrohError { name = "IrohHandleError"; }
class IrohAbortError extends IrohError { name = "AbortError"; }
class IrohArgumentError extends IrohError { name = "TypeError"; }
class IrohBindError extends IrohError { name = "NetworkError"; }
class IrohStreamError extends IrohError { name = "IrohStreamError"; }
```

---

## Supporting Types

```ts
type RelayMode = "default" | "staging" | "disabled" | string | string[];

interface NodeAddrInfo {
  id: string;       // Base32-encoded public key
  addrs: string[];   // Relay URLs and/or "host:port" strings
}

interface PeerStats {
  relay: boolean;
  relayUrl: string | null;
  paths: PathInfo[];
}

interface PathInfo {
  relay: boolean;
  addr: string;
  active: boolean;
}

interface CloseOptions {
  closeCode?: number;
  reason?: string;
}
```

---

## Feature Interfaces

These are required only when the adapter claims to support the corresponding
feature.

### Streaming

Request and response bodies support the standard `ReadableStream` /
`WritableStream` APIs. No additional interfaces are defined — use the
Web Streams API directly. See [streaming.md](features/streaming.md) for
patterns.

### Sign / Verify

Provided by [`PublicKey.verify()`](#publickey) and [`SecretKey.sign()`](#secretkey).

| Value | Type | Description |
|---|---|---|
| signature | `Uint8Array` (64 bytes) | Ed25519 signature |
| `verify` result | `boolean` | `false` on invalid signature, never throws |

### Discovery (mDNS)

```ts
interface PeerDiscoveryEvent {
  isActive: boolean;  // true = appeared, false = departed
  nodeId: string;     // Base32-encoded public key
  addrs?: string[];   // Known socket addresses
}

interface MdnsOptions {
  serviceName?: string;  // Default: "iroh-http"
}
```

### Trailer Headers

```ts
// On incoming Request (inside serve handler):
req.trailers: Promise<Headers>;

// On outgoing Response:
(res as any).trailers = () => new Headers({ "x-checksum": value });
```

See [trailer-headers.md](features/trailer-headers.md) for details.

### Compression

Enabled via `NodeOptions.compression`. No additional runtime interfaces —
compression is transparent. See [compression.md](features/compression.md)
for configuration.

### Server Limits

Configured via `NodeOptions` or `ServeOptions`. See [server-limits.md](features/server-limits.md) for behaviour.

| Option | Attack vector | HTTP status on limit |
|---|---|---|
| `maxConcurrency` | Request flood | 408 Request Timeout |
| `maxConnectionsPerPeer` | Connection flood | Closed at QUIC level |
| `requestTimeout` | Slow request | 408 Request Timeout |
| `maxRequestBodyBytes` | Oversized body | 413 Content Too Large |
| `maxHeaderBytes` | Header flood | 431 Request Header Fields Too Large |

### WebTransport

```ts
interface IrohSession {
  readonly remoteId: PublicKey;
  readonly ready: Promise<undefined>;

  createBidirectionalStream(): Promise<WebTransportBidirectionalStream>;
  createUnidirectionalStream(): Promise<WritableStream<Uint8Array>>;

  readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;
  readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array>>;

  readonly datagrams: WebTransportDatagramDuplexStream;
  readonly closed: Promise<WebTransportCloseInfo>;

  close(info?: WebTransportCloseInfo): void;
  [Symbol.asyncDispose](): Promise<void>;
}

interface WebTransportBidirectionalStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
}

interface WebTransportDatagramDuplexStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  readonly maxDatagramSize: number | null;
  incomingHighWaterMark: number;
  outgoingHighWaterMark: number;
}

interface WebTransportCloseInfo {
  closeCode: number;
  reason: string;
}
```

### Tickets

No additional interfaces. `node.ticket()` returns a `string`;
`ticketNodeId(ticket)` extracts the node ID. See
[tickets.md](features/tickets.md).

---

## Conformance

An adapter is **conformant** when:

1. It exports `createNode` (or `create_node`) returning an object satisfying
   the `IrohNode` interface.
2. It re-exports `PublicKey` and `SecretKey` as named exports.
3. All error types match the [error contract](#error-contract).
4. For each feature the adapter claims, all interfaces in the corresponding
   feature section are implemented.
5. Non-implemented features throw `IrohError` with a descriptive message
   rather than silently failing.

---

## Building on Top

The core interfaces above can be composed into higher-level patterns.
See [recipes/](recipes/README.md) for practical examples including:

- [Sealed messages](recipes/sealed-messages.md) — encrypt to a peer's public key using ECIES (Ed25519→X25519 + AES-GCM)
- [Device handoff](recipes/device-handoff.md) — transfer sessions between devices
- [Local-first sync](recipes/local-first-sync.md) — CRDT-based data synchronisation
- [Capability tokens](recipes/capability-tokens.md) — delegated authorisation via signed tokens
