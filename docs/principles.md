# Principles

These principles govern how iroh-http is designed, built, and evaluated.
They are ordered by priority: when two principles conflict, the higher one
wins. They are invariants — not aspirations, not suggestions.

For language-specific coding conventions, see [guidelines/](guidelines/) or pick directly:
[Rust](guidelines/rust.md) · [JavaScript/TypeScript](guidelines/javascript.md) ·
[Tauri](guidelines/tauri.md)

---

## Hierarchy of Values

When values conflict, prefer the higher one:

1. **Correctness** — the code must be provably right
2. **Reliability** — the code must never silently fail
3. **Clarity** — the code must be understandable by a future maintainer
4. **Performance** — the code must not waste resources
5. **Ergonomics** — the code must be pleasant to use

Never sacrifice a higher value for a lower one.

---

## 1. Belong to the Platform

A developer using this library should never feel like they're fighting a
foreign abstraction. The API must feel native to whatever platform it runs
on — as if the platform team built it themselves.

This means adopting the platform's types, naming conventions, error handling
idioms, and async patterns wholesale. If the platform already has a type for
the concept, use that type. If the platform has a convention for
cancellation, follow it.

**The test:** can a developer who has never heard of Iroh read the API
signature and immediately understand what it does, based solely on their
existing platform knowledge?

**How to apply:**
- Before designing any public API, study how the platform's standard library
  and most popular frameworks express the same concept.
- When in doubt between "correct for our internals" and "familiar to the
  user," choose familiar.
- Each platform may have a different API shape, different error types, and
  different idioms. What they share is behaviour: the same request to the
  same peer produces the same result, regardless of which platform sent it.
- The shared Rust core provides behaviour. Platform adapters provide
  ergonomics. Never let the core's internal structure leak into any
  platform's public API.

---

## 2. Earn Every Concept

Every type, method, option, and parameter in the public API is a concept the
developer must learn. Each one must justify its existence.

Prefer composing existing primitives over introducing new ones. Prefer one
general-purpose method over two specialised ones. Prefer options objects over
long parameter lists when things get complex, but prefer no options at all
when sensible defaults suffice.

**The test:** if you removed this concept from the API, would the developer be
unable to accomplish something, or merely inconvenienced? If inconvenienced,
remove it.

**How to apply:**
- Start with the smallest possible surface. Add things when users ask, not
  when you imagine they might want them.
- Never expose implementation details (internal handles, indices, buffer
  sizes) at the public boundary. If the developer needs to know how the
  internals work to use the API correctly, the API is wrong.
- Duplicate concepts are a bug, not a feature. If two APIs do the same thing
  in slightly different ways, collapse them.

---

## 3. Leverage, Don't Reinvent

Before writing any non-trivial component, ask:

> *"Does a well-maintained library already solve this, and can it be
> composed with what I have?"*

If yes, use it. Every line of custom infrastructure has bugs you haven't
found, needs tests you haven't written, and must be maintained by people who
haven't arrived yet. Writing your own solution when a good one exists is a
liability, not a feature.

**Custom code must be explicitly justified.** If a component is written from
scratch, there must be a clear documented reason — a concrete incompatibility
or gap, not "I wasn't sure" or "I wanted more control."

This principle has already been proven: ~1,400 lines of custom HTTP framing,
QPACK encoding, streaming compression, and connection pooling were replaced
with hyper, tower-http, moka, and slotmap. Each replacement eliminated custom
bugs and gained ecosystem-maintained correctness.

| Concern | Solution | Never |
|---------|----------|-------|
| HTTP framing | hyper v1 | Custom parsers |
| Compression | tower-http `CompressionLayer` | Hand-rolled zstd |
| Connection pooling | moka async cache | Custom `Slot` enum |
| Handle arenas | slotmap generational keys | `HashMap<u32, T>` + counter |
| Async I/O | tokio | Custom runtime |

---

## 4. Primitives, Not Policies

