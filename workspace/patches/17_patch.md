---
status: done
---

# iroh-http — Patch 17: NodeOptions Configurability Gaps

## Problem

The gap analysis between the old `iroh` packages and the current `iroh-http`
packages reveals several areas where developers have significantly less control
over node behaviour — particularly around connectivity configuration, address
introspection, and discovery. These are not exotic QUIC concerns; they affect
real HTTP use cases and will surface as support questions.

This patch focuses on the subset of gaps that are **relevant to an HTTP-over-
QUIC library**: things that affect how nodes connect, how developers debug
connectivity, and how discovery is configured and observed. Raw stream types,
datagrams, ALPN routing, and path-change events are intentionally excluded —
those belong to a lower-level QUIC layer that `iroh-http` deliberately does not
expose.

---

## Gaps Selected and Rationale

### Gap A — Full node address (`addr`, `homeRelay`)

**Why it matters for HTTP:** HTTP clients need to share their own address with
a remote peer out-of-band (QR code, clipboard, deep link). Today a developer
can only get `node.nodeId` — the bare public key. They cannot retrieve the
relay URL or direct socket addresses that would let a remote peer actually reach
them. Without this, peer-exchange flows ("here is how to reach me") are broken.

**What to add:** A `node.addr()` method returning a typed `NodeAddr` object
with the relay URL and direct socket addresses. A `node.homeRelay()` shorthand.

---

### Gap B — `NodeAddr` as a typed value

**Why it matters for HTTP:** Developers need to pass a complete address — not
just a node ID — when they know the relay or direct address of the remote peer.
Today `node.fetch(peer, ...)` accepts only a `PublicKey | string` (bare node
ID). If the remote peer's relay or direct addresses are known in advance,
there's no way to supply them, so the first request always pays the relay
round-trip lookup cost.

**What to add:** A `NodeAddr` class analogous to `IrohEndpointAddr`, with
`.withRelay(url)` and `.withDirectAddr(addr)` builder methods. `node.fetch()`
and `node.createBidirectionalStream()` should accept `NodeAddr` in addition to
`PublicKey | string`.

---

### Gap C — `NodeOptions`: relay configuration

**Why it matters for HTTP:** The old `AdapterBindOptions` exposed
`relayMode` (`"default" | "staging" | "disabled" | "custom"`) and `relayUrls`.
The current `NodeOptions` has a `relays?: string[]` field but its behaviour is
not documented: does it *replace* the default relay list, *supplement* it, or
set a custom-only mode? `dnsDiscovery?: string` is similarly underdocumented.

In production deployments developers need to:
- Point nodes at a private relay (enterprise, offline lab).
- Disable relay entirely for a local-network-only deployment.
- Use the staging relay during development without modifying Rust code.

**What to add:** Clarify and extend `NodeOptions`:
```ts
interface NodeOptions {
  // existing
  relays?: string[];          // currently present but underdocumented

  // proposed additions
  relayMode?: "default" | "staging" | "disabled" | "custom";
  // "default"  — use iroh's built-in relay list (current behaviour when relays is omitted)
  // "staging"  — use iroh's staging relay (dev/testing)
  // "disabled" — no relay; direct connections only
  // "custom"   — use only the URLs in `relays`
}
```

---

### Gap D — `NodeOptions`: bind address control

**Why it matters for HTTP:** The old `AdapterBindOptions` had `bindAddrs:
string[]` — explicit UDP socket addresses to bind. `iroh-http` offers no
equivalent. This matters when:
- A host has multiple network interfaces and the developer wants to restrict
  which one the node listens on.
- A specific ephemeral port is required (e.g. a firewall rule or port-forward
  is pre-configured).

**What to add:**
```ts
interface NodeOptions {
  bindAddrs?: string[]; // e.g. ["0.0.0.0:0", "192.168.1.5:7000"]
}
```

---

### Gap E — `NodeOptions`: proxy configuration

