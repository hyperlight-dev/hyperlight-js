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
use spin::Lazy;

pub mod console;
pub mod crypto;
pub mod io;
pub mod require;

// A loader for native Rust modules
#[derive(Clone)]
pub struct NativeModuleLoader;

/// A function pointer type for declaring a module.
type ModuleDeclarationFn = for<'js> fn(Ctx<'js>, &str) -> Result<Module<'js>>;

/// This function returns a function pointer that when called declares a module
/// of type M.
/// Doing `declaration::<M>()(ctx, "some_name")` is technically the same as
/// doing `Module::declare_def::<M>(ctx, "some_name")`.
/// However, if we try to get a function pointer from `Module::declare_def::<M>` directly,
/// we get issues due to lifetime conflicts. This function works around that conflict
/// by explicitly defining the lifetimes and returning a function pointer with the correct signature.
fn declaration<M: ModuleDef>() -> ModuleDeclarationFn {
    fn declare<'js, M: ModuleDef>(ctx: Ctx<'js>, name: &str) -> Result<Module<'js>> {
        Module::declare_def::<M, _>(ctx, name)
    }
    declare::<M>
}

static NATIVE_MODULES: Lazy<HashMap<&str, ModuleDeclarationFn>> = Lazy::new(|| {
    HashMap::from([
        ("io", declaration::<io::js_io>()),
        ("crypto", declaration::<crypto::js_crypto>()),
        ("console", declaration::<console::js_console>()),
        ("require", declaration::<require::js_require>()),
    ])
});

impl Resolver for NativeModuleLoader {
    fn resolve(&mut self, _ctx: &Ctx<'_>, base: &str, name: &str) -> Result<String> {
        if NATIVE_MODULES.contains_key(name) {
            Ok(name.to_string())
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

impl Loader for NativeModuleLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
        if let Some(declaration) = NATIVE_MODULES.get(name) {
            declaration(ctx.clone(), name)
        } else {
            Err(rquickjs::Error::new_loading(name))
        }
    }
}