The core library does what only the core can do: operations that must happen
inside the Rust layer — stream interception, protocol negotiation, transport
security. Everything else is a policy decision that belongs to the
application or an ecosystem package.

**The test:** could a developer implement this correctly in a handler or a
middleware, using only what the library already exposes? If yes, it does not
belong in core. If it requires intercepting bytes before they cross the FFI
boundary, it belongs in core.

| Belongs in core | Belongs in userland |
|---|---|
| HTTP connection lifecycle | Retry logic, backoff |
| Compression negotiation | Caching strategies |
| Connection limits | Rate limiting |
| Transport-level timeouts | Auth / authorization |
| Upgrade handshakes | Tracing export config |
| FFI panic safety | Middleware / interceptors |

If a feature request would add something from the right column to core, the
correct response is to ensure core exposes enough information for userland to
do it.

---

## 5. Protect by Default

In a peer-to-peer network, any node can connect to any other node. The
developer cannot control who their peers are. The library must be safe
against hostile peers out of the box.

Every resource-consuming operation has a bound. Every long-running operation
has a timeout. Every peer has a fair share of capacity. These defaults are
conservative enough to prevent abuse but generous enough to never interfere
with legitimate use.

**The test:** if a hostile peer connects and tries the most obvious
resource-exhaustion attack (send infinite data, open infinite connections,
stall forever), does the library handle it gracefully without the developer
writing any defensive code?

**How to apply:**
- Every default must be safe. Unsafe behaviour is always opt-in, never
  opt-out.
- Authenticated identity is provided by the library, not asserted by the
  peer.
- Never expose raw transport primitives to user code. The abstraction
  boundary is also a security boundary.
- An operation without a timeout bound is a latent hang — treat it as a bug.
- Unbounded queues and channels are latent out-of-memory bugs. Every queue
  has a capacity. Every channel has a documented drain strategy.

---

## 6. Correctness Is Non-Negotiable

**Make invalid states unrepresentable.**
Use the type system to eliminate entire bug categories. If a code path should
be unreachable, make it unreachable at compile time — not via a runtime panic
or a comment.

**Errors are values, not afterthoughts.**
Every fallible operation surfaces its failure. No silent discards. No
swallowed errors. Every suppressed error has a comment explaining why
discarding it is provably correct. Use `anyhow::Context` in Rust, structured
error classes in TypeScript.

**Deviating from a specification is a bug.**
When the exposed API has a published specification or contract (the WHATWG
Fetch spec, the Deno.serve contract, an RFC), deviating from it is a
correctness failure — even if the deviation is more convenient to implement.

**Encode the error taxonomy.**
Errors that cross the FFI boundary use `CoreError`/`ErrorCode`. Error
classification is based on a finite set of machine-readable codes, never on
string matching. Adding a new failure mode means adding a new error code.

---

## 7. Standards Inform, They Don't Constrain

This is a new protocol on new transport. We are not bound by backward
compatibility with any existing HTTP stack.

However, we are deeply informed by standards. HTTP semantics, header
conventions, streaming patterns, and compression negotiation exist because
smart people solved real problems. We benefit from that work.

The distinction: the developer-facing API must feel standard because
familiarity is a feature. The wire format must be correct for our transport,
which may or may not match legacy protocols.

**How to apply:**
- When evaluating a feature, ask two separate questions: (1) does the API
  feel like something the developer already knows? (2) does the wire format
  need to match a legacy protocol? The first should almost always be yes.
  The second should almost always be no.
- When we diverge from a standard, document why.
- Prefer adopting well-maintained implementations over building from scratch.

---

## 8. The FFI Boundary Is a Trust Boundary

The FFI boundary is where Rust reliability guarantees meet runtimes that
cannot handle native crashes.

**Panics must never cross the FFI boundary.**
A Rust panic in a JS context is a hard process crash with no recovery. Every
FFI entry point catches panics and converts them to a representable error.
This is non-negotiable.

