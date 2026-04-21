//! C-ABI entry point for the Deno FFI adapter.
//!
//! Most dispatch goes through the JSON `iroh_http_call` symbol.  Hot-path
//! streaming operations use dedicated binary symbols to avoid base64 overhead:
//! - `iroh_http_next_chunk` — read a body chunk directly into a caller buffer
//! - `iroh_http_send_chunk` — write a body chunk directly from a caller buffer
//!
//! All three symbols are `nonblocking: true` in the Deno `dlopen` call.

mod dispatch;
mod serve_registry;

use iroh_http_core::registry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Global multi-threaded Tokio runtime.  Initialised once on the first FFI call.
/// Returns `None` if the OS could not create the required threads.
pub(crate) fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RT: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .ok()
    })
    .as_ref()
}

// ── Overflow response cache ───────────────────────────────────────────────────
//
// When dispatch produces a response larger than the caller-provided output
// buffer, we cache the encoded bytes under a monotonic token and return the
// token to the caller (written into the first 8 bytes of the output buffer).
// The caller retries with method `"__cached"` and the 8-byte token as payload,
// avoiding a second dispatch of the original method.

static OVERFLOW_COUNTER: AtomicU64 = AtomicU64::new(1);

/// ISS-014: maximum number of cached overflow entries to prevent unbounded growth.
const OVERFLOW_MAX_ENTRIES: usize = 256;
/// ISS-014: maximum total bytes across all cached entries.
const OVERFLOW_MAX_BYTES: usize = 64 * 1024 * 1024; // 64 MB

/// Overflow entry with insertion timestamp for TTL eviction.
struct OverflowEntry {
    data: Vec<u8>,
    created: std::time::Instant,
}

/// TTL for overflow cache entries.
const OVERFLOW_TTL: std::time::Duration = std::time::Duration::from_secs(30);

fn overflow_cache() -> &'static Mutex<HashMap<u64, OverflowEntry>> {
    static C: OnceLock<Mutex<HashMap<u64, OverflowEntry>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Evict expired and over-budget entries from the overflow cache.
fn evict_overflow(cache: &mut HashMap<u64, OverflowEntry>) {
    // Remove expired entries first.
    cache.retain(|_, e| e.created.elapsed() < OVERFLOW_TTL);
    // If still over budget, remove oldest entries until within limits.
    while cache.len() > OVERFLOW_MAX_ENTRIES
        || cache.values().map(|e| e.data.len()).sum::<usize>() > OVERFLOW_MAX_BYTES
    {
        if let Some((&oldest_key, _)) = cache.iter().min_by_key(|(_, e)| e.created) {
            cache.remove(&oldest_key);
        } else {
            break;
        }
    }
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
///
/// # Safety
/// `method_ptr`, `payload_ptr`, and `out_ptr` must be valid for the lengths
/// provided and must not overlap. Null pointers are only valid when the
/// corresponding length is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iroh_http_call(
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

    // SAFETY: Guards above ensure pointers are non-null when len > 0. Use empty
    // slices for zero-length inputs to avoid passing null to `from_raw_parts`,
    // which is UB even when len is 0 (SEC-001).
    let method_bytes: &[u8] = if method_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(method_ptr, method_len) }
    };
    let method = std::str::from_utf8(method_bytes).unwrap_or("__invalid_utf8__");
    let payload: &[u8] = if payload_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) }
    };

    // ── Cached-response retrieval (overflow retry path) ───────────────────
    if method == "__cached" {
        if payload_len >= 8 {
            let token = u64::from_le_bytes(
                payload[0..8]
                    .try_into()
                    .expect("payload_len >= 8 already checked"),
            );
            if let Some(entry) = overflow_cache()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&token)
            {
                if entry.data.len() <= out_cap {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            entry.data.as_ptr(),
                            out_ptr,
                            entry.data.len(),
                        );
                    }
                    return entry.data.len() as i32;
                }
                // Buffer still too small — put it back (shouldn't happen).
                let len = entry.data.len();
                overflow_cache()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(token, entry);
                return i32::try_from(len)
                    .map(|n| 0i32.wrapping_sub(n))
                    .unwrap_or(i32::MIN);
            }
        }
        return -1;
    }

    // ── Normal dispatch ───────────────────────────────────────────────────
    let rt = match runtime() {
        Some(rt) => rt,
        None => {
            let err =
                br#"{"err":"RUNTIME_INIT_FAILED: OS refused to create threads for Tokio runtime"}"#;
            if err.len() <= out_cap {
                unsafe { std::ptr::copy_nonoverlapping(err.as_ptr(), out_ptr, err.len()) };
                return err.len() as i32;
            }
            return i32::try_from(err.len())
                .map(|n| 0i32.wrapping_sub(n))
                .unwrap_or(i32::MIN);
        }
    };
    let response = rt.block_on(dispatch::dispatch(method, payload));

    let encoded = serde_json::to_vec(&response).unwrap_or_else(|e| {
        serde_json::to_vec(&serde_json::json!({ "err": e.to_string() }))
            .expect("static error JSON is always valid")
    });

    let len = encoded.len();
    if len > out_cap {
        // Cache the response and write a retrieval token into the output
        // buffer so the caller can retry without re-dispatching.
        let token = OVERFLOW_COUNTER.fetch_add(1, Ordering::Relaxed);
        if out_cap >= 8 {
            let token_bytes = token.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(token_bytes.as_ptr(), out_ptr, 8);
            }
        }
        let mut cache = overflow_cache().lock().unwrap_or_else(|e| e.into_inner());
        // ISS-014: evict before inserting to enforce size/time bounds.
        evict_overflow(&mut cache);
        cache.insert(
            token,
            OverflowEntry {
                data: encoded,
                created: std::time::Instant::now(),
            },
        );
        return i32::try_from(len)
            .map(|n| 0i32.wrapping_sub(n))
            .unwrap_or(i32::MIN);
    }
    // SAFETY: `out_ptr` is non-null (checked above) and `out_cap >= len`.
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), out_ptr, len);
    }
    len as i32
}

