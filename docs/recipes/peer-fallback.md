# Peer Fallback

Fetch from the best available peer, falling back to secondaries when the
primary is unreachable.

## Simple fallback

```ts
async function fetchWithFallback(
  node: IrohNode,
  peers: string[],
  path: string,
  init?: IrohFetchInit,
): Promise<Response> {
  let lastError: unknown;
  for (const peer of peers) {
    try {
      const res = await node.fetch(peer, path, init);
      if (res.ok) return res;
    } catch (err) {
      lastError = err;
    }
  }
  throw lastError ?? new Error('all peers failed');
}

// Usage:
const res = await fetchWithFallback(node, [primaryPeer, backupPeer1, backupPeer2], '/data');
```

## Race the fastest peer

Try all peers simultaneously and take the first successful response:

```ts
async function fetchFastest(
  node: IrohNode,
  peers: string[],
  path: string,
): Promise<Response> {
  const controller = new AbortController();

  const attempts = peers.map((peer) =>
    node.fetch(peer, path, { signal: controller.signal })
      .then((res) => {
        if (!res.ok) throw new Error(`${res.status}`);
        controller.abort();  // cancel the rest
        return res;
      }),
  );

  return Promise.any(attempts);
}
```

`Promise.any` resolves with the first non-rejected response. Once one succeeds
the `AbortSignal` cancels in-flight requests to the other peers.

## Retry with backoff

```ts
async function fetchWithRetry(
  node: IrohNode,
  peer: string,
  path: string,
  { retries = 3, baseDelayMs = 200 } = {},
): Promise<Response> {
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const res = await node.fetch(peer, path);
      if (res.status !== 503 && res.status !== 429) return res;

      const retryAfter = res.headers.get('Retry-After');
      const delay = retryAfter ? parseInt(retryAfter) * 1000 : baseDelayMs * 2 ** attempt;
      await new Promise((r) => setTimeout(r, delay));
    } catch {
      if (attempt === retries) throw;
      await new Promise((r) => setTimeout(r, baseDelayMs * 2 ** attempt));
    }
  }
  throw new Error('max retries exceeded');
}
```

## Notes

- `iroh-http` handles the QUIC-level connection pool automatically. A peer
  that was unreachable may become reachable after relay re-discovery — the
  retry pattern above benefits from this automatically.
- Tickets embed direct addresses. Using a ticket for the primary and a bare
  node ID for the fallback means the primary gets a direct connection attempt
  while the fallback goes via relay — a natural quality-of-service hierarchy.

## See also

- [Cooperative backup](cooperative-backup.md) — restore drives the same
  try-next-on-failure loop across a set of backup peers
- [Offline-first](offline-first.md) — when all peers fail, queue the write
  and replay it when they reappear
- [Reverse ingress](reverse-ingress.md) — the Pi use case where the relay
  path is the only path; fallback adds resilience
