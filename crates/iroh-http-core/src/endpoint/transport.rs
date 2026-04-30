//! [`Transport`] subsystem — raw iroh QUIC endpoint and stable identity.
//!
//! Per ADR-014 D1 this is one of the four named subsystems composed into
//! [`super::EndpointInner`]. It owns the immutable transport identity and
//! the underlying [`iroh::Endpoint`].

use iroh::Endpoint;

/// Raw QUIC transport state.
pub(crate) struct Transport {
    /// The bound iroh endpoint. Cloning is cheap (internally `Arc`).
    pub ep: Endpoint,
    /// The node's own base32-encoded public key. Stable for the lifetime
    /// of the secret key.
    pub node_id_str: String,
}
