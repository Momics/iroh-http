/**
 * iroh-http-deno — public API.
 *
 * ```ts
 * import { createNode } from "./mod.ts";
 *
 * const node = await createNode({ key: savedKey });
 * node.serve({}, req => new Response("hello"));
 * const res = await node.fetch(peerId, "/api");
 * ```
 */

import { buildNode, type NodeOptions, type IrohNode } from "npm:iroh-http-shared";
import {
  bridge,
  rawFetch,
  rawServe,
  rawConnect,
  allocBodyWriter,
  createEndpointInfo,
  closeEndpoint,
} from "./adapter.ts";

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
  );
}

export type { NodeOptions, IrohNode };
