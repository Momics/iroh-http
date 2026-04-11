//! QUIC connection pool — reuse connections across fetch/connect calls.
//!
//! The pool maps `(NodeId, ALPN) → Connection`.  Before every `connect()` call
//! the pool is checked; a live cached connection avoids a full QUIC handshake.
//!
//! When many callers request the same peer concurrently and no pooled connection
//! exists, only one caller performs the handshake while the others wait
//! (connection-storm prevention via per-slot `OnceCell`).
//!
//! A [`QpackCodec`] is stored alongside each connection for QPACK header
//! compression.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use iroh::endpoint::Connection;
use iroh::PublicKey;

use crate::qpack_bridge::QpackCodec;

/// A pooled connection with QPACK codec state.
#[derive(Clone)]
pub(crate) struct PooledConnection {
    pub conn: Connection,
    /// Per-connection QPACK encoder/decoder state.
    pub codec: Arc<tokio::sync::Mutex<QpackCodec>>,
}

impl PooledConnection {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            codec: Arc::new(tokio::sync::Mutex::new(QpackCodec::new())),
        }
    }
}

/// Key for a pooled connection: `(NodeId, ALPN bytes)`.
#[derive(Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    node_id: PublicKey,
    alpn: Vec<u8>,
}

/// A slot in the pool.  While a connection is being established the slot
/// holds a `Connecting` future that waiters can subscribe to.
enum Slot {
    /// A live, cached connection with the last-used timestamp for LRU eviction.
    Ready(PooledConnection, std::time::Instant),
    /// A connection attempt is in progress.  Waiters subscribe to the channel.
    Connecting(tokio::sync::watch::Receiver<Option<Result<PooledConnection, String>>>),
}

/// Thread-safe QUIC connection pool.
pub(crate) struct ConnectionPool {
    inner: Mutex<PoolInner>,
}

struct PoolInner {
    conns: HashMap<PoolKey, Slot>,
    max_idle: Option<usize>,
}

impl ConnectionPool {
    /// Create a new pool.  `max_idle` limits cached connections (`None` = unlimited).
    pub fn new(max_idle: Option<usize>) -> Self {
        Self {
            inner: Mutex::new(PoolInner {
                conns: HashMap::new(),
                max_idle,
            }),
        }
    }

    /// Get an existing live connection, or establish a new one.
    ///
    /// `connect_fn` is called at most once per concurrent batch of requests
    /// to the same `(node_id, alpn)` pair.
    pub async fn get_or_connect<F, Fut>(
        &self,
        node_id: PublicKey,
        alpn: &[u8],
        connect_fn: F,
    ) -> Result<PooledConnection, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Connection, String>>,
    {
        let key = PoolKey {
            node_id,
            alpn: alpn.to_vec(),
        };

        // Phase 1: check the pool (short lock, no await).
        enum Action {
            Hit(PooledConnection),
            Wait(tokio::sync::watch::Receiver<Option<Result<PooledConnection, String>>>),
            Connect(tokio::sync::watch::Sender<Option<Result<PooledConnection, String>>>),
        }

        let action = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(slot) = inner.conns.get_mut(&key) {
                match slot {
                    Slot::Ready(pooled, last_used) => {
                        if pooled.conn.close_reason().is_none() {
                            *last_used = std::time::Instant::now();
                            Action::Hit(pooled.clone())
                        } else {
                            inner.conns.remove(&key);
                            let (tx, rx) = tokio::sync::watch::channel(None);
                            inner.conns.insert(key.clone(), Slot::Connecting(rx));
                            Action::Connect(tx)
                        }
                    }
                    Slot::Connecting(rx) => Action::Wait(rx.clone()),
                }
            } else {
                let (tx, rx) = tokio::sync::watch::channel(None);
                inner.conns.insert(key.clone(), Slot::Connecting(rx));
                Action::Connect(tx)
            }
            // MutexGuard dropped here
        };

        match action {
            Action::Hit(pooled) => Ok(pooled),
            Action::Wait(mut rx) => wait_for_connection(&mut rx).await,
            Action::Connect(tx) => {
                // Phase 2: perform the actual QUIC handshake (no lock held).
                let result = connect_fn().await;

                let pooled_result = result.map(|conn| PooledConnection::new(conn));

                // Phase 3: store the result (short lock, no await).
                {
                    let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    match &pooled_result {
                        Ok(pooled) => {
                            if let Some(max) = inner.max_idle {
                                evict_if_needed(&mut inner.conns, max);
                            }
                            inner.conns.insert(key, Slot::Ready(pooled.clone(), std::time::Instant::now()));
                        }
                        Err(_) => {
                            inner.conns.remove(&key);
                        }
                    }
                }

                // Wake all waiters.
                let _ = tx.send(Some(pooled_result.clone()));
                pooled_result
            }
        }
    }

    /// Return the number of entries currently in the pool (for testing).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).conns.iter().filter(|(_, s)| matches!(s, Slot::Ready(_, _))).count()
    }

    /// Remove a specific connection from the pool (e.g. after a fatal error).
    #[allow(dead_code)]
    pub fn remove(&self, node_id: &PublicKey, alpn: &[u8]) {
        let key = PoolKey {
            node_id: *node_id,
            alpn: alpn.to_vec(),
        };
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).conns.remove(&key);
    }
}

/// Wait for an in-flight connection attempt to complete.
async fn wait_for_connection(
    rx: &mut tokio::sync::watch::Receiver<Option<Result<PooledConnection, String>>>,
) -> Result<PooledConnection, String> {
    loop {
        rx.changed().await.map_err(|_| "connection attempt dropped".to_string())?;
        let val = rx.borrow().clone();
        if let Some(result) = val {
            return result;
        }
    }
}

/// If the pool has more than `max` Ready entries, remove the oldest ones.
fn evict_if_needed(conns: &mut HashMap<PoolKey, Slot>, max: usize) {
    // Count ready connections.
    let ready_count = conns.values().filter(|s| matches!(s, Slot::Ready(_, _))).count();
    if ready_count < max {
        return;
    }
    // Remove stale connections first.
    let stale_keys: Vec<PoolKey> = conns
        .iter()
        .filter_map(|(k, s)| match s {
            Slot::Ready(pooled, _) if pooled.conn.close_reason().is_some() => Some(k.clone()),
            _ => None,
        })
        .collect();
    for k in stale_keys {
        conns.remove(&k);
    }
    // If still over limit, evict the least-recently-used ready entry.
    while conns.values().filter(|s| matches!(s, Slot::Ready(_, _))).count() >= max {
        if let Some(key) = conns
            .iter()
            .filter_map(|(k, s)| match s {
                Slot::Ready(_, last_used) => Some((k.clone(), *last_used)),
                _ => None,
            })
            .min_by_key(|(_, t)| *t)
            .map(|(k, _)| k)
        {
            conns.remove(&key);
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_starts_empty() {
        let pool = ConnectionPool::new(None);
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn evict_respects_max() {
        let mut conns = HashMap::new();
        evict_if_needed(&mut conns, 5);
        assert!(conns.is_empty());
    }
}