**Why it matters for HTTP:** Corporate networks route all UDP through an HTTP
proxy. The old `AdapterBindOptions` had `proxyUrl?: string` and
`proxyFromEnv: boolean`. Without these, `iroh-http` is unusable in those
environments. This is a connectivity blocker, not a nice-to-have.

**What to add:**
```ts
interface NodeOptions {
  proxyUrl?: string;       // explicit proxy for relay traffic
  proxyFromEnv?: boolean;  // read HTTP_PROXY / HTTPS_PROXY env vars
}
```

---

### Gap F — `NodeOptions`: mDNS advertise + service name

**Why it matters for HTTP:** `NodeOptions.discovery` exists and has an `mdns`
boolean. But the old API also exposed `mdnsAdvertise` (opt-out of sending
announcements while still listening) and `mdnsServiceName` (the swarm
identifier — different apps on the same LAN use different service names to
avoid cross-talk). Without `mdnsServiceName`, all `iroh-http` nodes on a LAN
will see each other regardless of which application they belong to.

**What to add:**
```ts
interface DiscoveryOptions {
  mdns?: boolean;
  mdnsAdvertise?: boolean;    // default true when mdns is true
  mdnsServiceName?: string;   // default: iroh's built-in name
}
```

---

### Gap G — mDNS discovery: `"expired"` events and peer addresses

**Why it matters for HTTP:** `node.onPeerDiscovered?.(callback)` gives a bare
node ID string when a peer appears. Developers cannot tell when a peer leaves
the network, and they receive no address hints — so they still pay the relay
lookup if they want to fetch from that peer. The old `endpoint.watchMdns()`
yielded `{ type, nodeId: IrohPublicKey, addrs: string[] }`.

**What to add:** Extend the discovery callback or introduce an event-based API:
```ts
interface PeerDiscoveryEvent {
  type: "discovered" | "expired";
  nodeId: string;
  addrs?: string[]; // relay URL and/or direct IPs if known
}

interface IrohNode {
  onPeerDiscovered?(callback: (event: PeerDiscoveryEvent) => void): () => void;
}
```

The returned function unsubscribes — current API already returns a cleanup
function, so the signature change is backward-compatible if the callback
argument shape changes.

---

### Gap H — `node.addr()` / `remoteInfo(nodeId)` for diagnostics

**Why it matters for HTTP:** Developers debugging a "cannot connect" problem
need to know two things: "what address am I advertising?" and "what addresses
does the endpoint know about the remote peer?" Neither is currently available.

The old adapter had `adapter.nodeAddr(endpoint)` and
`adapter.remoteInfo(endpoint, nodeIdBytes)`.

**What to add:**
```ts
interface IrohNode {
  /** Resolves with this node's full address (relay + direct IPs). */
  addr(): Promise<NodeAddr>;

  /** Returns cached address info for a peer, or null if unknown. */
  peerInfo(peer: PublicKey | string): Promise<NodeAddr | null>;
}
```

---

## Proposed Changes

### 1. `NodeOptions` interface (iroh-http-shared, `bridge.ts`)

```ts
interface NodeOptions {
  key?: SecretKey | Uint8Array;
  idleTimeout?: number;

  // — Relay —
  relayMode?: "default" | "staging" | "disabled" | "custom";
  relays?: string[];          // used when relayMode = "custom"; supplements default otherwise

  // — Network —
  bindAddrs?: string[];       // UDP socket addresses to bind; [] = OS-assigned
  proxyUrl?: string;          // HTTP proxy for relay traffic
  proxyFromEnv?: boolean;     // read HTTP_PROXY / HTTPS_PROXY

  // — Discovery —
  dnsDiscovery?: string;
  discovery?: DiscoveryOptions;

  // — Transport tuning (existing) —
  channelCapacity?: number;
  maxChunkSizeBytes?: number;
  maxConsecutiveErrors?: number;
  drainTimeout?: number;
  handleTtl?: number;

  // — Mobile lifecycle (existing) —
  lifecycle?: LifecycleOptions;
}

interface DiscoveryOptions {
  mdns?: boolean;
  mdnsAdvertise?: boolean;    // NEW: opt-out of sending announcements
  mdnsServiceName?: string;   // NEW: swarm identifier
  serviceName?: string;       // DNS-SD / pkarr service name
  advertise?: boolean;
}
```