**The FFI surface must be minimal and stable.**
Every function exposed across the boundary is a contract that is expensive to
change. Design it as if it will never change.

**Errors must be native to the calling language.**
JS users see `DOMException` subtypes. Opaque integers and raw error strings
are never acceptable at the user-facing layer.

**Memory ownership at the boundary must be explicit.**
All state lives in Rust (slotmap registries); platform adapters hold only
opaque `u64` handles. This zero-double-ownership model eliminates an entire
class of memory bugs. Slotmap generational keys prevent stale-handle
use-after-free at the type level.

---

## 9. Test What Matters, Test It Honestly

If a feature isn't tested end-to-end — two real Iroh nodes exchanging real
data over real QUIC connections — it doesn't work. Unit tests verify
components; integration tests verify the product.

Tests are the primary specification. When the docs and the tests disagree,
the tests are right.

**How to apply:**
- Every public-facing behaviour has at least one integration test exercising
  it through the same code path a real user would hit.
- Tests must be deterministic. Flaky tests are bugs. No `sleep`-based
  timing — use `Notify` / `oneshot` for synchronization.
- Test hostile inputs, not just happy paths. Every limit has a test that
  exceeds it.
- Every security invariant has a dedicated test.

---

## 10. Document for the Tooltip

A developer should be able to use this library entirely from IDE
autocompletion and inline documentation, without opening a browser or reading
source code.

Every public symbol — function, type, field, variant — must have
documentation that answers: what does this do, what does it expect, what does
it return, and what can go wrong.

**How to apply:**
- Write documentation before or alongside the code, not after.
- Examples must be realistic and copy-paste-ready, not pseudocode.
- Optional parameters document their default values. Functions that can fail
  document what failures look like.
- Internal symbols need enough context for a contributor to understand the
  design intent, but don't need the same level of polish.

---

## 11. Code Quality Is a Hard Gate

**Linters are not suggestions.**
The codebase passes `cargo clippy -D warnings` and TypeScript strict mode
cleanly. Every suppressed lint has a comment explaining why. CI enforces
this.

**Clever code is a liability.**
If there are two implementations — one straightforward and one ingenious —
choose the straightforward one unless there is a measured, documented
performance reason. Clever code is hard to review, hard to debug, and hard
to change.

**Ownership and structure are designed, not wrestled.**
If the architecture requires repeated fighting against the language's type
system or ownership model, that is a design signal — not a limitation of the
language. Step back and redesign.

---

## Resolving Conflicts

These principles are ordered. When they conflict:

- **Platform feel** beats internal consistency. The platform wins at the
  public boundary.
- **Minimalism** beats convenience. Don't add API surface to save the user
  one line of code.
- **Safety** beats performance. A safe default that's slightly slower is
  better than a fast default that's exploitable.
- **Standards** inform but never override platform feel.
- **Robustness now beats speculative portability.** We can choose host-only
  dependencies when they significantly improve quality, as long as embedded
  portability is preserved by documented protocol boundaries.

When none of these clearly apply, ask: what would the developer expect?
Then do that.

---

## Self-Evaluation Checklist

Before considering any component complete:

- [ ] Is there an existing library that does this? If not used, why not?
- [ ] Are all error paths handled and tested?
- [ ] Are all silenced or swallowed errors justified in comments?
- [ ] Are all I/O and async operations timeout-bounded?
- [ ] Are all queues and channels bounded with a documented capacity?
- [ ] Does cancellation leave the system in a consistent state?
- [ ] Does this feature belong in the core, or does it belong in userland?
- [ ] Does linting pass cleanly?
- [ ] Are failure-path tests written, including hostile inputs?
- [ ] If FFI entry point: is it panic-safe, errors representable in calling language?
- [ ] If implementing a spec: does it comply, deviations documented?
- [ ] Are all resource handles using slotmap generational keys?
- [ ] Is the public API surface minimal — does every exposed item justify its existence?
- [ ] Does the API feel native to the target platform?
