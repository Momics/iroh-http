import { createNode } from "../../packages/iroh-http-deno/mod.ts";

const payload = new TextEncoder().encode('{"ok":true}');

const tcpServer = Deno.serve({ hostname: "127.0.0.1", port: 0 }, () => new Response(payload));
const tcpAddr = tcpServer.addr as Deno.NetAddr;
const tcpBase = `http://${tcpAddr.hostname}:${tcpAddr.port}`;

const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const abort = new AbortController();
const serveHandle = server.serve({ signal: abort.signal }, () => new Response(payload));

await client.fetch(serverId, "httpi://bench.local/latency", { directAddrs: serverAddrs });

Deno.bench("latency tcp 1KB-ish", { group: "latency", baseline: true }, async () => {
  const res = await fetch(`${tcpBase}/latency`);
  await res.arrayBuffer();
});

Deno.bench("latency iroh 1KB-ish", { group: "latency" }, async () => {
  const res = await client.fetch(serverId, "httpi://bench.local/latency", {
    directAddrs: serverAddrs,
  });
  await res.arrayBuffer();
});

Deno.bench("multiplexing iroh 32 streams", { group: "multiplexing" }, async () => {
  await Promise.all(
    Array.from({ length: 32 }, () =>
      client
        .fetch(serverId, "httpi://bench.local/latency", { directAddrs: serverAddrs })
        .then((res) => res.arrayBuffer()),
    ),
  );
});

globalThis.addEventListener("unload", () => {
  abort.abort();
  void serveHandle.finished.catch(() => {});
  void server.close();
  void client.close();
  tcpServer.shutdown();
});
