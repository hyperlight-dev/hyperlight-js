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
use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::ser::SerializeSeq;
use serde::Serialize;
use serde_json::Value as JsonValue;

// Unlike hyperlight-host's Function, this Function trait uses `serde`'s Serialize and DeserializeOwned traits for input and output types.

/// A trait representing a host function that can be called from the guest JavaScript code.
///
/// This trait lets us workaround the lack of variadic generics in Rust by defining implementations
/// for tuples of different sizes.
/// The `call` method takes a single argument of type `Args`, which is expected to be a tuple
/// containing all the arguments for the function, and spreads them to the arguments n-arity when calling
/// the underlying function.
///
/// This trait has a blanket implementation for any function that takes arguments that are serde deserializable,
/// and return a serde serializable result, so you would never need to implement this trait directly.
pub trait Function<Output: Serialize, Args: DeserializeOwned> {
    fn call(&self, args: Args) -> Output;
}

// This blanket implementation allows us to implement the `Function` trait for any function that takes
// arguments that are serde deserializable, and return a serde serializable result.
impl<Output, Args, F> Function<Output, Args> for F
where
    Output: Serialize,
    Args: DeserializeOwned,
    F: fn_traits::Fn<Args, Output = Output>,
{
    fn call(&self, args: Args) -> Output {
        F::call(self, args)
    }
}

type JsonFn = std::sync::Arc<dyn Fn(String) -> crate::Result<String> + Send + Sync>;

/// Re-export the unified return type from the common crate.
pub use hyperlight_js_common::FnReturn;

/// The closure type for JS bridge host functions.
///
/// Receives the parsed JSON arguments (with `{"__bin__": N}` placeholders
/// still in place) and the decoded individual binary blobs. This avoids a
/// redundant stringify→parse round-trip that would occur if we passed a
/// pre-processed JSON string.
type BinaryFn =
    std::sync::Arc<dyn Fn(JsonValue, Vec<Vec<u8>>) -> crate::Result<FnReturn> + Send + Sync>;

/// A registered host function — either typed (serde) or JS bridge.
///
/// This enum allows a single `HashMap` to store both variants, eliminating
/// the need for parallel maps and cross-removal bookkeeping.
#[derive(Clone)]
enum HostFn {
    /// Typed: receives a JSON args string, deserializes via serde,
    /// returns a JSON result string. Does not support binary args.
    Typed(JsonFn),
    /// JS bridge: receives parsed JSON args + binary blobs, returns a
    /// tagged result (JSON or binary).
    JsBridge(BinaryFn),
}

fn type_erased<Output: Serialize, Args: DeserializeOwned>(
    func: impl Function<Output, Args> + Send + Sync + 'static,
) -> JsonFn {
    std::sync::Arc::new(move |args: String| {
        let args: Args = serde_json::from_str(&args)?;
        let output: Output = func.call(args);
        Ok(serde_json::to_string(&output)?)
    })
}

/// Decodes the sidecar binary format into individual blobs.
///
/// Thin wrapper around [`hyperlight_js_common::decode_binaries`] that maps
/// the common crate's `DecodeError` into the host's `HyperlightError`.
pub(crate) fn decode_binaries(data: &[u8]) -> crate::Result<Vec<Vec<u8>>> {
    hyperlight_js_common::decode_binaries(data)
        .map_err(|e| crate::HyperlightError::Error(e.to_string()))
}

/// A module containing host functions that can be called from the guest JavaScript code.
#[derive(Default, Clone)]
pub struct HostModule {
    functions: HashMap<String, HostFn>,
}

// The serialization of this struct has to match the deserialization in
// register_host_modules in src/hyperlight-js-runtime/src/main/hyperlight.rs
impl Serialize for HostModule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq_serializer = serializer.serialize_seq(Some(self.functions.len()))?;
        for key in self.functions.keys() {
            seq_serializer.serialize_element(key)?;
        }
        seq_serializer.end()
    }
}

