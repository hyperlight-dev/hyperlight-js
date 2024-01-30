use alloc::vec::Vec;

use rquickjs::{Exception, Result, Value};

/// Converts a JavaScript value to a byte vector.
/// The value can be a String, or a Uint8Array
pub fn as_bytes(key: Value) -> Result<Vec<u8>> {
    // TODO: implement ArrayBuffer, DataView and other TypedArray's

    if let Some(txt) = key.as_string() {
        return Ok(txt.to_string()?.as_bytes().to_vec());
    }

    if let Some(obj) = key.as_object()
        && let Some(array) = obj.as_typed_array::<u8>()
    {
        return Ok(array.as_bytes().unwrap().to_vec());
    };

    Err(Exception::throw_type(
        key.ctx(),
        "Expected a String or Uint8Array",
    ))
}
