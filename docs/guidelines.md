# iroh-http — Design Principles

These principles govern how we design, build, and evaluate every part of this
project. They are ordered by priority: when two principles conflict, the
higher one wins.

---

## 1. Belong to the platform

A developer using this library should never feel like they're fighting a
foreign abstraction. The API must feel native to whatever platform it runs
on — as if the platform team built it themselves.

This means adopting the platform's types, naming conventions, error handling
idioms, and async patterns wholesale. If the platform already has a type for
the concept, use that type. If the platform has a convention for
cancellation, follow it. If the platform names things in `snake_case`, so do
we.

The test: can a developer who has never heard of iroh read the API signature
and immediately understand what it does, based solely on their existing
platform knowledge?

**How to apply this:**
- Before designing any public API, study how the platform's standard library
  and most popular frameworks express the same concept.
- When in doubt between "correct for our internals" and "familiar to the
  user," choose familiar.
- Platform-specific configuration belongs in platform-specific config files
  or conventions, not in the core API surface.

---

## 2. Earn every concept

Every type, method, option, and parameter in the public API is a concept the
developer must learn. Each one must justify its existence.

Prefer composing existing primitives over introducing new ones. Prefer one
general-purpose method over two specialised ones. Prefer options objects over
long parameter lists when things get complex, but prefer no options at all
when sensible defaults suffice.

The test: if you removed this concept from the API, would the developer be
unable to accomplish something, or merely inconvenienced? If inconvenienced,
remove it.

**How to apply this:**
- Start with the smallest possible surface. Add things when users ask, not
  when you imagine they might want them.
- Never expose implementation details (internal handles, indices, buffer
  sizes) at the public boundary. If the developer needs to know how the
  internals work to use the API correctly, the API is wrong.
- Duplicate concepts are a bug, not a feature. If two APIs do the same thing
  in slightly different ways, collapse them.

---

## 3. Primitives, not policies

The core library does what only the core can do: operations that must happen
inside the Rust layer — stream interception, protocol negotiation, transport
security. Everything else is a policy decision that belongs to the application
or an ecosystem package.

Caching strategies, token formats, group membership, rate limiting rules —
these all depend on context the library cannot know. Attempting to own them
produces a bloated core and forces every user to pay for decisions that only
some users need.

The test: could a developer implement this correctly in a handler or a
middleware, using only what the library already exposes? If yes, it does not
belong in core.

**How to apply this:**
- If a feature requires intercepting bytes before they cross the FFI boundary
  (compression, framing, trailers), it belongs in core.
- If a feature is pure logic on top of things a handler already has access to
  (headers, peer identity, request/response), it belongs outside core.
- Ecosystem packages are the right place for policies. Core provides the
  primitives they build on.

---

## 4. Protect by default

In a peer-to-peer network, any node can connect to any other node. The
developer cannot control who their peers are. The library must be safe
against hostile peers out of the box, without requiring the developer to
opt in to protection.

This means every resource-consuming operation has a bound. Every long-running
operation has a timeout. Every peer has a fair share of capacity. These
defaults must be conservative enough to prevent abuse, but generous enough to
never interfere with legitimate use.

The test: if a hostile peer connects and tries the most obvious
resource-exhaustion attack (send infinite data, open infinite connections,
stall forever), does the library handle it gracefully without the developer
writing any defensive code?

**How to apply this:**
- Every default must be safe. Unsafe behaviour is always opt-in, never
  opt-out.
- Authenticated identity is provided by the library, not asserted by the
  peer. The developer should be able to trust identity information without
  additional verification.
- Never expose raw transport primitives to user code. The abstraction
  boundary is also a security boundary.

---

## 5. Standards inform, they don't constrain

This is a new protocol on new transport. We are not bound by backward
compatibility with any existing HTTP stack. This is a deliberate freedom.

However, we are deeply informed by standards. Where an existing standard
provides the right abstraction for our problem — and it usually does — we
adopt it. HTTP semantics, header conventions, streaming patterns, and
compression negotiation exist because smart people solved real problems. We
benefit from that work.

