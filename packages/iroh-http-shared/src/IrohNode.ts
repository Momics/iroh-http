import { PublicKey, resolveNodeId, SecretKey } from "./keys.js";
import { type FetchFn, makeFetch } from "./fetch.js";
import {
  makeServe,
  type ServeFn,
  type ServeHandle,
  type ServeHandler,
  type ServeOptions,
} from "./serve.js";
import {
  buildSession,
  type IrohSession,
  type WebTransportCloseInfo,
} from "./session.js";
import {
  type CloseOptions,
  type EndpointInfo,
  type IrohAdapter,
  type IrohFetchInit,
  type NodeAddrInfo,
} from "./IrohAdapter.js";
import type {
  EndpointStats,
  PathInfo,
  PeerStats,
  TransportEventPayload,
} from "./observability.js";
import type {
  AdvertiseOptions,
  BrowseOptions,
  DiscoveredPeer,
  PeerDiscoveryEvent,
} from "./discovery.js";
import type { NodeOptions } from "./options/NodeOptions.js";

const _INTERNAL = Symbol("IrohNode._create");

export class IrohNode extends EventTarget {
  readonly publicKey: PublicKey;
  readonly secretKey: SecretKey;
  readonly closed: Promise<WebTransportCloseInfo>;

  #adapter: IrohAdapter;
  #endpointHandle: number;
  #nodeId: string;
  #nativeClosed: Promise<void>;
  #resolveClose!: (info: WebTransportCloseInfo) => void;
  #fetchFn: FetchFn;
  #serveFn: ServeFn;

  private constructor(
    guard: symbol,
    adapter: IrohAdapter,
    info: EndpointInfo,
    options: NodeOptions | undefined,
    nativeClosed: Promise<void>,
  ) {
    if (guard !== _INTERNAL) {
      throw new TypeError("IrohNode must be created via IrohNode._create()");
    }
    super();
    this.#adapter = adapter;
    this.#endpointHandle = info.endpointHandle;
    this.#nodeId = info.nodeId;
    this.#nativeClosed = nativeClosed;

    let resolveClose!: (info: WebTransportCloseInfo) => void;
    this.closed = new Promise<WebTransportCloseInfo>((r) => {
      resolveClose = r;
    });
    this.#resolveClose = resolveClose;

    nativeClosed.then(() =>
      resolveClose({ closeCode: 0, reason: "native shutdown" })
    );

    this.publicKey = PublicKey.fromString(info.nodeId);
    this.secretKey = SecretKey._fromBytesWithPublicKey(
      info.keypair,
      this.publicKey,
    );

    this.#fetchFn = makeFetch(adapter, info.endpointHandle);
    this.#serveFn = makeServe(
      adapter,
      info.endpointHandle,
      info.nodeId,
      this.closed.then(() => {}),
    );

