---
id: "TAURI-009"
title: "iOS native layer is structurally wrong and missing Package.swift"
status: closed
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "ios"]
---

# [TAURI-009] iOS native layer is structurally wrong and missing Package.swift

## Summary

The iOS directory is missing `Package.swift` entirely, and the sole Swift file `IrohDiscoveryPlugin.swift` has the wrong structure for a Tauri v2 iOS plugin: it does not import `Tauri`, does not extend `Plugin`, and uses `static` methods instead of `@objc func` instance methods. The methods themselves are `// TODO` stubs.

## Evidence

- `packages/iroh-http-tauri/ios/Sources/IrohDiscoveryPlugin.swift:1-13`:
  - No `import Tauri` statement
  - `class IrohDiscoveryPlugin: NSObject` — should extend `Plugin` (from the Tauri Swift SDK)
  - `@objc public static func startDiscovery(...)` — Tauri iOS plugins use instance methods invoked via `@objc func` decorated command handlers, not static methods
  - Body is `// TODO: implement using NWBrowser / NWListener`
- No `packages/iroh-http-tauri/ios/Package.swift` exists — Xcode cannot resolve the Tauri Swift dependency without it.
- Reference `dns-sd-tauri/ios/Package.swift`: declares `tauri-plugin-mdns-sd` package, `.package(name: "Tauri", path: "../.tauri/tauri-api")` dependency, `staticlib` product.
- Reference `dns-sd-tauri/ios/Sources/MdnsSdPlugin.swift`: `import Tauri`, `class MdnsSdPlugin: Plugin`, instance methods with `@objc` decorator wired to `NWBrowser` / `NWListener`.

## Impact

- iOS builds fail at the Swift compilation stage because `Tauri` is not importable without `Package.swift`.
- Even if compilation succeeded, the class cannot be registered as a Tauri plugin because it does not extend `Plugin`.
- mDNS browse and advertise are completely non-functional on iOS.

## Remediation

1. Create `packages/iroh-http-tauri/ios/Package.swift` following the reference pattern:
   - Product name `tauri-plugin-iroh-http`, `.static` library type.
   - Dependency on `Tauri` from `"../.tauri/tauri-api"`.

2. Rewrite `IrohDiscoveryPlugin.swift` (or rename/recreate as e.g. `IrohHttpPlugin.swift`):
   - `import Foundation`, `import Network`, `import Tauri`.
   - `@objc(IrohHttpPlugin) class IrohHttpPlugin: Plugin { ... }`.
   - Implement `browse_start`, `browse_stop`, `advertise_start`, `advertise_stop` as `@objc func` methods using `NWBrowser` / `NWListener`.
   - Use `Channel` from the Tauri Swift SDK to push discovery events back to the JS layer.

## Acceptance criteria

1. `ios/Package.swift` exists and correctly declares the Tauri dependency.
2. The Swift plugin class extends `Plugin` and compiles with `import Tauri`.
3. `browse_start` and `advertise_start` commands invoke real `NWBrowser` / `NWListener` sessions.
4. A mobile Tauri app on iOS can call `mdns_browse` and receive discovery events via the channel.
