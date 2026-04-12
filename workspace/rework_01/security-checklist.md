# Security and Behavior Parity Checklist

This checklist is mandatory for the Hyper rework.

If any item fails, the rework is not complete.

## 1) Resource bounds

1. Max request-header bytes enforced.
2. Max request-body bytes enforced.
3. Trailer bytes bounded (new explicit limit if not already present).
4. Per-request timeout enforced.
5. Global concurrency limit enforced.
6. Per-peer fairness enforced beyond raw connection count when needed.

## 2) Cancellation and lifecycle

1. Fetch cancellation aborts transport work and body readers deterministically.
2. Stream cancellation paths do not silently convert errors into EOF.
3. Serve stop/drain does not orphan tasks or leave unresolved requests.
4. Trailer send/receive paths resolve exactly once (including failure paths).

## 3) Protocol correctness

1. `httpi://` semantics are preserved at public API boundaries.
2. ALPN versioning is explicit and documented for wire-format breaks.
3. Duplex upgrade behavior remains deterministic (`101` + raw stream transition).
4. Method validation allows extension methods (valid token syntax), not just
   common verbs.

## 4) Error contract

1. Core error-code taxonomy is canonical and tested.
2. Adapters map core codes consistently.
3. No plain-string/opaque errors leak through public API where typed mapping is expected.

## 5) Testing gates

1. Existing integration tests pass.
2. New regression tests added for each changed invariant.
3. Tests are deterministic (no timing sleeps as correctness mechanism).
4. Large-body and hostile-input cases are included.
