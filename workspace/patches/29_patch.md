---
status: discussion
---

# Patch 29 — `NodeOptions` Developer Experience Improvements

Findings from a DX review of `createNode()` options in `packages/iroh-http-shared/src/bridge.ts`.
No code changes yet — this patch captures the problems and proposed resolutions.

---

## Problem 1 — `discovery` and `dnsDiscovery` are split across two top-level keys

### Current

```ts
dnsDiscovery?: string;           // top-level: custom DNS server URL
discovery?: DiscoveryOptions;    // nested: { dns?: boolean }
```

Developers have to know that DNS-discovery-on/off lives under `discovery.dns`
but the server URL override lives at the top level as `dnsDiscovery`. The split
leaks an internal distinction that doesn't belong at the API surface.

### Proposed

Remove `dnsDiscovery` as a standalone key and absorb it into `discovery`:

```ts
discovery?: {
  dns?: boolean | { serverUrl?: string };
  mdns?: boolean | { serviceName?: string };
};
```

- `dns: true` — on with n0 defaults (same as today's default)
- `dns: false` — disabled (same as `dns: false` today)
- `dns: { serverUrl: "https://…" }` — custom server (replaces `dnsDiscovery`)
- `mdns: true` — on with default service name `"iroh-http"`
- `mdns: { serviceName: "my-app" }` — on with custom name

`DiscoveryOptions` and `MdnsOptions` become internal types; only the merged
shape is public in `NodeOptions`.

Migration: keep `dnsDiscovery` and the old `discovery.dns: boolean` form
as deprecated aliases during one release cycle.

---

## Problem 2 — Streaming/internal knobs scattered at the top level

### Current (top-level fields)

- `channelCapacity` — capacity in chunks of each body channel
- `maxChunkSizeBytes` — maximum byte length of a single chunk
- `drainTimeout` — ms to wait for a slow body reader before dropping
- `handleTtl` — TTL for slab handle entries

These appear at the same level as user-facing fields like `key` and `relayMode`,
polluting autocomplete for developers who will never need them.

`handleTtl` is particularly opaque — "slab handle entries" is an implementation
detail that only makes sense if you know the Rust internals.

### Proposed

Move all four under an `advanced` (or `streaming`) nested key:

```ts
advanced?: {
  /** Capacity (in chunks) of each body channel.
   *  Raise this if your handler reads the request body slowly and you see
   *  stalls; lower it to cap memory under high concurrency.  Default: 32. */
  channelCapacity?: number;
  /** Maximum byte length of a single chunk.  Larger chunks are split into
   *  multiple channel messages.  Default: 65536 (64 KB). */
  maxChunkSizeBytes?: number;
  /** Milliseconds to wait for a slow body reader to consume a chunk before
   *  the connection is dropped.  Default: 30 000. */
  drainTimeout?: number;
  /** TTL in milliseconds for internal handle-table entries.  Set to 0 to
   *  disable periodic sweeping.  Incorrect values can cause premature handle
   *  invalidation or unbounded memory growth.  Default: 300 000. */
  handleTtl?: number;
};
```

This makes the top-level completion list clean for 95% of users while keeping
the knobs accessible for the 5% who need them.

---

## Problem 3 — `channelCapacity` JSDoc lacks the "why"

### Current

> "Capacity (in chunks) of each body channel. Default: 32."

Tells you the unit, not the reason. Developers don't know whether to increase
or decrease it, or what symptom indicates they should touch it.

### Proposed JSDoc addition

> "Controls backpressure between the Rust pump and your JS handler. If your
> handler reads the request body slowly, the channel fills up and the Rust
> sender pauses. Raising this value uses more memory but reduces stalls under
> slow consumers. Lowering it tightens memory use under high concurrency.
> Default: 32."

This is addressed by the `advanced` block above, which already includes the
richer comment.

---

## Problem 4 — `lifecycle` is labeled "mobile/background", confusing for Node.js users

### Current

```ts
/** Mobile/background lifecycle options. */
lifecycle?: LifecycleOptions;

export interface LifecycleOptions {
  /** Automatically reconnect if the endpoint goes dead.  Default: false. */
  autoReconnect?: boolean;
  /** Maximum reconnect attempts before marking the node dead.  Default: 3. */
  maxRetries?: number;
}
```

The content (`autoReconnect`, `maxRetries`) is useful on any platform. But the
"mobile/background" framing makes Node.js and desktop developers feel this
doesn't apply to them — it does.

### Proposed

Two options (pick one):

**Option A** — rename the key to `reconnect`:

```ts
reconnect?: {
  /** Automatically reconnect if the QUIC endpoint becomes unreachable.
   *  Default: false. */
  auto?: boolean;
  /** Maximum reconnect attempts before marking the node as permanently dead.
   *  Default: 3. */
  maxRetries?: number;
};
```

**Option B** — keep `lifecycle` but rewrite the comment to be platform-neutral:

```ts
/** Options for automatic reconnection when the endpoint becomes unreachable.
 *  On mobile, this also handles app-backgrounding/suspend cycles. */
lifecycle?: LifecycleOptions;
```

Option A is cleaner for the API surface; Option B is a smaller diff.

---

## Problem 5 — `idleTimeout` is missing connection-vs-stream context

### Current

> "Idle connection timeout in milliseconds. Default: 60000."

For a QUIC transport, "idle" can mean different things at the connection level
vs. the stream level. The comment should clarify:

> "QUIC connection-level idle timeout in milliseconds. If no new streams are
> opened within this window, the connection closes. Default: 60 000."

---

## Summary of proposed changes

| Item | Change |
|---|---|
| `dnsDiscovery` + `discovery.dns` | Merge into `discovery: { dns?: boolean \| { serverUrl? }, mdns?: boolean \| { serviceName? } }` |
| `channelCapacity`, `maxChunkSizeBytes`, `drainTimeout`, `handleTtl` | Move under `advanced?: { … }` with richer JSDoc |
| `lifecycle` / "mobile/background" | Rename to `reconnect` or neutralise the comment |
| `idleTimeout` | Clarify it is connection-level in the JSDoc |
| `handleTtl` | Add explicit warning about incorrect values in JSDoc |

Files affected (when implemented):

- `packages/iroh-http-shared/src/bridge.ts` — `NodeOptions`, `DiscoveryOptions`, `LifecycleOptions`
- `packages/iroh-http-node/index.d.ts` — auto-generated; driven by Rust changes
- `packages/iroh-http-node/src/lib.rs` — flatten `advanced.*` back to flat struct for FFI
- `packages/iroh-http-deno/src/` — same flattening
- `packages/iroh-http-tauri/src/` — same flattening
- `packages/iroh-http-py/src/` — Python bindings reflect the same field rename
