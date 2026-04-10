# @momics/iroh-http-deno

Deno native library for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over QUIC.

## Usage

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

node.serve({}, (req) => new Response("Hello from Deno iroh-http!"));

const res = await node.fetch(remotePeerId, "/hello");
console.log(await res.text());
await node.close();
```

## Build from source

```sh
cd packages/iroh-http-deno
deno task build
```

The native library is placed in `lib/`.

## Options

```ts
const node = await createNode({
  key: savedKey,
  discovery: { mdns: true, serviceName: "my-app.iroh-http" },
  drainTimeout: 30_000,
});
```

## License

MIT OR Apache-2.0
