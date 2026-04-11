---
status: done
refs: features/observability.md
---

# Patch 23 — Observability: Connection Stats and Path Info

Expose connection-level metrics and network path information through the JS/TS
surface as described in [observability.md](../features/observability.md).

## Problem

`iroh::Endpoint::connection_info(node_id)` provides full RTT, path, and
byte-transfer data at the Rust level. This data is not surfaced in the JS
bindings. Developers cannot tell whether a connection is relayed or direct,
nor monitor bandwidth or latency.

## Changes

### 1. Rust — `iroh-http-core/src/bridge.rs`

Add three FFI functions:

```rust
/// Node-level aggregate stats.
pub fn node_stats(handle: u32) -> NodeStats

/// Per-peer stats. Returns None if not connected.
pub fn peer_stats(handle: u32, node_id: &str) -> Option<PeerStats>

/// Long-poll: resolves when the active path to a peer changes.
/// Returns the new PathInfo.
pub fn next_path_change(handle: u32, node_id: &str) -> Option<PathInfo>
```

Types:

```rust
#[derive(Serialize)]
pub struct NodeStats {
    pub connections: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Serialize)]
pub struct PeerStats {
    pub rtt_ms: f64,
    pub path: PathInfo,
    pub paths: Vec<PathInfo>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Serialize, Clone)]
pub struct PathInfo {
    pub relay: bool,
    pub relay_url: Option<String>,
    pub addr: String,
    pub rtt_ms: Option<f64>,
    pub selected: bool,
}
```

Backed by `iroh::Endpoint::connection_info(node_id)` → `ConnectionInfo`.
`PathInfo.relay` maps to `iroh::endpoint::PathInfo::is_relay()`.

### 2. TypeScript — `packages/iroh-http-shared/src/index.ts`

Add to `IrohNode`:

```ts
/** Aggregate stats for this node. */
stats(): Promise<NodeStats>;

/** Stats for a connected peer. Returns null if not connected. */
peerStats(nodeId: string): Promise<PeerStats | null>;

/** Async iterable that yields each time the active path to a peer changes. */
pathChanges(nodeId: string, signal?: AbortSignal): AsyncIterable<PathInfo>;
```

`pathChanges` implementation: call `next_path_change` in a loop, yield each
result, check `signal.aborted` or `return()` from the iterator.

### 3. Optional `iroh-relay` and `iroh-rtt-ms` headers

When `NodeOptions.injectHeaders` includes `"relay"` or `"rtt"`, the Rust
request/response path injects the corresponding headers:

- `iroh-relay: true` / `iroh-relay: false` — from `PathInfo.relay`
- `iroh-rtt-ms: <value>` — from `PeerStats.rttMs`, rounded to integer ms

Injected in `crates/iroh-http-core/src/server.rs` (request) and
`crates/iroh-http-core/src/client.rs` (response).

### 4. Platform adapters

Wire `node_stats`, `peer_stats`, and `next_path_change` through each adapter:
- Node.js napi: `packages/iroh-http-node/src/`
- Deno FFI: `packages/iroh-http-deno/src/`
- Tauri invoke bridge: `packages/iroh-http-tauri/src/`
- Python: `packages/iroh-http-py/src/`

## Files

- `crates/iroh-http-core/src/bridge.rs` — new FFI functions + types
- `crates/iroh-http-core/src/server.rs` — optional header injection
- `crates/iroh-http-core/src/client.rs` — optional header injection
- `packages/iroh-http-shared/src/index.ts` — `IrohNode` method signatures
- All four adapter packages
