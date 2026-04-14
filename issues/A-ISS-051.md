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

- `docs/guidelines/javascript.md` — mixes coding conventions (naming, idioms) with interface contracts (error class table, platform type table, serve handler signature). Neither section is labelled normative. Interface code blocks appear inline with style guidance.
- `docs/guidelines/python.md` — embeds the handler return contract (`{"status": int, "headers": [...], "body": bytes}`), the full `IrohRequest` interface, and async body-consumption examples inline with unrelated naming and idiom guidance.
- `docs/features/*.md` — individual feature docs each embed their own TypeScript/Python code blocks showing interfaces (e.g. streaming body types, sign/verify signatures, ticket shapes). These are not cross-referenced or linked to any normative source.
- `docs/README.md` — lists no "Specification" section; there is no entry point for "what must a conformant implementation expose".

## Impact

- New adapter authors have no authoritative checklist of required interfaces.
- Users cannot determine whether a platform adapter is complete without reading multiple docs.
- Interface contracts and coding style advice are coupled, making both harder to maintain independently.
- Interface code blocks are duplicated across guidelines and feature docs; a change to a method signature must be updated in multiple places with no enforcement.
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

2. **Audit `docs/guidelines/javascript.md` and `docs/guidelines/python.md`** — for every code block or table that defines an interface shape (method signatures, type mappings, handler contracts, error hierarchies):
   - Move the canonical version into `docs/specification.md`.
   - Replace the original with a one-line prose summary and a markdown link: e.g. `See [IrohRequest — specification](../specification.md#irohrequest)`.
   - Retain only coding-style and idiom guidance (naming conventions, async patterns, import style) in the guidelines.

3. **Audit `docs/features/*.md`** — for every code block that shows interface shapes (not usage examples):
   - Move the canonical version into the corresponding feature interface sub-section of `docs/specification.md`.
   - Replace the original with a link to the spec section: e.g. `See [Streaming interface — specification](../specification.md#streaming)`.
   - Retain prose describing behaviour, not the interface shape itself.

4. **Update `docs/README.md`** — add a "Specification" section above "Coding Guidelines" linking to `docs/specification.md` as the normative reference.

5. **Update `.github/copilot-instructions.md`** — add `docs/specification.md` to the Context section so it is consulted when adding or changing adapter APIs.

## Acceptance criteria

1. `docs/specification.md` exists and contains TypeScript `interface` blocks and Python `Protocol` blocks for all core adapter surfaces.
2. Every interface member has a one-line behavioural description.
3. The error contract in the spec matches the error tables currently in `guidelines/javascript.md` and `guidelines/python.md`.
4. `guidelines/javascript.md` and `guidelines/python.md` contain no standalone interface code blocks; every removed block is replaced with a markdown link into `docs/specification.md`.
5. `docs/features/*.md` files contain no standalone interface shape code blocks; every removed block is replaced with a markdown link into `docs/specification.md`.
6. `docs/README.md` links `docs/specification.md`.
7. A new adapter author can determine the complete required API surface from `docs/specification.md` alone without reading the guidelines or feature docs.
8. No interface definition appears in more than one document; the spec is the single source of truth.
