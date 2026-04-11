//! `iroh-http-discovery` — local mDNS peer discovery for iroh-http.
//!
//! Implements Iroh's address-lookup trait using mDNS so nodes on the same
//! local network can find each other without a relay server.
//!
//! Use [`start_browse`] and [`start_advertise`] to start discovery sessions.
//!
//! # Platform notes
//!
//! - Desktop (macOS, Linux, Windows): enabled with the `mdns` feature (default).
//! - iOS / Android (Tauri mobile): use the platform's native service discovery;

#[cfg(feature = "mdns")]
use iroh::address_lookup::{DiscoveryEvent, MdnsAddressLookup};
#[cfg(feature = "mdns")]
use std::sync::Arc;

// ── Peer discovery event ─────────────────────────────────────────────────────

/// A discovery event suitable for FFI transport.
#[derive(Debug, Clone)]
pub struct PeerDiscoveryEvent {
    /// `true` = peer appeared or updated; `false` = peer expired.
    pub is_active: bool,
    /// Base32 public key of the discovered peer.
    pub node_id: String,
    /// Known addresses: relay URLs and/or `ip:port` strings.
    pub addrs: Vec<String>,
}

// ── Browse session ───────────────────────────────────────────────────────────

/// An active browse session that yields discovery events.
///
/// Drop to stop receiving events.
#[cfg(feature = "mdns")]
pub struct BrowseSession {
    rx: tokio::sync::mpsc::Receiver<DiscoveryEvent>,
    _mdns: Arc<MdnsAddressLookup>,
}

#[cfg(feature = "mdns")]
impl BrowseSession {
    /// Returns the next event, or `None` when the session is closed.
    pub async fn next_event(&mut self) -> Option<PeerDiscoveryEvent> {
        use iroh::TransportAddr;

        let ev = self.rx.recv().await?;
        Some(match ev {
            DiscoveryEvent::Discovered { endpoint_info, .. } => {
                let node_id = endpoint_info.endpoint_id.to_string();
                let mut addrs = Vec::new();
                for a in endpoint_info.data.addrs() {
                    match a {
                        TransportAddr::Ip(sock) => addrs.push(sock.to_string()),
                        TransportAddr::Relay(url) => addrs.push(url.to_string()),
                        other => addrs.push(format!("{:?}", other)),
                    }
                }
                PeerDiscoveryEvent {
                    is_active: true,
                    node_id,
                    addrs,
                }
            }
            DiscoveryEvent::Expired { endpoint_id } => PeerDiscoveryEvent {
                is_active: false,
                node_id: endpoint_id.to_string(),
                addrs: Vec::new(),
            },
        })
    }
}

/// Start a browse session: discover peers on the local network via mDNS.
///
/// Creates an `MdnsAddressLookup` with `advertise(false)`, registers it on the
/// endpoint, and subscribes to discovery events.
#[cfg(feature = "mdns")]
pub async fn start_browse(
    ep: &iroh::Endpoint,
    service_name: &str,
) -> Result<BrowseSession, String> {
    let mdns = Arc::new(
        MdnsAddressLookup::builder()
            .advertise(false)
            .service_name(service_name)
            .build(ep.id())
            .map_err(|e| e.to_string())?,
    );
    ep.address_lookup().add(Arc::clone(&mdns));

    // subscribe() returns impl Stream — we manually drive it into an mpsc channel
    // so BrowseSession has a concrete Receiver type.
    use n0_future::StreamExt;
    let mut stream = mdns.subscribe().await;
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(ev) = stream.next().await {
            if tx.send(ev).await.is_err() {
                break;
            }
        }
    });

    Ok(BrowseSession { rx, _mdns: mdns })
}

// ── Advertise session ────────────────────────────────────────────────────────

/// An active advertise session. Drop to stop advertising.
#[cfg(feature = "mdns")]
pub struct AdvertiseSession {
    _mdns: Arc<MdnsAddressLookup>,
}

/// Start advertising this node on the local network via mDNS.
///
/// The node remains advertised until the returned `AdvertiseSession` is dropped.
#[cfg(feature = "mdns")]
pub fn start_advertise(
    ep: &iroh::Endpoint,
    service_name: &str,
) -> Result<AdvertiseSession, String> {
    let mdns = Arc::new(
        MdnsAddressLookup::builder()
            .advertise(true)
            .service_name(service_name)
            .build(ep.id())
            .map_err(|e| e.to_string())?,
    );
    ep.address_lookup().add(Arc::clone(&mdns));
    Ok(AdvertiseSession { _mdns: mdns })
}
