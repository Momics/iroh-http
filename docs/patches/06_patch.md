---
status: pending
---

# iroh-http — Patch 06: Discovery

This document specifies `iroh-http-discovery`: cross-platform local peer
discovery for iroh-http nodes. Discovery is a **completely separate, optional
package** — nothing in `iroh-http-core` or any platform adapter changes.

---

## Background — why a separate package?

The brief marks `iroh-http-discovery` as an optional crate with a short
comment. This patch expands that into a full specification.

The separation is deliberate for three reasons:

1. **Not every application needs it.** Many apps distribute node IDs via QR
   codes, config files, invite links, or a relay/signalling service. Bundling
   mDNS into the core forces those users to carry dead code and extra binary
   weight.

2. **Discovery implementations are completely platform-divergent.** Desktop
   uses mDNS. iOS uses `NWBrowser` (Network.framework). Android uses
   `NsdManager`. The JS runtimes need thin native-addon or FFI shims on top.
   Merging this into `iroh-http-core` would turn it into an `#[cfg]` maze.

3. **No double-bundling.** At the Cargo level, workspace deduplication means
   the `iroh` crate is only compiled once regardless of how many crates depend
   on it (`[workspace.dependencies]` enforces the shared version). At the JS
   package level, each discovery package must declare the corresponding
   iroh-http runtime package as a `peerDependency` and call into its
   already-loaded native addon rather than shipping a second binary.

---

## Rust crate — `iroh-http-discovery`

### Layout

```
crates/iroh-http-discovery/
├── Cargo.toml
└── src/
    ├── lib.rs          # public API, re-exports, cfg routing
    ├── desktop.rs      # mDNS via iroh/address-lookup-mdns (macOS, Linux, Windows)
    ├── ios.rs          # NWBrowser via objc2 (cfg target_os = "ios")
    └── android.rs      # NsdManager via jni crate (cfg target_os = "android")
```

### Features

```toml
[features]
default = ["mdns"]
mdns    = ["iroh/address-lookup-mdns"]   # desktop
ios     = ["objc2", "objc2-foundation"]  # enabled automatically on iOS targets
android = ["jni"]                        # enabled automatically on Android targets
```

`ios` and `android` are activated automatically through Cargo's
`[target.'cfg(target_os = "ios")'.dependencies]` / `android` blocks, so
callers never need to set them explicitly.

### Public API

All three platform modules expose the same two functions behind a unified
re-export in `lib.rs`:

```rust
/// Start advertising this node on the local network and listening for peers.
///
/// `service_name` — unique per application, e.g. `"my-app._iroh-http._udp"`.
/// `advertise`    — whether this node should announce itself.
///
/// Returns an opaque `DiscoveryHandle`. Drop it to stop discovery.
pub fn start_discovery(
    ep: &iroh::Endpoint,
    service_name: &str,
    advertise: bool,
) -> Result<DiscoveryHandle, DiscoveryError>;

/// Subscribe to peers found via discovery.
///
/// `handle`    — the handle returned by `start_discovery`.
/// `on_found`  — called on every newly discovered peer; may be called from
///               any thread.
pub fn subscribe(
    handle: &DiscoveryHandle,
    on_found: impl Fn(iroh::NodeId) + Send + Sync + 'static,
);
```

`DiscoveryHandle` implements `Drop` to tear down the underlying service
cleanly. It is `Send + Sync`.

### Platform implementations

#### Desktop (`desktop.rs`)

Wraps `iroh::address_lookup::MdnsAddressLookup` exactly as the current
`lib.rs` draft does, but moved into its own file. No functional change.

#### iOS (`ios.rs`)

Uses `NWBrowser` from Apple's Network.framework via `objc2` + `objc2-foundation`:

- Creates an `NWBrowser` for `_iroh-http._udp` services on the local network.
- Creates an `NWListener` if `advertise = true` to publish a Bonjour record
  containing the node's `PublicKey` as a TXT attribute.
- On peer found, extracts the node ID from the TXT record and calls
  `ep.address_lookup().add(...)`.
- Requires the `NSLocalNetworkUsageDescription` and
  `NSBonjourServices` entries in `Info.plist` — the calling app must supply
  these; the library documents the requirement.

#### Android (`android.rs`)

Uses `NsdManager` via the `jni` crate:

- Calls `NsdManager.discoverServices("_iroh-http._udp", ...)` to find peers.
- Calls `NsdManager.registerService(...)` with a service record containing
  the node ID in a TXT-record attribute if `advertise = true`.
- On peer found, extracts the node ID and calls `ep.address_lookup().add(...)`.
- Requires `CHANGE_WIFI_MULTICAST_STATE` and `INTERNET` permissions in
  `AndroidManifest.xml` — the calling app must supply these.
- **JVM handle:** the Android module requires a `jni::JavaVM` reference.
  `start_discovery` on Android takes an additional `vm: &Arc<JavaVM>`
  argument. Tauri mobile provides this via its application context; callers
  on bare Android obtain it from `ndk_context`.

---

## JS / TypeScript packages

### Overview

Three thin packages mirror the three platform adapters. Each is a peer-
dependent optional add-on; none ships its own native binary.

```
packages/
├── iroh-http-discovery-node/    # napi-rs bindings — peerDep on iroh-http-node
├── iroh-http-discovery-deno/    # Deno FFI bindings — peerDep on iroh-http-deno
└── iroh-http-discovery-tauri/   # Tauri plugin extension — peerDep on iroh-http-tauri
```