impl HostModule {
    /// Register a typed host function that can be called from the guest
    /// JavaScript code.
    ///
    /// Arguments are deserialized from JSON via serde and the return value
    /// is serialized back to JSON automatically.
    ///
    /// This variant does **not** support `Uint8Array`/`Buffer` arguments.
    /// For binary data support, use the JS bridge API instead.
    ///
    /// ```text
    /// module.register("add", |a: i32, b: i32| a + b);
    /// ```
    ///
    /// Registering a function with the same `name` as an existing function
    /// overwrites the previous registration.
    pub fn register<Output: Serialize, Args: DeserializeOwned>(
        &mut self,
        name: impl Into<String>,
        func: impl Function<Output, Args> + Send + Sync + 'static,
    ) -> &mut Self {
        self.functions
            .insert(name.into(), HostFn::Typed(type_erased(func)));
        self
    }

    /// Register a host function for the JavaScript bridge (NAPI layer).
    ///
    /// This is an internal API used by the `js-host-api` NAPI bridge.
    /// Rust users should use [`register`](Self::register) instead, which
    /// handles binary data transparently via serde.
    ///
    /// The closure receives parsed `JsonValue` args and decoded binary
    /// blobs directly. Return [`FnReturn::Json`] or [`FnReturn::Binary`].
    #[doc(hidden)]
    pub fn register_js(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(JsonValue, Vec<Vec<u8>>) -> crate::Result<FnReturn> + Send + Sync + 'static,
    ) -> &mut Self {
        self.functions
            .insert(name.into(), HostFn::JsBridge(std::sync::Arc::new(func)));
        self
    }

    /// Dispatch a guest→host function call.
    ///
    /// Decodes the binary sidecar (if present) and routes to the
    /// appropriate handler variant.
    ///
    /// For `Typed` functions, binary blobs in the sidecar are rejected —
    /// use `register_js` for functions that need binary data.
    ///
    /// Always returns a tagged result:
    /// - `TAG_JSON (0x00)` + JSON bytes for JSON returns
    /// - `TAG_BINARY (0x01)` + raw bytes for binary returns
    pub(crate) fn call(
        &self,
        name: &str,
        args_json: String,
        binaries: Option<Vec<u8>>,
    ) -> crate::Result<Vec<u8>> {
        let blobs = if let Some(bin_data) = binaries {
            decode_binaries(&bin_data)?
        } else {
            Vec::new()
        };

        match self.functions.get(name) {
            Some(HostFn::JsBridge(func)) => {
                // JS bridge path: parse JSON and pass blobs directly.
                let json_value: JsonValue = serde_json::from_str(&args_json)?;
                match func(json_value, blobs)? {
                    FnReturn::Json(json) => Ok(hyperlight_js_common::encode_json_return(&json)),
                    FnReturn::Binary(bytes) => {
                        Ok(hyperlight_js_common::encode_binary_return(&bytes))
                    }
                }
            }
            Some(HostFn::Typed(func)) => {
                // Typed path: serde deserializes args from JSON. Binary
                // data is not supported — reject if blobs are present.
                if !blobs.is_empty() {
                    return Err(crate::HyperlightError::Error(format!(
                        "Function '{name}' received {} binary argument(s) but was registered \
                         with `register` (typed JSON-only). Use `register_js` for functions \
                         that accept Uint8Array/Buffer arguments.",
                        blobs.len()
                    )));
                }
                let result = func(args_json)?;
                Ok(hyperlight_js_common::encode_json_return(&result))
            }
            None => Err(crate::HyperlightError::Error(format!(
                "Function '{}' not found",
                name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_typed_no_binaries() {
        let mut module = HostModule::default();
        module.register("add", |a: i32, b: i32| a + b);

        // count=0 sidecar
        let sidecar = vec![0u8, 0, 0, 0];
        let result = module
            .call("add", "[3,4]".to_string(), Some(sidecar))
            .unwrap();
        assert_eq!(result[0], hyperlight_js_common::TAG_JSON);
        assert_eq!(&result[1..], b"7");
    }

    #[test]
    fn call_typed_rejects_binary_args() {
        let mut module = HostModule::default();
        module.register("add", |a: i32, b: i32| a + b);

        // Sidecar with one blob — typed functions should reject this
        let sidecar = hyperlight_js_common::encode_binaries(&[b"ABC" as &[u8]]);
        let err = module
            .call("add", "[1,2]".to_string(), Some(sidecar))
            .unwrap_err();
        assert!(err.to_string().contains("binary argument"));
        assert!(err.to_string().contains("register_js"));
    }

    #[test]
    fn call_not_found() {
        let module = HostModule::default();
        let err = module.call("nope", "[]".to_string(), None).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
