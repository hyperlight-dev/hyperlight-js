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
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString as _};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::{Ref, RefCell, RefMut};
use core::ptr::NonNull;

use anyhow::{bail, ensure, Context as _};
use hashbrown::HashMap;
use hyperlight_js_common::{FnReturn, MAX_JSON_DEPTH, PLACEHOLDER_BIN};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::prelude::Rest;
use rquickjs::{Array, Ctx, Exception, Function, JsLifetime, Module, TypedArray, Value};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

/// A clone of rquickjs::Module so that we can access the ctx from it by transmuting.
struct NakedModule<'js> {
    _ptr: NonNull<rquickjs::qjs::JSModuleDef>,
    ctx: Ctx<'js>,
}

/// We will need to transmute `Declaration` / `Exports` to `Module` in the ModuleDef implementation,
/// so we need to make sure they have the same size and alignment.
/// `Declaration` / `Exports` are newtypes around a `Module`, so this should be the case, but we
/// assert it here to be sure.
/// We should be able to remove this once https://github.com/DelSkayn/rquickjs/pull/621 is merged and
/// released.
const _: () = {
    assert!(
        core::mem::size_of::<rquickjs::Module>() == core::mem::size_of::<Declarations>(),
        "Size of Module and Declarations must be the same"
    );
    assert!(
        core::mem::align_of::<rquickjs::Module>() == core::mem::align_of::<Declarations>(),
        "Alignment of Module and Declarations must be the same"
    );
    assert!(
        core::mem::size_of::<rquickjs::Module>() == core::mem::size_of::<Exports>(),
        "Size of Module and Exports must be the same"
    );
    assert!(
        core::mem::align_of::<rquickjs::Module>() == core::mem::align_of::<Exports>(),
        "Alignment of Module and Exports must be the same"
    );
    assert!(
        core::mem::size_of::<rquickjs::Module>() == core::mem::size_of::<NakedModule>(),
        "Size of Module and NakedModule must be the same"
    );
    assert!(
        core::mem::align_of::<rquickjs::Module>() == core::mem::align_of::<NakedModule>(),
        "Alignment of Module and NakedModule must be the same"
    );
};

/// A type that implements `ModuleDef` and can be used to declare and evaluate host modules.
struct HostModuleDef;

/// Rust doesn't have a great way to specify lifetimes in closures,
/// See: https://github.com/rust-lang/rust/issues/97362
/// However, Rust it can infer the lifetimes when the closure is used in a context that does
/// specify the lifetimes.
///
/// This function is used to coerce a closure so that the returned `Value<'_>` shares the same
/// lifetime as the `Ctx<'_>` argument.
/// Without this, Rust will assume that the lifetimes are independent and the returned `Value<'_>`
/// could outlive the `Ctx<'_>` argument.
fn coerce_fn_signature<F, E>(f: F) -> F
where
    F: for<'js> Fn(Ctx<'js>, Rest<Value<'js>>) -> Result<Value<'js>, E>,
{
    f
}

/// Checks if a JS value is a Uint8Array and extracts its bytes.
fn try_extract_uint8array(value: &Value<'_>) -> Option<Vec<u8>> {
    let obj = value.as_object()?;
    let typed_array = obj.as_typed_array::<u8>()?;
    typed_array.as_bytes().map(|b| b.to_vec())
}

