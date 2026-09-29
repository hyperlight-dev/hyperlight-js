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

use bytes::Buf;
/// Shared byte storage used by Hyperlight's `ByteChunks` transport values.
pub use bytes::Bytes;

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
    let mut buf = Vec::with_capacity(encoded_binaries_len(blobs)?);
    buf.extend_from_slice(&(blobs.len() as u32).to_le_bytes());

    for blob in blobs {
        let bytes = blob.as_ref();
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }

    Ok(buf)
}

/// Encodes a sidecar as chunks without copying the owned binary payloads.
///
/// The chunks form the same logical byte stream as [`encode_binaries`].
/// Headers are stored separately from the blobs, whose allocations are moved
/// into [`Bytes`]. Chunk boundaries are not part of the wire format and may
/// change during transport.
pub fn encode_binaries_chunks(blobs: Vec<Vec<u8>>) -> Result<Vec<Bytes>, DecodeError> {
    encoded_binaries_len(&blobs)?;

    let mut chunks = Vec::with_capacity(1 + 2 * blobs.len());
    chunks.push(Bytes::copy_from_slice(&(blobs.len() as u32).to_le_bytes()));

    for blob in blobs {
        chunks.push(Bytes::copy_from_slice(&(blob.len() as u32).to_le_bytes()));
        chunks.push(Bytes::from(blob));
    }

    Ok(chunks)
}

/// Validates sidecar counts and lengths before calculating its encoded size.
fn encoded_binaries_len<B: AsRef<[u8]>>(blobs: &[B]) -> Result<usize, DecodeError> {
    if blobs.len() > u32::MAX as usize {
        return Err(DecodeError::new(alloc::format!(
            "encode_binaries: blob count ({}) exceeds u32::MAX",
            blobs.len()
        )));
    }

    blobs.iter().try_fold(4usize, |size, blob| {
        let bytes = blob.as_ref();

        if bytes.len() > u32::MAX as usize {
            return Err(DecodeError::new(alloc::format!(
                "encode_binaries: blob length ({}) exceeds u32::MAX",
                bytes.len()
            )));
        }

        size.checked_add(4)
            .and_then(|size| size.checked_add(bytes.len()))
            .ok_or_else(|| DecodeError::new("encode_binaries: total sidecar size overflowed usize"))
    })
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
    decode_binaries_from_buf(data)
}

/// Borrows blobs from a contiguous sidecar without copying their payloads.
///
/// The slices remain valid only while the input buffer is alive. Framing and
/// lengths are validated identically to [`decode_binaries`].
pub fn decode_binaries_ref(data: &[u8]) -> Result<Vec<&[u8]>, DecodeError> {
    decode_binaries_buf(data, |data, len| {
        let (blob, remaining) = (*data).split_at(len);
        *data = remaining;
        blob
    })
}

/// Decodes a chunked sidecar without first flattening the entire stream.
///
/// Headers and blobs may span any number of chunks, including empty chunks.
/// Each decoded blob is copied once into the owned buffer required by the
/// host-function bridge; transport-owned memory is never exposed to JavaScript.
pub fn decode_binaries_from_buf(data: impl Buf) -> Result<Vec<Vec<u8>>, DecodeError> {
    decode_binaries_buf(data, copy_blob)
}

/// Validates sidecar framing before reading each blob using the selected
/// ownership strategy. The reader must consume exactly the validated length.
fn decode_binaries_buf<B: Buf, T>(
    mut data: B,
    mut read_blob: impl FnMut(&mut B, usize) -> T,
) -> Result<Vec<T>, DecodeError> {
    if data.remaining() < 4 {
        return Err(DecodeError::new(
            "Binary sidecar too short for count header",
        ));
    }

    let count = data.get_u32_le() as usize;

    // Sanity check: each blob needs at least 4 bytes for length header.
    // This prevents allocation of a huge Vec when count is maliciously large.
    let max_possible_blobs = data.remaining() / 4;

    if count > max_possible_blobs {
        return Err(DecodeError::new(alloc::format!(
            "Binary sidecar count ({count}) exceeds maximum possible ({max_possible_blobs})"
        )));
    }

    let mut blobs = Vec::with_capacity(count);

    for i in 0..count {
        if data.remaining() < 4 {
            return Err(DecodeError::new(alloc::format!(
                "Binary sidecar truncated at blob {i} length header"
            )));
        }

        let len = data.get_u32_le() as usize;

        if len > data.remaining() {
            return Err(DecodeError::new(alloc::format!(
                "Binary sidecar truncated at blob {i} data (need {len} bytes, have {})",
                data.remaining()
            )));
        }

        blobs.push(read_blob(&mut data, len));
    }

    // Reject trailing data — the sidecar should be fully consumed.
    // Trailing bytes could indicate a version mismatch or corruption.
    if data.has_remaining() {
        return Err(DecodeError::new(alloc::format!(
            "Binary sidecar has {} trailing bytes after all {count} blobs",
            data.remaining()
        )));
    }

    Ok(blobs)
}

