//! Endpoint lifecycle: close, drain, serve-handle wiring, and events.
//!
//! Extracted from `mod.rs` to keep the façade ≤ 200 LoC (ADR-014 D1 AC #4).
//! All methods here operate on [`SessionRuntime`] and [`Transport`] fields
//! that live inside [`EndpointInner`].

use super::IrohEndpoint;

impl IrohEndpoint {
    /// Graceful close: signal the serve loop to stop accepting, wait for
    /// in-flight requests to drain, then close the QUIC endpoint.
    pub async fn close(&self) {
        let handle = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.drain().await;
        }
        self.inner.transport.ep.close().await;
        let _ = self.inner.session.closed_tx.send(true);
    }

    /// Immediate close: abort the serve loop with no drain period.
    pub async fn close_force(&self) {
        let handle = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.abort();
        }
        self.inner.transport.ep.close().await;
        let _ = self.inner.session.closed_tx.send(true);
    }

    /// Wait until this endpoint has been closed. Returns immediately if already closed.
    pub async fn wait_closed(&self) {
        let mut rx = self.inner.session.closed_rx.clone();
        let _ = rx.wait_for(|v| *v).await;
    }

    /// Store a serve handle so that `close()` can drain it.
    pub fn set_serve_handle(&self, handle: crate::http::server::ServeHandle) {
        *self
            .inner
            .session
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle.subscribe_done());
        *self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }

    /// Signal the serve loop to stop accepting new connections.
    pub fn stop_serve(&self) {
        if let Some(h) = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.shutdown();
        }
    }

    /// Wait until the serve loop has fully exited.
    pub async fn wait_serve_stop(&self) {
        let rx = self
            .inner
            .session
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(mut rx) = rx {
            let _ = rx.wait_for(|v| *v).await;
        }
    }

    /// Take the transport event receiver, handing it off to a platform drain task.
    /// May only be called once per endpoint.
    pub fn subscribe_events(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::http::events::TransportEvent>> {
        self.inner
            .session
            .event_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}
