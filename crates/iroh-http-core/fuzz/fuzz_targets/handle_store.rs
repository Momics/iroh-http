#![no_main]

use libfuzzer_sys::fuzz_target;
use iroh_http_core::{HandleStore, StoreConfig};

// Fuzz the HandleStore with arbitrary u64 handle values.
// Exercises invalid-handle paths: take_req_sender, cancel_reader,
// finish_body, cancel_in_flight, lookup_session, claim_pending_reader.
// None of these should ever panic.
fuzz_target!(|handle: u64| {
    let store = HandleStore::new(StoreConfig {
        max_handles: 16,
        ..Default::default()
    });
    // All of these must be safe on any u64.
    let _ = store.take_req_sender(handle);
    store.cancel_reader(handle);
    let _ = store.finish_body(handle);
    store.cancel_in_flight(handle);
    store.remove_fetch_token(handle);
    let _ = store.lookup_session(handle);
    let _ = store.remove_session(handle);
    let _ = store.claim_pending_reader(handle);
});