/// Copies a validated blob directly into its final owned allocation.
fn copy_blob(data: &mut impl Buf, len: usize) -> Vec<u8> {
    let mut blob = Vec::with_capacity(len);

    while blob.len() < len {
        let chunk = data.chunk();
        let take = chunk.len().min(len - blob.len());
        blob.extend_from_slice(&chunk[..take]);
        data.advance(take);
    }

    blob
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

/// A tagged return value borrowing its payload from the input buffer.
#[derive(Debug, Clone, Copy)]
pub enum FnReturnRef<'a> {
    /// JSON string payload without embedded binary data.
    Json(&'a str),
    /// Raw binary payload.
    Binary(&'a [u8]),
    /// JSON with placeholders referencing a borrowed binary sidecar.
    JsonWithBinaries(&'a str, &'a [u8]),
}

/// Decodes a tagged return value from the host.
///
/// The first byte is a tag (see [`TAG_JSON`] / [`TAG_BINARY`]),
/// the rest is the payload.
pub fn decode_return(data: &[u8]) -> Result<FnReturn, DecodeError> {
    Ok(match decode_return_ref(data)? {
        FnReturnRef::Json(json) => FnReturn::Json(json.into()),
        FnReturnRef::Binary(bytes) => FnReturn::Binary(bytes.to_vec()),
        FnReturnRef::JsonWithBinaries(json, sidecar) => {
            FnReturn::JsonWithBinaries(json.into(), sidecar.to_vec())
        }
    })
}

/// Validates and borrows a tagged return without allocating payload copies.
///
/// Consumers must copy binary data before exposing it as independently mutable
/// JavaScript arrays. JSON can be parsed directly from the borrowed string.
pub fn decode_return_ref(data: &[u8]) -> Result<FnReturnRef<'_>, DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::new("Empty return payload"));
    }

    match data[0] {
        TAG_JSON => {
            let json = core::str::from_utf8(&data[1..]).map_err(|e| {
                DecodeError::new(alloc::format!("Invalid UTF-8 in JSON return: {e}"))
            })?;
            Ok(FnReturnRef::Json(json))
        }
        TAG_BINARY => Ok(FnReturnRef::Binary(&data[1..])),
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
            let sidecar = &data[5..sidecar_end];
            let json = core::str::from_utf8(&data[sidecar_end..]).map_err(|e| {
                DecodeError::new(alloc::format!(
                    "Invalid UTF-8 in JSON-with-binaries return: {e}"
                ))
            })?;
            Ok(FnReturnRef::JsonWithBinaries(json, sidecar))
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

    /// Chunked encoding retains the payload allocations and existing wire format.
    #[test]
    fn chunked_encoding_reuses_binary_allocations() {
        let mut first = Vec::with_capacity(32);
        first.extend_from_slice(b"ABC");
        let second = b"XY".to_vec();
        let first_ptr = first.as_ptr();
        let second_ptr = second.as_ptr();
        let expected = encode_binaries(&[first.as_slice(), second.as_slice()]).unwrap();
        let chunks = encode_binaries_chunks(vec![first, second]).unwrap();

        assert_eq!(chunks[2].as_ptr(), first_ptr);
        assert_eq!(chunks[4].as_ptr(), second_ptr);
        assert_eq!(chunks.concat(), expected);
        assert_eq!(
            decode_binaries(&chunks.concat()).unwrap(),
            decode_binaries(&expected).unwrap()
        );
    }

    /// Empty blobs and an empty blob list keep distinct sidecar counts.
    #[test]
    fn chunked_encoding_preserves_empty_blobs() {
        for blobs in [Vec::new(), vec![Vec::new()], vec![Vec::new(), Vec::new()]] {
            let chunks = encode_binaries_chunks(blobs.clone()).unwrap();

            assert_eq!(decode_binaries(&chunks.concat()).unwrap(), blobs);
        }
    }

    /// Every possible split, including within headers, decodes identically.
    #[test]
    fn chunked_decoding_handles_arbitrary_boundaries() {
        let blobs = vec![b"ABC".to_vec(), Vec::new(), b"XYZ".to_vec()];
        let encoded = Bytes::from(encode_binaries(&blobs).unwrap());

        for split in 0..=encoded.len() {
            let chunks = Bytes::new()
                .chain(encoded.slice(..split))
                .chain(Bytes::new())
                .chain(encoded.slice(split..))
                .chain(Bytes::new());

            assert_eq!(decode_binaries_from_buf(chunks).unwrap(), blobs);
        }
    }

    /// Truncation is rejected regardless of which chunk contains the last byte.
    #[test]
    fn chunked_decoding_rejects_truncated_sidecars() {
        let encoded = Bytes::from(encode_binaries(&[b"ABC", b"XYZ"]).unwrap());

        for end in 0..encoded.len() {
            let split = end / 2;
            let chunks = encoded
                .slice(..split)
                .chain(Bytes::new())
                .chain(encoded.slice(split..end));

            assert!(
                decode_binaries_from_buf(chunks).is_err(),
                "prefix length {end}"
            );
        }
    }

    /// Malformed counts, lengths, and trailing bytes cannot bypass validation.
    #[test]
    fn chunked_decoding_rejects_malformed_sidecars() {
        let malformed = [
            vec![255, 255, 255, 255],
            vec![1, 0, 0, 0, 255, 255, 255, 255],
            vec![2, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4],
            vec![0, 0, 0, 0, 99],
        ];

        for bytes in malformed {
            let expected = decode_binaries(&bytes).unwrap_err().to_string();
            let encoded = Bytes::from(bytes);

            for split in 0..=encoded.len() {
                let chunks = encoded
                    .slice(..split)
                    .chain(Bytes::new())
                    .chain(encoded.slice(split..));

                let error = decode_binaries_from_buf(chunks).unwrap_err();
                assert_eq!(error.to_string(), expected);
            }
        }
    }

    /// Borrowed decoding retains slices into the original sidecar allocation.
    #[test]
    fn borrowed_binaries_reuse_sidecar_storage() {
        let encoded = encode_binaries(&[b"ABC".as_slice(), b"", b"XYZ"]).unwrap();
        let blobs = decode_binaries_ref(&encoded).unwrap();

        assert_eq!(blobs, [b"ABC".as_slice(), b"", b"XYZ"]);
        assert_eq!(blobs[0].as_ptr(), encoded[8..].as_ptr());
        assert_eq!(blobs[2].as_ptr(), encoded[19..].as_ptr());
    }

    /// All tagged return variants borrow their payloads without reallocation.
    #[test]
    fn borrowed_returns_reuse_payload_storage() {
        let encoded = encode_json_return(r#"{"value":42}"#);
        let FnReturnRef::Json(json) = decode_return_ref(&encoded).unwrap() else {
            panic!("Expected JSON return");
        };

        assert_eq!(json.as_ptr(), encoded[1..].as_ptr());

        let encoded = encode_binary_return(b"ABC");
        let FnReturnRef::Binary(bytes) = decode_return_ref(&encoded).unwrap() else {
            panic!("Expected binary return");
        };

        assert_eq!(bytes, b"ABC");
        assert_eq!(bytes.as_ptr(), encoded[1..].as_ptr());

        let sidecar = encode_binaries(&[b"ABC"]).unwrap();
        let encoded = encode_json_with_binaries_return(r#"{"__bin__":0}"#, &sidecar).unwrap();
        let FnReturnRef::JsonWithBinaries(json, borrowed) = decode_return_ref(&encoded).unwrap()
        else {
            panic!("Expected JSON-with-binaries return");
        };

        assert_eq!(borrowed, sidecar);
        assert_eq!(borrowed.as_ptr(), encoded[5..].as_ptr());
        assert_eq!(json.as_ptr(), encoded[5 + sidecar.len()..].as_ptr());
    }

    /// Borrowing preserves the owned decoders' rejection of malformed input.
    #[test]
    fn borrowed_decoders_preserve_validation() {
        for bytes in [
            vec![],
            vec![255, 255, 255, 255],
            vec![1, 0, 0, 0, 255, 255, 255, 255],
            vec![0, 0, 0, 0, 99],
        ] {
            assert_eq!(
                decode_binaries_ref(&bytes).unwrap_err().to_string(),
                decode_binaries(&bytes).unwrap_err().to_string()
            );
        }

        for bytes in [
            vec![],
            vec![TAG_JSON, 255],
            vec![TAG_JSON_WITH_BINARIES, 1, 2, 3],
            vec![TAG_JSON_WITH_BINARIES, 4, 0, 0, 0],
            vec![TAG_JSON_WITH_BINARIES, 0, 0, 0, 0, 255],
            vec![255],
        ] {
            assert_eq!(
                decode_return_ref(&bytes).unwrap_err().to_string(),
                decode_return(&bytes).unwrap_err().to_string()
            );
        }
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
