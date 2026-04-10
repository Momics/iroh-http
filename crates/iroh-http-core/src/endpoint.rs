//! Iroh endpoint lifecycle — create, share, and close.

use std::sync::Arc;
use std::time::Duration;
use iroh::{Endpoint, RelayMode, SecretKey};
use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher};
use iroh::endpoint::{IdleTimeout, QuicTransportConfig};

use crate::ALPN;

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
}

impl IrohEndpoint {
    /// Bind an Iroh endpoint with the supplied options.
    pub async fn bind(opts: NodeOptions) -> Result<Self, String> {
        let relay_mode = if opts.relays.is_empty() {
            RelayMode::Default
        } else {
            let urls = opts
                .relays
                .iter()
                .map(|u| u.parse::<iroh::RelayUrl>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            RelayMode::custom(urls)
        };

        let mut builder = Endpoint::empty_builder(relay_mode)
            .address_lookup(PkarrPublisher::n0_dns())
            .address_lookup(DnsAddressLookup::n0_dns())
            .alpns(vec![ALPN.to_vec()]);

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

        let node_id_str = crate::base32_encode(ep.id().as_bytes());

        Ok(Self {
            inner: Arc::new(EndpointInner { ep, node_id_str }),
        })
    }

    /// The node's public key as a lowercase base32 string.
    pub fn node_id(&self) -> &str {
        &self.inner.node_id_str
    }

    /// The node's raw secret key bytes (32 bytes).
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.inner.ep.secret_key().to_bytes()
    }

    /// Close the endpoint and all active connections.
    pub async fn close(&self) {
        self.inner.ep.close().await;
    }

    pub(crate) fn raw(&self) -> &Endpoint {
        &self.inner.ep
    }
}
