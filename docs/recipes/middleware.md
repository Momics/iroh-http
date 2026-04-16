# Middleware

Serve handlers are plain functions `(req: Request) => Response`. Middleware is
a function that wraps a handler: `(handler) => handler`. They compose cleanly
without any framework magic. Define these utilities in your own application
code.

## `compose()`

```ts
type Handler = (req: Request) => Response | Promise<Response>;
type Middleware = (next: Handler) => Handler;

function compose(...fns: [...Middleware[], Handler]): Handler {
  const handler = fns[fns.length - 1] as Handler;
  const middlewares = fns.slice(0, -1) as Middleware[];
  return middlewares.reduceRight((h, m) => m(h), handler);
}
```

Usage:

```ts
node.serve({}, compose(
  logger(),
  rateLimit({ requestsPerSecond: 10 }),
  requireToken(trustedKey),
  myHandler,
));
```

Middlewares run left-to-right; `myHandler` runs last.

## Rate limiting

```ts
function rateLimit(opts: { requestsPerSecond: number; burst?: number }): Middleware {
  const buckets = new Map<string, { tokens: number; last: number }>();
  const burst = opts.burst ?? opts.requestsPerSecond;

  return (next) => (req) => {
    const peer = req.headers.get('Peer-Id') ?? '';
    const now = Date.now() / 1000;
    let b = buckets.get(peer) ?? { tokens: burst, last: now };
    b.tokens = Math.min(burst, b.tokens + (now - b.last) * opts.requestsPerSecond);
    b.last = now;
    buckets.set(peer, b);

    if (b.tokens < 1) {
      const retryAfter = Math.ceil((1 - b.tokens) / opts.requestsPerSecond);
      return new Response('Too Many Requests', {
        status: 429,
        headers: { 'Retry-After': String(retryAfter) },
      });
    }
    b.tokens -= 1;
    return next(req);
  };
}
```

`Peer-Id` is injected by the Rust layer on every request — it is the
peer's verified Ed25519 public key, not spoofable. No additional auth needed
to identify the caller for rate limiting purposes.

## Logging

```ts
function logger(): Middleware {
  return (next) => async (req) => {
    const start = Date.now();
    const res = await next(req);
    const peer = req.headers.get('Peer-Id')?.slice(0, 8) ?? '?';
    console.log(`${req.method} ${new URL(req.url).pathname} ${res.status} ${Date.now() - start}ms [${peer}]`);
    return res;
  };
}
```

## Auth guard

See [capability-tokens.md](capability-tokens.md) for a full token-based auth
middleware.