/// Recursively processes a JS value, extracting binary data and replacing with placeholders.
/// Returns a serde_json::Value with placeholders and collects binary blobs.
fn value_to_json_with_binaries<'js>(
    ctx: &Ctx<'js>,
    value: Value<'js>,
    binaries: &mut Vec<Vec<u8>>,
    depth: usize,
) -> anyhow::Result<serde_json::Value> {
    if depth > MAX_JSON_DEPTH {
        anyhow::bail!("JSON nesting depth exceeds maximum ({MAX_JSON_DEPTH})");
    }

    // Check for Uint8Array first
    if let Some(bytes) = try_extract_uint8array(&value) {
        let index = binaries.len();
        binaries.push(bytes);
        return Ok(json!({PLACEHOLDER_BIN: index}));
    }

    // Handle null/undefined
    if value.is_null() || value.is_undefined() {
        return Ok(serde_json::Value::Null);
    }

    // Handle booleans
    if let Some(b) = value.as_bool() {
        return Ok(serde_json::Value::Bool(b));
    }

    // Handle numbers
    // QuickJS stores numbers as doubles internally but optimises small
    // integers into SMIs. We check as_int() first for integer fidelity,
    // falling back to as_float() for all other numeric values.
    // For floats that represent whole numbers (e.g. 42.0 from JSON.parse),
    // we emit them as integers to match JSON.stringify behaviour and
    // preserve serde integer deserialization on the host side.
    if let Some(n) = value.as_int() {
        return Ok(serde_json::Value::Number(n.into()));
    }
    if let Some(n) = value.as_float() {
        // Handle NaN and Infinity as null (like JSON.stringify)
        if n.is_finite() {
            // If the float is a whole number that fits in i64, emit as integer
            // to match JSON.stringify behaviour (42.0 → 42, not 42.0)
            if n == (n as i64) as f64 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                return Ok(serde_json::Value::Number((n as i64).into()));
            }
            if let Some(num) = serde_json::Number::from_f64(n) {
                return Ok(serde_json::Value::Number(num));
            }
        }
        return Ok(serde_json::Value::Null);
    }

    // Handle strings
    if let Some(s) = value.as_string() {
        let s = s.to_string()?;
        return Ok(serde_json::Value::String(s));
    }

    // Handle arrays
    if let Some(array) = value.as_array() {
        let mut json_array = Vec::with_capacity(array.len());
        for item in array.iter::<Value>() {
            let item = item?;
            json_array.push(value_to_json_with_binaries(ctx, item, binaries, depth + 1)?);
        }
        return Ok(serde_json::Value::Array(json_array));
    }

    // Handle objects
    if let Some(obj) = value.as_object() {
        // Check for toJSON() method — matches JSON.stringify behaviour
        // (e.g., Date.toJSON() returns an ISO string, not {}).
        if let Ok(to_json) = obj.get::<_, Function>("toJSON") {
            let result: Value = to_json.call((rquickjs::function::This(obj.clone()),))?;
            return value_to_json_with_binaries(ctx, result, binaries, depth + 1);
        }

        let mut json_obj = serde_json::Map::new();
        for entry in obj.props::<String, Value>() {
            let (key, val) = entry?;
            json_obj.insert(
                key,
                value_to_json_with_binaries(ctx, val, binaries, depth + 1)?,
            );
        }
        return Ok(serde_json::Value::Object(json_obj));
    }

    // Fallback: use JSON.stringify for anything else
    let json_str = ctx
        .json_stringify(value)?
        .map(|s| s.to_string())
        .transpose()?
        .unwrap_or_else(|| "null".into());
    let parsed: serde_json::Value = serde_json::from_str(&json_str)?;
    Ok(parsed)
}

/// Extracts binary data from JS arguments, replacing with placeholders.
/// Returns the JSON string with placeholders and the collected binary blobs.
fn extract_binaries<'js>(
    ctx: &Ctx<'js>,
    args: Vec<Value<'js>>,
) -> anyhow::Result<(String, Vec<Vec<u8>>)> {
    let mut binaries = Vec::new();
    let mut json_args = Vec::with_capacity(args.len());

    for arg in args {
        json_args.push(value_to_json_with_binaries(ctx, arg, &mut binaries, 0)?);
    }

    let json = serde_json::to_string(&json_args)?;
    Ok((json, binaries))
}

