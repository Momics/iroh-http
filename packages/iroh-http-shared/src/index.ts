/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */

export type { Bridge, FfiRequest, FfiResponseHead, FfiResponse, RequestPayload,
              NodeOptions, IrohNode, EndpointInfo, RawServeFn, RawFetchFn, AllocBodyWriterFn,
              FfiDuplexStream, BidirectionalStream, DuplexStream, RawConnectFn } from "./bridge.js";
export { makeReadable, pipeToWriter, bodyInitToStream } from "./streams.js";
export { makeFetch, makeConnect } from "./fetch.js";
export { makeServe } from "./serve.js";
export { PublicKey, SecretKey, resolveNodeId } from "./keys.js";
export {
  IrohError, IrohBindError, IrohConnectError, IrohStreamError, IrohProtocolError,
  classifyError, classifyBindError,
} from "./errors.js";

import type { Bridge, EndpointInfo, NodeOptions, IrohNode, RawServeFn, RawFetchFn, AllocBodyWriterFn, RawConnectFn } from "./bridge.js";
import { makeFetch, makeConnect } from "./fetch.js";
import { makeServe } from "./serve.js";
import { PublicKey, SecretKey } from "./keys.js";

/**
 * Factory that constructs an `IrohNode` from platform primitives.
 *
 * Each platform adapter calls this after binding an endpoint.
 *
 * @param bridge          Platform bridge implementation.
 * @param info            Endpoint info returned by the low-level bind.
 * @param rawFetch        Low-level fetch function (platform-specific).
 * @param rawServe        Low-level serve function (platform-specific).
 * @param rawConnect      Low-level duplex connect function (platform-specific).
 * @param allocBodyWriter Synchronously allocates a body writer handle.
 * @param closeEndpoint   Closes the bound endpoint.
 */
export function buildNode(
  bridge: Bridge,
  info: EndpointInfo,
  rawFetch: RawFetchFn,
  rawServe: RawServeFn,
  rawConnect: RawConnectFn,
  allocBodyWriter: AllocBodyWriterFn,
  closeEndpoint: (handle: number) => Promise<void>
): IrohNode {
  let resolveClosed!: () => void;
  const closedPromise = new Promise<void>((resolve) => {
    resolveClosed = resolve;
  });

  const publicKey = PublicKey.fromString(info.nodeId);
  const secretKey = SecretKey._fromBytesWithPublicKey(info.keypair, publicKey);

  return {
    publicKey,
    secretKey,
    nodeId: info.nodeId,
    keypair: info.keypair,
    fetch: makeFetch(bridge, info.endpointHandle, rawFetch, allocBodyWriter),
    serve: makeServe(bridge, info.endpointHandle, rawServe),
    createBidirectionalStream: makeConnect(bridge, info.endpointHandle, rawConnect),
    closed: closedPromise,
    close: async () => {
      await closeEndpoint(info.endpointHandle);
      resolveClosed();
    },
  };
}
