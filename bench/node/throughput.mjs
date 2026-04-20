import { baseline, bench, group, run } from "mitata";
import { createServer } from "node:http";
import { once } from "node:events";
import { createNode } from "../../packages/iroh-http-node/lib.js";

const SIZES = [1, 1024, 64 * 1024, 1024 * 1024];

function payload(size) {
  return Buffer.alloc(size, 0x61);
}

async function makeTcpServer() {
  const server = createServer((req, res) => {
    const size = Number(new URL(req.url ?? "/", "http://localhost").searchParams.get("size") ?? 1024);
    const body = payload(size);
    res.writeHead(200, {
      "content-type": "application/octet-stream",
      "content-length": body.length,
    });
    res.end(body);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const addr = server.address();
  const port = typeof addr === "object" && addr ? addr.port : 0;
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

const tcp = await makeTcpServer();
const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const serveAbort = new AbortController();
const serveHandle = server.serve({ signal: serveAbort.signal }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size));
});

try {
  await client.fetch(serverId, "httpi://bench.local/data?size=1024", { directAddrs: serverAddrs });

  group("throughput", () => {
    for (const size of SIZES) {
      baseline(`tcp ${size}B`, async () => {
        const res = await fetch(`${tcp.baseUrl}/data?size=${size}`);
        await res.arrayBuffer();
      });

      bench(`iroh ${size}B`, async () => {
        const res = await client.fetch(serverId, `httpi://bench.local/data?size=${size}`, {
          directAddrs: serverAddrs,
        });
        await res.arrayBuffer();
      });
    }
  });

  await run();
} finally {
  serveAbort.abort();
  await serveHandle.finished.catch(() => {});
  await server.close();
  await client.close();
  await tcp.close();
}
