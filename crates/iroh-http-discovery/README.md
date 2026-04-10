# iroh-http-discovery

Optional mDNS local network discovery for [iroh-http](https://github.com/momics/iroh-http).

Implements [Iroh's](https://iroh.computer) `Discovery` trait using mDNS, allowing nodes to find each other on the same local network without relay servers.

## When to use

- **Desktop apps** (macOS, Linux, Windows) that need local peer discovery
- **Node.js** servers on a LAN

## When not to use

- **iOS/Android** — use native service discovery APIs (`NSDNetService`, `NsdManager`) via Tauri mobile plugins instead
- **Environments with only relay/DNS discovery** — this crate isn't needed

## Usage

This crate is typically not used directly. Enable the `mdns` feature in your platform adapter or pass it to `iroh-http-core` via `NodeOptions`:

```ts
const node = await createNode({
  discovery: { mdns: true, serviceName: "my-app.iroh-http" }
});
```

## License

MIT OR Apache-2.0
