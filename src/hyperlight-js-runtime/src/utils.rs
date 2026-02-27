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
