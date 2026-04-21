import { PublicKey, SecretKey } from './keys.js';
import { type ServeHandle, type ServeHandler, type ServeOptions } from './serve.js';
import { type IrohSession, type WebTransportCloseInfo } from './session.js';
import { type IrohAdapter, type EndpointInfo, type IrohFetchInit, type CloseOptions, type NodeAddrInfo } from './IrohAdapter.js';
import type { PeerStats, EndpointStats, PathInfo } from './observability.js';
import type { DiscoveredPeer, BrowseOptions, AdvertiseOptions } from './discovery.js';
import type { NodeOptions } from './options/NodeOptions.js';
export declare class IrohNode extends EventTarget {
    #private;
    readonly publicKey: PublicKey;
    readonly secretKey: SecretKey;
    readonly closed: Promise<WebTransportCloseInfo>;
    private constructor();
    static _create(adapter: IrohAdapter, info: EndpointInfo, options: NodeOptions | undefined, nativeClosed: Promise<void>): IrohNode;
    fetch(input: string | URL, init?: IrohFetchInit): Promise<Response>;
    fetch(peer: PublicKey | string, input: string | URL, init?: IrohFetchInit): Promise<Response>;
    serve(handler: ServeHandler): ServeHandle;
    serve(options: ServeOptions, handler: ServeHandler): ServeHandle;
    serve(options: ServeOptions & {
        handler: ServeHandler;
    }): ServeHandle;
    connect(peer: PublicKey | string, init?: {
        directAddrs?: string[];
    }): Promise<IrohSession>;
    browse(options?: BrowseOptions): AsyncIterable<DiscoveredPeer>;
    advertise(options?: AdvertiseOptions): Promise<void>;
    addr(): Promise<NodeAddrInfo>;
    ticket(): Promise<string>;
    homeRelay(): Promise<string | null>;
    peerInfo(peer: PublicKey | string): Promise<NodeAddrInfo | null>;
    peerStats(peer: PublicKey | string): Promise<PeerStats | null>;
    stats(): Promise<EndpointStats>;
    pathChanges(peer: PublicKey | string, pollIntervalMs?: number): ReadableStream<PathInfo>;
    close(options?: CloseOptions): Promise<void>;
    [Symbol.asyncDispose](): Promise<void>;
}
//# sourceMappingURL=IrohNode.d.ts.map