/// Converts a `serde_json::Value` tree into an `rquickjs::Value`, delegating
/// binary-marker resolution to `resolve_binary`.
///
/// When the traversal encounters a JSON object, it first calls
/// `resolve_binary(&obj, ctx)`. If the closure returns `Ok(Some(value))` the
/// object is replaced with that JS value (typically a `Uint8Array`). If it
/// returns `Ok(None)` the object is treated as a regular JS object.
///
/// This keeps the conversion logic in one place while allowing different
/// strategies for resolving binary markers (`__bin__` placeholder path).
fn json_to_js_value<'js, F>(
    ctx: &Ctx<'js>,
    value: serde_json::Value,
    resolve_binary: &F,
    depth: usize,
) -> anyhow::Result<Value<'js>>
where
    F: Fn(
        &serde_json::Map<String, serde_json::Value>,
        &Ctx<'js>,
    ) -> anyhow::Result<Option<Value<'js>>>,
{
    if depth > MAX_JSON_DEPTH {
        anyhow::bail!("JSON nesting depth exceeds maximum ({MAX_JSON_DEPTH})");
    }

    match value {
        serde_json::Value::Null => Ok(Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(Value::new_bool(ctx.clone(), b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64()
                && let Ok(i32_val) = i32::try_from(i)
            {
                Ok(Value::new_int(ctx.clone(), i32_val))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::new_float(ctx.clone(), f))
            } else {
                Ok(Value::new_null(ctx.clone()))
            }
        }
        serde_json::Value::String(s) => {
            let js_str = rquickjs::String::from_str(ctx.clone(), &s)?;
            Ok(js_str.into_value())
        }
        serde_json::Value::Array(arr) => {
            let js_array = Array::new(ctx.clone())?;
            for (i, item) in arr.into_iter().enumerate() {
                let js_item = json_to_js_value(ctx, item, resolve_binary, depth + 1)?;
                js_array.set(i, js_item)?;
            }
            Ok(js_array.into_value())
        }
        serde_json::Value::Object(obj) => {
            // Let the caller decide if this object is a binary marker.
            if let Some(resolved) = resolve_binary(&obj, ctx)? {
                return Ok(resolved);
            }
            // Regular object
            let js_obj = rquickjs::Object::new(ctx.clone())?;
            for (key, val) in obj {
                let js_val = json_to_js_value(ctx, val, resolve_binary, depth + 1)?;
                js_obj.set(&key, js_val)?;
            }
            Ok(js_obj.into_value())
        }
    }
}

/// Converts a serde_json Value to a rquickjs Value without any
/// binary marker resolution (plain JSON objects pass through as-is).
fn json_to_plain_value<'js>(
    ctx: &Ctx<'js>,
    value: serde_json::Value,
    depth: usize,
) -> anyhow::Result<Value<'js>> {
    json_to_js_value(ctx, value, &|_obj, _ctx| Ok(None), depth)
}

/// Converts a serde_json Value to a rquickjs Value, resolving `{"__bin__": N}`
/// placeholders from the provided binary blobs (optimised sidecar path).
fn json_to_value_with_blobs<'js>(
    ctx: &Ctx<'js>,
    value: serde_json::Value,
    blobs: &[Vec<u8>],
    depth: usize,
) -> anyhow::Result<Value<'js>> {
    json_to_js_value(
        ctx,
        value,
        &|obj: &serde_json::Map<String, serde_json::Value>, ctx: &Ctx<'js>| {
            // Check for {"__bin__": N} placeholder
            if obj.len() == 1
                && let Some(serde_json::Value::Number(n)) = obj.get(PLACEHOLDER_BIN)
                && let Some(idx) = n.as_u64()
            {
                let idx = idx as usize;
                if idx < blobs.len() {
                    let array = TypedArray::<u8>::new(ctx.clone(), blobs[idx].clone())?;
                    return Ok(Some(array.into_value()));
                }
                anyhow::bail!(
                    "Binary placeholder index {idx} out of bounds (have {} blobs)",
                    blobs.len()
                );
            }
            Ok(None)
        },
        depth,
    )
}

/// A `ModuleDef` implementation that can be used to declare and evaluate host modules.
/// This module will look up the module name in the ctx userdata and declare/evaluate
/// the functions in the module accordingly.
impl ModuleDef for HostModuleDef {
    /// Declare the functions in the module.
    /// This is called immediately when we create the `Module` with `Module::declare_def`, and is used to
    /// declare the functions that the module exports.
    fn declare<'js>(decl: &Declarations<'js>) -> rquickjs::Result<()> {
        // these transmutes should be ok as we have asserted the sizes above
        // and we have tests to check this works as expected
        let module: &Module = unsafe { core::mem::transmute(decl) };
        let naked_module: &NakedModule = unsafe { core::mem::transmute(module) };
        let module_name: String = module.name()?;
        let ctx = &naked_module.ctx;

        // We don't have access to self in this function, so we can't pass rich data to this function.
        // Instead, we use a userdata in the context to get the list of functions to declare.
        let Some(loader) = ctx.userdata::<HostModuleLoader>() else {
            return Err(Exception::throw_internal(ctx, "HostModuleLoader not found"));
        };
        let modules = loader.modules.borrow();

        let Some(module) = modules.get(&module_name) else {
            return Ok(());
        };

        for (name, _) in module.functions.iter() {
            decl.declare(name.as_str())?;
        }

        Ok(())
    }

    /// Evaluate the module.
    /// This is called when the module evaluated, usually when it is imported for the first time,
    /// and is used to assign values to the declared exports.
    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
        // this transmute should be ok as we have asserted the sizes above
        // and we have tests to check this works as expected
        let module: &Module = unsafe { core::mem::transmute(exports) };
        let module_name: String = module.name()?;

        // We don't have access to self in this function, so we can't pass rich data to this function.
        // Instead, we use a userdata in the context to get the list of functions to export.
        let Some(loader) = ctx.userdata::<HostModuleLoader>() else {
            return Err(Exception::throw_internal(ctx, "HostModuleLoader not found"));
        };
        let modules = loader.modules.borrow();

        let Some(module) = modules.get(&module_name) else {
            return Ok(());
        };

        for (name, func) in module.functions.iter() {
            let func = func.clone();
            let func = coerce_fn_signature(move |ctx, args| func.call(&ctx, args));
            let func = Function::new(ctx.clone(), func)?.with_name(name)?;
            exports.export(name.as_str(), func)?;
        }

        Ok(())
    }
}

