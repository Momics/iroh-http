import { createNode } from "../../packages/iroh-http-deno/mod.ts";

const SIZES = [1, 1024, 64 * 1024, 1024 * 1024];

function payload(size: number): Uint8Array {
  return new Uint8Array(size).fill(0x61);
}

const tcpHandler = async (req: Request): Promise<Response> => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size), {
    headers: { "content-type": "application/octet-stream" },
  });
};

const tcpServer = Deno.serve({ hostname: "127.0.0.1", port: 0 }, tcpHandler);
const tcpAddr = tcpServer.addr as Deno.NetAddr;
const tcpBase = `http://${tcpAddr.hostname}:${tcpAddr.port}`;

const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const abort = new AbortController();
const serveHandle = server.serve({ signal: abort.signal }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size));
});

await client.fetch(serverId, "httpi://bench.local/data?size=1024", { directAddrs: serverAddrs });

for (const size of SIZES) {
  Deno.bench(`throughput iroh ${size}B`, { group: "throughput" }, async () => {
    const res = await client.fetch(serverId, `httpi://bench.local/data?size=${size}`, {
      directAddrs: serverAddrs,
    });
    await res.arrayBuffer();
  });

  Deno.bench(`throughput tcp ${size}B`, { group: "throughput", baseline: true }, async () => {
    const res = await fetch(`${tcpBase}/data?size=${size}`);
    await res.arrayBuffer();
  });
}

globalThis.addEventListener("unload", () => {
  abort.abort();
  void serveHandle.finished.catch(() => {});
  void server.close();
  void client.close();
  tcpServer.shutdown();
});
