---
id: "TAURI-008"
title: "Rust plugin has no mobile bridge layer — native iOS/Android code is never registered"
status: closed
priority: P0
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "ios", "android", "architecture"]
---

# [TAURI-008] Rust plugin has no mobile bridge layer — native iOS/Android code is never registered

## Summary

The Rust plugin is missing the entire mobile bridge layer. There is no `src/mobile.rs`, no `tauri::ios_plugin_binding!()` macro, no `api.register_android_plugin()` / `api.register_ios_plugin()` calls, and no `.setup()` callback in `init()`. The iOS Swift class and Android Kotlin class are therefore never registered with the Tauri runtime on mobile builds.

## Evidence

- `packages/iroh-http-tauri/src/lib.rs:10-49` — `Builder::new("iroh-http")` calls only `.invoke_handler()` and `.build()`. No `.setup()` callback.
- No `packages/iroh-http-tauri/src/mobile.rs` exists.
- No `packages/iroh-http-tauri/src/desktop.rs` exists.
- Reference pattern in `dns-sd-tauri/src/lib.rs`: uses `.setup(|app, api| { #[cfg(mobile)] let x = mobile::init(app, api)?; ... })`.
- Reference `dns-sd-tauri/src/mobile.rs`: `tauri::ios_plugin_binding!(init_plugin_mdns_sd)`, `api.register_android_plugin(...)`, `api.register_ios_plugin(...)`.

## Impact

On any iOS or Android Tauri build:
- The `IrohDiscoveryPlugin` Swift class is never instantiated or registered.
- The `IrohDiscoveryPlugin` Kotlin class is never instantiated or registered.
- Any `run_mobile_plugin` call would panic or error at runtime.
- The plugin silently falls back to the Rust-only code paths (which themselves may not compile on mobile targets — see TAURI-011 and TAURI-012).

## Remediation

Follow the `tauri-plugin-mdns-sd` (`dns-sd-tauri`) reference pattern:

1. Create `packages/iroh-http-tauri/src/mobile.rs`:
   - Declare `tauri::ios_plugin_binding!(init_plugin_iroh_http)`.
   - In `pub fn init()`: call `api.register_android_plugin("com.iroh.http", "IrohHttpPlugin")` for Android and `api.register_ios_plugin(init_plugin_iroh_http)` for iOS.
   - Wrap a `PluginHandle<R>` for delegating mobile-specific commands (mDNS at minimum).

2. Create `packages/iroh-http-tauri/src/desktop.rs` (optional but recommended for symmetry):
   - Desktop init that sets up the existing Rust-based state.

3. Update `packages/iroh-http-tauri/src/lib.rs`:
   - Add `#[cfg(desktop)] mod desktop; #[cfg(mobile)] mod mobile;`.
   - Add a `.setup(|app, api| { #[cfg(mobile)] mobile::init(app, api)?; ... Ok(()) })` callback to the `Builder`.

## Acceptance criteria

1. `src/mobile.rs` exists, compiles, and registers both Android and iOS plugin handles.
2. `src/lib.rs` `init()` includes a `.setup()` callback that calls `mobile::init()` on `#[cfg(mobile)]`.
3. A mobile Tauri app that imports the plugin can call `invoke("plugin:iroh-http|mdns_browse", ...)` without a runtime error.
