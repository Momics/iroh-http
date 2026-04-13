---
id: "TAURI-010"
title: "Android native layer is structurally wrong and missing Gradle project files"
status: open
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "android"]
---

# [TAURI-010] Android native layer is structurally wrong and missing Gradle project files

## Summary

The Android directory is missing all Gradle project files (`build.gradle.kts`, `settings.gradle`, `AndroidManifest.xml`) and the sole Kotlin file `IrohDiscoveryPlugin.kt` has the wrong structure for a Tauri v2 Android plugin: no `@TauriPlugin` annotation, class does not extend `Plugin`, no `@Command` / `@InvokeArg` annotations, and methods are `// TODO` stubs.

## Evidence

- `packages/iroh-http-tauri/android/src/main/java/com/iroh/http/discovery/IrohDiscoveryPlugin.kt`:
  - No `import app.tauri.annotation.*` or `import app.tauri.plugin.*`
  - `class IrohDiscoveryPlugin` — should be `class IrohDiscoveryPlugin(private val activity: Activity): Plugin(activity)` with `@TauriPlugin` annotation
  - No `@Command` on `startDiscovery` / `stopDiscovery`
  - No `@InvokeArg` annotated argument classes
  - Both methods are `// TODO: implement using NsdManager`
- No `android/build.gradle.kts` — Android Gradle cannot build the module.
- No `android/settings.gradle` — module is not declared to Gradle.
- No `android/src/main/AndroidManifest.xml` — required for any Android library module.
- Reference `dns-sd-tauri/android/src/main/java/app/amble/mdnssd/MdnsSdPlugin.kt`: full `@TauriPlugin`, `Plugin(activity)` base class, `@Command` methods, `@InvokeArg` arg classes, real `NsdManager` implementation.
- Reference has `build.gradle.kts`, `settings.gradle`, and `AndroidManifest.xml`.

## Impact

- The Android module cannot be compiled by Gradle at all.
- Even if it compiled, the Kotlin class cannot be discovered by the Tauri Android runtime because it lacks `@TauriPlugin`.
- mDNS browse and advertise are completely non-functional on Android.

## Remediation

1. Add `android/build.gradle.kts` with the Tauri plugin Android SDK dependency.
2. Add `android/settings.gradle` declaring the module.
3. Add `android/src/main/AndroidManifest.xml` (minimal manifest for an Android library).
4. Rewrite `IrohDiscoveryPlugin.kt`:
   - `import app.tauri.annotation.{Command, InvokeArg, TauriPlugin}` and `import app.tauri.plugin.{Channel, Invoke, JSObject, Plugin}`.
   - `@TauriPlugin` on the class, extend `Plugin(activity)`.
   - Define `@InvokeArg` data classes for each command's arguments.
   - Define `@Command` methods: `browseStart`, `browseStop`, `advertiseStart`, `advertiseStop`.
   - Implement with `NsdManager` (see reference implementation for full pattern).

## Acceptance criteria

1. `android/build.gradle.kts`, `android/settings.gradle`, and `android/src/main/AndroidManifest.xml` all exist.
2. `IrohDiscoveryPlugin.kt` (or renamed) has `@TauriPlugin`, extends `Plugin`, and has `@Command`-annotated methods.
3. `./gradlew build` in `android/` succeeds.
4. A mobile Tauri app on Android can call `mdns_browse` and receive discovery events via the channel.
