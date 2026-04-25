---
id: "006"
title: "What iroh-http is in the ecosystem: library, runtime, or infrastructure"
status: accepted
date: 2026-04-13
resolved: 2026-04-25
area: ecosystem
tags: [positioning, distribution, versioning, embedding, ergonomics]
---

# [006] What iroh-http is in the ecosystem: library, runtime, or infrastructure

## Context

There is a question that has not been explicitly answered: what *kind* of
thing is iroh-http? The answer shapes every downstream decision — versioning
policy, distribution strategy, API stability guarantees, target audience, and
success criteria.

Three plausible answers exist and they lead to different products:

> **Resolved.** iroh-http is a library. See [Decisions](#decisions).

- **A library** developers opt into, like `axios` or `reqwest`. Ergonomics and
  a familiar API surface matter most. Users pick it up and drop it freely.
- **A runtime component** embedded in a larger Iroh-based platform, like a
  QUIC-native HTTP layer inside a bigger system. Correctness, embeddability,
  and stability of internals matter most.
- **Infrastructure** that other libraries build on, like `hyper` in the Rust
  ecosystem. The audience is developers building higher-level abstractions, not
  end users.

## Questions

1. What is the primary target: end-user developers or library/platform authors?
2. Should iroh-http be designed for embedding inside larger systems without
   exposing its own API surface?
3. Does the answer change per platform — could it be a library in Python but
   infrastructure in Tauri?
4. How does the answer affect v1.0 API stability guarantees?

## What we know

- The current docs and examples treat iroh-http as a library (they show
  `fetch`/`serve` call sites for end user developers).
- The Tauri adapter is consumed as a plugin, which is closer to infrastructure
  than a library.
- The roadmap describes an open-source path suggesting external adoption is
  intended — which implies end-user ergonomics matter.
- The four FFI adapters serve genuinely different communities with different
  expectations.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Commit to "library" positioning | Clear target; ergonomics-first design | May under-serve embedding use cases |
| Commit to "infrastructure" positioning | Enables richer ecosystems on top | Higher barrier; less accessible |
| Accept that it's both and design for it | Realistic for a multi-runtime project | Risk of serving neither well |
| Decide per platform (library in JS/Python, infra in Rust) | Matches actual usage patterns | Harder to communicate a coherent identity |

## Decisions

**Q1 — Primary target?** End-user developers. iroh-http is a library that
provides HTTP over Iroh. Developers import it, call `fetch()` and `serve()`,
and build applications on top. It sits at the same layer as a networking
library, not as infrastructure or a runtime.

**Q2 — Designed for embedding?** Not specifically. It can be embedded (the
Tauri adapter is consumed as a plugin), but the API is designed for direct
use by application developers, not for wrapping by other libraries.

**Q3 — Different per platform?** No. It is a library on all platforms. The
Tauri adapter uses Tauri's plugin system because that is how Tauri distributes
libraries — it does not change iroh-http's nature.

**Q4 — Effect on v1.0 stability?** The v1.0 API stability guarantee applies
to the public JS/TS interface documented in `specification.md`. The Rust
core's public API (`iroh-http-core`) follows its own semver but is not
the primary stability surface.

## Implications

- Ergonomics and a familiar API surface matter most. The WHATWG alignment
  (`fetch`/`serve`) is the right design choice for a library.
- The README, docs, and examples should address application developers, not
  platform builders.
- The Rust core is an implementation detail from the library user's
  perspective.

## Next steps

- [x] Agree on positioning — library.
- [x] Review whether the API surface matches — yes, fetch/serve for developers.
- [ ] Add a clear positioning statement to the README or principles doc.
