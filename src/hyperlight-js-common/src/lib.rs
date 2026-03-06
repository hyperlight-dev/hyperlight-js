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
//! - `0x02` + sidecar + JSON → JSON with binary blobs in sidecar
//!
//! The `0x02` tag uses the same sidecar format as arguments:
//! `[TAG_JSON_WITH_BINARIES] [sidecar_len: u32-le] [sidecar...] [json...]`

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

/// Tag byte indicating the return payload is JSON with an embedded
/// binary sidecar. The format is:
/// `[0x02] [sidecar_len: u32-le] [sidecar_bytes...] [json_bytes...]`
///
/// The JSON may contain `{"__bin__": N}` placeholders that reference
/// blobs in the sidecar, exactly like the argument direction.
pub const TAG_JSON_WITH_BINARIES: u8 = 0x02;

/// JSON key used as a placeholder in serialised arguments to mark the
/// position of a binary blob that has been moved to the sidecar channel.
/// The value is the zero-based index into the sidecar blob array.
///
/// **Reserved key:** Do not use `"__bin__"` as a regular key in JSON
/// data passed through `FnReturn::JsonWithBinaries` — it will be
/// interpreted as a binary placeholder.
///
/// Example: `{"__bin__": 0}` means "insert sidecar blob 0 here".
pub const PLACEHOLDER_BIN: &str = "__bin__";

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
pub fn encode_binaries<B: AsRef<[u8]>>(blobs: &[B]) -> Result<Vec<u8>, DecodeError> {
    // Validate that count fits in u32 — the wire format uses u32-le.
    if blobs.len() > u32::MAX as usize {
        return Err(DecodeError::new(alloc::format!(
            "encode_binaries: blob count ({}) exceeds u32::MAX",
            blobs.len()
        )));
    }

    // Calculate total size: 4 bytes for count + (4 bytes length + data) per blob.
    // Use checked arithmetic to detect overflow — a corrupt or adversarial
    // input could otherwise wrap `usize` and cause an undersized allocation.
    let total_size = blobs
        .iter()
        .try_fold(4usize, |acc, b| {
            acc.checked_add(4)?.checked_add(b.as_ref().len())
        })
        .ok_or_else(|| DecodeError::new("encode_binaries: total sidecar size overflowed usize"))?;

    let mut buf = Vec::with_capacity(total_size);

    // Write count
    buf.extend_from_slice(&(blobs.len() as u32).to_le_bytes());

    // Write each blob with length prefix
    for blob in blobs {
        let bytes = blob.as_ref();
        if bytes.len() > u32::MAX as usize {
            return Err(DecodeError::new(alloc::format!(
                "encode_binaries: blob length ({}) exceeds u32::MAX",
                bytes.len()
            )));
        }
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }

    Ok(buf)
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

/// Encodes a JSON return value that contains binary sidecar data.
///
/// Format: `[TAG_JSON_WITH_BINARIES] [sidecar_len: u32-le] [sidecar...] [json...]`
///
/// The `sidecar` should be the output of [`encode_binaries`] and the
/// `json` string should contain `{"__bin__": N}` placeholders that
/// reference blobs in the sidecar.
///
/// Returns an error if the sidecar length exceeds `u32::MAX`.
pub fn encode_json_with_binaries_return(
    json: &str,
    sidecar: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    let sidecar_len: u32 = sidecar
        .len()
        .try_into()
        .map_err(|_| DecodeError::new("sidecar length exceeds u32::MAX"))?;
    // 1 (tag) + 4 (sidecar len) + sidecar + json
    let mut buf = Vec::with_capacity(1 + 4 + sidecar.len() + json.len());
    buf.push(TAG_JSON_WITH_BINARIES);
    buf.extend_from_slice(&sidecar_len.to_le_bytes());
    buf.extend_from_slice(sidecar);
    buf.extend_from_slice(json.as_bytes());
    Ok(buf)
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

    let mut offset: usize = 4;
    let mut blobs = Vec::with_capacity(count);

    for i in 0..count {
        let header_end = offset.checked_add(4).ok_or_else(|| {
            DecodeError::new(alloc::format!(
                "Binary sidecar offset overflow at blob {i} length header"
            ))
        })?;
        if header_end > data.len() {
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

        let blob_end = offset.checked_add(len).ok_or_else(|| {
            DecodeError::new(alloc::format!(
                "Binary sidecar offset overflow at blob {i} data"
            ))
        })?;
        if blob_end > data.len() {
            return Err(DecodeError::new(alloc::format!(
                "Binary sidecar truncated at blob {i} data (need {len} bytes, have {})",
                data.len() - offset
            )));
        }

        blobs.push(data[offset..blob_end].to_vec());
        offset = blob_end;
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
    /// JSON string payload (no embedded binary data).
    Json(String),
    /// Raw binary payload (single buffer return).
    Binary(Vec<u8>),
    /// JSON string payload with binary sidecar.
    ///
    /// The JSON contains `{"__bin__": N}` placeholders referencing
    /// blobs in the sidecar `Vec<u8>` (packed with [`encode_binaries`]).
    JsonWithBinaries(String, Vec<u8>),
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
        TAG_JSON_WITH_BINARIES => {
            // [0x02] [sidecar_len: u32-le] [sidecar...] [json...]
            if data.len() < 5 {
                return Err(DecodeError::new(
                    "JSON-with-binaries return too short for sidecar length header",
                ));
            }
            let sidecar_len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
            let sidecar_end = 5usize.checked_add(sidecar_len).ok_or_else(|| {
                DecodeError::new("JSON-with-binaries sidecar length overflows usize")
            })?;
            if data.len() < sidecar_end {
                return Err(DecodeError::new(alloc::format!(
                    "JSON-with-binaries return truncated: need {sidecar_end} bytes, have {}",
                    data.len()
                )));
            }
            let sidecar = data[5..sidecar_end].to_vec();
            let json = core::str::from_utf8(&data[sidecar_end..]).map_err(|e| {
                DecodeError::new(alloc::format!(
                    "Invalid UTF-8 in JSON-with-binaries return: {e}"
                ))
            })?;
            Ok(FnReturn::JsonWithBinaries(json.into(), sidecar))
        }
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
        let encoded = encode_binaries::<&[u8]>(&[]).unwrap();
        assert_eq!(encoded, vec![0, 0, 0, 0]); // count = 0

        let decoded = decode_binaries(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_encode_decode_single() {
        let blob = b"hello";
        let encoded = encode_binaries(&[blob]).unwrap();

        // count=1, len=5, "hello"
        let expected: Vec<u8> = vec![1, 0, 0, 0, 5, 0, 0, 0, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(encoded, expected);

        let decoded = decode_binaries(&encoded).unwrap();
        assert_eq!(decoded, vec![b"hello".to_vec()]);
    }

    #[test]
    fn test_encode_decode_multiple() {
        let blobs: &[&[u8]] = &[b"abc", b"", b"xy"];
        let encoded = encode_binaries(blobs).unwrap();

        let decoded = decode_binaries(&encoded).unwrap();
        assert_eq!(decoded, vec![b"abc".to_vec(), b"".to_vec(), b"xy".to_vec()]);
    }

    #[test]
    fn test_encode_decode_vec_of_vecs() {
        let blobs: Vec<Vec<u8>> = vec![b"ABC".to_vec(), b"XY".to_vec()];
        let encoded = encode_binaries(&blobs).unwrap();

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
        let mut data = encode_binaries(&[b"abc" as &[u8]]).unwrap();
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
    fn test_return_json_with_binaries() {
        let json = r#"{"data":{"__bin__":0}}"#;
        let sidecar = encode_binaries(&[b"hello" as &[u8]]).unwrap();
        let encoded = encode_json_with_binaries_return(json, &sidecar).unwrap();
        assert_eq!(encoded[0], TAG_JSON_WITH_BINARIES);

        match decode_return(&encoded).unwrap() {
            FnReturn::JsonWithBinaries(j, s) => {
                assert_eq!(j, json);
                // Verify the sidecar round-trips correctly
                let blobs = decode_binaries(&s).unwrap();
                assert_eq!(blobs, vec![b"hello".to_vec()]);
            }
            _ => panic!("Expected JsonWithBinaries return"),
        }
    }

    #[test]
    fn test_return_json_with_binaries_empty_sidecar() {
        let json = r#"{"result":42}"#;
        let sidecar = encode_binaries::<&[u8]>(&[]).unwrap();
        let encoded = encode_json_with_binaries_return(json, &sidecar).unwrap();

        match decode_return(&encoded).unwrap() {
            FnReturn::JsonWithBinaries(j, s) => {
                assert_eq!(j, json);
                let blobs = decode_binaries(&s).unwrap();
                assert!(blobs.is_empty());
            }
            _ => panic!("Expected JsonWithBinaries return"),
        }
    }

    #[test]
    fn test_return_json_with_binaries_truncated() {
        // Tag + only 3 bytes (need 4 for sidecar length)
        let result = decode_return(&[TAG_JSON_WITH_BINARIES, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_error_display() {
        let err = DecodeError::new("something went wrong");
        assert_eq!(err.to_string(), "something went wrong");
    }
}
