//! C-ABI entry point for the Deno FFI adapter.
//!
//! All dispatch goes through a single `iroh_http_call` symbol.
//! The function signature is intentionally identical to the one used in the
//! legacy `iroh-deno` reference so the TypeScript adapter pattern is fully
//! portable.

mod dispatch;
mod serve_registry;

use std::sync::OnceLock;

/// Global multi-threaded Tokio runtime.  Initialised once on the first FFI call.
pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build Tokio runtime")
    })
}

/// Single-dispatch FFI entry point.
///
/// Parameters:
/// - `method_ptr` / `method_len` — UTF-8 encoded method name.
/// - `payload_ptr` / `payload_len` — JSON-encoded payload bytes.
/// - `out_ptr` / `out_cap` — caller-allocated output buffer.
///
/// Return value:
/// - `>= 0` — number of bytes written to `out_ptr`.
/// - `< 0`  — `-(required_size)`; caller must retry with a larger buffer.
///
/// The output buffer always contains a JSON object of the form
/// `{"ok": <value>}` on success or `{"err": "<message>"}` on failure.
///
/// This symbol is declared `nonblocking: true` in the Deno `dlopen` call, so
/// it is invoked on the Deno thread pool and returns a `Promise<i32>`.
#[unsafe(no_mangle)]
pub extern "C" fn iroh_http_call(
    method_ptr: *const u8,
    method_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32 {
    // Validate all pointer/length combinations at the FFI boundary.
    if method_len > 0 && method_ptr.is_null() {
        return -1;
    }
    if payload_len > 0 && payload_ptr.is_null() {
        return -1;
    }
    if out_cap > 0 && out_ptr.is_null() {
        return -1;
    }

    // SAFETY: Deno passes valid, non-overlapping, non-null (for nonzero lengths)
    // pointers for the complete duration of this call.
    let method_bytes = unsafe { std::slice::from_raw_parts(method_ptr, method_len) };
    let method = std::str::from_utf8(method_bytes).unwrap_or("__invalid_utf8__");
    let payload = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };

    let response = runtime().block_on(dispatch::dispatch(method, payload));

    let encoded = serde_json::to_vec(&response).unwrap_or_else(|e| {
        serde_json::to_vec(&serde_json::json!({ "err": e.to_string() })).unwrap()
    });

    let len = encoded.len();
    if len > out_cap {
        return -(len as i32);
    }
    // SAFETY: `out_ptr` is non-null (checked above) and `out_cap >= len`.
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), out_ptr, len);
    }
    len as i32
}
