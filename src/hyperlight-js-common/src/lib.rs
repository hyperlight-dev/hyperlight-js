/*
Copyright 2026  The Hyperlight Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

//! Shared constants and binary framing utilities for hyperlight-js.
//!
//! This crate is the **single source of truth** for the wire-format used to
//! pass binary data (`Uint8Array` / `Buffer`) between guest JavaScript and
//! host functions. Both `hyperlight-js` (host) and `hyperlight-js-runtime`
//! (guest, `no_std`) depend on this crate instead of duplicating the logic.
//!
//! # Wire Format — Binary Sidecar
//!
//! Binary blobs are packed into a length-prefixed sidecar:
//!
//! ```text
//! [count: u32-le] [len0: u32-le] [bytes0...] [len1: u32-le] [bytes1...] ...
//! ```
//!
//! # Wire Format — Tagged Returns
//!
//! Host function returns use a single-byte tag prefix:
//! - `0x00` + payload → JSON string follows
//! - `0x01` + payload → raw binary follows (single buffer return)

#![no_std]
extern crate alloc;

use alloc::fmt;
use alloc::string::String;
use alloc::vec::Vec;

// ── Constants ────────────────────────────────────────────────────────

/// Tag byte indicating the return payload is JSON.
pub const TAG_JSON: u8 = 0x00;

/// Tag byte indicating the return payload is raw binary.
pub const TAG_BINARY: u8 = 0x01;

/// JSON key used as a placeholder in serialised arguments to mark the
/// position of a binary blob that has been moved to the sidecar channel.
/// The value is the zero-based index into the sidecar blob array.
///
/// Example: `{"__bin__": 0}` means "insert sidecar blob 0 here".
pub const PLACEHOLDER_BIN: &str = "__bin__";

/// JSON key used as a base64-encoded binary marker in the NAPI ↔ JS
/// bridge. The value is a base64 string representation of the bytes.
///
/// Example: `{"__buffer__": "SGVsbG8="}`
pub const MARKER_BUFFER: &str = "__buffer__";

// ── Error type ───────────────────────────────────────────────────────

/// Lightweight decoding error — `no_std`-compatible (no `anyhow`, no `std`).
///
/// Both the host (`hyperlight-js`) and guest (`hyperlight-js-runtime`)
/// convert this into their own error types via `From` impls.
#[derive(Debug, Clone)]
pub struct DecodeError(String);

impl DecodeError {
    /// Create a new decode error with the given message.
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Encoding ─────────────────────────────────────────────────────────

/// Encodes multiple binary blobs into the sidecar format.
///
/// Format: `[count: u32-le] [len0: u32-le] [bytes0...] [len1: u32-le] [bytes1...] ...`
///
/// Accepts any slice of items that implement `AsRef<[u8]>` — e.g.
/// `&[Vec<u8>]`, `&[&[u8]]`, `&[Box<[u8]>]` — so callers don't need to
/// build an intermediate `Vec<&[u8]>` just to satisfy the signature.
pub fn encode_binaries<B: AsRef<[u8]>>(blobs: &[B]) -> Vec<u8> {
    // Validate that count and blob lengths fit in u32 — the wire format
    // uses u32-le for these fields. Overflow would create a corrupt sidecar
    // that the decoder would reject, but we fail early with a clear message.
    assert!(
        blobs.len() <= u32::MAX as usize,
        "encode_binaries: blob count ({}) exceeds u32::MAX",
        blobs.len()
    );

    // Calculate total size: 4 bytes for count + (4 bytes length + data) per blob
    let total_size = 4 + blobs.iter().map(|b| 4 + b.as_ref().len()).sum::<usize>();
    let mut buf = Vec::with_capacity(total_size);

    // Write count
    buf.extend_from_slice(&(blobs.len() as u32).to_le_bytes());

    // Write each blob with length prefix
    for blob in blobs {
        let bytes = blob.as_ref();
        assert!(
            bytes.len() <= u32::MAX as usize,
            "encode_binaries: blob length ({}) exceeds u32::MAX",
            bytes.len()
        );
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }

    buf
}

/// Encodes a JSON return value with the appropriate tag.
pub fn encode_json_return(json: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + json.len());
    buf.push(TAG_JSON);
    buf.extend_from_slice(json.as_bytes());
    buf
}

/// Encodes a binary return value with the appropriate tag.
pub fn encode_binary_return(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + data.len());
    buf.push(TAG_BINARY);
    buf.extend_from_slice(data);
    buf
}

// ── Decoding ─────────────────────────────────────────────────────────

/// Decodes the sidecar format into individual binary blobs.
///
/// Returns a [`DecodeError`] if the buffer is malformed (truncated,
/// invalid lengths, or suspiciously large blob counts).
pub fn decode_binaries(data: &[u8]) -> Result<Vec<Vec<u8>>, DecodeError> {
    if data.len() < 4 {
        return Err(DecodeError::new(
            "Binary sidecar too short for count header",
        ));
    }

    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    // Sanity check: each blob needs at least 4 bytes for length header.
    // This prevents allocation of a huge Vec when count is maliciously large.
    let max_possible_blobs = (data.len().saturating_sub(4)) / 4;
    if count > max_possible_blobs {
        return Err(DecodeError::new(alloc::format!(
            "Binary sidecar count ({count}) exceeds maximum possible ({max_possible_blobs})"
        )));
    }

    let mut offset = 4;
    let mut blobs = Vec::with_capacity(count);

    for i in 0..count {
        if offset + 4 > data.len() {
            return Err(DecodeError::new(alloc::format!(
                "Binary sidecar truncated at blob {i} length header"
            )));
        }

        let len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + len > data.len() {
            return Err(DecodeError::new(alloc::format!(
                "Binary sidecar truncated at blob {i} data (need {len} bytes, have {})",
                data.len() - offset
            )));
        }

        blobs.push(data[offset..offset + len].to_vec());
        offset += len;
    }

    // Reject trailing data — the sidecar should be fully consumed.
    // Trailing bytes could indicate a version mismatch or corruption.
    if offset != data.len() {
        return Err(DecodeError::new(alloc::format!(
            "Binary sidecar has {} trailing bytes after all {count} blobs",
            data.len() - offset
        )));
    }

    Ok(blobs)
}

/// Maximum recursion depth for JSON tree traversal.
/// Shared across host and NAPI layers to limit stack usage.
pub const MAX_JSON_DEPTH: usize = 64;

/// Result of decoding a tagged return value.
#[derive(Debug, Clone)]
pub enum FnReturn {
    /// JSON string payload.
    Json(String),
    /// Raw binary payload.
    Binary(Vec<u8>),
}

/// Decodes a tagged return value from the host.
///
/// The first byte is a tag (see [`TAG_JSON`] / [`TAG_BINARY`]),
/// the rest is the payload.
pub fn decode_return(data: &[u8]) -> Result<FnReturn, DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::new("Empty return payload"));
    }

    match data[0] {
        TAG_JSON => {
            let json = core::str::from_utf8(&data[1..]).map_err(|e| {
                DecodeError::new(alloc::format!("Invalid UTF-8 in JSON return: {e}"))
            })?;
            Ok(FnReturn::Json(json.into()))
        }
        TAG_BINARY => Ok(FnReturn::Binary(data[1..].to_vec())),
        tag => Err(DecodeError::new(alloc::format!(
            "Unknown return tag: 0x{tag:02x}"
        ))),
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_encode_decode_empty() {
        let encoded = encode_binaries::<&[u8]>(&[]);
        assert_eq!(encoded, vec![0, 0, 0, 0]); // count = 0

        let decoded = decode_binaries(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_encode_decode_single() {
        let blob = b"hello";
        let encoded = encode_binaries(&[blob]);

        // count=1, len=5, "hello"
        let expected: Vec<u8> = vec![1, 0, 0, 0, 5, 0, 0, 0, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(encoded, expected);

        let decoded = decode_binaries(&encoded).unwrap();
        assert_eq!(decoded, vec![b"hello".to_vec()]);
    }

    #[test]
    fn test_encode_decode_multiple() {
        let blobs: &[&[u8]] = &[b"abc", b"", b"xy"];
        let encoded = encode_binaries(blobs);

        let decoded = decode_binaries(&encoded).unwrap();
        assert_eq!(decoded, vec![b"abc".to_vec(), b"".to_vec(), b"xy".to_vec()]);
    }

    #[test]
    fn test_encode_decode_vec_of_vecs() {
        let blobs: Vec<Vec<u8>> = vec![b"ABC".to_vec(), b"XY".to_vec()];
        let encoded = encode_binaries(&blobs);

        let decoded = decode_binaries(&encoded).unwrap();
        assert_eq!(decoded, blobs);
    }

    #[test]
    fn test_decode_truncated_count() {
        let result = decode_binaries(&[1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_truncated_length() {
        // count=1 but no length header
        let result = decode_binaries(&[1, 0, 0, 0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_truncated_data() {
        // count=1, len=10 but only 3 bytes of data
        let result = decode_binaries(&[1, 0, 0, 0, 10, 0, 0, 0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_trailing_data() {
        // Valid sidecar with one blob "abc" followed by trailing garbage
        let mut data = encode_binaries(&[b"abc" as &[u8]]);
        data.push(0xFF); // trailing byte
        let result = decode_binaries(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("trailing"));
    }

    #[test]
    fn test_return_json() {
        let json = r#"{"result":42}"#;
        let encoded = encode_json_return(json);
        assert_eq!(encoded[0], TAG_JSON);

        match decode_return(&encoded).unwrap() {
            FnReturn::Json(s) => assert_eq!(s, json),
            _ => panic!("Expected JSON return"),
        }
    }

    #[test]
    fn test_return_binary() {
        let data = b"\x00\x01\x02\xff";
        let encoded = encode_binary_return(data);
        assert_eq!(encoded[0], TAG_BINARY);

        match decode_return(&encoded).unwrap() {
            FnReturn::Binary(b) => assert_eq!(b, data),
            _ => panic!("Expected binary return"),
        }
    }

    #[test]
    fn test_return_empty() {
        let result = decode_return(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_return_unknown_tag() {
        let result = decode_return(&[0x99, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_error_display() {
        let err = DecodeError::new("something went wrong");
        assert_eq!(err.to_string(), "something went wrong");
    }
}