/// Raw-buffer `nextChunk` — bypasses JSON dispatch for streaming throughput.
///
/// Writes the next chunk bytes directly into `out_ptr[0..out_cap]`.
///
/// Return value:
/// - `n > 0`  — bytes written into the buffer.
/// - `n == 0` — end of stream; no more chunks.
/// - `n < 0`  — `|n|` bytes required; caller must retry with a larger buffer.
///
/// This symbol is declared `nonblocking: true` in the Deno `dlopen` call.
///
/// # Safety
/// `out_ptr` must be valid for `out_cap` bytes and must not alias any other
/// active reference for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iroh_http_next_chunk(
    endpoint_handle: u32,
    handle: u64,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32 {
    if out_cap > 0 && out_ptr.is_null() {
        return -1;
    }

    let ep = match registry::get_endpoint(endpoint_handle as u64) {
        Some(ep) => ep,
        None => return -1,
    };

    let result = match runtime() {
        Some(rt) => rt.block_on(ep.handles().next_chunk(handle)),
        None => return -1,
    };

    match result {
        Err(_) => -1,
        Ok(None) => 0,
        Ok(Some(b)) => {
            let len = b.len();
            if len > out_cap {
                return i32::try_from(len)
                    .map(|n| 0i32.wrapping_sub(n))
                    .unwrap_or(i32::MIN);
            }
            // SAFETY: caller guarantees out_ptr is valid for out_cap bytes,
            // and we have verified len <= out_cap.
            unsafe {
                std::ptr::copy_nonoverlapping(b.as_ptr(), out_ptr, len);
            }
            len as i32
        }
    }
}

/// Raw-buffer `sendChunk` — bypasses JSON dispatch for streaming throughput.
///
/// Copies `len` bytes from `ptr` into a new chunk and sends it to the body
/// writer at `handle`.
///
/// Return value:
/// - `0`  — success.
/// - `-1` — error (endpoint gone, handle invalid, or channel closed).
///
/// This symbol is declared `nonblocking: true` in the Deno `dlopen` call.
///
/// # Safety
/// `ptr` must be valid for `len` bytes for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iroh_http_send_chunk(
    endpoint_handle: u32,
    handle: u64,
    ptr: *const u8,
    len: usize,
) -> i32 {
    if len > 0 && ptr.is_null() {
        return -1;
    }

    let ep = match registry::get_endpoint(endpoint_handle as u64) {
        Some(ep) => ep,
        None => return -1,
    };

    // SAFETY: Guard above ensures ptr is non-null when len > 0. Use an empty
    // slice for zero-length inputs to avoid null-pointer UB (SEC-001).
    let slice: &[u8] = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let bytes = bytes::Bytes::copy_from_slice(slice);

    match runtime().map(|rt| rt.block_on(ep.handles().send_chunk(handle, bytes))) {
        Some(Ok(())) => 0,
        _ => -1,
    }
}

/// Force-close all open endpoints.  Call from a Deno signal listener or
/// `unload` event handler so QUIC connections send a proper CONNECTION_CLOSE
/// frame before the process exits.
#[unsafe(no_mangle)]
pub extern "C" fn iroh_http_close_all() {
    registry::close_all_endpoints();
}
