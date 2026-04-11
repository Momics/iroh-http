---
status: integrated
---

# iroh-http — Patch 06: Discovery

Discovery (mDNS) is **compiled into each platform binary** behind a Cargo
feature flag. Users activate it via `NodeOptions`; the code is dormant unless
explicitly enabled. No separate JS discovery packages are published.

> **Prior art:** `.old_references/iroh-tauri` contains a working
> implementation of mDNS discovery for desktop, iOS, and Android inside an
> earlier Tauri plugin. The mobile approach there — a Rust `mobile_mdns.rs`
> bridge calling into a Swift plugin (`IrohPlugin.swift`) on iOS and a Kotlin
> plugin (`IrohPlugin.kt`) on Android via Tauri's `PluginHandle::run_mobile_plugin()`
> — is the pattern this patch follows for Tauri mobile.

---

## Design — compiled-in, dormant by default

### Why not separate packages?

The previous draft proposed separate `iroh-http-discovery-node`,
`iroh-http-discovery-deno`, and `iroh-http-discovery-tauri` packages. This
is abandoned in favour of compiling discovery directly into the platform
binaries because:

1. **DX wins.** One `npm install`, one import, one `discovery` option.
   Users don't need to know about peer dependencies or version alignment.
2. **Binary cost is negligible.** The mDNS dependency adds ~100–200 KB to the
   native binary — insignificant next to the Iroh/QUIC stack.
3. **Zero runtime cost when unused.** If the user doesn't pass
   `discovery: { mdns: true }`, no mDNS code runs, no sockets are opened.
4. **Rust users retain full control.** The `discovery` Cargo feature can be
   disabled at compile time for builds that truly need the smallest possible
   binary.

### User-facing API

```ts
const node = await createNode({
  discovery: {
    mdns: true,                    // enable mDNS (default: false)
    serviceName: "my-app",         // required when mdns is true
    advertise: true,               // default: true — announce this node
  }
})

// Peers discovered via mDNS are automatically added to the endpoint's
// address book. No explicit subscribe/poll is needed for basic use.

// For advanced use — react to newly found peers:
node.onPeerDiscovered((nodeId: string) => {
  console.log("found peer:", nodeId)
})
```

### Error when feature is compiled out

If a custom build strips the `discovery` feature but the user passes
`discovery: { mdns: true }`, the error must be maximally helpful:

```
IrohBindError: mDNS discovery was requested but this build of iroh-http
was compiled without the "discovery" feature.

To fix this:
  • If you installed from npm/JSR: this is a bug — file an issue, the
    prebuilt binary should include discovery support.
  • If you built from source: add the feature flag:
      cargo build --features discovery
    or enable it in Cargo.toml:
      [dependencies]
      iroh-http-node = { version = "0.1", features = ["discovery"] }
  • If you don't need discovery: remove the `discovery` option from
    your createNode() call.
```

This error is returned from Rust as a structured `{ code, message }` (see
patch 09 for the structured error format) and classified as `IrohBindError`
on the JS side.

---

## Rust crate — `iroh-http-discovery`

Remains as a standalone crate on crates.io for Rust-only users. No changes
to its current layout or API.

```
crates/iroh-http-discovery/
├── Cargo.toml
└── src/
    └── lib.rs      # mDNS via iroh/address-lookup-mdns (macOS, Linux, Windows)
```

### Features

```toml
[features]
default = ["mdns"]
mdns    = ["iroh/address-lookup-mdns"]
```

### Public API (unchanged)

```rust
pub fn add_mdns(
    ep: &iroh::Endpoint,
    service_name: &str,
    advertise: bool,
) -> Result<Arc<MdnsAddressLookup>, String>;
```

---

## Integration into platform adapters

### Cargo.toml changes per platform crate

Each platform crate gets an optional dependency on `iroh-http-discovery`:

```toml
[dependencies]
iroh-http-discovery = { workspace = true, optional = true }

[features]
default = ["discovery"]
discovery = ["iroh-http-discovery/mdns"]
```

### `NodeOptions` extension

Add to the shared `NodeOptions` type:

```ts
interface DiscoveryOptions {
  /** Enable mDNS local network discovery. Default: false. */
  mdns?: boolean;
  /** Application-specific service name. Required when mdns is true. */
  serviceName?: string;
  /** Advertise this node on the local network. Default: true. */
  advertise?: boolean;
}

interface NodeOptions {
  // ... existing fields ...
  discovery?: DiscoveryOptions;
}
```

