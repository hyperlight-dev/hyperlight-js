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

use super::snapshot::HostFunctionManifest;

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

type JsonFn = Box<dyn Fn(String) -> crate::Result<String> + Send + Sync>;

/// Re-export the unified return type from the common crate.
pub use hyperlight_js_common::FnReturn;

/// Expose a logical host-function manifest to the guest module loader.
pub(crate) fn register_host_function_manifest(
    sandbox: &mut hyperlight_host::MultiUseSandbox,
    manifest: &HostFunctionManifest,
) -> crate::Result<()> {
    let manifest_json = serde_json::to_string(manifest)?;
    sandbox.call("RegisterHostModules", manifest_json)
}

/// Return the registered host functions grouped by module.
pub(crate) fn host_function_manifest(
    modules: &HashMap<String, HostModule>,
) -> HostFunctionManifest {
    modules
        .iter()
        .map(|(module, functions)| {
            let mut functions = functions
                .function_names()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            functions.sort();
            (module.clone(), functions)
        })
        .collect()
}

/// Build the single Hyperlight callback that dispatches logical host functions.
pub(crate) fn host_function_dispatch(
    modules: HashMap<String, HostModule>,
) -> impl Fn(String, String, String, Vec<u8>) -> crate::Result<Vec<u8>> + Send + Sync + 'static {
    move |module_name, function_name, args_json, binaries| {
        let module = modules
            .get(&module_name)
            .ok_or_else(|| crate::new_error!("Host module '{}' not found", module_name))?;
        module
            .call(&function_name, args_json, Some(binaries))
            .map_err(|error| {
                crate::new_error!(
                    "Error calling host function '{}' in module '{}': {}",
                    function_name,
                    module_name,
                    error
                )
            })
    }
}

/// Selects host functions to install when building a sandbox.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HostFunctionFilter {
    /// Make every supplied host function available to the sandbox.
    ///
    /// Restoring a loaded snapshot preserves its captured requirements. Extra
    /// supplied functions become visible only after unloading back to a
    /// [`crate::JSSandbox`] and loading newly staged handlers.
    #[default]
    All,
    /// Install only functions declared by the snapshot requirements.
    ///
    /// Extra supplied functions are discarded and do not become available after
    /// unload. This filter is valid only with
    /// [`crate::SandboxBuilder::build_from_snapshot`].
    SnapshotRequirements,
}

impl HostFunctionFilter {
    /// Make every supplied host function available to the sandbox lifecycle.
    pub const fn all() -> Self {
        Self::All
    }

    /// Make only functions required by the restored snapshot available.
    pub const fn snapshot_requirements() -> Self {
        Self::SnapshotRequirements
    }
}

/// The closure type for JS bridge host functions.
///
/// Receives the parsed JSON arguments (with `{"__bin__": N}` placeholders
/// still in place) and the decoded individual binary blobs. This avoids a
/// redundant stringify→parse round-trip that would occur if we passed a
/// pre-processed JSON string.
type BinaryFn = Box<dyn Fn(JsonValue, Vec<Vec<u8>>) -> crate::Result<FnReturn> + Send + Sync>;

