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
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString as _};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::ModuleDef;
use rquickjs::{Ctx, Module, Result};
use spin::{LazyLock, Mutex};

#[doc(hidden)]
pub mod console;
#[doc(hidden)]
pub mod crypto;
#[doc(hidden)]
pub mod io;
#[doc(hidden)]
pub mod require;

/// A function pointer type for declaring a native module.
#[doc(hidden)]
pub type ModuleDeclarationFn = for<'js> fn(Ctx<'js>, &str) -> Result<Module<'js>>;

/// Returns a function pointer that declares a module of type `M`.
#[doc(hidden)]
pub fn declaration<M: ModuleDef>() -> ModuleDeclarationFn {
    fn declare<'js, M: ModuleDef>(ctx: Ctx<'js>, name: &str) -> Result<Module<'js>> {
        Module::declare_def::<M, _>(ctx, name)
    }
    declare::<M>
}

// A `BTreeMap` (rather than a hash map) keeps the module registries in a
// deterministic, seed-independent iteration order, so `builtin_module_names()`
// and module resolution behave identically across runs and builds.
static BUILTIN_MODULES: LazyLock<BTreeMap<&str, ModuleDeclarationFn>> = LazyLock::new(|| {
    BTreeMap::from([
        ("io", declaration::<io::js_io>()),
        ("crypto", declaration::<crypto::js_crypto>()),
        ("console", declaration::<console::js_console>()),
        ("require", declaration::<require::js_require>()),
    ])
});

/// Returns the names of all built-in native modules.
pub fn builtin_module_names() -> alloc::vec::Vec<&'static str> {
    BUILTIN_MODULES.keys().copied().collect()
}

// ── Custom module registry ─────────────────────────────────────────────────
//
// Extender crates register their custom native modules here via
// `register_native_module`. The NativeModuleLoader checks this registry
// first, then falls back to the built-in modules.

static CUSTOM_MODULES: LazyLock<Mutex<BTreeMap<&'static str, ModuleDeclarationFn>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Register a custom native module by name.
///
/// The module will be available to JavaScript via `import { ... } from "name"`.
/// Custom modules take priority over built-in modules with the same name,
/// allowing extender crates to replace built-ins (e.g. `io`, `crypto`,
/// `console`) with custom implementations.
///
/// The `require` module cannot be overridden — it is part of the runtime's
/// core module loading infrastructure.
///
/// This is typically called via the [`native_modules!`] macro rather than
/// directly.
///
/// # Panics
///
/// Panics if `name` is `"require"`.
pub fn register_native_module(name: &'static str, decl: ModuleDeclarationFn) {
    if name == "require" {
        panic!("Cannot override the 'require' module — it is part of the runtime's core infrastructure");
    }
    CUSTOM_MODULES.lock().insert(name, decl);
}

// Flag to ensure custom modules are initialised before the loader is used.
// The init_native_modules symbol is provided by the binary crate via the
// native_modules! macro. We call it lazily on first loader access so that
// neither the native CLI nor extender binaries need to call it explicitly.
static CUSTOM_MODULES_INIT: spin::Once = spin::Once::new();

fn ensure_custom_modules_init() {
    CUSTOM_MODULES_INIT.call_once(|| {
        unsafe extern "Rust" {
            fn init_native_modules();
        }
        unsafe { init_native_modules() };
    });
}

// ── NativeModuleLoader ─────────────────────────────────────────────────────

/// The unified loader for all native (Rust-implemented) modules.
///
/// Checks the custom module registry first (populated via
/// [`register_native_module`] or [`native_modules!`]), then falls back to
/// the built-in modules (io, crypto, console, require).
#[derive(Clone)]
pub struct NativeModuleLoader;

