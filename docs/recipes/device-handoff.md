# Device Handoff

Transfer state from one device to another by scanning a QR code. No account,
no cloud, no pairing ceremony. Scan → connect → done.

## The insight

A ticket encodes a node ID plus a one-use path. It fits in a QR code. The
receiving device scans it, connects directly to the sender, fetches the
payload, and the transfer is complete. The sender can expire the ticket after
one use.

```
┌─────────────┐    QR / deep link    ┌─────────────┐
│  Device A   │ ──── ticket ───────► │  Device B   │
│  (sender)   │                      │  (receiver) │
│             │ ◄─── iroh QUIC ───── │             │
│             │   (direct or relay)  │             │
└─────────────┘                      └─────────────┘
```

The ticket is just the sender's node ID and a random one-time path. No server
is involved after the QR code is displayed.

## Ticket format

```ts
interface HandoffTicket {
  nodeId: string;  // sender's node ID in hex
  path: string;    // e.g. "/handoff/a3f92c"
  port?: number;   // defaults to 8080
}

function encodeTicket(t: HandoffTicket): string {
  return btoa(JSON.stringify(t))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}

function decodeTicket(s: string): HandoffTicket {
  return JSON.parse(atob(s.replace(/-/g, '+').replace(/_/g, '/')));
}
```

For a QR code, encode as a deep link: `iroh://handoff/<base64url-ticket>`.

## Sender side

```ts
import { IrohNode } from 'iroh-http';

async function offerHandoff(node: IrohNode, payload: unknown): Promise<string> {
  // Generate a one-time random path
  const token = crypto.randomUUID().replace(/-/g, '').slice(0, 12);
  const path = `/handoff/${token}`;
  let claimed = false;

  const ac = new AbortController();

  node.serve({ signal: ac.signal }, async (req) => {
    const url = new URL(req.url);
    if (url.pathname !== path) return new Response('Not Found', { status: 404 });
    if (claimed) return new Response('Gone', { status: 410 });

    // Verify the requester is on the same trust tier as expected.
    // Optional: require a capability token here for stricter control.
    claimed = true;
    ac.abort(); // stop serving after first claim

    return Response.json(payload);
  });

  const ticket: HandoffTicket = {
    nodeId: node.nodeId(),
    path,
    port: 8080,
  };

  return encodeTicket(ticket);
}
```

## Receiver side

```ts
async function claimHandoff(node: IrohNode, ticketStr: string): Promise<unknown> {
  const ticket = decodeTicket(ticketStr);
  const url = `iroh://${ticket.nodeId}:${ticket.port ?? 8080}${ticket.path}`;
  const res = await node.fetch(url);
  if (!res.ok) throw new Error(`Handoff failed: ${res.status}`);
  return res.json();
}
```

## QR display (browser / Tauri)

```ts
import QRCode from 'qrcode'; // any QR library

const ticketStr = await offerHandoff(node, { clipboard: 'Hello from Device A' });
const deepLink = `iroh://handoff/${ticketStr}`;
const dataUrl = await QRCode.toDataURL(deepLink);
document.querySelector('img')!.src = dataUrl;
```

## Use cases

### Clipboard share

```ts
// Device A — copy
const ticket = await offerHandoff(node, {
  type: 'clipboard',
  text: await navigator.clipboard.readText(),
});
displayQR(ticket);

// Device B — paste
const { text } = await claimHandoff(node, scannedTicket) as any;
await navigator.clipboard.writeText(text);
```

### File transfer

```ts
// Stream a file instead of JSON for large payloads
async function offerFile(node: IrohNode, file: File): Promise<string> {
  const token = crypto.randomUUID().replace(/-/g, '').slice(0, 12);
  const path = `/file/${token}`;
  let claimed = false;
  const ac = new AbortController();

  node.serve({ signal: ac.signal }, async (req) => {
    if (new URL(req.url).pathname !== path) {
      return new Response('Not Found', { status: 404 });
    }
    if (claimed) return new Response('Gone', { status: 410 });
    claimed = true;
    ac.abort();

    return new Response(file.stream(), {
      headers: {
        'Content-Type': file.type || 'application/octet-stream',
        'Content-Disposition': `attachment; filename="${file.name}"`,
        'Content-Length': String(file.size),
      },
    });
  });

  return encodeTicket({ nodeId: node.nodeId(), path });
}
```

### Session continuation

Transfer auth state from a desktop app to a mobile device — no login required
on the second device:

```ts
const ticket = await offerHandoff(node, {
  type: 'session',
  accessToken: currentSession.accessToken,
  refreshToken: currentSession.refreshToken,
});
```

## Security properties

- The ticket contains no secret. Anyone who obtains it can claim the payload —
  treat it like a link to a private file.
- `claimed = true` + `ac.abort()` ensures the payload is served exactly once.
- For sensitive payloads (session tokens, private keys), add a
  [capability token](capability-tokens.md) check: the QR code embeds both
  the ticket and a short-lived auth token, and the sender verifies it before
  serving.
- Because the transport is iroh QUIC, the sender knows the receiver's node ID.
  You can log it or add it to a trusted list after a successful handoff.

## See also

- [Tickets](../features/tickets.md) — how ticket strings encode node IDs and
  are parsed by `node.fetch()`
- [Proximity trust](proximity-trust.md) — grant extra trust if the claiming
  device is on the same LAN
- [Reverse ingress](reverse-ingress.md) — same ticket mechanism used to reach
  a device behind CGNAT
