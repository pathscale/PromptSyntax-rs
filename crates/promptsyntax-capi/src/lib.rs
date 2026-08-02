//! Versioned C ABI for the Prompt Syntax Rust parser.
//!
//! The parser remains implemented by the safe `promptsyntax` crate. This crate owns the
//! deliberately small unsafe boundary needed to accept C pointers and return opaque,
//! Rust-owned handles.

use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use std::str;

use promptsyntax::Parser;
use serde::Serialize;

/// ABI version 1.0, encoded as `major << 16 | minor`.
pub const PS_ABI_VERSION: u32 = 0x0001_0000;

/// The operation completed successfully.
pub const PS_STATUS_OK: u32 = 0;
/// A required pointer was null.
pub const PS_STATUS_NULL_POINTER: u32 = 1;
/// Input bytes were not valid UTF-8.
pub const PS_STATUS_INVALID_UTF8: u32 = 2;
/// A parse result could not be serialized.
pub const PS_STATUS_SERIALIZATION_ERROR: u32 = 3;
/// Rust code panicked at the ABI boundary. No unwind crossed into the caller.
pub const PS_STATUS_PANIC: u32 = 4;

const PARSE_RESULT_SCHEMA: &str = "org.promptsyntax.parse-result/0.1";

/// Opaque parser state. C callers only receive a pointer to this type.
pub struct PsParser {
    inner: Parser,
}

/// Opaque owned result. The JSON view remains valid until this handle is freed.
pub struct PsParseResult {
    json: Vec<u8>,
}

#[derive(Serialize)]
struct ParseEnvelope<'a> {
    schema: &'static str,
    parser_version: &'static str,
    source: &'a str,
    data_plane: String,
    segments: &'a [promptsyntax::Segment],
    diagnostics: &'a [promptsyntax::Diagnostic],
}

/// Return the ABI version as `major << 16 | minor`.
#[unsafe(no_mangle)]
pub extern "C" fn ps_abi_version() -> u32 {
    PS_ABI_VERSION
}

/// Return a static, null-terminated message for a status code.
///
/// The returned pointer is always non-null and must not be freed.
#[unsafe(no_mangle)]
pub extern "C" fn ps_status_message(status: u32) -> *const c_char {
    let message: &'static [u8] = match status {
        PS_STATUS_OK => b"ok\0",
        PS_STATUS_NULL_POINTER => b"null pointer\0",
        PS_STATUS_INVALID_UTF8 => b"invalid UTF-8\0",
        PS_STATUS_SERIALIZATION_ERROR => b"serialization error\0",
        PS_STATUS_PANIC => b"panic contained at Rust ABI boundary\0",
        _ => b"unknown Prompt Syntax status\0",
    };
    message.as_ptr().cast()
}

/// Allocate a parser with an empty resolver configuration.
///
/// # Safety
///
/// `out_parser` must point to writable memory for one pointer. On every ordinary return,
/// it is either null or owns a handle that must be released exactly once with
/// [`ps_parser_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_new(out_parser: *mut *mut PsParser) -> u32 {
    status_guard(|| {
        initialize_handle(out_parser)?;
        let parser = Box::new(PsParser {
            inner: Parser::new(),
        });
        // SAFETY: initialize_handle validated the required non-null out pointer. The
        // caller's writable-pointer obligation is documented on this function.
        unsafe { out_parser.write(Box::into_raw(parser)) };
        Ok(())
    })
}

/// Free a parser allocated by [`ps_parser_new`]. A null pointer is ignored.
///
/// # Safety
///
/// `parser` must be null or a live handle returned by [`ps_parser_new`] that has not
/// already been freed. It must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_free(parser: *mut PsParser) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !parser.is_null() {
            // SAFETY: ownership and single-free requirements are the caller contract.
            unsafe { drop(Box::from_raw(parser)) };
        }
    }));
}

/// Declare one environment-resolvable bare entity name.
///
/// # Safety
///
/// `parser` must be a live, exclusively accessible parser handle. When `length` is
/// nonzero, `data` must address `length` readable bytes. The bytes need not be
/// null-terminated and may contain embedded nulls, but they must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_add_entity(
    parser: *mut PsParser,
    data: *const u8,
    length: usize,
) -> u32 {
    status_guard(|| {
        // SAFETY: upheld by this function's caller contract.
        unsafe { update_parser(parser, data, length, |inner, value| inner.entity(value)) }
    })
}

/// Declare one environment-resolvable slash action name.
///
/// # Safety
///
/// `parser` must be a live, exclusively accessible parser handle. When `length` is
/// nonzero, `data` must address `length` readable bytes containing valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_add_action(
    parser: *mut PsParser,
    data: *const u8,
    length: usize,
) -> u32 {
    status_guard(|| {
        // SAFETY: upheld by this function's caller contract.
        unsafe { update_parser(parser, data, length, |inner, value| inner.action(value)) }
    })
}