### 2. `NodeAddr` type (iroh-http-shared, new `addr.ts`)

```ts
export class NodeAddr {
  readonly nodeId: string;
  readonly relayUrl?: string;
  readonly directAddrs: readonly string[];

  static fromNodeId(id: PublicKey | string): NodeAddr;
  withRelay(url: string): NodeAddr;
  withDirectAddr(addr: string): NodeAddr;

  /** Serialise to a string for sharing (QR code, clipboard, deep link). */
  toString(): string;
  static fromString(s: string): NodeAddr;
}
```

### 3. `IrohNode` additions (iroh-http-shared, `bridge.ts`)

```ts
interface IrohNode {
  // existing
  fetch(peer: PublicKey | string | NodeAddr, input: string | URL, init?: RequestInit): Promise<Response>;
  createBidirectionalStream(peer: PublicKey | string | NodeAddr, path: string, init?: RequestInit): Promise<BidirectionalStream>;

  // NEW
  addr(): Promise<NodeAddr>;
  homeRelay(): Promise<string | null>;
  peerInfo(peer: PublicKey | string): Promise<NodeAddr | null>;

  // CHANGED signature (backward-compatible callback shape change)
  onPeerDiscovered?(callback: (event: PeerDiscoveryEvent) => void): () => void;
}

interface PeerDiscoveryEvent {
  type: "discovered" | "expired";
  nodeId: string;
  addrs?: string[];
}
```

### 4. Rust-side additions (`iroh-http-core`)

Each new JS-facing method needs a corresponding Tauri command / napi function:

| JS method | Rust function |
|---|---|
| `node.addr()` | `endpoint_addr(handle) -> NodeAddrInfo` |
| `node.homeRelay()` | `endpoint_home_relay(handle) -> Option<String>` |
| `node.peerInfo(nodeId)` | `endpoint_peer_info(handle, node_id) -> Option<NodeAddrInfo>` |
| `NodeOptions.bindAddrs` | passed into `Endpoint::builder().bind_addr()` |
| `NodeOptions.relayMode` | passed into `Endpoint::builder().relay_mode()` |
| `NodeOptions.proxyUrl` | passed into `Endpoint::builder().proxy_url()` |
| `NodeOptions.proxyFromEnv` | `Endpoint::builder().proxy_from_env()` |
| `DiscoveryOptions.mdnsAdvertise` | mDNS discovery builder flag |
| `DiscoveryOptions.mdnsServiceName` | mDNS service name string |
| Discovery `"expired"` events | emit from the existing mDNS poll loop |

---

## Priority Order

1. **Gap C** (relay mode) — highest impact; blocks private/offline deployments
2. **Gap A + Gap H** (addr introspection) — blocks peer-exchange UX
3. **Gap B** (NodeAddr type) — blocks hinting known addresses at connect time
4. **Gap G** (expired events + addrs in discovery) — blocks LAN-first flows
5. **Gap F** (mdnsServiceName) — blocks multi-app coexistence on same LAN
6. **Gap D** (bindAddrs) — needed for multi-interface / port-forward scenarios
7. **Gap E** (proxy) — needed for corporate network deployments

---

## Out of Scope for This Patch

- `IrohRouter` / multi-ALPN dispatch — belongs to a future raw-QUIC layer
- `IrohConnection` object — intentionally hidden by the HTTP abstraction
- Unidirectional streams — no HTTP use case
- QUIC datagrams — no HTTP use case
- Path-change events — internal to QUIC; HTTP handles reconnects transparently
- Connection stats — useful but lower priority; can be a separate diagnostics patch
- `streamBodyToFile` — server-side HTTP concern; separate patch

---

## Interface & Abstraction Improvements (from Old Package)

Beyond adding missing functionality, there are several places where the *shape*
of the existing API is weaker than the old package's equivalent. These are
type-level and documentation-level changes that cost nothing in Rust but
meaningfully improve the developer experience.

