//! `iroh-http-discovery` — local mDNS peer discovery for iroh-http.
//!
//! Implements Iroh's address-lookup trait using mDNS so nodes on the same
//! local network can find each other without a relay server.
//!
//! # Usage (desktop)
//!
//! After binding an `IrohEndpoint`, attach mDNS discovery to the live endpoint:
//!
//! ```rust,no_run
//! # use iroh::Endpoint;
//! # use std::sync::Arc;
//! # async fn example(ep: Endpoint) -> Result<(), Box<dyn std::error::Error>> {
//! iroh_http_discovery::add_mdns(&ep, "my-app.iroh-http", true)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Platform notes
//!
//! - Desktop (macOS, Linux, Windows): enabled with the `mdns` feature (default).
//! - iOS / Android (Tauri mobile): use the platform's native service discovery;
//!   this crate is not used on those targets.

#[cfg(feature = "mdns")]
use std::sync::Arc;
#[cfg(feature = "mdns")]
use iroh::address_lookup::MdnsAddressLookup;

/// Attach mDNS discovery to a live `Endpoint`.
///
/// `service_name` — unique per application, e.g. `"my-app.iroh-http"`.
/// `advertise`    — whether this node should advertise itself via mDNS.
///
/// Returns the `Arc<MdnsAddressLookup>` so the caller can keep it alive and
/// create subscriptions if needed.
#[cfg(feature = "mdns")]
pub fn add_mdns(
    ep: &iroh::Endpoint,
    service_name: &str,
    advertise: bool,
) -> Result<Arc<MdnsAddressLookup>, String> {
    let svc = Arc::new(
        MdnsAddressLookup::builder()
            .advertise(advertise)
            .service_name(service_name)
            .build(ep.id())
            .map_err(|e| e.to_string())?,
    );
    ep.address_lookup().add(Arc::clone(&svc));
    Ok(svc)
}

/// Stub for non-mdns builds.
#[cfg(not(feature = "mdns"))]
pub fn add_mdns(
    _ep: &iroh::Endpoint,
    _service_name: &str,
    _advertise: bool,
) -> Result<(), String> {
    Err("iroh-http-discovery compiled without the 'mdns' feature".into())
}