The distinction: the developer-facing API must feel standard because
familiarity is a feature. The wire format must be correct for our transport,
which may or may not look like what legacy protocols do.

**How to apply this:**
- When evaluating a feature, ask two separate questions: (1) does the API
  feel like something the developer already knows? (2) does the wire format
  need to match a legacy protocol? The answer to the first should almost
  always be yes. The answer to the second should almost always be no.
- When we diverge from a standard, document why. The bar for divergence is
  high, but the bar for slavish compatibility is equally high.
- Prefer adopting well-maintained implementations (crates, libraries) over
  building equivalents from scratch. The maintenance cost of custom code
  must be justified by a real constraint.

---

## 6. Every platform is a first-class citizen

Each platform target — whether it runs on a server, in a browser context, on
a phone, or on a microcontroller — deserves an API designed for that
platform's strengths and constraints. A Python developer should never feel
like they're using a JavaScript API with the names changed. An embedded
developer should never pay for abstractions they can't use.

This means each platform may have a different API shape, different error
types, and different idioms. What they share is behaviour: the same request
to the same peer produces the same result, regardless of which platform sent
it.

Embedded targets are a strategic goal, but they are not a veto on improving
robustness for currently supported host platforms. If a battle-tested crate
materially improves safety and correctness on host platforms, we may adopt it
even if it is not immediately reusable on microcontrollers. The condition is
that protocol and wire-level behaviour stay clearly specified and testable so
an embedded implementation can be added later without guessing.

**How to apply this:**
- Define each platform's conventions (naming, async model, error handling)
  before writing implementation code.
- The shared Rust core provides behaviour. Platform adapters provide
  ergonomics. Never let the core's internal structure leak into any
  platform's public API.
- When adding a new platform target, add its conventions to this document
  first.
- Keep protocol semantics and wire behaviour documented independently of
  runtime choices. See `docs/embedded-roadmap.md`.

---

## 7. Test what matters, test it honestly

If a feature isn't tested end-to-end — two real nodes exchanging real data
over real QUIC connections — it doesn't work. Unit tests verify components;
integration tests verify the product.

Tests are the primary specification. When the docs and the tests disagree,
the tests are right.

**How to apply this:**
- Every public-facing behaviour has at least one integration test that
  exercises it through the same code path a real user would hit.
- Tests must be deterministic. Flaky tests are bugs, not acceptable
  baselines.
- Test hostile inputs, not just happy paths. If patch 14 adds a limit, the
  test suite must include a case that exceeds it.

---

## 8. Document for the tooltip

A developer should be able to use this library entirely from IDE
autocompletion and inline documentation, without opening a browser or reading
source code.

Every public symbol — function, type, field, variant — must have
documentation that answers: what does this do, what does it expect, what does
it return, and what can go wrong. If a developer reading the docs for the
first time would have a question, the answer belongs in the docs.

**How to apply this:**
- Write documentation before or alongside the code, not after.
- Examples must be realistic and copy-paste-ready, not pseudocode.
- Optional parameters document their default values. Functions that can fail
  document what failures look like.
- Internal symbols don't need public-quality docs, but they do need enough
  context for a contributor to understand the design intent.

---

## Resolving conflicts

These principles are ordered. When they conflict:

- **Platform feel** beats internal consistency. If the platform convention
  disagrees with our Rust naming, the platform wins at the public boundary.
- **Minimalism** beats convenience. Don't add API surface to save the user
  one line of code.
- **Safety** beats performance. A safe default that's slightly slower is
  better than a fast default that's exploitable.
- **Standards** inform but never override platform feel. If the standard type
  is unidiomatic on the platform, wrap it.
- **Robustness now beats speculative portability.** We can choose host-only
  dependencies when they significantly improve quality, as long as embedded
  portability is preserved by documented protocol boundaries and conformance
  tests.

When none of these principles clearly apply, ask: what would the developer
expect? Then do that.
