#![no_main]

use libfuzzer_sys::fuzz_target;
use iroh_http_core::{HandleStore, StoreConfig};

// Fuzz `respond()` with arbitrary status codes and header data.
// Exercises header-name / header-value validation, status-code range checks,
// and the oneshot rendezvous path. Must not panic.
fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    let store = HandleStore::new(StoreConfig::default());

    // Use first 2 bytes as status code.
    let status = u16::from_le_bytes([data[0], data[1]]);

    // Use next 2 bytes as header count.
    let header_count = u16::from_le_bytes([data[2], data[3]]) as usize % 16;

    // Build header pairs from remaining data.
    let rest = &data[4..];
    let mut headers = Vec::new();
    let mut offset = 0;
    for _ in 0..header_count {
        if offset + 2 > rest.len() {
            break;
        }
        let name_len = rest[offset] as usize % 64;
        offset += 1;
        let val_len = rest[offset] as usize % 128;
        offset += 1;
        if offset + name_len + val_len > rest.len() {
            break;
        }
        if let (Ok(name), Ok(value)) = (
            std::str::from_utf8(&rest[offset..offset + name_len]),
            std::str::from_utf8(&rest[offset + name_len..offset + name_len + val_len]),
        ) {
            headers.push((name.to_string(), value.to_string()));
        }
        offset += name_len + val_len;
    }

    // respond() should return Err on invalid input, never panic.
    // Use handle 0 — no req_sender exists, so it will fail after validation.
    let _ = iroh_http_core::respond(&store, 0, status, headers);
});