---

### Improvement 1 — `relay` as a single discriminated union instead of two fields

**Current shape:**
```ts
interface NodeOptions {
  relayMode?: "default" | "staging" | "disabled" | "custom";
  relays?: string[];  // only meaningful when relayMode = "custom"
}
```

**Problem:** Two fields that must be kept in sync. TypeScript does not enforce
that `relays` is only valid when `relayMode = "custom"`. A developer who sets
`relays: ["https://myrelay.example.com"]` without setting `relayMode: "custom"`
gets silently ignored configuration.

**Old package shape (`RelayMode`):**
```ts
type RelayMode =
  | "default"
  | "staging"
  | "disabled"
  | string        // a single custom relay URL
  | string[];     // multiple custom relay URLs
```

**Proposed shape:**
```ts
/**
 * Relay server configuration.
 *
 * Relays are QUIC-over-HTTPS servers that keep peers reachable behind NATs.
 *
 *   "default"          n0's public production relays (recommended).
 *   "staging"          n0's canary relays — for testing pre-release infra.
 *   "disabled"         No relay. Direct UDP only; may fail behind strict NATs.
 *   "https://…"        A single custom relay URL.
 *   ["https://…", …]   Multiple custom relay URLs (first is preferred).
 */
export type RelayMode =
  | "default"
  | "staging"
  | "disabled"
  | string
  | string[];

interface NodeOptions {
  relay?: RelayMode;  // replaces relayMode + relays
  // ...
}
```

One field. No inconsistent state. The doc comment on the type is the entire
spec — no cross-referencing two fields. `relay: "https://myrelay.example.com"`
just works.

---

### Improvement 2 — `DiscoveryOptions.mdns` as `boolean | { ... }` instead of flat fields

**Current / proposed-in-gaps shape (flat):**
```ts
interface DiscoveryOptions {
  mdns?: boolean;
  mdnsAdvertise?: boolean;
  mdnsServiceName?: string;
}
```

**Problem:** `mdnsAdvertise` and `mdnsServiceName` appear at the same level as
the `mdns` toggle. A developer can set `mdnsServiceName: "myapp"` while
`mdns` is `false` or unset — meaningless configuration with no TS error.

**Old package shape:**
```ts
interface DiscoveryOptions {
  dns?: boolean;
  mdns?: boolean | {
    advertise?: boolean;
    serviceName?: string;
  };
}
```

**Proposed shape (adopted from old package):**
```ts
interface DiscoveryOptions {
  /**
   * Global discovery via DNS / pkarr publishing to iroh.link.
   * Requires internet access. Omit or set false for LAN-only deployments.
   * Default: true.
   */
  dns?: boolean;

  /**
   * Local network discovery via mDNS.
   *
   * Set to `true` for defaults, or pass an object to customise.
   * Omit (or `false`) to disable entirely.
   */
  mdns?: boolean | {
    /**
     * Advertise this node on the LAN. Set false to listen without being found.
     * Default: true.
     */
    advertise?: boolean;
    /**
     * mDNS swarm identifier. Change this to isolate app nodes from others on
     * the same network. Default: iroh's built-in service name.
     */
    serviceName?: string;
  };
}
```

Sub-options are scoped inside the object they configure. Sub-options are
unreachable when `mdns` is `false`. Also adds the missing `dns?: boolean`
flag (currently `dnsDiscovery?: string` is ambiguous — looks like URL, not
toggle).

---

### Improvement 3 — `bindAddr?: string | string[]` instead of `bindAddrs?: string[]`

**Current proposed shape:** `bindAddrs?: string[]`

**Proposed shape (adopted from old package):**
```ts
interface NodeOptions {
  /**
   * Bind the UDP socket on a specific address and/or port.
   *
   * Default: OS-assigned port on all interfaces (`"0.0.0.0:0"`).
   * Accepts a single address or an array for multi-socket binding.
   *
   * Examples: `"192.168.1.5:0"`, `["0.0.0.0:0", "[::]:0"]`
   */
  bindAddr?: string | string[];
}
```

