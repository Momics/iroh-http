//! Transport subsystem — raw QUIC endpoint and stable identity.

use iroh::Endpoint;

/// Raw QUIC transport state.
pub(in crate::endpoint) struct Transport {
    /// The bound iroh endpoint. Cloning is cheap (internally `Arc`).
    pub(in crate::endpoint) ep: Endpoint,
    /// The node's own base32-encoded public key. Stable for the lifetime
    /// of the secret key.
    pub(in crate::endpoint) node_id_str: String,
}