impl Resolver for NativeModuleLoader {
    fn resolve(
        &mut self,
        _ctx: &Ctx<'_>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'_>>,
    ) -> Result<String> {
        ensure_custom_modules_init();
        // Copy result out before dropping the lock to avoid holding it during resolution
        let found_custom = CUSTOM_MODULES.lock().contains_key(name);
        if found_custom || BUILTIN_MODULES.contains_key(name) {
            Ok(name.to_string())
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

impl Loader for NativeModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js>> {
        ensure_custom_modules_init();
        // Copy the fn pointer out while holding the lock, then drop the guard
        // before calling the declaration. This avoids deadlock if decl()
        // triggers a nested module load that tries to lock CUSTOM_MODULES
        // (spin::Mutex is not re-entrant).
        let custom_decl = CUSTOM_MODULES.lock().get(name).copied();
        if let Some(decl) = custom_decl {
            return decl(ctx.clone(), name);
        }
        // Fall back to built-in modules
        if let Some(decl) = BUILTIN_MODULES.get(name) {
            return decl(ctx.clone(), name);
        }
        Err(rquickjs::Error::new_loading(name))
    }
}

/// Register custom native modules and generate the `init_native_modules`
/// entry point that the hyperlight guest calls during startup.
///
/// # Example
///
/// ```rust,ignore
/// #[rquickjs::module(rename_vars = "camelCase")]
/// mod math {
///     #[rquickjs::function]
///     pub fn add(a: f64, b: f64) -> f64 { a + b }
/// }
///
/// hyperlight_js_runtime::native_modules! {
///     "math" => js_math,
/// }
/// ```
///
/// Custom modules take priority over built-in modules with the same name,
/// allowing extender crates to replace built-ins with custom implementations.
#[macro_export]
macro_rules! native_modules {
    ($($name:expr => $module:ty),* $(,)?) => {
        /// Called by the hyperlight guest entry point to register custom
        /// native modules before the JS runtime is initialised.
        #[unsafe(no_mangle)]
        pub fn init_native_modules() {
            $(
                $crate::modules::register_native_module(
                    $name,
                    $crate::modules::declaration::<$module>(),
                );
            )*
        }
    };
}

// ── Custom globals ─────────────────────────────────────────────────────────

/// Call the extender crate's custom globals setup function.
///
/// The `init_custom_globals` symbol is generated by the [`custom_globals!`]
/// macro. We call it once during `JsRuntime::new()`, after the built-in
/// globals (console, require, print, etc.) have been set up.
pub fn setup_custom_globals(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    unsafe extern "Rust" {
        fn init_custom_globals(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()>;
    }
    // SAFETY: init_custom_globals is generated by the custom_globals! macro.
    // Every binary that links hyperlight-js-runtime must invoke the macro
    // (even if empty) to provide this symbol — same pattern as native_modules!.
    unsafe { init_custom_globals(ctx) }
}

/// Register custom global setup functions for the JS runtime.
///
/// Custom globals are installed after built-in globals (console, require, etc.)
/// during `JsRuntime::new()`. Each setup function receives the QuickJS `Ctx`
/// and can register constructors, objects, or values on `ctx.globals()`.
///
/// Supports both Rust-implemented globals (via rquickjs class/function
/// attributes) and JavaScript polyfills (via `ctx.eval()`).
///
/// # Example — Rust class
///
/// ```rust,ignore
/// #[rquickjs::class]
/// #[derive(Trace, JsLifetime)]
/// pub struct TextEncoder {}
///
/// #[rquickjs::methods]
/// impl TextEncoder {
///     #[qjs(constructor)]
///     pub fn new() -> Self { TextEncoder {} }
///     pub fn encode<'js>(&self, ctx: Ctx<'js>, input: String)
///         -> rquickjs::Result<TypedArray<'js, u8>> {
///         TypedArray::new(ctx, input.into_bytes())
///     }
/// }
///
/// fn setup_text_encoding(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
///     // `Class::define` installs the constructor on the target object under
///     // the class's name ("TextEncoder") so handlers can `new TextEncoder()`.
///     rquickjs::Class::<TextEncoder>::define(&ctx.globals())?;
///     Ok(())
/// }
///
/// hyperlight_js_runtime::custom_globals! {
///     setup_text_encoding,
/// }
/// ```
///
/// # Example — JavaScript polyfill
///
/// ```rust,ignore
/// fn setup_polyfills(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
///     ctx.eval::<(), _>(r#"
///         globalThis.MY_CONSTANT = 42;
///     "#)?;
///     Ok(())
/// }
///
/// hyperlight_js_runtime::custom_globals! {
///     setup_polyfills,
/// }
/// ```
#[macro_export]
macro_rules! custom_globals {
    ($($setup_fn:expr),* $(,)?) => {
        /// Called by the hyperlight runtime to register custom globals
        /// after built-in globals are set up.
        #[unsafe(no_mangle)]
        pub fn init_custom_globals(
            ctx: &$crate::rquickjs::Ctx<'_>,
        ) -> $crate::rquickjs::Result<()> {
            $( ($setup_fn)(ctx)?; )*
            Ok(())
        }
    };
}
