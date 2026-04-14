# Peer Discovery

Two mechanisms for discovering other iroh-http nodes:

- **DNS discovery** — global, always-on. Any node that publishes its address
  via Pkarr can be resolved by public key using standard DNS.
- **mDNS** — local network. Nodes announce their presence on the LAN and can
  find each other without internet connectivity, via `node.advertise()` and
  `node.browse()`.

## DNS discovery

DNS discovery is enabled by default and configured at node creation:

```ts
await createNode({
  // Default: true — uses n0's hosted DNS infrastructure.
  dns: true,

  // Custom resolver:
  // dns: { resolverUrl: 'https://dns.example.com' },

  // Disable entirely (air-gapped / embedded):
  // dns: false,
});
```

When enabled, node startup automatically publishes a signed Pkarr record
containing the node's relay URL and direct socket addresses. On `node.fetch`,
if the peer's address isn't already known, Iroh resolves it via DNS before
the QUIC handshake — transparently, with no extra code.

## `node.advertise()`

Announce this node on the local network via mDNS until the signal fires:

```ts
const controller = new AbortController();
node.advertise({ serviceName: 'my-app' }, controller.signal);

// Stop advertising:
controller.abort();
```

Returns a `Promise<void>` that resolves when advertising stops. Calling it
without a signal advertises until the node is closed.

## `node.browse()`

Discover peers on the local network as an async iterable:

```ts
for await (const event of node.browse({ serviceName: 'my-app' })) {
  if (event.isActive) {
    console.log('found peer:', event.nodeId, event.addrs);
  } else {
    console.log('peer left:', event.nodeId);
  }
}
```

```ts
interface PeerDiscoveryEvent {
  /** true = peer appeared; false = peer left or announcement timed out. */
  isActive: boolean;
  /** Base32-encoded public key of the peer. */
  nodeId: string;
  /** Known socket addresses for this peer. */
  addrs?: string[];
}
```

Cancel by passing an `AbortSignal` or by breaking from the loop — both clean
up the underlying mDNS listener:

```ts
const controller = new AbortController();
for await (const event of node.browse({}, controller.signal)) { ... }
controller.abort();

// Or just break:
for await (const event of node.browse({ serviceName: 'my-app' })) {
  if (done) break;
}
```

## mDNS options

```ts
interface MdnsOptions {
  /**
   * Swarm identifier. Only nodes with the same serviceName see each other.
   * Different applications on the same LAN should use different names.
   * Default: 'iroh-http'.
   */
  serviceName?: string;
}
```

`browse` and `advertise` accept `MdnsOptions` as their first argument.
Both can run simultaneously on the same node — they are independent.

## Without discovery

When DNS is disabled and neither `browse` nor `advertise` is called, the node
operates in direct-address-only mode. Connections must use explicit addresses
(`directAddrs` in `IrohFetchInit`) or ticket strings (see [tickets](tickets.md)).

Appropriate for embedded targets, air-gapped networks, and integration tests.

## Platform support

| Feature | Node / Deno / Tauri | Python |
|---------|:---:|:---:|
| **DNS discovery** (auto-resolve by public key) | ✅ | ✅ |
| **`advertise()`** | ✅ (AbortSignal) | ✅ (`node.advertise(service_name)`) |
| **`browse()`** | ✅ (async iterable + AbortSignal) | ✅ (async iterator) |

> **Feature flag:** mDNS browse and advertise require the `mdns` compile-time
> feature in all Rust adapters.  In Python, calling `browse()` or
> `advertise()` without the feature raises `RuntimeError`.
>
> **Python API differences:** Python uses positional `service_name: str`
> instead of an options dict.  Cancellation is via `async for … break` or
> node close, not `AbortSignal`.

