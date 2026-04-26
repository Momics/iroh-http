# HTTP Gateway

Run an iroh-http node in front of any local HTTP service to make it reachable
from anywhere on the internet — no port forwarding, no VPN, no dynamic DNS.
Share a ticket string and it just works.

## The pattern

```ts
import { createNode } from 'iroh-http-node';

const node = await createNode();

// Print the ticket once — share it however you like (clipboard, QR code, env var)
console.log('ticket:', await node.ticket());

// Proxy every incoming iroh-http request to a local HTTP service
node.serve({}, async (req) => {
  const local = new URL(req.url);
  local.protocol = 'http:';
  local.host = '127.0.0.1:8080';   // ← your local service

  return fetch(local.toString(), {
    method: req.method,
    headers: req.headers,
    body: req.body,
    // @ts-expect-error duplex is required for streaming bodies
    duplex: 'half',
  });
});
```

That's the whole recipe. The caller:

```ts
const node = await createNode();
const ticket = 'nodeXXXX...';  // from the gateway

const res = await node.fetch(ticket.toURL('/api/sensors/temperature'));
console.log(await res.json());
```

## Why this works

iroh-http's `node.serve` handler receives a standard `Request`; returning a
standard `Response` is all that's needed. Standard `fetch` produces exactly
that — so the gateway is just one `fetch` call inside a `serve` handler.

The caller uses a full ticket (public key + relay URL + direct addresses) so
the QUIC connection is established directly. Once the QUIC handshake
completes, every request flows over the encrypted, authenticated channel.

## IoT / ESP32 pattern

An ESP32 or other microcontroller on your home LAN runs a plain HTTP server
on port 80 — no TLS, no auth, no QUIC. The gateway runs on a Raspberry Pi,
NAS, or always-on laptop on the same network:

```
Internet → [iroh QUIC, authenticated] → Gateway (home server)
                                              ↓
                                    [HTTP on LAN, port 80]
                                              ↓
                                         ESP32 / device
```

The ESP32 needs no iroh support at all. The gateway handles all the
cryptography, NAT traversal, and relay connectivity. The ESP32 just
speaks plain HTTP.

```ts
const DEVICE = 'http://esp32-greenhouse.local';

node.serve({}, async (req) => {
  const url = new URL(req.url);
  return fetch(DEVICE + url.pathname + url.search, {
    method: req.method,
    headers: req.headers,
    body: req.body,
    duplex: 'half',
  });
});
```

## Access control

Add a token check in front of the proxy to restrict which peers can reach the
local service:

```ts
// compose() and requireToken() from your own middleware — see recipes/middleware.md
import { compose } from './middleware.ts';
import { requireToken } from './capability-tokens.ts';

node.serve({}, compose(
  requireToken(mySecretKey.publicKey),
  async (req) => fetch(LOCAL + new URL(req.url).pathname, {
    method: req.method, headers: req.headers, body: req.body, duplex: 'half',
  }),
));
```

See [capability-tokens.md](capability-tokens.md).

## Header rewriting

Some local services care about the `Host` header. Set it to what the local
service expects:

```ts
node.serve({}, async (req) => {
  const headers = new Headers(req.headers);
  headers.set('Host', 'localhost:8080');
  headers.delete('Peer-Id');  // don't forward internal headers

  return fetch('http://127.0.0.1:8080' + new URL(req.url).pathname, {
    method: req.method,
    headers,
    body: req.body,
    duplex: 'half',
  });
});
```

## Path routing

Route different paths to different local services:

```ts
const ROUTES: Record<string, string> = {
  '/sensors': 'http://esp32.local',
  '/camera':  'http://192.168.1.50:8080',
  '/api':     'http://127.0.0.1:3000',
};

node.serve({}, async (req) => {
  const path = new URL(req.url).pathname;
  const base = Object.entries(ROUTES).find(([prefix]) => path.startsWith(prefix))?.[1];
  if (!base) return new Response('Not Found', { status: 404 });

  return fetch(base + path, {
    method: req.method, headers: req.headers, body: req.body, duplex: 'half',
  });
});
```

This is a minimal reverse proxy / API gateway — HTTP routing has been solved
for decades and every pattern applies here.

## See also

- [Reverse ingress](reverse-ingress.md) — the same one-line proxy pattern
  focused on the CGNAT / no-port-forwarding use case and security tiering
- [Proximity trust](proximity-trust.md) — restrict gateway access to LAN
  peers only; no token required on the home network
- [Device handoff](device-handoff.md) — use the ticket the gateway prints to
  QR-code the access link onto a physical device