/// Declare a standing authoring namespace.
///
/// # Safety
///
/// `parser` must be a live, exclusively accessible parser handle. When `length` is
/// nonzero, `data` must address `length` readable bytes containing valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_add_authoring_namespace(
    parser: *mut PsParser,
    data: *const u8,
    length: usize,
) -> u32 {
    status_guard(|| {
        // SAFETY: upheld by this function's caller contract.
        unsafe {
            update_parser(parser, data, length, |inner, value| {
                inner.authoring_namespace(value)
            })
        }
    })
}

/// Parse one provenance-approved authored UTF-8 segment.
///
/// The returned handle owns a UTF-8 JSON document using schema
/// `org.promptsyntax.parse-result/0.1`. The document contains the exact source, data-plane
/// projection, lossless segment tree, and diagnostics.
///
/// # Safety
///
/// `parser` must be a live handle and must not be mutated concurrently. When `length` is
/// nonzero, `data` must address `length` readable bytes. `out_result` must point to
/// writable memory for one pointer. A successful result must be released exactly once
/// with [`ps_parse_result_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parser_parse(
    parser: *const PsParser,
    data: *const u8,
    length: usize,
    out_result: *mut *mut PsParseResult,
) -> u32 {
    status_guard(|| {
        initialize_handle(out_result)?;
        // SAFETY: the caller contract requires a live parser pointer.
        let parser = unsafe { parser.as_ref() }.ok_or(PS_STATUS_NULL_POINTER)?;
        // SAFETY: the caller contract covers the input byte range.
        let source = unsafe { read_utf8(data, length) }?;
        let parsed = parser.inner.parse(source);
        let envelope = ParseEnvelope {
            schema: PARSE_RESULT_SCHEMA,
            parser_version: promptsyntax::VERSION,
            source: &parsed.source,
            data_plane: parsed.data_plane(),
            segments: &parsed.segments,
            diagnostics: &parsed.diagnostics,
        };
        let json = serde_json::to_vec(&envelope).map_err(|_| PS_STATUS_SERIALIZATION_ERROR)?;
        let result = Box::new(PsParseResult { json });
        // SAFETY: initialize_handle validated the out pointer.
        unsafe { out_result.write(Box::into_raw(result)) };
        Ok(())
    })
}

/// Borrow the UTF-8 JSON bytes owned by a parse-result handle.
///
/// The returned bytes are not null-terminated and remain valid only until
/// [`ps_parse_result_free`] is called for `result`.
///
/// # Safety
///
/// `result` must be a live result handle. `out_data` and `out_length` must each point to
/// writable memory for their respective values. The result must not be freed while the
/// borrowed bytes are being read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parse_result_json(
    result: *const PsParseResult,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> u32 {
    status_guard(|| {
        if out_data.is_null() || out_length.is_null() {
            return Err(PS_STATUS_NULL_POINTER);
        }
        // SAFETY: non-null writable output pointers are required by the caller contract.
        unsafe {
            out_data.write(ptr::null());
            out_length.write(0);
        }
        // SAFETY: the caller contract requires a live result pointer.
        let result = unsafe { result.as_ref() }.ok_or(PS_STATUS_NULL_POINTER)?;
        // SAFETY: output pointers were checked above. Vec storage is stable while the
        // result handle remains live and immutable.
        unsafe {
            out_data.write(result.json.as_ptr());
            out_length.write(result.json.len());
        }
        Ok(())
    })
}

/// Free a parse-result handle. A null pointer is ignored.
///
/// # Safety
///
/// `result` must be null or a live handle returned by [`ps_parser_parse`] that has not
/// already been freed. All borrowed JSON views become invalid after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_parse_result_free(result: *mut PsParseResult) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !result.is_null() {
            // SAFETY: ownership and single-free requirements are the caller contract.
            unsafe { drop(Box::from_raw(result)) };
        }
    }));
}

fn status_guard(operation: impl FnOnce() -> Result<(), u32>) -> u32 {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => PS_STATUS_OK,
        Ok(Err(status)) => status,
        Err(_) => PS_STATUS_PANIC,
    }
}

fn initialize_handle<T>(out: *mut *mut T) -> Result<(), u32> {
    if out.is_null() {
        return Err(PS_STATUS_NULL_POINTER);
    }
    // SAFETY: callers of this helper have an explicit writable-pointer contract.
    unsafe { out.write(ptr::null_mut()) };
    Ok(())
}

unsafe fn update_parser(
    parser: *mut PsParser,
    data: *const u8,
    length: usize,
    update: impl FnOnce(Parser, &str) -> Parser,
) -> Result<(), u32> {
    // SAFETY: this helper inherits the live, exclusive pointer contract from its caller.
    let parser = unsafe { parser.as_mut() }.ok_or(PS_STATUS_NULL_POINTER)?;
    // SAFETY: this helper inherits the readable input contract from its caller.
    let value = unsafe { read_utf8(data, length) }?;
    parser.inner = update(parser.inner.clone(), value);
    Ok(())
}