/// A host function that can be called from JavaScript. This is a wrapper around a Rust closure that
/// can be called from JavaScript.
///
/// The main purpose of this wrapper is that we can construct them from different types of closures,
/// and also handles the error conversion from `anyhow::Error` to `rquickjs::Error` in a consistent way.
#[derive(Clone)]
pub struct HostFunction {
    #[allow(clippy::type_complexity)]
    func: Arc<dyn for<'js> Fn(&Ctx<'js>, Rest<Value<'js>>) -> rquickjs::Result<Value<'js>>>,
}

impl HostFunction {
    /// Create a new `HostFunction` from a closure using rquickjs types directly.
    ///
    /// This is the most performant version of `HostFunction`, but requires interacting with
    /// rquickjs types directly.
    pub fn new(
        func: impl for<'js> Fn(&Ctx<'js>, Rest<Value<'js>>) -> anyhow::Result<Value<'js>> + 'static,
    ) -> Self {
        Self {
            func: Arc::new(
                move |ctx: &Ctx, args: Rest<Value>| -> rquickjs::Result<Value> {
                    func(ctx, args).map_err(|e| match e.downcast::<rquickjs::Error>() {
                        Ok(e) => e,
                        Err(e) => {
                            // Use Display chain ({e:#}) instead of Debug struct
                            // ({e:#?}) to keep the message compact and avoid
                            // truncation at the hyperlight guest↔host boundary.
                            Exception::throw_internal(ctx, &format!("Host function error: {e:#}"))
                        }
                    })
                },
            ),
        }
    }

    /// Create a new `HostFunction` from a closure that takes and returns JSON serialized strings.
    ///
    /// This is useful for hyperlight, where we use JSON as the serialization format for communication
    /// with the host.
    ///
    /// **Note:** This variant does not support `Uint8Array`/`Buffer` arguments —
    /// QuickJS's `JSON.stringify` will serialize them as plain objects with numeric
    /// keys (e.g. `{ "0": 1, "1": 2 }`), not as binary blobs. Use [`new_bin`](Self::new_bin)
    /// for functions that handle binary data.
    pub fn new_json(func: impl Fn(String) -> anyhow::Result<String> + 'static) -> Self {
        Self::new(
            move |ctx: &Ctx, args: Rest<Value>| -> anyhow::Result<Value> {
                let args = ctx
                    .json_stringify(args.into_inner())?
                    .map(|s| s.to_string())
                    .transpose()?
                    .context("Serializing host function arguments")?;
                let res = func(args).context("Calling host function")?;
                ctx.json_parse(res).context("Parsing host function result")
            },
        )
    }

    /// Create a new `HostFunction` from a closure that supports binary data.
    ///
    /// This variant detects `Uint8Array` arguments and passes them
    /// through a sidecar binary channel instead of JSON-encoding them. The JSON
    /// contains `{"__bin__": N}` placeholders that reference the sidecar blobs.
    ///
    /// The closure receives:
    /// - `args_json`: JSON string with placeholders for binary arguments
    /// - `binaries`: Packed binary sidecar (length-prefixed format)
    ///
    /// The closure returns a tagged result:
    /// - `0x00` + JSON = JSON return value
    /// - `0x01` + bytes = raw binary return (becomes `Uint8Array` on JS side)
    pub fn new_bin(func: impl Fn(String, Vec<u8>) -> anyhow::Result<Vec<u8>> + 'static) -> Self {
        Self::new(
            move |ctx: &Ctx, args: Rest<Value>| -> anyhow::Result<Value> {
                // Extract binary blobs and replace with placeholders
                let (json_args, binaries) = extract_binaries(ctx, args.into_inner())?;

                // Encode binaries into sidecar format — encode_binaries
                // accepts &[Vec<u8>] directly, no intermediate Vec<&[u8]> needed
                let packed = hyperlight_js_common::encode_binaries(&binaries)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                // Call the host function
                let result = func(json_args, packed).context("Calling binary host function")?;

                // Decode the tagged return value
                match hyperlight_js_common::decode_return(&result)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                {
                    FnReturn::Json(json) => {
                        // Plain JSON return — no binary markers to resolve.
                        let json_value: serde_json::Value =
                            serde_json::from_str(&json).context("Parsing JSON return from host")?;
                        json_to_plain_value(ctx, json_value, 0)
                    }
                    FnReturn::JsonWithBinaries(json, sidecar) => {
                        // Optimised sidecar path: decode blobs and resolve
                        // {"__bin__": N} placeholders by index lookup.
                        let blobs = hyperlight_js_common::decode_binaries(&sidecar)
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                        let json_value: serde_json::Value = serde_json::from_str(&json)
                            .context("Parsing JSON-with-binaries return from host")?;
                        json_to_value_with_blobs(ctx, json_value, &blobs, 0)
                    }
                    FnReturn::Binary(data) => {
                        // Create a Uint8Array from the binary data
                        let array = TypedArray::<u8>::new(ctx.clone(), data)?;
                        Ok(array.into_value())
                    }
                }
            },
        )
    }