### Shared TypeScript API

All three packages export the same surface from `iroh-http-discovery-shared`
(a TypeScript-only package with no native dependency):

```ts
interface DiscoveryOptions {
  /** Application-specific service name, e.g. "my-app". Becomes the mDNS
   *  label, NWBrowser type, or NsdManager service type automatically. */
  serviceName: string;

  /** If true, this node advertises itself on the local network.
   *  Default: true. */
  advertise?: boolean;
}

interface DiscoveryHandle {
  /** Subscribe to newly discovered peers. Callback fires for every peer
   *  found, including those already known. */
  onPeerFound(cb: (nodeId: string) => void): () => void;  // returns unsubscribe fn

  /** Stop discovery and clean up resources. */
  stop(): void;
}

/**
 * Start local peer discovery on an existing IrohNode.
 *
 * @param node    - The node returned by `createNode()`.
 * @param options - Discovery options.
 */
export function startDiscovery(
  node: IrohNode,
  options: DiscoveryOptions,
): DiscoveryHandle;
```

`IrohNode` here is the type exported by the runtime package
(`iroh-http-node`, `iroh-http-deno`, or `iroh-http-tauri`). Each platform
package re-exports `startDiscovery` while filling in the native call.

### `iroh-http-discovery-node`

```
packages/iroh-http-discovery-node/
├── Cargo.toml          # crate-type = ["cdylib"], napi feature, deps on iroh-http-discovery
├── package.json        # peerDependencies: { "iroh-http-node": "*" }
├── tsconfig.json
└── src/
    └── lib.rs          # #[napi] fn start_discovery / stop_discovery / on_peer_found
```

The napi bindings call into `iroh-http-discovery::start_discovery` using the
`Endpoint` extracted from the handle already stored in the `iroh-http-node`
global slab. No second endpoint is created.

### `iroh-http-discovery-deno`

```
packages/iroh-http-discovery-deno/
├── Cargo.toml          # crate-type = ["cdylib"], C ABI dispatcher for Deno FFI
├── deno.jsonc
└── guest-ts/
    ├── adapter.ts      # Deno.dlopen symbols for startDiscovery/stopDiscovery
    └── mod.ts          # startDiscovery() public export
```

Reuses the same `Deno.dlopen`-based dispatcher pattern as `iroh-http-deno`
(single `iroh_http_discovery_call` entry point, JSON payload, same output
buffer convention). Does not load a second `.dylib` — the discovery symbols
are exported from the same library file as the core ones by building a
combined cdylib via Cargo workspace.

> **Note:** combining the cdylib output requires a thin `iroh-http-deno-full`
> crate that depends on both `iroh-http-deno` and `iroh-http-discovery-deno`
> and re-exports both ABI entry points. This avoids shipping two `.dylib`
> files for Deno users who want discovery.

### `iroh-http-discovery-tauri`

```
packages/iroh-http-discovery-tauri/
├── Cargo.toml          # Tauri plugin crate, deps on iroh-http-discovery
├── package.json        # peerDependencies: { "@iroh-http/tauri": "*" }
└── src/
    ├── lib.rs          # #[tauri::command] start_discovery / stop_discovery
    ├── state.rs        # DiscoveryState — Arc-wrapped handle stored in Tauri state
    └── commands.rs     # command implementations
```

On iOS, `commands.rs` calls `iroh_http_discovery::ios::start_discovery`.
On Android, it passes the `JavaVM` from `tauri::AppHandle` context.
On desktop, it calls the mDNS path. The `#[cfg]` routing is inside the Rust
crate, invisible to the Tauri command layer.

---

## Peer-dependency contract

To guarantee no double-bundling:

| Package | `peerDependencies` | How it avoids a second binary |
|---|---|---|
| `iroh-http-discovery-node` | `iroh-http-node ^x.y` | Calls into the already-loaded napi addon via a re-exported internal binding; the `iroh_endpoint_ptr()` symbol gives access to the live `Endpoint`. |
| `iroh-http-discovery-deno` | `iroh-http-deno ^x.y` | Combined cdylib — single `.dylib`/`.so` per platform ships both core and discovery symbols. |
| `iroh-http-discovery-tauri` | `@iroh-http/tauri ^x.y` | Compiled into the same Tauri binary via `tauri-build`; no additional shared library. |

---

## `Cargo.toml` changes

Add the new packages to the workspace:

```toml
[workspace]
members = [
    "crates/iroh-http-framing",
    "crates/iroh-http-core",
    "crates/iroh-http-discovery",         # already present
    "packages/iroh-http-node",
    "packages/iroh-http-deno",
    "packages/iroh-http-py",
    "packages/iroh-http-discovery-node",  # new
    "packages/iroh-http-discovery-deno",  # new
    "packages/iroh-http-discovery-tauri", # new
]
```

---

## Open question — subpath export vs. separate package

An alternative to separate npm packages is a subpath export from the existing
runtime packages:

```ts
import { startDiscovery } from 'iroh-http-node/discovery';
```

**Pro:** no peer-dependency version skew; one fewer package to publish and
version.

**Con:** users who do not want discovery still pull in the Rust symbols (they
are tree-shaken at the JS level but not from the native binary).

The recommendation is **separate packages** as specified above: the binary
size saving is real on mobile, and the versioning of discovery can evolve
independently of the core transport. If the team prefers subpath exports,
the TypeScript layer is identical and only the packaging changes.
