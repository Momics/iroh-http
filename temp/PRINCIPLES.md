# Engineering Principles

These principles are code-agnostic and stable. They apply across the entire codebase regardless of language, module, or implementation detail. Treat them as invariants. When two approaches are in conflict, these principles — in order — are the tiebreaker.

---

## Hierarchy of Values

When principles conflict, prefer the higher value:

1. **Correctness** — the code must be provably right
2. **Reliability** — the code must never silently fail
3. **Clarity** — the code must be understandable by a future maintainer
4. **Performance** — the code must not waste resources
5. **Ergonomics** — the code must be pleasant to use

Never sacrifice a higher value for a lower one.

---

## Don't Invent What Already Exists

Before writing any non-trivial component, ask:

> *"Does a well-maintained library already solve this, and can it be composed with what I have?"*

If yes, use it. Every line of custom infrastructure is a line that can have bugs, needs tests, and must be maintained forever. Writing your own solution when a good one exists is a liability, not a feature.

**Custom code must be explicitly justified.** If a component is written from scratch, there must be a clear documented reason why no existing solution fit — not "I wasn't sure," but a concrete incompatibility or gap.

---

## Know What You Are

This library is **core infrastructure**, not a framework. Its job is to be a thin, correct, reliable transport layer that disappears underneath the code built on top of it.

The temptation will be to add convenience, abstractions, or features that "seem useful." Resist it. Every feature added to the core must work correctly, be tested, be documented, and be maintained. **If a feature can live in userland, it must live in userland.** The core executes. It does not make policy decisions.

The measure of success is: *does the exposed API behave exactly as the user of that API expects, based on the contract it claims to fulfil?* Nothing more, nothing less.

---

## Correctness Principles

**Make invalid states unrepresentable.**
Use the type system to eliminate entire bug categories. If a code path should be unreachable, make it unreachable at compile time — not via a runtime panic or a comment.

**Errors are values, not afterthoughts.**
Every fallible operation must surface its failure. No silent discards. No swallowed errors. Every suppressed error must have a comment explaining why discarding it is provably correct.

**Preserve error context.**
When propagating errors, never discard the source. A stripped error message that reaches the user is a debugging nightmare. Errors should tell the full story of what went wrong and where.

**Deviating from a specification is a bug.**
When the exposed API has a published specification or an established contract (a web standard, an RFC, a runtime API contract), deviating from it is a correctness failure — even if the deviation is more convenient to implement. Users will write code assuming spec-compliant behavior. Surprises are bugs.

---

## Reliability Principles

**Assume the network is hostile and broken.**
Every I/O operation can fail, hang, or return garbage. Always apply timeouts. An operation without a timeout bound is a latent hang. Treat a missing timeout as a bug.

**Backpressure is not optional.**
Unbounded queues and channels are latent out-of-memory bugs. Every queue must have a capacity. Every queue must have a documented drain strategy.

**Never silently swallow errors.**
Discarding an error without handling or logging it is a reliability hole. If an error is intentionally ignored, that decision must be documented inline.

**Resource cleanup is explicit and verified.**
Connections, file handles, and allocated resources must be released on every exit path — success, error, and panic. Use RAII. Test that resources are released under failure conditions, not just happy-path conditions.

**Cancellation must leave the system consistent.**
When an async operation is cancelled or a task is dropped, shared state must not be left corrupted, resources must not be leaked, and locks must not remain held. Design for cancellation explicitly.

---

## FFI Boundary Principles

The FFI boundary is where reliability guarantees meet runtimes that cannot handle native crashes or panics.

**Panics must never cross the FFI boundary.**
A native panic in a JS or Python context is a hard process crash with no recoverable error. Every FFI entry point must catch panics and convert them to a representable error before returning. This is non-negotiable.

**The FFI surface must be minimal and stable.**
Every function exposed across the boundary is a contract that is expensive to change. Design it as if it will never change. Keep the surface as small as possible.

**Errors must be representable in the calling language.**
Native result and error types cannot cross FFI. Define a clear, consistent error representation strategy and apply it everywhere. Errors reaching the user should look native to the language they are using.

**Memory ownership at the boundary must be explicit and documented.**
Who allocates, who frees, and when. Every buffer, string, or pointer that crosses the boundary needs a documented ownership contract. Memory leaks and use-after-free at FFI boundaries are silent and catastrophic.

---

## Code Quality Principles

**Linters are a hard gate, not a suggestion.**
The codebase must pass linting cleanly. Every suppressed lint must have a comment explaining why. Lint suppression without explanation is not acceptable.

**Tests are part of the implementation.**
Tests are not an afterthought. Unit tests cover individual behavior. Integration tests cover the public API contract. Failure paths must be tested — not just the happy path.

**Clever code is a liability.**
If there are two implementations — one straightforward and one ingenious — choose the straightforward one unless there is a measured, documented reason not to. Clever code is hard to review, hard to debug, and hard to change.

**Ownership and structure are designed, not wrestled.**
If the architecture requires repeated fighting against the language's type system or ownership model, that is a design signal — not a limitation of the language. Step back and redesign.

---

## Self-Evaluation Checklist

Before considering any component complete:

- [ ] Is there an existing library that does this? If not used, why not — and is that documented?
- [ ] Are all error paths handled and tested?
- [ ] Are all silenced or swallowed errors justified in comments?
- [ ] Are all I/O and async operations timeout-bounded?
- [ ] Are all queues and channels bounded with a documented capacity?
- [ ] Does cancellation leave the system in a consistent state?
- [ ] Does this feature belong in the core, or does it belong in userland?
- [ ] Does linting pass cleanly?
- [ ] Are failure-path tests written?
- [ ] If this is an FFI entry point: is it panic-safe, and are errors representable in the calling language?
- [ ] If this implements a specification: does it comply, and are any intentional deviations documented?
