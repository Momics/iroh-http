---
id: "TAURI-011"
title: "iroh-http-discovery dependency is not gated to non-mobile targets"
status: open
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "cargo", "build"]
---

# [TAURI-011] iroh-http-discovery dependency is not gated to non-mobile targets

## Summary

`Cargo.toml` declares `iroh-http-discovery` as an optional dependency but does not restrict it to non-mobile targets. On Android and iOS, mDNS must go through the native platform layer (see TAURI-008/009/010); the Rust `mdns-sd` crate that backs `iroh-http-discovery` will not compile or link correctly on those targets.

## Evidence

- `packages/iroh-http-tauri/Cargo.toml:23` — `iroh-http-discovery = { path = "../../crates/iroh-http-discovery", optional = true }` — no `[target]` restriction.
- Reference: `dns-sd-tauri/Cargo.toml` uses `[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]` to include the Rust `mdns-sd` crate only for desktop.

## Impact

On a mobile target build (`aarch64-linux-android`, `aarch64-apple-ios`, etc.) the Rust mdns-sd crate is pulled in as a dependency, which either fails to compile (missing system APIs) or links against APIs that conflict with the native mDNS implementation. The `discovery` feature is effectively broken for mobile regardless of the native layer state.

## Remediation

1. In `packages/iroh-http-tauri/Cargo.toml`, move `iroh-http-discovery` from `[dependencies]` into a target-gated section:

```toml
[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]
iroh-http-discovery = { path = "../../crates/iroh-http-discovery", optional = true }
```

2. Update the `[features]` section if needed so that `discovery` still activates `iroh-http-discovery/mdns` only on non-mobile targets.

## Acceptance criteria

1. `Cargo.toml` gates `iroh-http-discovery` behind a `cfg(not(any(target_os = "android", target_os = "ios")))` target restriction.
2. `cargo build --target aarch64-linux-android` (or equivalent) does not pull in the Rust mdns-sd crate.
3. `cargo build` for desktop targets still compiles with the `discovery` feature enabled.
