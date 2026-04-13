---
id: "PARITY-001"
title: "Tauri missing top-level crypto utilities (generateSecretKey, secretKeySign, publicKeyVerify)"
status: closed
priority: P2
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, parity, crypto, api]
---

# [PARITY-001] Tauri missing top-level crypto utilities

## Summary

Node, Deno, and Python all export `generateSecretKey`, `secretKeySign`, and `publicKeyVerify`. The Tauri package exports none of these. While WebView has native WebCrypto, there is no iroh-http wrapper for Ed25519 key operations.

## Evidence

From the API surface parity analysis:

| Export | Node | Deno | Tauri | Python |
|--------|------|------|-------|--------|
| `generateSecretKey` | ✅ | ✅ | ❌ | ✅ |
| `secretKeySign` | ✅ | ✅ | ❌ | ✅ |
| `publicKeyVerify` | ✅ | ✅ | ❌ | ✅ |

## Impact

Tauri applications must implement Ed25519 key generation and signing outside the iroh-http abstraction layer, diverging from all other platforms.

## Remediation

1. Expose `generateSecretKey`, `secretKeySign`, and `publicKeyVerify` from the Tauri guest-js package, either via Rust commands or WebCrypto wrappers using the same Ed25519 keys.

## Acceptance criteria

1. All three crypto utilities are available and have the same signature as the Node/Deno versions.