unsafe fn read_utf8<'a>(data: *const u8, length: usize) -> Result<&'a str, u32> {
    let bytes = if length == 0 {
        &[]
    } else {
        if data.is_null() {
            return Err(PS_STATUS_NULL_POINTER);
        }
        // SAFETY: the caller guarantees a readable allocation of at least length bytes.
        unsafe { slice::from_raw_parts(data, length) }
    };
    str::from_utf8(bytes).map_err(|_| PS_STATUS_INVALID_UTF8)
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use serde_json::Value;

    use super::*;

    #[test]
    fn parses_through_owned_handles() {
        // SAFETY: every pointer in this test follows the public ownership contract.
        unsafe {
            let mut parser = ptr::null_mut();
            assert_eq!(ps_parser_new(&raw mut parser), PS_STATUS_OK);

            let entity = b"opus";
            assert_eq!(
                ps_parser_add_entity(parser, entity.as_ptr(), entity.len()),
                PS_STATUS_OK
            );
            let action = b"concise";
            assert_eq!(
                ps_parser_add_action(parser, action.as_ptr(), action.len()),
                PS_STATUS_OK
            );

            let source = "@opus Summarize @file:q3.md /concise";
            let mut result = ptr::null_mut();
            assert_eq!(
                ps_parser_parse(
                    parser.cast_const(),
                    source.as_ptr(),
                    source.len(),
                    &raw mut result
                ),
                PS_STATUS_OK
            );

            let mut json_data = ptr::null();
            let mut json_length = 0;
            assert_eq!(
                ps_parse_result_json(result, &raw mut json_data, &raw mut json_length),
                PS_STATUS_OK
            );
            let json = slice::from_raw_parts(json_data, json_length);
            let document: Value = serde_json::from_slice(json).expect("valid result JSON");
            assert_eq!(document["schema"], PARSE_RESULT_SCHEMA);
            assert_eq!(document["source"], source);
            assert_eq!(document["data_plane"], " Summarize  ");
            assert_eq!(document["diagnostics"], serde_json::json!([]));

            ps_parse_result_free(result);
            ps_parser_free(parser);
        }
    }

    #[test]
    fn null_errors_leave_output_values_empty() {
        // SAFETY: outputs are valid; intentionally null input handles exercise typed
        // boundary errors without violating any non-null pointer contract.
        unsafe {
            assert_eq!(ps_parser_new(ptr::null_mut()), PS_STATUS_NULL_POINTER);

            let name = b"opus";
            assert_eq!(
                ps_parser_add_entity(ptr::null_mut(), name.as_ptr(), name.len()),
                PS_STATUS_NULL_POINTER
            );

            let mut result = ptr::without_provenance_mut(1);
            assert_eq!(
                ps_parser_parse(ptr::null(), name.as_ptr(), name.len(), &raw mut result),
                PS_STATUS_NULL_POINTER
            );
            assert!(result.is_null());

            let mut json_data = ptr::without_provenance(1);
            let mut json_length = 99;
            assert_eq!(
                ps_parse_result_json(ptr::null(), &raw mut json_data, &raw mut json_length),
                PS_STATUS_NULL_POINTER
            );
            assert!(json_data.is_null());
            assert_eq!(json_length, 0);
        }
    }

    #[test]
    fn null_data_is_valid_for_empty_utf8() {
        // SAFETY: a null data pointer with zero length is explicitly supported.
        unsafe {
            let mut parser = ptr::null_mut();
            assert_eq!(ps_parser_new(&raw mut parser), PS_STATUS_OK);
            assert_eq!(ps_parser_add_action(parser, ptr::null(), 0), PS_STATUS_OK);

            let mut result = ptr::null_mut();
            assert_eq!(
                ps_parser_parse(parser, ptr::null(), 0, &raw mut result),
                PS_STATUS_OK
            );
            let mut json_data = ptr::null();
            let mut json_length = 0;
            assert_eq!(
                ps_parse_result_json(result, &raw mut json_data, &raw mut json_length),
                PS_STATUS_OK
            );
            let document: Value =
                serde_json::from_slice(slice::from_raw_parts(json_data, json_length))
                    .expect("valid result JSON");
            assert_eq!(document["source"], "");

            ps_parse_result_free(result);
            ps_parser_free(parser);
        }
    }

    #[test]
    fn rejects_invalid_utf8_and_initializes_output() {
        // SAFETY: every pointer in this test follows the public ownership contract.
        unsafe {
            let mut parser = ptr::null_mut();
            assert_eq!(ps_parser_new(&raw mut parser), PS_STATUS_OK);
            let invalid = [0xff];
            let mut result = ptr::without_provenance_mut(1);
            assert_eq!(
                ps_parser_parse(parser, invalid.as_ptr(), invalid.len(), &raw mut result),
                PS_STATUS_INVALID_UTF8
            );
            assert!(result.is_null());
            ps_parser_free(parser);
        }
    }
}
