---
id: "A-ISS-051"
title: "Create docs/specification.md: single normative interface contract for all adapters"
status: open
priority: P2
date: 2026-04-14
area: docs
package: "iroh-http-shared, iroh-http-node, iroh-http-deno, iroh-http-tauri, iroh-http-py"
tags: [docs, spec, api-design, interfaces, typescript, python, parity]
---

# [A-ISS-051] Create docs/specification.md: single normative interface contract for all adapters

## Summary

The library has no single document that states what interfaces a conformant adapter must expose. Interface contracts are currently scattered across `docs/guidelines/javascript.md`, `docs/guidelines/python.md`, and individual feature docs. There is no canonical, normative source a new adapter author or a user can consult to understand exactly what the library promises to provide.

## Evidence

- `docs/guidelines/javascript.md` — mixes coding conventions (naming, idioms) with interface contracts (error class table, platform type table). Neither section is labelled normative.
- `docs/guidelines/python.md` — embeds the handler return contract (`{"status": int, "headers": [...], "body": bytes}`) and `IrohRequest` interface inline with unrelated style guidance.
- `docs/features/*.md` — individual feature specs define add-on interfaces (compression, signing, streaming, etc.) with no shared structure or cross-reference.
- `docs/README.md` — lists no "Specification" section; there is no entry point for "what must a conformant implementation expose".

## Impact

- New adapter authors have no authoritative checklist of required interfaces.
- Users cannot determine whether a platform adapter is complete without reading multiple docs.
- Interface contracts and coding style advice are coupled, making both harder to maintain independently.
- No structural parity check exists between the JS and Python surfaces.

## Remediation

1. **Create `docs/specification.md`** as the normative interface contract for all adapters. Use the following structure:

   - **Overview** — purpose, scope (JS adapters, Python adapter), relationship to guidelines.
   - **Core interfaces** — `IrohNode`, `IrohFetch`, `IrohServe`, `IrohRequest`, `IrohResponse`. Each entry has:
     - A TypeScript `interface` block as the canonical shape (structural typing).
     - A Python `Protocol` class block as the Python equivalent.
     - A short prose description of each member's behavioural contract.
   - **Error contract** — the typed error hierarchy both JS and Python must expose.
   - **Feature interfaces** — optional/add-on surfaces (sign/verify, streaming, tickets, etc.), one sub-section per feature, same TypeScript + Python dual-block format.
   - **Conformance** — what "conformant" means: all core interfaces required, feature interfaces required only when the feature is claimed.

2. **Update `docs/guidelines/javascript.md` and `docs/guidelines/python.md`** — remove or replace embedded interface contracts with a reference to `docs/specification.md`. Keep only coding-style and idiom guidance in the guidelines.

3. **Update `docs/README.md`** — add a "Specification" section above "Coding Guidelines" linking to `docs/specification.md` as the normative reference.

4. **Update `.github/copilot-instructions.md`** — add `docs/specification.md` to the Context section so it is consulted when adding or changing adapter APIs.

## Acceptance criteria

1. `docs/specification.md` exists and contains TypeScript `interface` blocks and Python `Protocol` blocks for all core adapter surfaces.
2. Every interface member has a one-line behavioural description.
3. The error contract in the spec matches the error tables currently in `guidelines/javascript.md` and `guidelines/python.md`.
4. `guidelines/javascript.md` and `guidelines/python.md` no longer duplicate the interface contracts; they reference the spec instead.
5. `docs/README.md` links `docs/specification.md`.
6. A new adapter author can determine the complete required API surface from `docs/specification.md` alone without reading the guidelines or feature docs.
