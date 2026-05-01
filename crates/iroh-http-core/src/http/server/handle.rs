//! `ServeHandle` — the join handle / shutdown switch returned by
//! [`crate::http::server::serve_with_events`].
//!
//! Split out of `mod.rs` per Slice C.7 of #182.

use std::sync::Arc;

pub struct ServeHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
    pub(super) shutdown_notify: Arc<tokio::sync::Notify>,
    pub(super) drain_timeout: std::time::Duration,
    /// Resolves to `true` once the serve task has fully exited.
    pub(super) done_rx: tokio::sync::watch::Receiver<bool>,
}

impl ServeHandle {
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }
    pub async fn drain(self) {
        self.shutdown();
        let _ = self.join.await;
    }
    pub fn abort(&self) {
        self.join.abort();
    }
    pub fn drain_timeout(&self) -> std::time::Duration {
        self.drain_timeout
    }
    /// Subscribe to the serve-loop-done signal.
    ///
    /// The returned receiver resolves (changes to `true`) once the serve task
    /// has fully exited, including the drain phase.
    pub fn subscribe_done(&self) -> tokio::sync::watch::Receiver<bool> {
        self.done_rx.clone()
    }
}
