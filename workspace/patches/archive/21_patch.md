---
status: done
refs: features/discovery.md
---

# Patch 21 — Discovery: `browse()` and `advertise()` Methods

Replace the `onPeerDiscovered` callback and the `discovery.mdns` node option
with two explicit, cancellable methods: `node.browse()` and `node.advertise()`.

## Problem

`node.onPeerDiscovered` is a callback with a cleanup function — no backpressure,
non-standard cancellation, and not composable with `async`/`await`. The
`discovery.mdns` option in `NodeOptions` conflates "start advertising" and
"start listening" into a single construction-time flag, making it impossible to
start or stop mDNS dynamically.

The correct split:
- **`browse()`** — discover others; an async iterable (continuous stream, cancellable)
- **`advertise()`** — announce yourself; a `Promise<void>` (runs until signal fires)

Both mirror how the `dns-sd` package modelled multicast DNS.

## Design

### `node.browse(options?, signal?)`

```ts
interface IrohNode {
  browse(options?: MdnsOptions, signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent>;
}

interface PeerDiscoveryEvent {
  isActive: boolean;   // true = appeared; false = left / timed out
  nodeId: string;      // base32 public key
  addrs?: string[];
}

interface MdnsOptions {
  serviceName?: string;  // default: 'iroh-http'
}
```

Usage:

```ts
for await (const event of node.browse({ serviceName: 'my-app' })) {
  if (event.isActive) { ... }
}
// or:
const controller = new AbortController();
for await (const event of node.browse({}, controller.signal)) { ... }
controller.abort();
```

Breaking from the loop or firing the signal both stop the underlying mDNS
listener with no extra cleanup call needed.

### `node.advertise(options?, signal?)`

```ts
interface IrohNode {
  advertise(options?: MdnsOptions, signal?: AbortSignal): Promise<void>;
}
```

Usage:

```ts
const controller = new AbortController();
void node.advertise({ serviceName: 'my-app' }, controller.signal);
// ...
controller.abort(); // stop announcing
```

Returns `Promise<void>` that resolves when advertising stops. Calling without a
signal advertises until the node closes.

### `NodeOptions` change

Remove `discovery.mdns` from `NodeOptions`. DNS config stays, simplified:

```ts
interface NodeOptions {
  // Before: discovery: { dns: boolean, mdns: { advertise, serviceName } }
  // After:
  dns?: boolean | { resolverUrl: string };  // default: true
}
```

## Changes

### `iroh-http-core/src/` — Rust

Replace the `on_peer_discovered` callback with a **channel-backed queue** per
browse session.

**New file: `mdns.rs`**

```rust
pub struct BrowseSession {
    rx: mpsc::Receiver<PeerDiscoveryEvent>,
}

impl BrowseSession {
    /// Returns the next event, or None when the session is closed.
    pub async fn next_event(&mut self) -> Option<PeerDiscoveryEvent> {
        self.rx.recv().await
    }
}
```

Two new FFI functions (replaces callback registration):

```rust
/// Start a browse session. Returns a session handle.
pub async fn mdns_browse(node_handle: u32, service_name: &str) -> u32

/// Poll for the next discovery event. Returns null when closed.
pub async fn mdns_next_event(browse_handle: u32) -> Option<PeerDiscoveryEvent>

/// Stop a browse session (called on break / signal).
pub fn mdns_browse_close(browse_handle: u32)

/// Start advertising. Runs until the returned handle is dropped.
pub async fn mdns_advertise(node_handle: u32, service_name: &str) -> u32

/// Stop advertising.
pub fn mdns_advertise_close(advertise_handle: u32)
```

### `iroh-http-shared/src/index.ts`

```ts
browse(options?: MdnsOptions, signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent> {
  return {
    [Symbol.asyncIterator]() {
      let browseHandle: number | null = null;
      return {
        async next() {
          if (!browseHandle) {
            browseHandle = await bridge.mdnsBrowse(info.endpointHandle, options?.serviceName ?? 'iroh-http');
          }
          if (signal?.aborted) {
            bridge.mdnsBrowseClose(browseHandle);
            return { done: true, value: undefined };
          }
          const event = await bridge.mdnsNextEvent(browseHandle);
          if (event === null) return { done: true, value: undefined };
          return { done: false, value: event };
        },
        return() {
          if (browseHandle !== null) bridge.mdnsBrowseClose(browseHandle);
          return Promise.resolve({ done: true, value: undefined });
        },
      };
    },
  };
},

async advertise(options?: MdnsOptions, signal?: AbortSignal): Promise<void> {
  const handle = await bridge.mdnsAdvertise(info.endpointHandle, options?.serviceName ?? 'iroh-http');
  if (signal) {
    signal.addEventListener('abort', () => bridge.mdnsAdvertiseClose(handle), { once: true });
  }
  // resolves when the node closes (handle invalidated)
},
```

### Platform adapters

Wire `mdnsBrowse`, `mdnsNextEvent`, `mdnsBrowseClose`, `mdnsAdvertise`,
`mdnsAdvertiseClose` through each adapter (napi, Deno FFI, Tauri, Python).

Remove `onPeerDiscovered` from all adapters.

## Removed

- `onPeerDiscovered?(callback): () => void` — removed from `IrohNode` and `Bridge`
- `NodeOptions.discovery.mdns` — removed in favour of method calls

## Files

- `crates/iroh-http-core/src/mdns.rs` — new browse/advertise session types
- `crates/iroh-http-core/src/bridge.rs` — five new FFI functions
- `packages/iroh-http-shared/src/index.ts` — `browse()`, `advertise()` methods
- All four adapter packages — wire new FFI, remove old callback

## References

- `workspace/old_references/dns-sd/src/dns_sd/browse.ts` — AsyncIterable browse pattern
- `workspace/features/discovery.md`

