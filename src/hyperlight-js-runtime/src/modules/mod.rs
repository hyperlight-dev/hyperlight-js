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
use alloc::string::{String, ToString as _};

use hashbrown::HashMap;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::module::ModuleDef;
use rquickjs::{Ctx, Module, Result};
use spin::{Lazy, Mutex};

pub(crate) mod console;
pub(crate) mod crypto;
pub(crate) mod io;
pub(crate) mod require;

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

// ── Built-in modules ───────────────────────────────────────────────────────

static BUILTIN_MODULES: Lazy<HashMap<&str, ModuleDeclarationFn>> = Lazy::new(|| {
    HashMap::from([
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

static CUSTOM_MODULES: Lazy<Mutex<HashMap<&'static str, ModuleDeclarationFn>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Register a custom native module by name.
///
/// The module will be available to JavaScript via `import { ... } from "name"`.
/// Custom modules cannot shadow built-in modules (io, crypto, console, require).
///
/// This is typically called via the [`native_modules!`] macro rather than
/// directly.
///
/// # Panics
///
/// Panics if `name` collides with a built-in module name.
pub fn register_native_module(name: &'static str, decl: ModuleDeclarationFn) {
    if BUILTIN_MODULES.contains_key(name) {
        panic!(
            "Cannot register custom native module '{name}': name conflicts with a built-in module"
        );
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
    fn resolve(&mut self, _ctx: &Ctx<'_>, base: &str, name: &str) -> Result<String> {
        ensure_custom_modules_init();
        if CUSTOM_MODULES.lock().contains_key(name) || BUILTIN_MODULES.contains_key(name) {
            Ok(name.to_string())
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

impl Loader for NativeModuleLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
        ensure_custom_modules_init();
        // Check custom modules first
        if let Some(decl) = CUSTOM_MODULES.lock().get(name) {
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
/// Custom module names **cannot** shadow built-in modules (`io`, `crypto`,
/// `console`, `require`). Attempting to do so will panic at startup.
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
