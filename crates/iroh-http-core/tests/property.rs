//! Property-based tests for iroh-http-core.
//!
//! Each section targets a different module boundary. When adding new public
//! APIs, add a corresponding proptest section here so every entry point has
//! at least a "never panics on arbitrary input" invariant.
//!
//! These run in normal `cargo test` / CI alongside the deterministic suite.

use iroh_http_core::{
    parse_node_addr, respond, HandleStore, NodeAddrInfo, StoreConfig,
    base32_encode, core_error_to_json, format_error_json,
    secret_key_sign, public_key_verify, generate_secret_key,
    parse_direct_addrs, CoreError, ErrorCode,
};
use proptest::prelude::*;

// ── lib.rs: parse_node_addr ──────────────────────────────────────────────────

proptest! {
    #[test]
    fn parse_node_addr_never_panics(s in "\\PC{0,512}") {
        let _ = parse_node_addr(&s);
    }

    #[test]
    fn parse_node_addr_json_arbitrary_addrs(
        key_bytes in prop::array::uniform32(any::<u8>()),
        addrs in prop::collection::vec("\\PC{0,128}", 0..8),
    ) {
        let b32 = base32_encode(&key_bytes);
        let info = NodeAddrInfo { id: b32, addrs };
        if let Ok(ticket) = serde_json::to_string(&info) {
            let _ = parse_node_addr(&ticket);
        }
    }
}

// ── lib.rs: base32 roundtrip ─────────────────────────────────────────────────

proptest! {
    #[test]
    fn base32_encode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let encoded = base32_encode(&bytes);
        // Encoded result must be non-empty for non-empty input, empty for empty.
        if bytes.is_empty() {
            prop_assert!(encoded.is_empty());
        } else {
            prop_assert!(!encoded.is_empty());
        }
    }
}

// ── lib.rs: crypto operations ────────────────────────────────────────────────

proptest! {
    /// sign+verify roundtrip: valid keys always produce verifiable signatures.
    #[test]
    fn sign_verify_roundtrip(data in prop::collection::vec(any::<u8>(), 0..512)) {
        let sk_bytes = generate_secret_key().unwrap();
        let sig = secret_key_sign(&sk_bytes, &data).unwrap();
        let sk = iroh::SecretKey::from_bytes(&sk_bytes);
        let pk_bytes = sk.public().as_bytes().clone();
        prop_assert!(public_key_verify(&pk_bytes, &data, &sig));
    }

    /// Arbitrary 32+64 byte arrays must never panic in verify — just return bool.
    #[test]
    fn public_key_verify_never_panics(
        pk in prop::array::uniform32(any::<u8>()),
        data in prop::collection::vec(any::<u8>(), 0..128),
        sig_lo in prop::array::uniform32(any::<u8>()),
        sig_hi in prop::array::uniform32(any::<u8>()),
    ) {
        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(&sig_lo);
        sig[32..].copy_from_slice(&sig_hi);
        let _ = public_key_verify(&pk, &data, &sig);
    }

    /// Arbitrary 32-byte keys must never panic in sign — returns Ok or Err.
    #[test]
    fn secret_key_sign_never_panics(
        sk in prop::array::uniform32(any::<u8>()),
        data in prop::collection::vec(any::<u8>(), 0..128),
    ) {
        let _ = secret_key_sign(&sk, &data);
    }
}

// ── lib.rs: error serialization ──────────────────────────────────────────────

proptest! {
    /// core_error_to_json must produce valid JSON for any message content.
    #[test]
    fn core_error_to_json_always_valid(msg in "\\PC{0,256}") {
        let err = CoreError { code: ErrorCode::Internal, message: msg };
        let json = core_error_to_json(&err);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(parsed["code"].as_str().unwrap(), "UNKNOWN");
    }

    /// format_error_json must produce valid JSON for any code/message.
    #[test]
    fn format_error_json_always_valid(
        code in "[A-Z_]{1,20}",
        msg in "\\PC{0,256}",
    ) {
        let json = format_error_json(&code, &msg);
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }
}

