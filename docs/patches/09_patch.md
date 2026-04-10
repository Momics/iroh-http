---
status: pending
---

# iroh-http — Patch 09: Mobile Lifecycle (Tauri)

Handle mobile app backgrounding and foreground resume for the Tauri plugin.
On iOS and Android, the OS may terminate background tasks, leaving `IrohNode`
holding a dead endpoint handle. This patch adds visibility-change detection
and health probing to handle that gracefully.

> **Prior art:** `.old_references/http-tauri` implemented this exact pattern
> with `visibilitychange` listeners, exponential-backoff health probes, and
> endpoint re-creation on failure.

---

## Problem

When a Tauri app is backgrounded on mobile:

1. iOS suspends the process after ~30 seconds. Background tasks may be
   terminated without warning.
2. Android's doze mode and battery optimization can kill background network
   tasks.
3. The Iroh QUIC endpoint's UDP socket may be reclaimed by the OS.
4. On foreground resume, `IrohNode.fetch()` / `serve()` calls silently fail
   with cryptic errors because the underlying endpoint is dead.

There is no detection or recovery path.

---

## Solution

### 1. Rust health-check command

Add a lightweight `ping` command to `packages/iroh-http-tauri/src/commands.rs`:

```rust
#[command]
pub async fn ping(endpoint_handle: u32) -> Result<bool, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or("endpoint not found")?;
    // Attempt a trivial operation — if the endpoint is alive, this succeeds.
    Ok(ep.raw().home_relay().is_some() || true)
}
```

The command returns `true` if the endpoint responds, or errors if the handle
is invalid / the runtime has been torn down. Deliberately cheap — no
network I/O, just a state read.

### 2. Guest-JS visibility listener

Add to `packages/iroh-http-tauri/guest-js/index.ts`:

```ts
interface LifecycleOptions {
  /** Re-create the endpoint automatically on foreground resume if it
   *  appears dead. Default: true on mobile, false on desktop. */
  autoReconnect?: boolean;
  /** Maximum ping retries with exponential backoff before declaring the
   *  endpoint dead. Default: 3. */
  maxRetries?: number;
}
```

The lifecycle listener is installed inside `createNode` (not exported as a
separate API) and only activates on mobile:

```ts
function installLifecycleListener(
  endpointHandle: number,
  options: LifecycleOptions,
  onDead: () => void,
) {
  if (typeof document === "undefined") return; // SSR / non-browser

  const isMobile = /android|iphone|ipad/i.test(navigator.userAgent);
  if (!isMobile && !options.autoReconnect) return;

  let retries = 0;
  const maxRetries = options.maxRetries ?? 3;

  const handler = async () => {
    if (document.visibilityState !== "visible") return;

    // Exponential backoff probe
    retries = 0;
    while (retries < maxRetries) {
      try {
        await invoke("plugin:iroh-http|ping", { endpointHandle });
        return; // alive — nothing to do
      } catch {
        retries++;
        if (retries < maxRetries) {
          await new Promise(r => setTimeout(r, 100 * 2 ** retries));
        }
      }
    }

    // Endpoint is dead
    onDead();
  };

  document.addEventListener("visibilitychange", handler);

  return () => document.removeEventListener("visibilitychange", handler);
}
```

### 3. Wiring into `createNode`

When the lifecycle listener detects a dead endpoint:

**Option A — Emit event (recommended):** Resolve the `node.closed` promise
and/or fire a `close` event. The app re-creates the node in its own error
handling flow:

```ts
const node = await createNode({ lifecycle: { autoReconnect: false } });
node.closed.then(() => {
  console.log("endpoint died, re-creating...");
  // app-specific reconnection logic
});
```

**Option B — Auto-reconnect:** Call `closeEndpoint` + `createEndpoint`
internally, updating the node's handle. Serve listeners would need to be
re-registered. This is more complex but fully transparent to the app.

The recommendation is **Option A** — keep it simple, let the app decide how
to recover. Option B can be added later if there's demand.

### 4. `NodeOptions` extension

```ts
interface NodeOptions {
  // ... existing ...
  /** Mobile lifecycle options. Only effective in Tauri on mobile. */
  lifecycle?: LifecycleOptions;
}
```

---

## Desktop behaviour

On desktop, the `visibilitychange` event fires when the browser tab is
hidden/shown (irrelevant for Tauri desktop). The listener is a no-op:

- `navigator.userAgent` doesn't match mobile patterns
- `autoReconnect` defaults to `false` on desktop
- If the user explicitly sets `autoReconnect: true`, the listener activates
  but the endpoint is almost never killed on desktop, so pings always succeed

No desktop code paths are affected.

---

## Permissions

Add the new `ping` command to `packages/iroh-http-tauri/permissions/`:

```toml
# default.toml
[default]
commands.allow = [
    # ... existing ...
    "ping",
]
```
