#![no_main]

use libfuzzer_sys::fuzz_target;

// Fuzz `parse_node_addr` with arbitrary byte strings.
// Must never panic regardless of input — should return Ok or Err.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = iroh_http_core::parse_node_addr(s);
    }
});