    /// Create a new `HostFunction` from a closure that takes and returns any type that can be
    /// serialized by serde.
    ///
    /// This is a more convenient way to write host functions for the native binary, as we can work
    /// with Rust types directly.
    ///
    /// Currently this goes through a round of JSON serialization/deserialization, but in the
    /// future we could optimize this by using rquickjs-serde when it adds no_std support.
    pub fn new_serde<Args: DeserializeOwned, Output: Serialize>(
        func: impl fn_traits::Fn<Args, Output = anyhow::Result<Output>> + 'static,
    ) -> Self {
        // TODO(jprendes): can we use rquickjs-serde to avoid the
        // serialization/deserialization cycle here?
        Self::new_json(move |args: String| -> anyhow::Result<String> {
            let args: Args =
                serde_json::from_str(&args).context("Deserializing arguments for host function")?;
            let output: Output = func.call(args)?;
            let output =
                serde_json::to_string(&output).context("Serializing output of host function")?;
            Ok(output)
        })
    }

    pub fn call<'js>(
        &self,
        ctx: &Ctx<'js>,
        args: Rest<Value<'js>>,
    ) -> rquickjs::Result<Value<'js>> {
        (self.func)(ctx, args)
    }
}

/// A host module that can be imported from JavaScript. This is a collection of `HostFunction`s that
/// can be imported as a module from JavaScript.
#[derive(Default, JsLifetime)]
pub struct HostModule {
    functions: HashMap<String, HostFunction>,
}

impl HostModule {
    /// Add a function to the host module.
    pub fn add_function(&mut self, name: impl Into<String>, func: HostFunction) -> &mut Self {
        self.functions.insert(name.into(), func);
        self
    }
}

/// A module loader that can load host modules. This is used to load the host modules when they are
/// imported from JavaScript.
/// This struct should be stored as a userdata in the context before using the loader, which can be done
/// with the `install` function.
/// The modules instantiated by this loader will use the `HostModuleDef` type for its definition, which
/// uses the `HostModuleLoader` userdata in the context to find the function to declare and evaluate the
/// module.
#[derive(Clone, Default, JsLifetime)]
pub struct HostModuleLoader {
    modules: Rc<RefCell<HashMap<String, HostModule>>>,
}

impl Resolver for HostModuleLoader {
    fn resolve(
        &mut self,
        _ctx: &Ctx<'_>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'_>>,
    ) -> rquickjs::Result<String> {
        if !self.borrow().contains_key(name) {
            return Err(rquickjs::Error::new_resolving(base, name));
        }
        Ok(name.to_string())
    }
}

impl Loader for HostModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        if !self.borrow().contains_key(name) {
            return Err(rquickjs::Error::new_loading(name));
        }
        Module::declare_def::<HostModuleDef, _>(ctx.clone(), name)
    }
}

impl HostModuleLoader {
    pub(crate) fn install(&self, ctx: &Ctx) -> anyhow::Result<()> {
        ensure!(
            ctx.userdata::<Self>().is_none(),
            "HostModuleLoader is already installed"
        );
        let Ok(None) = ctx.store_userdata(self.clone()) else {
            bail!("Failed to install HostModuleLoader");
        };
        Ok(())
    }

    pub(crate) fn borrow(&self) -> Ref<'_, HashMap<String, HostModule>> {
        self.modules.borrow()
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, HashMap<String, HostModule>> {
        self.modules.borrow_mut()
    }
}
