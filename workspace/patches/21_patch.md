---
status: pending
---

# iroh-http — Patch 21: Discovery Events as AsyncIterable

## Problem

`node.onPeerDiscovered` is the current API for receiving mDNS local peer
discovery events:

```ts
const stop = node.onPeerDiscovered?.((event) => {
  if (event.type === 'discovered') { ... }
});
stop?.();
```

This is a callback with a cleanup function — the weakest possible interface for
a continuous stream of events:

- **No backpressure.** If the handler is slow, events are delivered into an
  unbounded queue or dropped silently depending on the implementation.
- **Non-standard cancellation.** Callers must hold a reference to `stop` and
  remember to call it. `AbortSignal` — the platform standard — is not supported.
- **Not composable.** Callbacks cannot participate in `async`/`await` control
  flow, cannot be piped, and cannot be aggregated with other async sources.
- **Optional chaining required.** The `?.` dance on every call site is a signal
  that the interface is awkward.

Peer discovery is a **continuous stream** — not a one-shot event. The correct
WHATWG-aligned primitive is an async iterable, exactly as the old `dns-sd`
package used for `browse()`:

```ts
for await (const service of browse({ signal })) {
  if (service.isActive) { ... }
}
```

## Design

Replace `onPeerDiscovered` with `node.peers()` returning
`AsyncIterable<PeerDiscoveryEvent>`:

```ts
interface IrohNode {
  // Replaces onPeerDiscovered:
  peers(signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent>;
}

interface PeerDiscoveryEvent {
  /**
   * `"discovered"` — peer appeared or updated its addresses.
   * `"expired"`    — peer left the network or its announcement timed out.
   */
  type: 'discovered' | 'expired';
  /** Base32-encoded public key of the peer. */
  nodeId: string;
  /** Known addresses (relay URLs and/or `ip:port`). Present on `discovered`. */
  addrs?: string[];
}
```

### Usage

```ts
const node = await createNode({ discovery: { mdns: true } });

// Iterate until AbortSignal fires:
const controller = new AbortController();
setTimeout(() => controller.abort(), 30_000);

for await (const event of node.peers(controller.signal)) {
  if (event.type === 'discovered') {
    console.log('peer on LAN:', event.nodeId, event.addrs);
  } else {
    console.log('peer left:', event.nodeId);
  }
}
```

Breaking from the `for await` loop also stops discovery cleanly — no separate
cleanup call needed.

### Cancellation

The iterable respects `AbortSignal` in two ways:
1. If the signal is already aborted before iteration starts, the iterable
   immediately returns (zero events, no error).
2. If the signal fires mid-iteration, the current `yield` is interrupted and
   the iterable returns cleanly (no `AbortError` thrown into the loop body —
   the loop simply ends, consistent with how `AbortSignal` works with
   `ReadableStream` async iteration).

### Relationship to mDNS being disabled

When `discovery.mdns` is not enabled in `NodeOptions`, `node.peers()` returns
an async iterable that immediately completes (zero events). It does not throw.
This avoids conditional call-site guards.

## Changes

### `iroh-http-shared/src/bridge.ts`

Remove `onPeerDiscovered` from `IrohNode`. Add:

```ts
interface IrohNode {
  peers(signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent>;
}
```

Remove `onPeerDiscovered` from `Bridge` (currently optional). Add an equivalent
raw polling function to `Bridge` that the shared layer wraps into the iterable:

```ts
interface Bridge {
  // Poll for the next discovery event. Resolves null when discovery is closed.
  nextPeerEvent(endpointHandle: number): Promise<PeerDiscoveryEvent | null>;
}
```

### `iroh-http-shared/src/index.ts`

In `buildNode`, replace the `onPeerDiscovered` wiring with:

```ts
peers(signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent> {
  return {
    [Symbol.asyncIterator]() {
      return {
        async next() {
          if (signal?.aborted) return { done: true, value: undefined };
          const event = await bridge.nextPeerEvent(info.endpointHandle);
          if (event === null) return { done: true, value: undefined };
          return { done: false, value: event };
        },
        return() {
          // Called when the consumer breaks from the loop.
          return Promise.resolve({ done: true, value: undefined });
        },
      };
    },
  };
},
```

`AbortSignal` is wired by checking `signal.aborted` at the top of each `next()`
call and by passing the signal into the underlying Rust poll if the platform
supports it.

### Rust side (`iroh-http-core`, all platform bridges)

The existing `on_peer_discovered` callback in the Rust serve loop is replaced
by a **channel-backed queue**: discovery events are pushed into a
`tokio::sync::mpsc::channel` by the mDNS loop; the FFI exposes a
`next_peer_event(handle) -> Option<PeerDiscoveryEvent>` function that pops from
the channel, blocking until an event arrives or the endpoint closes.

### Platform adapters

Each adapter (napi, Tauri invoke, Deno FFI) exposes `nextPeerEvent` in place of
the current callback registration.

## Removed

- `onPeerDiscovered?(callback): () => void` — removed from `IrohNode` and
  `Bridge`.
- The `PeerDiscoveryEvent.type` field changes from using a union of
  `"discovered" | "expired"` to using the `isActive: boolean` shape from the
  dns-sd package — whichever is chosen, it must be consistent with
  `discovery.md`.

## References

- `workspace/old_references/dns-sd/src/dns_sd/browse.ts` — async iterable
  browse pattern this patch mirrors
- `workspace/features/discovery.md` — discovery configuration
- `workspace/features/discovery-events.md` — rationale for this change
