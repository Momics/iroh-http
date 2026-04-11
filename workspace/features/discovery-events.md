---
status: partially-implemented
scope: core — IrohNode interface
---

# Feature: Peer Discovery Events

## Current state

`onPeerDiscovered` on `IrohNode` takes a callback and returns a cleanup function:

```ts
const stop = node.onPeerDiscovered?.((event) => { ... });
stop?.();
```

This is the weakest possible interface: callbacks have no backpressure, no
standard cancellation, and are not composable with other async primitives.

## What it should be

Peer discovery is a **continuous stream of events** — nodes appear and
disappear over time. The natural WHATWG-aligned interface is an async iterable,
exactly as the old dns-sd package used for `browse()`:

```ts
for await (const event of browse({
  service: { type: 'http', protocol: 'tcp' },
  multicastInterface,
  signal: controller.signal,
})) {
  if (event.isActive) { ... } else { /* peer left */ }
}
```

Applied to iroh-http, two separate entry points on `IrohNode`:

```ts
// Continuously yields peers as they appear and leave the local network.
// Cancellable by breaking the loop or via AbortSignal.
node.peers(signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent>
```

```ts
interface PeerDiscoveryEvent {
  /** Whether the peer just appeared or has left / timed out. */
  isActive: boolean;
  /** Base32-encoded public key of the peer. */
  nodeId: string;
  /** Known addresses for this peer. */
  addrs?: string[];
}
```

This is the same shape as `dns-sd`'s `Service` type, adapted to iroh's
address model.

## Why AsyncIterable over EventTarget

`EventTarget` (and its `addEventListener` / `dispatchEvent` pattern) is the
right model for **rare, unordered lifecycle events** — `"close"`, `"error"`.
Discovery is a **continuous, ordered stream** where:

- Events must not be dropped (no backpressure with callbacks)
- The consumer controls the pace of processing
- Cancellation should be structured (`break` or `AbortSignal`, not a separate
  `removeEventListener` call)
- The interface should work identically in Node.js, Deno, and browsers

`EventTarget` for discovery would require the caller to guard against dropped
events, manage listener lifetime, and cannot participate in async control flow.
AsyncIterable avoids all of this.

## Lifecycle events that should stay as EventTarget / Promise

| Event | Interface | Reason |
|---|---|---|
| Node closed | `node.closed: Promise<void>` | One-shot, no backpressure needed |
| Serve loop fatal error | `node.onerror?: (err: Error) => void` | One-shot, rare |
| Path change | `node.pathChanges(peer): AsyncIterable<PathInfo>` | Continuous stream — see observability.md |

## Migration of onPeerDiscovered

`onPeerDiscovered` should be deprecated and replaced with `node.peers()`. The
old callback form can be kept for one release as an alias.

## Reference

- Old dns-sd package: `workspace/old_references/dns-sd/src/dns_sd/browse.ts`
