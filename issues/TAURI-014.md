---
id: "TAURI-014"
title: "Mobile mDNS advertise path omits peer identity metadata required by browse"
status: open
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "discovery", "android", "ios"]
---

# [TAURI-014] Mobile mDNS advertise path omits peer identity metadata required by browse

## Summary

Mobile browse code expects TXT metadata containing `pk` (node identity), but mobile advertise code does not publish that metadata. In Rust, mobile `mdns_advertise` also ignores `endpoint_handle`, so there is no source of node identity for native advertise calls.

## Evidence

- `packages/iroh-http-tauri/android/src/main/java/com/iroh/http/IrohHttpPlugin.kt:90-92` — browse discards services without TXT `pk`.
- `packages/iroh-http-tauri/android/src/main/java/com/iroh/http/IrohHttpPlugin.kt:180-184` — advertise registers `NsdServiceInfo` with service type/port only; no attributes for `pk`/`relay`.
- `packages/iroh-http-tauri/ios/Sources/IrohHttpPlugin.swift:157` — browse requires `txt["pk"]` and skips entries without it.
- `packages/iroh-http-tauri/ios/Sources/IrohHttpPlugin.swift:222-223` — advertise sets only `NWListener.Service(type: ...)`; no TXT payload is attached.
- `packages/iroh-http-tauri/src/commands.rs:784-787` — mobile `mdns_advertise` ignores `_endpoint_handle`.

## Impact

Mobile peers may fail to discover each other because advertisements do not include the identity field required by the discovery parser. This can make mobile mDNS appear flaky or non-functional even when browse/advertise calls succeed.

## Remediation

1. Use `endpoint_handle` in mobile `mdns_advertise` Rust command to retrieve node identity and relay info.
2. Pass identity payload to native advertise start (`pk` required, `relay` optional).
3. Publish TXT metadata consistently on Android and iOS in the same format consumed by browse.
4. Add validation/logging for malformed advertisements so failures are diagnosable.

## Acceptance criteria

1. Mobile advertise publishes TXT metadata containing `pk` for every advertised service.
2. A second mobile client browsing the same service receives a `discovered` event with non-empty `nodeId`.
3. mDNS discovery interop works between desktop and mobile builds for the same service name.
