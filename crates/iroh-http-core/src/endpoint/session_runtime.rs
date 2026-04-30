//! [`SessionRuntime`] subsystem — serve loop, lifecycle signals, transport events.
//!
//! Per ADR-014 D1 this is one of the four named subsystems composed into
//! [`super::EndpointInner`]. It owns the active serve handle, lifecycle
//! signals (closed_tx/rx, serve_done_rx), and the transport event channel
//! plus path-change subscriptions.

use crate::http::server::ServeHandle;
use crate::stats::PathInfo;

/// Server-side runtime: the `serve()` task, lifecycle signals, and
/// observability fan-out (transport events, per-peer path subscriptions).
pub(crate) struct SessionRuntime {
    /// Active serve handle, if `serve()` has been called.
    pub serve_handle: std::sync::Mutex<Option<ServeHandle>>,
    /// Done-signal receiver from the active serve task. Stored separately
    /// so `wait_serve_stop()` can await without holding the `serve_handle` lock.
    pub serve_done_rx: std::sync::Mutex<Option<tokio::sync::watch::Receiver<bool>>>,
    /// Signals `true` when the endpoint has fully closed (either explicitly
    /// or because the serve loop exited due to native shutdown).
    pub closed_tx: tokio::sync::watch::Sender<bool>,
    pub closed_rx: tokio::sync::watch::Receiver<bool>,
    /// Sender for transport-level events (pool hits/misses, path changes, sweep).
    pub event_tx: tokio::sync::mpsc::Sender<crate::events::TransportEvent>,
    /// Receiver for transport-level events. Wrapped in Mutex+Option so
    /// `subscribe_events()` can take it exactly once for the platform drain task.
    pub event_rx:
        std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<crate::events::TransportEvent>>>,
    /// Per-peer path-change subscriptions. Key: `node_id_str`. Populated
    /// lazily when `subscribe_path_changes` is called.
    pub path_subs: dashmap::DashMap<String, tokio::sync::mpsc::UnboundedSender<PathInfo>>,
}

#[cfg(test)]
impl SessionRuntime {
    /// Construct a minimal `SessionRuntime` for unit tests. No serve task
    /// is attached; `serve_handle` is empty.
    pub fn new_for_test() -> Self {
        let (closed_tx, closed_rx) = tokio::sync::watch::channel(false);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        Self {
            serve_handle: std::sync::Mutex::new(None),
            serve_done_rx: std::sync::Mutex::new(None),
            closed_tx,
            closed_rx,
            event_tx,
            event_rx: std::sync::Mutex::new(Some(event_rx)),
            path_subs: dashmap::DashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_for_test_starts_unclosed_with_no_serve_handle() {
        let rt = SessionRuntime::new_for_test();
        assert!(!*rt.closed_rx.borrow());
        assert!(rt
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
        assert_eq!(rt.path_subs.len(), 0);
    }
}