For 99% of callers the value is one string. `bindAddr: "192.168.1.5:0"` is
cleaner than `bindAddrs: ["192.168.1.5:0"]`. The implementation normalises to
an array internally.

---

### Improvement 4 — `SecretKey.generate()` static method

**Current state:** Generating a fresh key before calling `createNode` requires:
```ts
const bytes = new Uint8Array(32);
crypto.getRandomValues(bytes);
const key = SecretKey.fromBytes(bytes);
```

The developer must know the key is 32 bytes, must reach for Web Crypto
themselves, and must hope the byte layout is correct.

**Old package equivalent:** `IrohSecretKey.generate()` — public static, zero
arguments.

**Proposed addition to `keys.ts`:**
```ts
export class SecretKey {
  // ... existing methods ...

  /**
   * Generate a fresh random Ed25519 secret key.
   *
   * Use this to create a new node identity. Persist `key.toBytes()` to restore
   * the same identity on the next run.
   *
   * @example
   * const key = SecretKey.generate();
   * localStorage.setItem("iroh-key", btoa(String.fromCharCode(...key.toBytes())));
   * const node = await createNode({ key });
   */
  static generate(): SecretKey {
    const bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    return SecretKey.fromBytes(bytes);
  }
}
```

---

### Improvement 5 — Power-user field grouping in `NodeOptions`

The old `EndpointOptions` used a region comment to separate advanced fields
from everyday ones. `NodeOptions` currently presents `maxChunkSizeBytes`,
`handleTtl`, `channelCapacity`, `drainTimeout`, and `maxConsecutiveErrors`
at the same visual level as `key` and `relay` — with no signal that the tuning
fields are dangerous.

**Proposed comment grouping for `NodeOptions` in `bridge.ts`:**
```ts
interface NodeOptions {
  // ── Identity ─────────────────────────────────────────────────────────────
  /** 32-byte Ed25519 secret key. Omit to generate a fresh identity. */
  key?: SecretKey | Uint8Array;

  // ── Connectivity ──────────────────────────────────────────────────────────
  relay?: RelayMode;
  bindAddr?: string | string[];
  idleTimeout?: number;

  // ── Discovery ─────────────────────────────────────────────────────────────
  discovery?: DiscoveryOptions;

  // ── Power-user options ────────────────────────────────────────────────────
  //
  // Leave unset unless you have a specific reason. Incorrect values can
  // silently break connectivity or degrade performance.
  proxyUrl?: string;
  proxyFromEnv?: boolean;
  channelCapacity?: number;
  maxChunkSizeBytes?: number;
  maxConsecutiveErrors?: number;
  drainTimeout?: number;
  handleTtl?: number;

  // ── Mobile / background lifecycle ─────────────────────────────────────────
  lifecycle?: LifecycleOptions;
}
```

---

### Improvement 6 — `IrohStreamError.resetCode` for stream reset errors

**Current state:** When a peer resets a body stream mid-transfer, the JS side
receives an `IrohStreamError` with `.code = "STREAM_RESET"` but no numeric
application reset code. All resets look the same.

**Old package:** `IrohReadError` carried `resetCode: number | undefined` — the
application-level QUIC reset code. This mattered for distinguishing intentional
cancellation (`RESET:0`) from peer-side errors (`RESET:1`, etc.).

**Proposed addition to `errors.ts`:**
```ts
export class IrohStreamError extends IrohError {
  /**
   * Application-level QUIC reset code sent by the remote peer, if this error
   * was caused by a stream reset. `undefined` for non-reset stream errors.
   */
  readonly resetCode: number | undefined;

  constructor(message: string, code: string, resetCode?: number) {
    super(message, code);
    this.name = "IrohStreamError";
    this.resetCode = resetCode;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}
```

This also requires the Rust error serialisation to include the reset code in
the error string (already the case in the old layer: `"RESET:0: ..."`) and
`classifyError` in `errors.ts` to parse and forward it.

---

