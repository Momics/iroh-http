/**
 * iroh-http-deno — public API.
 *
 * ```ts
 * import { createNode } from "@momics/iroh-http-deno";
 *
 * const node = await createNode({ key: savedKey });
 * const server = node.serve(req => new Response("hello"));
 * await server.finished;
 * const res = await node.fetch(peerId, "/api");
 * ```
 */

import {
  buildNode,
  type IrohNode,
  type NodeOptions,
} from "@momics/iroh-http-shared";
import {
  makeAllocBodyWriter,
  makeBridge,
  closeEndpoint,
  createEndpointInfo,
  denoAddrFns,
  denoDiscoveryFns,
  makeDenoSessionFns,
  generateSecretKey,
  publicKeyVerify,
  rawConnect,
  rawFetch,
  rawServe,
  secretKeySign,
  stopServe,
  waitEndpointClosed,
} from "./src/adapter.ts";
export { generateSecretKey, publicKeyVerify, secretKeySign };
export { PublicKey, SecretKey } from "@momics/iroh-http-shared";

/**
 * Create an Iroh node for peer-to-peer HTTP.
 *
 * @param options Optional configuration.  Omit `key` to generate a fresh identity.
 */
export async function createNode(options?: NodeOptions): Promise<IrohNode> {
  const info = await createEndpointInfo(options);
  return buildNode({
    bridge: makeBridge(info.endpointHandle),
    info,
    rawFetch,
    rawServe,
    rawConnect,
    allocBodyWriter: makeAllocBodyWriter(info.endpointHandle),
    closeEndpoint,
    stopServe,
    nativeClosed: waitEndpointClosed(info.endpointHandle),
    addrFns: denoAddrFns,
    discoveryFns: denoDiscoveryFns,
    sessionFns: makeDenoSessionFns(info.endpointHandle),
  });
}

export type { IrohNode, NodeOptions };
