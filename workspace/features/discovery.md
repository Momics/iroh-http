---
status: implemented
scope: core — node configuration
---

# Feature: Peer Discovery

## What

Two mechanisms for discovering other iroh-http nodes without knowing their address in advance:

- **DNS discovery** — global reach via n0's hosted infrastructure. Any node that publishes its address via Pkarr can be found by its public key using standard DNS queries.
- **mDNS discovery** — local network discovery. Nodes broadcast their presence on the LAN and can find each other without any internet connectivity.

Both mechanisms are optional and independently configurable. They affect how quickly a cold connection to an unknown peer can be established.

## Configuration

```ts
await createNode({
  // DNS discovery is enabled by default.
  // Override the resolver URL, or disable entirely:
  dnsDiscovery: 'https://dns.example.com',   // custom resolver
  // dnsDiscovery: false,                     // (via discovery.dns: false)

  discovery: {
    // DNS discovery toggle. Default: true.
    dns: true,

    // mDNS local network discovery. Default: false (disabled).
    // Pass true for defaults, or an object to customise.
    mdns: {
      // Advertise this node on the LAN. Default: true.
      advertise: true,
      // Swarm identifier — nodes with different names don't see each other.
      // Use your app name to avoid cross-talk with other iroh-http apps.
      serviceName: 'my-app',
    },
  },
});
```

## Discovery events

When mDNS is enabled, the node emits discovery events as peers come and go on the local network:

```ts
const node = await createNode({ discovery: { mdns: true } });

const unsubscribe = node.onPeerDiscovered?.((event) => {
  if (event.type === 'discovered') {
    console.log('new peer on LAN:', event.nodeId, event.addrs);
  } else {
    console.log('peer left:', event.nodeId);
  }
});

// Later, stop receiving events:
unsubscribe?.();
```

`event.type` is `"discovered"` when a peer appears and `"expired"` when it leaves or its announcement times out.

## How it works

### DNS discovery

iroh publishes a node's current relay URL and direct socket addresses as a [Pkarr](https://pkarr.org) record — a signed DNS packet stored under the node's public key in a DHT. On connect, if the target peer's address is not already known, the Iroh endpoint queries n0's DNS servers to resolve the key to a `NodeAddr`. This happens transparently before the QUIC handshake.

### mDNS

The `iroh-http-discovery` crate implements Iroh's `Discovery` trait using multicast DNS. It periodically broadcasts the node's presence on the LAN and listens for announcements from other nodes. When `advertise: false`, the node listens without broadcasting — useful for scanners or clients that don't want to be reachable.

The `serviceName` field acts as a swarm identifier: only nodes with the same service name see each other. Different applications running on the same LAN should use different names.

## Without discovery

If both DNS and mDNS are disabled (`relay: "disabled"` and `discovery: { dns: false }`), the node operates in direct-address-only mode. Connections must be established using explicit socket addresses (via `directAddrs` in `IrohFetchInit`) or via tickets that embed address hints.

This mode is appropriate for embedded targets, air-gapped networks, and integration tests.
