---
id: "TAURI-004"
title: "Default permission set omits session, mDNS, and crypto command groups"
status: closed
priority: P2
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, permissions, capabilities, acl]
---

# [TAURI-004] Default permission set omits exposed command groups

## Summary

`iroh-http:default` only grants the base fetch/serve commands, but the plugin registers session, mDNS, and crypto command groups too. The README tells users to apply only `iroh-http:default`, which silently denies access to those additional APIs.

## Evidence

- `packages/iroh-http-tauri/permissions/default.toml:5` — only base commands in default set
- `packages/iroh-http-tauri/build.rs:32` — plugin registers more command groups
- `packages/iroh-http-tauri/README.md:50` — users told to use only `iroh-http:default`

## Impact

Callers following the README who try to use session, mDNS, or crypto features receive permission errors with no explanation.

## Remediation

1. Either add the missing command groups to `iroh-http:default`, or document the additional per-feature capability grants required.

## Acceptance criteria

1. Users applying `iroh-http:default` as documented can call all published plugin APIs without permission errors.
