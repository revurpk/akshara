//! # akshara-ffi
//!
//! The C ABI used by the Flutter UI (through `dart:ffi`) to talk to the Rust
//! cores. The whole interface is three functions:
//!
//! ```c
//! uint8_t *akshara_alloc(size_t len);                 // buffer for a request
//! void     akshara_free(uint8_t *ptr, size_t len);    // free any buffer
//! uint8_t *akshara_call(const uint8_t *req, size_t len, size_t *out_len);
//! ```
//!
//! `akshara_call` takes a UTF-8 JSON request and returns a newly allocated UTF-8
//! JSON response whose length is written to `*out_len`; the caller frees it
//! with `akshara_free`. A JSON protocol keeps the unsafe surface tiny (these
//! functions are the only `unsafe` code in the project), needs no code
//! generator, and works identically on every platform.
//!
//! The operations are documented in `docs/ffi-protocol.md`; the dispatcher
//! is [`api::Registry::dispatch`].

pub mod api;

pub use api::handle;

fn into_raw(bytes: Vec<u8>, out_len: &mut usize) -> *mut u8 {
    let boxed = bytes.into_boxed_slice();
    *out_len = boxed.len();
    Box::into_raw(boxed).cast::<u8>()
}

/// Allocates a zeroed buffer of `len` bytes for a request. Free it with
/// [`akshara_free`]. Returns null for `len == 0` or absurd sizes.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn akshara_alloc(len: usize) -> *mut u8 {
    if len == 0 || len > api::MAX_REQUEST_LEN {
        return std::ptr::null_mut();
    }
    let mut ignored = 0;
    into_raw(vec![0u8; len], &mut ignored)
}

/// Frees a buffer returned by [`akshara_alloc`] or [`akshara_call`].
///
/// # Safety
///
/// `ptr` must come from `akshara_alloc(len)` or be the result of `akshara_call` with
/// `len` its reported length, and must not be used afterwards. Null is
/// ignored.
#[no_mangle]
#[allow(unsafe_code)]
pub unsafe extern "C" fn akshara_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: per the contract, `ptr` came from `Box<[u8]>::into_raw` with
    // exactly `len` elements and ownership is transferred back here.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// Executes a JSON request and returns the JSON response.
///
/// # Safety
///
/// `req` must point to `len` readable bytes (or be null with `len == 0`) and
/// `out_len` must point to writable memory for one `usize`. The returned
/// buffer must be released with `akshara_free(ptr, *out_len)`.
#[no_mangle]
#[allow(unsafe_code)]
pub unsafe extern "C" fn akshara_call(req: *const u8, len: usize, out_len: *mut usize) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    let request: &[u8] = if req.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller guarantees `req` points to `len` readable bytes
        // that stay valid for the duration of this call.
        unsafe { std::slice::from_raw_parts(req, len) }
    };
    let response = handle(request);
    let mut written = 0usize;
    let ptr = into_raw(response, &mut written);
    // SAFETY: the caller guarantees `out_len` is valid for writes.
    unsafe { out_len.write(written) };
    ptr
}
