---
id: "TAURI-012"
title: "mDNS commands.rs unconditionally calls Rust mdns-sd with no mobile dispatch path"
status: closed
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "discovery"]
---

# [TAURI-012] mDNS commands.rs unconditionally calls Rust mdns-sd with no mobile dispatch path

## Summary

The mDNS Tauri commands (`mdns_browse`, `mdns_next_event`, `mdns_browse_close`, `mdns_advertise`, `mdns_advertise_close`) in `src/commands.rs` call `iroh_http_discovery::start_browse` and `iroh_http_discovery::start_advertise` directly, guarded only by `#[cfg(feature = "discovery")]`. There is no `#[cfg(not(mobile))]` guard and no mobile dispatch path that delegates to the native `PluginHandle`. On mobile, these commands would need to delegate to the iOS/Android native layer via `run_mobile_plugin`.

## Evidence

- `packages/iroh-http-tauri/src/commands.rs:676-757` — `mdns_browse`, `mdns_advertise`, etc. call `iroh_http_discovery::start_browse` / `start_advertise` under `#[cfg(feature = "discovery")]` only.
- No `#[cfg(not(mobile))]` / `#[cfg(mobile)]` conditional in any mDNS command.
- Reference pattern: `dns-sd-tauri/src/mobile.rs` exposes `browse_start` etc. via `self.0.run_mobile_plugin("browse_start", ...)` on mobile; `dns-sd-tauri/src/commands.rs` routes to either desktop or mobile `MdnsSd` state.

## Impact

On a correctly-built mobile target:
- The `#[cfg(feature = "discovery")]` code paths call into `iroh_http_discovery` which is not available on mobile (see TAURI-011), causing a compile error.
- Even after TAURI-011 is fixed, the commands would simply return a "feature not enabled" error because the Rust crate is excluded and `#[cfg(not(feature = "discovery"))]` stubs fire — the native layer is never invoked.

## Remediation

After TAURI-008, TAURI-011 are resolved (mobile bridge and dep gating in place):

1. Gate all `iroh_http_discovery` call sites in `commands.rs` with `#[cfg(not(mobile))]` in addition to `#[cfg(feature = "discovery")]`.
2. Add `#[cfg(mobile)]` variants of `mdns_browse`, `mdns_advertise`, etc. that delegate to the `PluginHandle` via `run_mobile_plugin`.
3. Alternatively, restructure following the reference: have `commands.rs` only handle routing to a `MdnsSd<R>` state object (desktop or mobile impl), with the actual dispatch logic in `desktop.rs` and `mobile.rs`.

## Acceptance criteria

1. On a `#[cfg(mobile)]` target, mDNS commands compile without referencing `iroh_http_discovery`.
2. On a `#[cfg(mobile)]` target, `mdns_browse` delegates to the native iOS/Android plugin handle.
3. On a `#[cfg(desktop)]` target, behaviour is unchanged — `iroh_http_discovery` is used as before.
