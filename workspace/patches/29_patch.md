---
status: open
---

# Patch 29 — `NodeOptions` Developer Experience Improvements

DX review of `createNode()` options in `packages/iroh-http-shared/src/bridge.ts`.
All five changes are approved and ready to implement.

---

## Change 1 — Merge `dnsDiscovery` and `discovery` into a single key

**Problem.** DNS on/off lives under `discovery.dns` but the DNS server URL override
is a separate top-level field `dnsDiscovery`. The split leaks an internal distinction
that has no place at the API surface.

**Action.** Remove `dnsDiscovery` as a top-level key. Absorb it into `discovery`:

```ts
discovery?: {
  dns?: boolean | { serverUrl?: string };
  mdns?: boolean | { serviceName?: string };
};
```

Semantics:

- `dns: true` — on with n0 defaults (current default behaviour)
- `dns: false` — disabled
- `dns: { serverUrl: "https://…" }` — custom server (replaces `dnsDiscovery`)
- `mdns: true` — on, service name `"iroh-http"`
- `mdns: { serviceName: "my-app" }` — on with custom name

`DiscoveryOptions` and `MdnsOptions` become internal types and are no longer
exported. The merged shape is the only public surface.

**Migration.** Keep `dnsDiscovery` and the old `discovery.dns: boolean` form as
`@deprecated` aliases for one release cycle. Log a console warning when the old
forms are used.

---

## Change 2 — Move internal knobs under `advanced`

**Problem.** Five implementation-level fields pollute the top-level autocomplete
alongside user-facing fields like `key` and `relayMode`:

- `channelCapacity` — internal body channel backpressure
- `maxChunkSizeBytes` — chunk split threshold
- `drainTimeout` — slow reader eviction window
- `handleTtl` — slab handle TTL (references Rust internals directly in the name)
- `maxConsecutiveErrors` — serve loop circuit breaker

No developer tuning a relay URL or passing a key should be scrolling past these.

**Action.** Delete all five from the top level of `NodeOptions` and add them under
an `advanced` key with richer JSDoc:

```ts
advanced?: {
  /**
   * Controls backpressure between the Rust pump and your JS handler.
   * If your handler reads the request body slowly, the channel fills up and
   * the Rust sender pauses. Raise this to reduce stalls under slow consumers;
   * lower it to tighten memory use under high concurrency. Default: 32.
   */
  channelCapacity?: number;
  /**
   * Maximum byte length of a single chunk. Larger payloads are split into
   * multiple channel messages. Default: 65536 (64 KB).
   */
  maxChunkSizeBytes?: number;
  /**
   * Milliseconds to wait for a slow body reader to consume a chunk before
   * the connection is dropped. Default: 30 000.
   */
  drainTimeout?: number;
  /**
   * TTL in milliseconds for internal handle-table entries. Set to 0 to
   * disable periodic sweeping. Incorrect values can cause premature handle
   * invalidation or unbounded memory growth. Default: 300 000.
   */
  handleTtl?: number;
  /**
   * Number of consecutive accept errors before the serve loop gives up.
   * Increase if you see spurious shutdowns under adversarial load. Default: 5.
   */
  maxConsecutiveErrors?: number;
};
```

Note: the JSDoc for `channelCapacity` implicitly resolves the old Problem 3
(weak "why" comment). No separate change needed.

---

## Change 3 — Rename `lifecycle` to `reconnect`

**Problem.** `lifecycle?: LifecycleOptions` with its "mobile/background" JSDoc
makes Node.js and desktop developers think this doesn't apply to them. It does.
The `autoReconnect` field name also front-loads "auto" in a way that hides the
paired `maxRetries` setting.

**Action.** Delete `lifecycle` and `LifecycleOptions`. Replace with:

```ts
reconnect?: {
  /**
   * Automatically reconnect if the QUIC endpoint becomes unreachable.
   * On mobile, this also handles app-backgrounding/suspend cycles.
   * Default: false.
   */
  auto?: boolean;
  /**
   * Maximum reconnect attempts before marking the node as permanently dead.
   * Default: 3.
   */
  maxRetries?: number;
};
```

Keep `lifecycle` as a `@deprecated` alias for one release cycle.

---

## Change 4 — Clarify `idleTimeout` scope in JSDoc

**Problem.** "Idle connection timeout in milliseconds" is ambiguous for a QUIC
transport — idle can mean different things at the connection level vs. the stream
level.

**Action.** Replace the current JSDoc:

```ts
/** Idle connection timeout in milliseconds. @default 60000 */
idleTimeout?: number;
```

with:

```ts
/**
 * QUIC connection-level idle timeout in milliseconds. If no new streams are
 * opened within this window, the connection closes. Does not affect the
 * lifetime of individual in-progress streams. Default: 60 000.
 */
idleTimeout?: number;
```

---

## Implementation checklist

### `packages/iroh-http-shared/src/bridge.ts`

- [ ] Replace `DiscoveryOptions` + `MdnsOptions` + `dnsDiscovery` with merged `discovery` shape
- [ ] Add `@deprecated` overloads for `dnsDiscovery` and `discovery.dns: boolean`
- [ ] Move `channelCapacity`, `maxChunkSizeBytes`, `drainTimeout`, `handleTtl`, `maxConsecutiveErrors` under `advanced`
- [ ] Delete `LifecycleOptions`; replace `lifecycle` with `reconnect`; add `@deprecated lifecycle` alias
- [ ] Update `idleTimeout` JSDoc

### Rust FFI flattening (must stay in sync across all three platforms)

The TypeScript `advanced.*` and `reconnect.*` nests must be flattened back to
the existing flat Rust struct fields before crossing the FFI boundary. This is
the most error-prone part of this patch — a field left unmapped will silently
use the Rust default with no warning.

- [ ] `packages/iroh-http-node/src/lib.rs` — flatten `advanced.*` → existing flat `NodeOpts` fields; rename `lifecycle` → `reconnect`
- [ ] `packages/iroh-http-deno/src/` — same flattening
- [ ] `packages/iroh-http-tauri/src/` — same flattening
- [ ] `packages/iroh-http-py/src/` — rename `lifecycle` → `reconnect`; flatten `advanced`

### Generated types

- [ ] `packages/iroh-http-node/index.d.ts` — verify generated types match the new `NodeOptions` shape after Rust changes