// ── endpoint.rs: parse_direct_addrs ──────────────────────────────────────────

proptest! {
    #[test]
    fn parse_direct_addrs_never_panics(
        addrs in prop::collection::vec("\\PC{0,64}", 0..8),
    ) {
        let _ = parse_direct_addrs(&Some(addrs));
    }

    #[test]
    fn parse_direct_addrs_none_is_none(_dummy in 0u8..1) {
        assert!(parse_direct_addrs(&None).unwrap().is_none());
    }
}

// ── server.rs: respond ───────────────────────────────────────────────────────

proptest! {
    /// respond() with arbitrary status/headers must never panic.
    #[test]
    fn respond_never_panics(
        status in any::<u16>(),
        names in prop::collection::vec("[a-z0-9\\-]{1,32}", 0..8),
        values in prop::collection::vec("\\PC{0,64}", 0..8),
    ) {
        let store = HandleStore::new(StoreConfig::default());
        let headers: Vec<_> = names.into_iter().zip(values).map(|(n, v)| (n, v)).collect();
        let _ = respond(&store, 0, status, headers);
    }
}

// ── stream.rs: HandleStore ───────────────────────────────────────────────────

proptest! {
    /// insert_reader never panics; after max_handles inserts it returns Err.
    #[test]
    fn handle_store_capacity_bounded(count in 1usize..=200) {
        let store = HandleStore::new(StoreConfig {
            max_handles: 64,
            ..Default::default()
        });
        let mut ok_count = 0u64;
        for _ in 0..count {
            let (_, reader) = store.make_body_channel();
            match store.insert_reader(reader) {
                Ok(_) => ok_count += 1,
                Err(_) => break,
            }
        }
        prop_assert!(ok_count <= 64);
    }

    /// Arbitrary u64 handles on an empty store never panic.
    #[test]
    fn handle_store_invalid_handle_safe(handle in any::<u64>()) {
        let store = HandleStore::new(StoreConfig::default());
        assert!(store.take_req_sender(handle).is_none());
        store.cancel_reader(handle);
        store.finish_body(handle).ok();
        store.cancel_in_flight(handle);
        store.remove_fetch_token(handle);
        assert!(store.lookup_session(handle).is_none());
        assert!(store.remove_session(handle).is_none());
        assert!(store.claim_pending_reader(handle).is_none());
    }

    /// insert → remove roundtrip: every handle returned by insert is valid.
    #[test]
    fn handle_store_reader_roundtrip(count in 1usize..=32) {
        let store = HandleStore::new(StoreConfig::default());
        let mut handles = Vec::new();
        for _ in 0..count {
            let (_, reader) = store.make_body_channel();
            handles.push(store.insert_reader(reader).unwrap());
        }
        for h in &handles {
            store.cancel_reader(*h);
        }
        // After cancel, handles are gone.
        for h in &handles {
            store.finish_body(*h).unwrap_err();
        }
    }

    /// pending_reader store→claim roundtrip.
    #[test]
    fn handle_store_pending_reader_roundtrip(count in 1usize..=16) {
        let store = HandleStore::new(StoreConfig::default());
        let mut writer_handles = Vec::new();
        for _ in 0..count {
            let (wh, reader) = store.alloc_body_writer().unwrap();
            store.store_pending_reader(wh, reader);
            writer_handles.push(wh);
        }
        for wh in &writer_handles {
            prop_assert!(store.claim_pending_reader(*wh).is_some());
            // Second claim returns None.
            prop_assert!(store.claim_pending_reader(*wh).is_none());
        }
    }
}

// ── registry.rs: endpoint registry ───────────────────────────────────────────
// Not tested here — requires real IrohEndpoint (async + network). The
// deterministic integration tests cover this adequately.
