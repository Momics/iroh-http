---
status: pending
---

# iroh-http — Patch 06: Discovery

This document specifies `iroh-http-discovery`: cross-platform local peer
discovery for iroh-http nodes. Discovery is a **completely separate, optional
package** — nothing in `iroh-http-core` or any platform adapter changes.

> **Prior art:** `.old_references/iroh-tauri` contains a working
> implementation of mDNS discovery for desktop, iOS, and Android inside an
> earlier Tauri plugin. The mobile approach there — a Rust `mobile_mdns.rs`
> bridge calling into a Swift plugin (`IrohPlugin.swift`) on iOS and a Kotlin
> plugin (`IrohPlugin.kt`) on Android via Tauri's `PluginHandle::run_mobile_plugin()`
> — is the pattern this patch follows exactly.

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

This crate handles **desktop only**. Mobile discovery lives in the Tauri
plugin package (see below) because iOS and Android require native Swift/Kotlin
code that cannot be expressed as Rust Cargo dependencies.

### Layout

```
crates/iroh-http-discovery/
├── Cargo.toml
└── src/
    └── lib.rs      # mDNS via iroh/address-lookup-mdns (macOS, Linux, Windows)
```

This is identical to the current `lib.rs` draft — no restructuring required.

### Features

```toml
[features]
default = ["mdns"]
mdns    = ["iroh/address-lookup-mdns"]
```

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

### Platform implementation

#### Desktop

Wraps `iroh::address_lookup::MdnsAddressLookup` exactly as the current
`lib.rs` draft does. No change required.

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

This package follows the same structure as `.old_references/iroh-tauri`,
which already solved this problem. The key insight is that iOS and Android
cannot use Rust `jni` or `objc2` crates directly — raw UDP multicast is
restricted on both platforms. Instead, the native OS APIs are called from
Swift (iOS) and Kotlin (Android) plugins, and a Rust bridge
(`mobile_mdns.rs`) talks to them via Tauri's `PluginHandle::run_mobile_plugin()`.

```
packages/iroh-http-discovery-tauri/
├── Cargo.toml               # staticlib + cdylib + rlib, peerDep on iroh-http-tauri
├── build.rs                 # tauri-plugin build
├── package.json             # peerDependencies: { "@iroh-http/tauri": "*" }
├── src/
│   ├── lib.rs               # plugin init, #[cfg(desktop)] / #[cfg(mobile)] routing
│   ├── commands.rs          # #[tauri::command] start_discovery / stop_discovery
│   ├── mobile_mdns.rs       # Rust bridge → PluginHandle::run_mobile_plugin()
│   └── state.rs             # DiscoveryState in Tauri managed state
├── ios/
│   └── Sources/
│       └── IrohDiscoveryPlugin.swift   # NWListener (advertise) + NWBrowser (browse)
└── android/
    └── src/main/java/com/iroh/http/discovery/
        └── IrohDiscoveryPlugin.kt      # NsdManager (advertise + browse)
```

#### Desktop path (`#[cfg(desktop)]`)

`commands.rs` calls `iroh_http_discovery::start_discovery` directly — the
existing desktop mDNS crate. Identical to how the old reference wired
discovery for desktop.

#### Mobile path (`#[cfg(mobile)]`)

`mobile_mdns.rs` exposes a `MobileMdns<R>` struct (generic over Tauri
`Runtime`) that wraps a `PluginHandle<R>`. It implements six operations
matching the reference implementation exactly:

```rust
impl<R: Runtime> MobileMdns<R> {
    pub fn advertise_start(&self, node_id: &str, relay_url: Option<&str>, service_name: &str) -> Result<u64, String>;
    pub fn advertise_stop(&self, advertise_id: u64) -> Result<(), String>;
    pub fn browse_start(&self, service_name: &str) -> Result<u64, String>;
    pub fn browse_poll(&self, browse_id: u64) -> Result<Vec<NativeMdnsEvent>, String>;
    pub fn browse_stop(&self, browse_id: u64) -> Result<(), String>;
}
```

Each call serialises a JSON payload and calls
`self.0.run_mobile_plugin("mdns_<op>", payload)`, routing to the native
layer. Events are **poll-based** — the native layer buffers discovered peers
in a `pendingEvents` queue and drains it on each `browse_poll` call. A
background Tokio task polls on a timer interval and feeds results into the
`DiscoveryHandle` subscriber.

#### iOS — `IrohDiscoveryPlugin.swift`

Based directly on `.old_references/iroh-tauri/ios/Sources/IrohPlugin.swift`:

- **Advertise:** `NWListener` with a UDP service. TXT record carries `pk`
  (base32 node ID) and optionally `relay` (relay URL). Service type:
  `_<serviceName>._udp`. Uses an OS-assigned port (avoids `EADDRINUSE` on
  hot-reload). Handles `kDNSServiceErr_DefunctConnection` by cancelling and
  removing the session so re-advertising always succeeds.
- **Browse:** `NWBrowser` for `_<serviceName>._udp` on `.local`. On each
  result, extracts `pk` from the TXT record, deduplicates by `fullName`, and
  appends a `{type: "found", nodeId, addrs}` event to `pendingEvents`.
- **Plist requirements** (app must supply):
  - `NSLocalNetworkUsageDescription`
  - `NSBonjourServices` → `_<serviceName>._udp`

#### Android — `IrohDiscoveryPlugin.kt`

Based directly on `.old_references/iroh-tauri/android/src/main/java/com/momics/iroh/IrohPlugin.kt`:

- **Advertise:** `NsdManager.registerService()` with a `NsdServiceInfo` that
  sets `setAttribute("pk", nodeId)` and optionally `setAttribute("relay", ...)`.
- **Browse:** `NsdManager.discoverServices()`. On each result,
  `NsdManager.resolveService()` is called to obtain the TXT attributes. The
  `pk` attribute is extracted and a `found` event is appended to
  `pendingEvents`.
- **Manifest requirements** (app must supply):
  - `CHANGE_WIFI_MULTICAST_STATE`
  - `INTERNET`
- Tauri provides the `Activity` context automatically — no `jni::JavaVM`
  handle is needed in Rust.

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