### Updated Rust-side additions table

| Change | File | Notes |
|---|---|---|
| `relay?: RelayMode` union | `bridge.ts` | Collapse `relayMode + relays` into one field; Rust side reads the union |
| `DiscoveryOptions.mdns` union | `bridge.ts` | Normalise in `buildNode` before passing to Rust |
| `bindAddr?: string \| string[]` | `bridge.ts` | Normalise to array in bridge before FFI call |
| `DiscoveryOptions.dns?: boolean` | `bridge.ts` | Maps to existing `dnsDiscovery` Rust option |
| `SecretKey.generate()` | `keys.ts` | Pure JS — no Rust change needed |
| Option grouping comments | `bridge.ts` | Docs only — no Rust change |
| `IrohStreamError.resetCode` | `errors.ts` | Parse from existing `"RESET:<n>:"` prefix in `classifyError` |

---

## Rust-Side Gaps (from Old Bridge)

The previous sections describe the JS/TypeScript surface. The following are
gaps in `iroh-http-core` and the napi/Tauri bindings that are not yet covered
by any patch. They are relevant to HTTP because they affect bind reliability,
debugging, and correctness.

---

### Rust Gap 1 — `classify_bind_error` chain is broken (bug)

**What the old bridge did:**
```rust
// ops.rs
let ep = builder.bind().await.map_err(classify_bind_error)?;

fn classify_bind_error(e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    if msg contains "address in use" => "ADDRESS_IN_USE: ..."
    if msg contains "permission denied" => "PERMISSION_DENIED: ..."
    else => "UNKNOWN: ..."
}
```

The JS `mapBindErrorCode` function strips this prefix into a typed
`IrohBindError` code.

**Current state:**
```rust
// endpoint.rs
let ep = builder.bind().await.map_err(|e| e.to_string())?;
```

Plain `.to_string()`. The `classifyBindError` function in `iroh-http-shared`
exists and looks for those prefixes, but the Rust side never emits them. Every
bind error in JS has code `"UNKNOWN"` regardless of cause.

**Fix required (`iroh-http-core/src/endpoint.rs`):**
```rust
let ep = builder.bind().await.map_err(classify_bind_error)?;

fn classify_bind_error(e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("address") && lower.contains("in use") {
        format!("ADDRESS_IN_USE: {msg}")
    } else if lower.contains("permission") {
        format!("PERMISSION_DENIED: {msg}")
    } else {
        format!("UNKNOWN: {msg}")
    }
}
```

This is a bug fix, not a new feature. Priority: high.

---

### Rust Gap 2 — `keylog` option missing from bind path

**What the old bridge did:**
```rust
if opts.keylog {
    builder = builder.keylog(true);
}
```

**Current state:** `JsNodeOptions` and `NodeOptions` in `endpoint.rs` have no
`keylog` field. The `Endpoint::builder().keylog()` API is available in iroh but
never called.

This is the only option that enables TLS pre-master key export for Wireshark /
Zeek traffic decryption — the primary tool for debugging connectivity problems
in the field.

**Fix required:**

1. Add to `NodeOptions` in `endpoint.rs`:
```rust
/// Write TLS session keys to $SSLKEYLOGFILE.
/// Dev/debug only — never enable in production.
pub keylog: bool,
```

2. Add to `JsNodeOptions` in napi `lib.rs`:
```rust
pub keylog: Option<bool>,
```

3. Wire in `IrohEndpoint::bind`:
```rust
if opts.keylog {
    builder = builder.keylog(true);
}
```

4. Add to `NodeOptions` in `bridge.ts` (power-user section):
```ts
/**
 * Log TLS pre-master session keys to $SSLKEYLOGFILE.
 * DEV ONLY — enables Wireshark decryption. Never enable in production.
 */
keylog?: boolean;
```

---

### Rust Gap 3 — mDNS close-while-polling race (correctness bug, latent)

**What the old bridge did:**

