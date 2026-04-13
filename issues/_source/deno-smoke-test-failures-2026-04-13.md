---
date: 2026-04-13
status: open
---

# Deno smoke test failures (2026-04-13)

Discovered by running `deno test --allow-ffi --allow-read --allow-net test/smoke.test.ts` in `packages/iroh-http-deno` after adding inline 20s timeouts to the networking tests.

---

## BUG-001 (`P1`) Concurrent Deno responses are mis-routed to wrong callers

**Evidence**

```
error: AssertionError: Values are not equal: response 8 body mismatch
  Actual:   echo:/path6
  Expected: echo:/path8
```

10 concurrent `client.fetch()` calls receive each other's response bodies. The Deno serve polling loop dispatches responses FIFO from the mpsc queue, but `Promise.all` correlates results by index, not by `reqHandle`. Response bodies are mis-routed.

**Root cause**

`nextRequest` returns items in arrival order. Multiple concurrent inflight fetches receive responses out of order relative to the originating `fetch()` call. JavaScript's `Promise.all` expects result[i] to correspond to input[i], but the polling loop has no per-handle correlation — it returns whatever came off the queue first.

**Impact**

In production, two concurrent Deno clients can silently receive each other's response bodies. This is a correctness bug, not just a test flakiness issue.

**Files**

- `packages/iroh-http-deno/src/adapter.ts` — `rawServe` polling loop
- `packages/iroh-http-deno/src/dispatch.rs` — `next_request`, `serve_start`

**Remediation**

The polling loop dispatches each queued request to the user callback independently (fire-and-forget). The correlation issue is actually at the test level — `Promise.all` over 10 *independent* fetches is fine because each `rawFetch` call awaits its own response. Re-run with `--trace-leaks` and add per-fetch correlation logging to confirm whether the mis-routing is in the serve dispatch path or in the client-side fetch queue.

---

## BUG-002 (`P1`) QUIC connection to LAN IP hangs for 20s; basic serve/fetch tests time out

**Evidence**

```
server addrs: ["192.168.50.16:51908"]
serve + fetch — basic round-trip ... FAILED (20s)
serve + fetch — POST with body ... FAILED (20s)
```

`server.addr()` returns the **LAN WiFi interface address** (`192.168.50.16`), not loopback. The QUIC connection from client to that address stalls, eventually falling back through the relay which takes longer than the 20s test timeout.

**Root cause**

iroh binds on all interfaces by default. On macOS, `addr()` returns the first non-loopback address. Passing that as `directAddrs` to `rawFetch` tells iroh to prefer a LAN path, but same-machine LAN traffic may not route correctly or may be blocked by packet filter rules.

Node tests work because napi internally resolves same-process connections faster, or because the connection pool already has an open connection.

**Impact**

Any Deno test or production code that connects two nodes on the same machine using `addr().addrs` will hang or timeout unless a loopback address is among the returned addrs.

**Files**

- `packages/iroh-http-deno/test/smoke.test.ts` — networking tests
- `crates/iroh-http-core/src/endpoint.rs` — address reporting

**Remediation**

Option A (test fix): In test `createNode` calls, pass `bindAddr: "127.0.0.1:0"` so iroh binds to loopback only, and `addr()` returns a loopback `directAddrs` entry.

Option B (library fix): When no `directAddrs` are specified by the caller, have the client attempt a loopback path first if both nodes share the same process/machine (not always detectable).

Option A is the immediate fix. Option B is a broader improvement.

---

## BUG-003 (`P3`) `withTimeout` timer leaks when a test errors mid-flight

**Evidence**

```
error: Leaks detected:
  - A timer was started in this test, but never completed.
  - An async operation to do a non blocking ffi call was started in this test, but never completed.
```

The inline `withTimeout` helper leaves a dangling `setTimeout` when the inner promise rejects before the timer fires (BUG-001 causes BUG-003 here).

**Root cause**

Consequence of BUG-001. Once BUG-001 is fixed the leak disappears. If `withTimeout` is kept, it should call `clearTimeout` in a `finally` block.

**Remediation**

Fix BUG-001 first. If `withTimeout` is retained, use:

```ts
function withTimeout<T>(ms: number, fn: () => Promise<T>): Promise<T> {
  let id: ReturnType<typeof setTimeout>;
  const timer = new Promise<never>((_, reject) => {
    id = setTimeout(() => reject(new Error(`Test timed out after ${ms}ms`)), ms);
  });
  return Promise.race([fn().finally(() => clearTimeout(id!)), timer]);
}
```
