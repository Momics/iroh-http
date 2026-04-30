//! [`FfiBridge`] subsystem — opaque handles owned by the FFI surface.
//!
//! Per ADR-014 D1 this is one of the four named subsystems composed into
//! [`super::EndpointInner`]. JS holds opaque `u64` handles into the
//! [`HandleStore`]; this subsystem is the entire universe of state JS can
//! reach by handle.

use crate::stream::HandleStore;

/// FFI handle store and any future JS-facing token registries.
///
/// Kept as a thin wrapper today (one field) so future additions —
/// fetch-cancel slabs, request-head slabs, etc. — have a named home rather
/// than landing back in [`super::EndpointInner`].
pub(crate) struct FfiBridge {
    /// Per-endpoint handle store — owns all body readers, writers,
    /// sessions, request-head channels, and fetch-cancel tokens.
    pub handles: HandleStore,
}

#[cfg(test)]
impl FfiBridge {
    /// Construct a minimal `FfiBridge` for unit tests with default store config.
    pub fn new_for_test() -> Self {
        Self {
            handles: HandleStore::new(crate::stream::StoreConfig::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_for_test_has_empty_handle_store() {
        let bridge = FfiBridge::new_for_test();
        let (readers, writers, sessions, heads) = bridge.handles.count_handles();
        assert_eq!((readers, writers, sessions, heads), (0, 0, 0, 0));
    }
}