The old registry kept `mdns_subs_closing: HashSet<Handle>`. When
`mdns_close_sub` was called while a concurrent `mdns_next_event` had already
taken the stream out of the map, it marked the handle in the closing set.
When the poll returned, it checked `consume_mdns_sub_closing`; if set, it
discarded rather than reinserting the stream.

**Current state:** The new code has no mDNS subscription handling yet —
but when it is added (as required by Gap G), this race will arise immediately.
Two ways it manifests:

1. `node.onPeerDiscovered` callback returns its cleanup function. If JS calls
   the cleanup while the Rust poll is awaited, one of two bad things happens:
   (a) the sub handle is removed from the map and the poll panics on the next
   wake, or (b) the poll reinserts the stream after it was closed, leaking it.

2. Rapid subscribe/unsubscribe during fast peer churn will accumulate ghost
   subs if the race is not handled.

**Fix required (when mDNS subscription is implemented):** Follow the old
bridge's pattern — a `HashSet` (or `DashSet` for async safety) of
"closing" handles. `close_mdns_sub` marks rather than removes; the poll
loop checks the mark on return before reinserting.

---

### Rust Gap 4 — `node_addr()`, `home_relay()`, `peerInfo()` not implemented in Rust

The JS gaps (A, H) call for `node.addr()`, `node.homeRelay()`, and
`node.peerInfo()`. These require Rust backing functions. Currently
`IrohEndpoint` in `endpoint.rs` exposes:

```rust
pub fn node_id(&self) -> &str { ... }          // ✓ exists
pub fn secret_key_bytes(&self) -> [u8; 32] { } // ✓ exists
pub fn bound_sockets(&self) -> Vec<SocketAddr> { } // ✓ exists
// node_addr / home_relay / peer_info — MISSING
```

**Fix required (`iroh-http-core/src/endpoint.rs`):**

```rust
/// Serialised full node address: node ID + relay URL + direct socket addrs.
pub fn node_addr_json(&self) -> String {
    let addr = self.inner.ep.addr();
    // serialise to { id, addrs: [...] } JSON — same format as old bridge
    endpoint_addr_to_json(&addr)
}

/// Home relay URL, or None if not connected to a relay.
pub fn home_relay(&self) -> Option<String> {
    self.inner.ep.addr().relay_urls().next().map(|u| u.to_string())
}

/// Known addresses for a remote peer, or None if not in the endpoint's cache.
pub async fn peer_info(&self, node_id_b32: &str) -> Option<NodeAddrJson> {
    let bytes = base32_decode(node_id_b32).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    let pk = iroh::PublicKey::from_bytes(&arr).ok()?;
    let info = self.inner.ep.remote_info(pk)?;
    Some(endpoint_info_to_json(&info))
}
```

These three functions have no external dependencies and are straightforward
wrappers over existing iroh `Endpoint` methods.

---

### Revised Rust-side additions table (complete)

| Change | File(s) | Priority |
|---|---|---|
| `endpoint_addr(handle)` | `endpoint.rs` + napi `lib.rs` + Tauri `commands.rs` | High |
| `endpoint_home_relay(handle)` | same | High |
| `endpoint_peer_info(handle, nodeId)` | same | High |
| `NodeOptions.bindAddr` → `builder.bind_addr()` | `endpoint.rs` + `lib.rs` | High |
| `NodeOptions.relay` discriminant (`"staging"`, `"disabled"`) | `endpoint.rs` + `lib.rs` | High |
| `NodeOptions.proxyUrl` / `proxyFromEnv` → builder | `endpoint.rs` + `lib.rs` | High |
| Fix `classify_bind_error` prefix on Rust bind error | `endpoint.rs` | High (bug fix) |
| `NodeOptions.keylog` → `builder.keylog(true)` | `endpoint.rs` + `lib.rs` | Medium |
| mDNS close-while-polling race guard | future mDNS impl | Medium (pre-emptive) |
| `DiscoveryOptions.mdnsAdvertise` + `mdnsServiceName` | `endpoint.rs` + `lib.rs` | Medium |
| Discovery `"expired"` events emitted from mDNS poll loop | future mDNS impl | Medium |
