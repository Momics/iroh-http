/** A peer found (or lost) during mDNS/DNS-SD browsing. */
export interface DiscoveredPeer {
  /** Base32 public key of the discovered peer. */
  nodeId: string;
  /** Network addresses (e.g. QUIC socket addrs) reported for this peer. */
  addrs: string[];
  /** `true` while the peer is reachable; `false` after it expires. */
  isActive: boolean;
}

/** Options for `node.browse()` — discover peers on the local network. */
export interface BrowseOptions {
  /** mDNS service name to browse. Defaults to the iroh-http service type. */
  serviceName?: string;
  /** Abort signal to stop browsing. */
  signal?: AbortSignal;
}

/** Options for `node.advertise()` — announce this node on the local network. */
export interface AdvertiseOptions {
  /** mDNS service name to advertise under. Defaults to the iroh-http service type. */
  serviceName?: string;
  /** Abort signal to stop advertising. */
  signal?: AbortSignal;
}

/** Event emitted by `node.browse()` when a peer is discovered or expires. */
export interface PeerDiscoveryEvent {
  /** Whether the peer was just discovered or has expired. */
  type: "discovered" | "expired";
  /** Base32 public key of the peer. */
  nodeId: string;
  /** Network addresses, present on `"discovered"` events. */
  addrs?: string[];
}
