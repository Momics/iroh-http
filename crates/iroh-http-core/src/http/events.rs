//! Transport-level events emitted by the endpoint.

use serde::Serialize;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A transport-level event emitted by the endpoint.
///
/// Serialised with serde for the FFI boundary.  Adapters forward these as
/// `CustomEvent('transport', { detail })` on the JS `IrohNode` instance.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TransportEvent {
    #[serde(rename = "pool:hit")]
    PoolHit {
        #[serde(rename = "peerId")]
        peer_id: String,
        timestamp: u64,
    },
    #[serde(rename = "pool:miss")]
    PoolMiss {
        #[serde(rename = "peerId")]
        peer_id: String,
        timestamp: u64,
    },
    #[serde(rename = "pool:evict")]
    PoolEvict {
        #[serde(rename = "peerId")]
        peer_id: String,
        timestamp: u64,
    },
    #[serde(rename = "path:change")]
    PathChange {
        #[serde(rename = "peerId")]
        peer_id: String,
        addr: String,
        relay: bool,
        timestamp: u64,
    },
    #[serde(rename = "handle:sweep")]
    HandleSweep { evicted: u64, timestamp: u64 },
}

impl TransportEvent {
    pub fn pool_hit(peer_id: impl Into<String>) -> Self {
        Self::PoolHit {
            peer_id: peer_id.into(),
            timestamp: now_ms(),
        }
    }
    pub fn pool_miss(peer_id: impl Into<String>) -> Self {
        Self::PoolMiss {
            peer_id: peer_id.into(),
            timestamp: now_ms(),
        }
    }
    pub fn pool_evict(peer_id: impl Into<String>) -> Self {
        Self::PoolEvict {
            peer_id: peer_id.into(),
            timestamp: now_ms(),
        }
    }
    pub fn path_change(peer_id: impl Into<String>, addr: impl Into<String>, relay: bool) -> Self {
        Self::PathChange {
            peer_id: peer_id.into(),
            addr: addr.into(),
            relay,
            timestamp: now_ms(),
        }
    }
    pub fn handle_sweep(evicted: u64) -> Self {
        Self::HandleSweep {
            evicted,
            timestamp: now_ms(),
        }
    }
}
