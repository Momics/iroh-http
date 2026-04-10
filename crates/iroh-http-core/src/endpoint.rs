//! Iroh endpoint lifecycle — create, share, and close.

use std::sync::Arc;
use std::time::Duration;
use iroh::{Endpoint, RelayMode, SecretKey};
use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher};
use iroh::endpoint::{IdleTimeout, QuicTransportConfig};

use crate::{ALPN, ALPN_DUPLEX, ALPN_TRAILERS, ALPN_FULL};

/// Configuration for mDNS local network discovery.
#[derive(Debug, Default, Clone)]
pub struct DiscoveryConfig {
    /// Enable mDNS local discovery.  Default: false.
    pub mdns: bool,
    /// Application-specific mDNS service name.  Required when `mdns` is true.
    pub service_name: Option<String>,
    /// Whether to advertise this node.  Default: true.
    pub advertise: bool,
}

/// Configuration passed to [`IrohEndpoint::bind`].
#[derive(Debug, Default, Clone)]
pub struct NodeOptions {
    /// 32-byte Ed25519 secret key.  Generate a fresh one when `None`.
    pub key: Option<[u8; 32]>,
    /// Milliseconds before an idle QUIC connection is cleaned up.
    pub idle_timeout_ms: Option<u64>,
    /// Custom relay server URLs.  Uses Iroh's default public relays when empty.
    pub relays: Vec<String>,
    /// DNS discovery server URL override.  Uses n0 DNS defaults when `None`.
    pub dns_discovery: Option<String>,
    /// Capabilities to advertise via ALPN.  When empty, all supported capabilities
    /// are advertised in preference order: `iroh-http/1-full`, `-duplex`,
    /// `-trailers`, `iroh-http/1`.
    pub capabilities: Vec<String>,
    /// Capacity (in chunks) of each body channel.  Default: 32.
    /// Increase for large fast producers; decrease to tighten backpressure.
    pub channel_capacity: Option<usize>,
    /// Maximum byte length of a single chunk in `send_chunk`.
    /// Chunks larger than this are silently split internally.
    /// Default: 65536 (64 KB).
    pub max_chunk_size_bytes: Option<usize>,
    /// Number of consecutive accept errors before the serve loop gives up.  Default: 5.
    pub max_consecutive_errors: Option<usize>,
    /// Local peer discovery configuration.
    pub discovery: Option<DiscoveryConfig>,
    /// Disable relay servers and DNS discovery entirely.
    /// Useful for in-process tests where endpoints connect via direct addresses.
    pub disable_networking: bool,
    /// Milliseconds to wait for a slow body reader before dropping the connection.
    /// Default: 30 000 (30 s).
    pub drain_timeout_ms: Option<u64>,
    /// TTL in milliseconds for slab handle entries.  Expired entries are swept
    /// every 60 s.  `0` disables sweeping.  Default: 300 000 (5 min).
    pub handle_ttl_ms: Option<u64>,
}

/// A shared Iroh endpoint.
///
/// Clone-able (cheap Arc clone).  All fetch and serve calls on the same node
/// share one endpoint and therefore one stable QUIC identity.
#[derive(Clone)]
pub struct IrohEndpoint {
    pub(crate) inner: Arc<EndpointInner>,
}

pub(crate) struct EndpointInner {
    pub ep: Endpoint,
    /// The node's own base32-encoded public key (stable for the lifetime of the key).
    pub node_id_str: String,
    /// Configured consecutive error limit for the serve accept loop.
    pub max_consecutive_errors: usize,
}

impl IrohEndpoint {
    /// Bind an Iroh endpoint with the supplied options.
    pub async fn bind(opts: NodeOptions) -> Result<Self, String> {
        let relay_mode = if opts.disable_networking {
            RelayMode::Disabled
        } else if opts.relays.is_empty() {
            RelayMode::Default
        } else {
            let urls = opts
                .relays
                .iter()
                .map(|u| u.parse::<iroh::RelayUrl>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            RelayMode::custom(urls)
        };

        let alpns: Vec<Vec<u8>> = if opts.capabilities.is_empty() {
            // Advertise all capabilities in preference order.
            vec![
                ALPN_FULL.to_vec(),
                ALPN_DUPLEX.to_vec(),
                ALPN_TRAILERS.to_vec(),
                ALPN.to_vec(),
            ]
        } else {
            let mut list: Vec<Vec<u8>> = opts
                .capabilities
                .iter()
                .map(|c| c.as_bytes().to_vec())
                .collect();
            // Always include the base protocol so the node can talk to base-only peers.
            if !list.iter().any(|a| a == ALPN) {
                list.push(ALPN.to_vec());
            }
            list
        };

        let mut builder = Endpoint::empty_builder(relay_mode)
            .alpns(alpns);

        if !opts.disable_networking {
            builder = builder
                .address_lookup(PkarrPublisher::n0_dns())
                .address_lookup(DnsAddressLookup::n0_dns());
        }

        if let Some(key_bytes) = opts.key {
            builder = builder.secret_key(SecretKey::from_bytes(&key_bytes));
        }

        if let Some(ms) = opts.idle_timeout_ms {
            let timeout = IdleTimeout::try_from(Duration::from_millis(ms))
                .map_err(|e| format!("idle_timeout_ms out of range: {e}"))?;
            let transport = QuicTransportConfig::builder()
                .max_idle_timeout(Some(timeout))
                .build();
            builder = builder.transport_config(transport);
        }

        let ep = builder.bind().await.map_err(|e| e.to_string())?;

        // Apply backpressure config for all future channel allocations.
        crate::stream::configure_backpressure(
            opts.channel_capacity.unwrap_or(crate::stream::DEFAULT_CHANNEL_CAPACITY),
            opts.max_chunk_size_bytes.unwrap_or(crate::stream::DEFAULT_MAX_CHUNK_SIZE),
            opts.drain_timeout_ms.unwrap_or(crate::stream::DEFAULT_DRAIN_TIMEOUT_MS),
        );

        // Start slab TTL sweep if configured.
        crate::stream::start_slab_sweep(opts.handle_ttl_ms.unwrap_or(crate::stream::DEFAULT_SLAB_TTL_MS));

        let node_id_str = crate::base32_encode(ep.id().as_bytes());

        Ok(Self {
            inner: Arc::new(EndpointInner {
                ep,
                node_id_str,
                max_consecutive_errors: opts.max_consecutive_errors.unwrap_or(5),
            }),
        })
    }

    /// The node's public key as a lowercase base32 string.
    pub fn node_id(&self) -> &str {
        &self.inner.node_id_str
    }

    /// The configured consecutive-error limit for the serve loop.
    pub fn max_consecutive_errors(&self) -> usize {
        self.inner.max_consecutive_errors
    }

    /// The node's raw secret key bytes (32 bytes).
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.inner.ep.secret_key().to_bytes()
    }

    /// Close the endpoint and all active connections.
    pub async fn close(&self) {
        self.inner.ep.close().await;
    }

    pub fn raw(&self) -> &Endpoint {
        &self.inner.ep
    }

    /// Returns the local socket addresses this endpoint is bound to.
    pub fn bound_sockets(&self) -> Vec<std::net::SocketAddr> {
        self.inner.ep.bound_sockets()
    }
}
