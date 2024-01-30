use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString as _};
use alloc::sync::Arc;
use core::cell::{Ref, RefCell, RefMut};
use core::ptr::NonNull;

use anyhow::{bail, ensure, Context as _};
use hashbrown::HashMap;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::prelude::Rest;
use rquickjs::{Ctx, Exception, Function, JsLifetime, Module, Value};
use serde::de::DeserializeOwned;
use serde::Serialize;

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

        // We don't have access to self in this function, so we can pass rich data to this function.
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
                            Exception::throw_internal(ctx, &format!("Host function error: {e:#?}"))
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
    fn resolve(&mut self, _ctx: &Ctx<'_>, base: &str, name: &str) -> rquickjs::Result<String> {
        if !self.borrow().contains_key(name) {
            return Err(rquickjs::Error::new_resolving(base, name));
        }
        Ok(name.to_string())
    }
}

impl Loader for HostModuleLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js>> {
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
