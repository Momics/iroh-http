export interface DiscoveredPeer {
  nodeId: string;
  addrs: string[];
  isActive: boolean;
}

export interface BrowseOptions {
  serviceName?: string;
  signal?: AbortSignal;
}

export interface AdvertiseOptions {
  serviceName?: string;
  signal?: AbortSignal;
}

export interface PeerDiscoveryEvent {
  type: "discovered" | "expired";
  nodeId: string;
  addrs?: string[];
}
