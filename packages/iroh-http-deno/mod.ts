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
  type IrohRequest,
  type NodeOptions,
} from "@momics/iroh-http-shared";
import {
  allocBodyWriter,
  bridge,
  closeEndpoint,
  createEndpointInfo,
  denoAddrFns,
  denoDiscoveryFns,
  denoSessionFns,
  generateSecretKey,
  publicKeyVerify,
  rawConnect,
  rawFetch,
  rawServe,
  secretKeySign,
  stopServe,
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
  return buildNode(
    bridge,
    info,
    rawFetch,
    rawServe,
    rawConnect,
    allocBodyWriter,
    closeEndpoint,
    stopServe,
    denoAddrFns,
    denoDiscoveryFns,
    denoSessionFns,
  );
}

export type { IrohNode, IrohRequest, NodeOptions };
