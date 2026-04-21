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
  IrohNode,
  type NodeOptions,
} from "@momics/iroh-http-shared";
import {
  DenoAdapter,
  createEndpointInfo,
  generateSecretKey,
  publicKeyVerify,
  secretKeySign,
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
  const adapter = new DenoAdapter(info.endpointHandle);
  return IrohNode._create(adapter, info, options, waitEndpointClosed(info.endpointHandle));
}

export type { IrohNode, NodeOptions };
