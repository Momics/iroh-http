---
id: "TAURI-007"
title: "build.rs does not register android_path or ios_path with tauri-plugin builder"
status: open
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "build"]
---

# [TAURI-007] build.rs does not register android_path or ios_path with tauri-plugin builder

## Summary

`packages/iroh-http-tauri/build.rs` calls `tauri_plugin::Builder::new(COMMANDS)` but never chains `.android_path("android")` or `.ios_path("ios")`. Without these calls the Tauri build system does not know the native Android/iOS project directories exist and will not integrate them into the mobile build.

## Evidence

- `packages/iroh-http-tauri/build.rs:8` — `tauri_plugin::Builder::new(COMMANDS)` with no `.android_path()` / `.ios_path()` calls
- Reference: `packages/dns-sd-tauri/build.rs` (from momics-vault) chains `.android_path("android").ios_path("ios")` explicitly

## Impact

On a mobile Tauri build the native Android Kotlin and iOS Swift sources are completely ignored, regardless of their content. The `android/` and `ios/` directories are dead code as far as the build is concerned.

## Remediation

1. In `packages/iroh-http-tauri/build.rs`, chain `.android_path("android")` and `.ios_path("ios")` before `.build()`.

```rust
tauri_plugin::Builder::new(COMMANDS)
    .android_path("android")
    .ios_path("ios")
    .build();
```

## Acceptance criteria

1. `build.rs` calls `.android_path("android").ios_path("ios")` on the builder.
2. `cargo build` for an Android or iOS target no longer silently skips the native directories.