    // Only start the transport event loop when explicitly opted in.
    // The loop calls pollTransportEvent() which blocks in Rust until an event
    // arrives — running it unconditionally would waste a background task slot
    // and drain events nobody is listening for.
    if (options?.observability?.transportEvents === true) {
      this.#startTransportEvents();
    }
  }

  static _create(
    adapter: IrohAdapter,
    info: EndpointInfo,
    options: NodeOptions | undefined,
    nativeClosed: Promise<void>,
  ): IrohNode {
    return new IrohNode(_INTERNAL, adapter, info, options, nativeClosed);
  }

  fetch(input: string | URL, init?: IrohFetchInit): Promise<Response>;
  fetch(
    peer: PublicKey | string,
    input: string | URL,
    init?: IrohFetchInit,
  ): Promise<Response>;
  fetch(...args: unknown[]): Promise<Response> {
    return (this.#fetchFn as (...a: unknown[]) => Promise<Response>)(...args);
  }

  serve(handler: ServeHandler): ServeHandle;
  serve(options: ServeOptions, handler: ServeHandler): ServeHandle;
  serve(options: ServeOptions & { handler: ServeHandler }): ServeHandle;
  serve(...args: unknown[]): ServeHandle {
    return (this.#serveFn as (...a: unknown[]) => ServeHandle)(...args);
  }

  async connect(
    peer: PublicKey | string,
    init?: { directAddrs?: string[] },
  ): Promise<IrohSession> {
    const sessionFns = this.#adapter.sessionFns;
    if (!sessionFns) {
      throw new Error("connect() not supported by this platform adapter");
    }
    const nodeId = resolveNodeId(peer);
    const directAddrs = init?.directAddrs ?? null;
    const sessionHandle = await sessionFns.connect(
      this.#endpointHandle,
      nodeId,
      directAddrs,
    );
    const remotePk = PublicKey.fromString(nodeId);
    return buildSession(this.#adapter, sessionHandle, remotePk, sessionFns);
  }

  browse(options?: BrowseOptions): AsyncIterable<DiscoveredPeer> {
    const adapter = this.#adapter;
    const handle = this.#endpointHandle;
    const svcName = options?.serviceName ?? "iroh-http";
    const signal = options?.signal;

    return {
      [Symbol.asyncIterator]() {
        let browseHandle: number | null = null;
        return {
          async next(): Promise<IteratorResult<DiscoveredPeer>> {
            if (browseHandle === null) {
              browseHandle = await adapter.mdnsBrowse(handle, svcName);
            }
            if (signal?.aborted) {
              adapter.mdnsBrowseClose(browseHandle);
              browseHandle = null;
              return { done: true as const, value: undefined };
            }

            let event: PeerDiscoveryEvent | null;
            if (signal) {
              const abortPromise = new Promise<null>((resolve) => {
                if (signal.aborted) {
                  resolve(null);
                  return;
                }
                signal.addEventListener("abort", () => resolve(null), {
                  once: true,
                });
              });
              event = await Promise.race([
                adapter.mdnsNextEvent(browseHandle),
                abortPromise,
              ]);
              if (signal.aborted && browseHandle !== null) {
                adapter.mdnsBrowseClose(browseHandle);
                browseHandle = null;
                return { done: true as const, value: undefined };
              }
            } else {
              event = await adapter.mdnsNextEvent(browseHandle);
            }

            if (event === null) {
              return { done: true as const, value: undefined };
            }
            const discovered: DiscoveredPeer = {
              nodeId: event.nodeId,
              addrs: event.addrs ?? [],
              isActive: event.type === "discovered",
            };
            return { done: false as const, value: discovered };
          },
          return(): Promise<IteratorResult<DiscoveredPeer>> {
            if (browseHandle !== null) {
              adapter.mdnsBrowseClose(browseHandle);
              browseHandle = null;
            }
            return Promise.resolve({ done: true as const, value: undefined });
          },
        };
      },
    };
  }

  async advertise(options?: AdvertiseOptions): Promise<void> {
    const svcName = options?.serviceName ?? "iroh-http";
    const signal = options?.signal;
    const advHandle = await this.#adapter.mdnsAdvertise(
      this.#endpointHandle,
      svcName,
    );
    if (signal) {
      return new Promise<void>((resolve) => {
        signal.addEventListener("abort", () => {
          this.#adapter.mdnsAdvertiseClose(advHandle);
          resolve();
        }, { once: true });
        if (signal.aborted) {
          this.#adapter.mdnsAdvertiseClose(advHandle);
          resolve();
        }
      });
    }
  }

  async addr(): Promise<NodeAddrInfo> {
    return this.#adapter.nodeAddr(this.#endpointHandle);
  }

  async ticket(): Promise<string> {
    return this.#adapter.nodeTicket(this.#endpointHandle);
  }

  async homeRelay(): Promise<string | null> {
    return this.#adapter.homeRelay(this.#endpointHandle);
  }

  async peerInfo(peer: PublicKey | string): Promise<NodeAddrInfo | null> {
    return this.#adapter.peerInfo(this.#endpointHandle, resolveNodeId(peer));
  }

  async peerStats(peer: PublicKey | string): Promise<PeerStats | null> {
    return this.#adapter.peerStats(this.#endpointHandle, resolveNodeId(peer));
  }

  async stats(): Promise<EndpointStats> {
    return this.#adapter.stats(this.#endpointHandle);
  }

  pathChanges(
    peer: PublicKey | string,
    options?: { signal?: AbortSignal },
  ): AsyncIterable<PathInfo> {
    const nodeId = resolveNodeId(peer);
    const adapter = this.#adapter;
    const endpointHandle = this.#endpointHandle;
    const signal = options?.signal;

    return {
      [Symbol.asyncIterator]() {
        return {
          async next(): Promise<IteratorResult<PathInfo>> {
            if (signal?.aborted) {
              return { done: true, value: undefined };
            }
            const path = await adapter.nextPathChange(endpointHandle, nodeId);
            if (path === null) {
              return { done: true, value: undefined };
            }
            return { done: false, value: path };
          },
          return(): Promise<IteratorResult<PathInfo>> {
            // break / return from for-await — nothing to clean up on JS side;
            // Rust watcher exits when the channel sender is dropped.
            return Promise.resolve({ done: true, value: undefined });
          },
        };
      },
    };
  }

  #startTransportEvents(): void {
    this.#adapter.startTransportEvents(
      this.#endpointHandle,
      (event) =>
        this.dispatchEvent(
          new CustomEvent<TransportEventPayload>("transport", {
            detail: event,
          }),
        ),
    );
  }

  async close(options?: CloseOptions): Promise<void> {
    await this.#adapter.closeEndpoint(this.#endpointHandle, options?.force);
    this.#resolveClose({ closeCode: 0, reason: "" });
    await this.#nativeClosed;
  }

  [Symbol.asyncDispose](): Promise<void> {
    return this.close();
  }
}