/// A registered host function — either typed (serde) or JS bridge.
///
/// This enum allows a single `HashMap` to store both variants, eliminating
/// the need for parallel maps and cross-removal bookkeeping.
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
    Box::new(move |args: String| {
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
#[derive(Default)]
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
    pub(crate) fn function_names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }

    fn filtered(mut self, names: &[String]) -> Self {
        self.functions.retain(|name, _| names.contains(name));
        self
    }

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
            .insert(name.into(), HostFn::JsBridge(Box::new(func)));
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
                    FnReturn::JsonWithBinaries(json, sidecar) => {
                        hyperlight_js_common::encode_json_with_binaries_return(&json, &sidecar)
                            .map_err(|e| crate::HyperlightError::Error(e.to_string()))
                    }
                }
            }
            Some(HostFn::Typed(func)) => {
                // Typed path: serde deserializes args from JSON. Binary
                // data is not supported — reject if blobs are present.
                if !blobs.is_empty() {
                    return Err(crate::HyperlightError::Error(format!(
                        concat!(
                            "Function '{}' received {} binary argument(s) but was registered ",
                            "with `register` (typed JSON-only). Use `register_js` for functions ",
                            "that accept Uint8Array/Buffer arguments.",
                        ),
                        name,
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

/// A named collection of host functions exposed to one JavaScript sandbox.
///
/// Host-function modules execute in the host process through
/// `CallHostJsFunction`. They are distinct from native modules compiled into the
/// guest runtime and JavaScript source modules executed inside the guest. Define
/// a constructor function when multiple sandboxes need equivalent modules; each
/// sandbox must receive a newly constructed module and callback implementation.
pub struct HostFunctionModule {
    name: String,
    module: HostModule,
}

impl HostFunctionModule {
    /// Create an empty host-function module with the supplied import name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: HostModule::default(),
        }
    }

    /// Return the exact module name visible to guest JavaScript.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Register a typed host function on this module.
    pub fn register<Output: Serialize, Args: DeserializeOwned>(
        &mut self,
        name: impl Into<String>,
        func: impl Function<Output, Args> + Send + Sync + 'static,
    ) -> &mut Self {
        self.module.register(name, func);
        self
    }

    /// Register a JavaScript-bridge host function on this module.
    #[doc(hidden)]
    pub fn register_js(
        &mut self,
        name: impl Into<String>,
        func: impl Fn(JsonValue, Vec<Vec<u8>>) -> crate::Result<FnReturn> + Send + Sync + 'static,
    ) -> &mut Self {
        self.module.register_js(name, func);
        self
    }
}

impl std::fmt::Debug for HostFunctionModule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostFunctionModule")
            .field("name", &self.name)
            .field(
                "functions",
                &self.module.function_names().collect::<Vec<_>>(),
            )
            .finish()
    }
}

pub(crate) fn select_host_function_modules(
    modules: Vec<HostFunctionModule>,
    filter: HostFunctionFilter,
    requirements: Option<&HostFunctionManifest>,
) -> crate::Result<HashMap<String, HostModule>> {
    if filter == HostFunctionFilter::SnapshotRequirements && requirements.is_none() {
        return Err(crate::new_error!(
            "HostFunctionFilter::SnapshotRequirements requires build_from_snapshot()"
        ));
    }

    let mut selected = HashMap::with_capacity(modules.len());
    for module in modules {
        let HostFunctionModule { name, module } = module;
        if selected.contains_key(&name) {
            return Err(crate::new_error!(
                "Duplicate host-function module '{}'",
                name
            ));
        }

        let functions = match filter {
            HostFunctionFilter::All => module,
            HostFunctionFilter::SnapshotRequirements => {
                let Some(requirements) = requirements else {
                    return Err(crate::new_error!(
                        "HostFunctionFilter::SnapshotRequirements requires build_from_snapshot()"
                    ));
                };
                let Some(names) = requirements.get(&name) else {
                    continue;
                };
                module.filtered(names)
            }
        };
        selected.insert(name, functions);
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn stateful_modules() -> Vec<HostFunctionModule> {
        let calls = AtomicUsize::new(0);
        let mut module = HostFunctionModule::new("state");
        module.register("next", move || calls.fetch_add(1, Ordering::Relaxed) + 1);
        vec![module]
    }

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
    fn modules_select_all_functions() {
        let mut module = HostFunctionModule::new("math");
        module
            .register("add", |a: i32, b: i32| a + b)
            .register("multiply", |a: i32, b: i32| a * b);

        let selected =
            select_host_function_modules(vec![module], HostFunctionFilter::All, None).unwrap();

        assert_eq!(
            host_function_manifest(&selected).get("math"),
            Some(&vec!["add".to_owned(), "multiply".to_owned()])
        );
    }

    #[test]
    fn repeated_factory_calls_create_independent_callback_state() {
        let first = select_host_function_modules(stateful_modules(), HostFunctionFilter::All, None)
            .unwrap()
            .remove("state")
            .unwrap();
        let second =
            select_host_function_modules(stateful_modules(), HostFunctionFilter::All, None)
                .unwrap()
                .remove("state")
                .unwrap();

        let first_call = hyperlight_js_common::encode_json_return("1");
        let second_call = hyperlight_js_common::encode_json_return("2");

        assert_eq!(
            first.call("next", "null".to_owned(), None).unwrap(),
            first_call
        );
        assert_eq!(
            first.call("next", "null".to_owned(), None).unwrap(),
            second_call
        );
        assert_eq!(
            second.call("next", "null".to_owned(), None).unwrap(),
            first_call
        );
    }

    #[test]
    fn modules_select_snapshot_requirements() {
        let mut module = HostFunctionModule::new("math");
        module
            .register("add", |a: i32, b: i32| a + b)
            .register("multiply", |a: i32, b: i32| a * b);
        let requirements =
            HostFunctionManifest::from([("math".to_owned(), vec!["multiply".to_owned()])]);

        let selected = select_host_function_modules(
            vec![module],
            HostFunctionFilter::SnapshotRequirements,
            Some(&requirements),
        )
        .unwrap();

        assert_eq!(
            host_function_manifest(&selected).get("math"),
            Some(&vec!["multiply".to_owned()])
        );
    }

    #[test]
    fn snapshot_filter_rejects_fresh_builds() {
        let result = select_host_function_modules(
            Vec::new(),
            HostFunctionFilter::SnapshotRequirements,
            None,
        );
        let error = match result {
            Ok(_) => panic!("snapshot filtering unexpectedly accepted a fresh build"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("requires build_from_snapshot()"));
    }

    #[test]
    fn duplicate_module_names_are_rejected() {
        let modules = vec![
            HostFunctionModule::new("math"),
            HostFunctionModule::new("math"),
        ];

        let result = select_host_function_modules(modules, HostFunctionFilter::All, None);
        let error = match result {
            Ok(_) => panic!("duplicate host-function modules were unexpectedly accepted"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("Duplicate host-function module 'math'"));
    }

    #[test]
    fn call_typed_rejects_binary_args() {
        let mut module = HostModule::default();
        module.register("add", |a: i32, b: i32| a + b);

        // Sidecar with one blob — typed functions should reject this
        let sidecar = hyperlight_js_common::encode_binaries(&[b"ABC" as &[u8]]).unwrap();
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

    #[test]
    fn js_bridge_passes_through_bin_key_without_sidecar() {
        // If a host function returns JSON containing {"__bin__": 0} but
        // with an empty sidecar (FnReturn::Json), the key must pass through
        // as a regular JSON object — it should NOT be treated as a binary
        // placeholder because there is no sidecar to resolve against.
        let mut module = HostModule::default();
        module.register_js("echo", |args, _blobs| Ok(FnReturn::Json(args.to_string())));

        // Args contain an object with the reserved key but no actual binary
        let args = r#"[{"__bin__": 0}]"#.to_string();
        let sidecar = vec![0u8, 0, 0, 0]; // count=0, no blobs
        let result = module.call("echo", args.clone(), Some(sidecar)).unwrap();
        assert_eq!(result[0], hyperlight_js_common::TAG_JSON);
        // The returned JSON should contain the __bin__ key as-is
        let returned_json = std::str::from_utf8(&result[1..]).unwrap();
        assert!(returned_json.contains("__bin__"));
    }
}
