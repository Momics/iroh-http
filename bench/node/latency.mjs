import { baseline, bench, group, run } from "mitata";
import { createServer } from "node:http";
import { once } from "node:events";
import { createNode } from "../../packages/iroh-http-node/lib.js";

const SMALL = Buffer.from('{"ok":true}');

async function makeTcpServer() {
  const server = createServer((_req, res) => {
    res.writeHead(200, {
      "content-type": "application/json",
      "content-length": SMALL.length,
    });
    res.end(SMALL);
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
const serveHandle = server.serve({ signal: serveAbort.signal }, () => new Response(SMALL));

try {
  await client.fetch(serverId, "httpi://bench.local/latency", { directAddrs: serverAddrs });

  group("latency", () => {
    baseline("tcp 1KB-ish", async () => {
      const res = await fetch(`${tcp.baseUrl}/latency`);
      await res.arrayBuffer();
    });

    bench("iroh 1KB-ish", async () => {
      const res = await client.fetch(serverId, "httpi://bench.local/latency", {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
    });
  });

  group("multiplexing", () => {
    bench("iroh 32 concurrent streams", async () => {
      await Promise.all(
        Array.from({ length: 32 }, () =>
          client
            .fetch(serverId, "httpi://bench.local/latency", { directAddrs: serverAddrs })
            .then((res) => res.arrayBuffer()),
        ),
      );
    });
  });

  await run();
} finally {
  serveAbort.abort();
  await serveHandle.finished.catch(() => {});
  await server.close();
  await client.close();
  await tcp.close();
}