### Rust-side wiring (all platforms)

In `createEndpoint` (or equivalent), after the endpoint is created:

```rust
#[cfg(feature = "discovery")]
if let Some(ref disc) = options.discovery {
    if disc.mdns.unwrap_or(false) {
        let service_name = disc.service_name.as_deref()
            .ok_or("discovery.serviceName is required when mdns is true")?;
        let advertise = disc.advertise.unwrap_or(true);
        iroh_http_discovery::add_mdns(&endpoint, service_name, advertise)
            .map_err(|e| format!("mDNS setup failed: {e}"))?;
    }
}

#[cfg(not(feature = "discovery"))]
if options.discovery.as_ref().map_or(false, |d| d.mdns.unwrap_or(false)) {
    return Err(/* the detailed error message above */);
}
```

### `onPeerDiscovered` callback

For the advanced use case (reacting to newly found peers), add an optional
event callback to `IrohNode`:

```ts
interface IrohNode {
  // ... existing ...
  onPeerDiscovered?(callback: (nodeId: string) => void): () => void;
}
```

This is wired through a subscription on `MdnsAddressLookup` in the Rust side,
forwarded via the platform's event channel (napi callback / Deno poll /
Tauri Channel). Only available when discovery is active.

---

## Tauri mobile — native service discovery

Tauri mobile (iOS/Android) cannot use the Rust mDNS crate — the OS restricts
raw UDP multicast. Instead, native Swift/Kotlin plugins handle discovery.

This code lives inside the existing `packages/iroh-http-tauri/` package, not
a separate crate.

### Layout additions to `packages/iroh-http-tauri/`

```
packages/iroh-http-tauri/
├── src/
│   ├── ... existing ...
│   └── mobile_mdns.rs       # Rust bridge → PluginHandle::run_mobile_plugin()
├── ios/
│   └── Sources/
│       └── IrohDiscoveryPlugin.swift
└── android/
    └── src/main/java/com/iroh/http/discovery/
        └── IrohDiscoveryPlugin.kt
```

### Rust bridge — `mobile_mdns.rs`

```rust
pub struct MobileMdns<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> MobileMdns<R> {
    pub fn advertise_start(&self, node_id: &str, relay_url: Option<&str>,
                           service_name: &str) -> Result<u64, String>;
    pub fn advertise_stop(&self, advertise_id: u64) -> Result<(), String>;
    pub fn browse_start(&self, service_name: &str) -> Result<u64, String>;
    pub fn browse_poll(&self, browse_id: u64) -> Result<Vec<NativeMdnsEvent>, String>;
    pub fn browse_stop(&self, browse_id: u64) -> Result<(), String>;
}
```

### iOS — `IrohDiscoveryPlugin.swift`

Based on `.old_references/iroh-tauri/ios/Sources/IrohPlugin.swift`:

- **Advertise:** `NWListener` with UDP. TXT record carries `pk` (base32 node
  ID) and optionally `relay`. Service type: `_<serviceName>._udp`.
- **Browse:** `NWBrowser` for `_<serviceName>._udp` on `.local`. Deduplicates
  by `fullName`, buffers events in `pendingEvents`.
- **Plist requirements:**
  - `NSLocalNetworkUsageDescription`
  - `NSBonjourServices` → `_<serviceName>._udp`

### Android — `IrohDiscoveryPlugin.kt`

Based on `.old_references/iroh-tauri/android/.../IrohPlugin.kt`:

- **Advertise:** `NsdManager.registerService()` with `setAttribute("pk", nodeId)`.
- **Browse:** `NsdManager.discoverServices()` + `resolveService()` for TXT attrs.
- **Manifest requirements:**
  - `CHANGE_WIFI_MULTICAST_STATE`
  - `INTERNET`

### Platform routing in `lib.rs`

```rust
#[cfg(desktop)]
{
    // Use iroh-http-discovery Rust crate directly
    iroh_http_discovery::add_mdns(&endpoint, service_name, advertise)?;
}

#[cfg(mobile)]
{
    // Route through native Swift/Kotlin via PluginHandle
    let mobile = MobileMdns::new(plugin_handle);
    if advertise {
        mobile.advertise_start(&node_id, relay_url.as_deref(), service_name)?;
    }
    mobile.browse_start(service_name)?;
}
```
