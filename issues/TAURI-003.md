---
id: "TAURI-003"
title: "session_connect drops invalid direct_addrs instead of rejecting them"
status: open
priority: P2
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, addressing, silent-failure, validation]
---

# [TAURI-003] `session_connect` drops invalid `direct_addrs` silently

## Summary

Invalid socket address strings in `direct_addrs` are silently discarded via `filter_map(...ok())` rather than reported as input errors, masking misconfiguration.

## Evidence

- `packages/iroh-http-tauri/src/commands.rs:473` — `filter_map(...ok())` used for address parsing

## Impact

A typo'd address is silently ignored, potentially causing connection to fall back to relay-only paths without any indication of the input error.

## Remediation

1. Return an error when any provided address string fails to parse.
2. Include the offending string in the error message.

## Acceptance criteria

1. Passing an invalid address string in `direct_addrs` returns a Tauri command error with the offending value.
