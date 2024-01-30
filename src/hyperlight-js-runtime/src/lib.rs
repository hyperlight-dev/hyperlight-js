#![no_std]
#![no_main]
extern crate alloc;

mod globals;
pub mod host;
mod host_fn;
mod modules;
pub(crate) mod utils;

use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};

use anyhow::{anyhow, Context as _};
use hashbrown::HashMap;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::promise::MaybePromise;
use rquickjs::{Context, Ctx, Function, Module, Persistent, Result, Runtime, Value};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::instrument;

use crate::host::Host;
use crate::host_fn::{HostFunction, HostModuleLoader};
use crate::modules::NativeModuleLoader;

/// A handler is a javascript function that takes a single `event` object parameter,
/// and is registered to the static `Context` instance
#[derive(Clone)]
struct Handler<'a> {
    func: Persistent<Function<'a>>,
}

/// This is the main entry point for the library.
/// It manages the QuickJS runtime, as well as the registered handlers and host modules.
pub struct JsRuntime {
    context: Context,
    handlers: HashMap<String, Handler<'static>>,
}

// SAFETY:
// This is safe. The reason it is not automatically implemented by the compiler
// is because `rquickjs::Context` is not `Send` because it holds a raw pointer.
// Raw pointers in rust are not marked as `Send` as lint rather than an actual
// safety concern (see https://doc.rust-lang.org/nomicon/send-and-sync.html).
// Moreover, rquickjs DOES implement Send for Context when the "parallel" feature
// is enabled, further indicating that it is safe for this to implement `Send`.
// Moreover, every public method of `JsRuntime` takes `&mut self`, and so we can
// be certain that there are no concurrent accesses to it.
unsafe impl Send for JsRuntime {}

impl JsRuntime {
    /// Create a new `JsRuntime` with the given host.
    /// The resulting runtime will have global objects registered.
    #[instrument(skip_all, level = "info")]
    pub fn new<H: Host + 'static>(host: H) -> anyhow::Result<Self> {
        let runtime = Runtime::new().context("Unable to initialize JS_RUNTIME")?;
        let context = Context::full(&runtime).context("Unable to create JS context")?;

        // Setup the module loader.
        // We need to do this before setting up the globals as many of the globals are implemented
        // as native modules, and so they need the module loader to be able to be loaded.
        let host_loader = HostModuleLoader::default();
        let native_loader = NativeModuleLoader;
        let module_loader = ModuleLoader::new(host);

        let loader = (host_loader.clone(), native_loader, module_loader);
        runtime.set_loader(loader.clone(), loader);

        context.with(|ctx| -> anyhow::Result<()> {
            // we need to install the host loader in the context as the loader uses the context to
            // store some global state needed for module instantiation.
            host_loader.install(&ctx)?;

            // Setup the global objects in the context, so they are available to the handler scripts.
            globals::setup(&ctx).catch(&ctx)
        })?;

        Ok(Self {
            context,
            handlers: HashMap::new(),
        })
    }

    /// Register a host function in the specified module.
    /// The function takes and returns a JSON string, which is deserialized and serialized by the runtime.
    /// The arguments are serialized as a JSON array containing all the arguments passed to the function.
    pub fn register_json_host_function(
        &mut self,
        module_name: impl Into<String>,
        function_name: impl Into<String>,
        function: impl Fn(String) -> anyhow::Result<String> + 'static,
    ) -> anyhow::Result<()> {
        self.context.with(|ctx| {
            ctx.userdata::<HostModuleLoader>()
                .context("HostModuleLoader not found in context")?
                .borrow_mut()
                .entry(module_name.into())
                .or_default()
                .add_function(function_name.into(), HostFunction::new_json(function));
            Ok(())
        })
    }

    /// Register a host function in the specified module.
    /// The function takes and returns any type that can be (de)serialized by `serde`.
    pub fn register_host_function<Args, Output>(
        &mut self,
        module_name: impl Into<String>,
        function_name: impl Into<String>,
        function: impl fn_traits::Fn<Args, Output = anyhow::Result<Output>> + 'static,
    ) -> anyhow::Result<()>
    where
        Args: DeserializeOwned,
        Output: Serialize,
    {
        self.context.with(|ctx| {
            ctx.userdata::<HostModuleLoader>()
                .context("HostModuleLoader not found in context")?
                .borrow_mut()
                .entry(module_name.into())
                .or_default()
                .add_function(function_name.into(), HostFunction::new_serde(function));
            Ok(())
        })
    }

    /// Register a handler function with the runtime.
    /// The handler script is a JavaScript module that exports a function named `handler`.
    /// The handler function takes a single argument, which is the event data deserialized from a JSON string.
    pub fn register_handler(
        &mut self,
        function_name: impl Into<String>,
        handler_script: impl Into<String>,
        handler_pwd: impl Into<String>,
    ) -> anyhow::Result<()> {
        let function_name = function_name.into();
        let handler_script = handler_script.into();
        let handler_pwd = handler_pwd.into();

        // If the handler script doesn't already export the handler function, we export it for the user.
        // This is a convenience for the common case where the handler script is just a single file that defines
        // the handler function, without needing to explicitly export it.
        let handler_script = if !handler_script.contains("export") {
            format!("{}\nexport {{ handler }};", handler_script)
        } else {
            handler_script
        };

        // We create a "virtual" path for the handler module based on the function name and the provided handler directory.
        let handler_path = make_handler_path(&function_name, &handler_pwd);

        let func = self.context.with(|ctx| -> anyhow::Result<_> {
            // Declare the module for the handler script, and evaluate it to get the exported handler function.
            let module =
                Module::declare(ctx.clone(), handler_path.as_str(), handler_script.clone())
                    .catch(&ctx)?;

            let (module, promise) = module.eval().catch(&ctx)?;

            promise.finish::<()>().catch(&ctx)?;

            // Get the exported handler function from the module namespace
            let handler_func: Function = module.get("handler").catch(&ctx)?;

            // Save the handler function as a Persistent so it can be returned outside of the `enter` closure.
            Ok(Persistent::save(&ctx, handler_func))
        })?;

        // Store the handler function in the `handlers` map, so it can be called later when the handler is triggered.
        self.handlers.insert(function_name, Handler { func });

        Ok(())
    }

    /// Run a registered handler function with the given event data.
    /// The event data is passed as a JSON string, and the handler function is expected to return a value that can be serialized to JSON.
    /// The result is returned as a JSON string.
    /// If `run_gc` is true, the runtime will run a garbage collection cycle after running the handler.
    pub fn run_handler(
        &mut self,
        function_name: String,
        event: String,
        run_gc: bool,
    ) -> anyhow::Result<String> {
        // Get the handler function from the `handlers` map. If there is no handler registered for the given function name, return an error.
        let handler = self
            .handlers
            .get(&function_name)
            .with_context(|| format!("No handler registered for function {function_name}"))?
            .clone();

        // Create a guard that will flush any output when dropped (i.e., after running the handler).
        // This makes sure that any output generated through libc is flushed out of the libc's stdout buffer.
        let _guard = FlushGuard;

        // Evaluate `handler(event)`, and get resulting object as String
        self.context.with(|ctx| {
            // Create a guard that will run a GC cycle when dropped if `run_gc` is true.
            let _gc_guard = MaybeRunGcGuard::new(run_gc, &ctx);

            // Restore the handler function from the Persistent reference.
            let func = handler.func.clone().restore(&ctx).catch(&ctx)?;

            // Call it with the event data parsed as a JSON value.
            let arg = ctx.json_parse(event).catch(&ctx)?;

            // If the handler returned a promise that resolves immediately, we resolve it.
            let promise: MaybePromise = func.call((arg,)).catch(&ctx)?;
            let obj: Value = promise.finish().catch(&ctx)?;

            // Serialize the result to a JSON string and return it.
            ctx.json_stringify(obj)
                .catch(&ctx)?
                .context("The handler function did not return a value")?
                .to_string()
                .catch(&ctx)
        })
    }
}

impl Drop for JsRuntime {
    fn drop(&mut self) {
        // make sure we flush any output when dropping the runtime
        modules::io::io::flush();
        // clear handlers to drop Persistent references before Context is dropped
        // otherwise the runtime will abort on drop due to the memory leak.
        self.handlers.clear();
    }
}

// A module loader that calls out to the host to resolve and load modules
#[derive(Clone)]
struct ModuleLoader {
    host: Rc<dyn Host>,
}

impl ModuleLoader {
    fn new(host: impl Host + 'static) -> Self {
        Self {
            host: Rc::new(host),
        }
    }
}

impl Resolver for ModuleLoader {
    fn resolve(&mut self, _ctx: &Ctx<'_>, base: &str, name: &str) -> Result<String> {
        // quickjs uses the module path as the base for relative imports
        // but oxc_resolver expects the directory as the base
        let (dir, _) = base.rsplit_once('/').unwrap_or((".", ""));

        let path = self
            .host
            .resolve_module(dir.to_string(), name.to_string())
            .map_err(|_err| rquickjs::Error::new_resolving(base, name))?;

        // convert backslashes to forward slashes for windows compatibility
        let path = path.replace('\\', "/");
        Ok(path)
    }
}

impl Loader for ModuleLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
        let source = self
            .host
            .load_module(name.to_string())
            .map_err(|_err| rquickjs::Error::new_loading(name))?;

        Module::declare(ctx.clone(), name, source)
    }
}

fn make_handler_path(function_name: &str, handler_dir: &str) -> String {
    let handler_dir = if handler_dir.is_empty() {
        "."
    } else {
        handler_dir
    };

    let function_name = if function_name.is_empty() {
        "handler"
    } else {
        function_name
    };

    let function_name = function_name.replace('\\', "/");
    let mut handler_path = handler_dir.replace('\\', "/");
    if !handler_path.ends_with('/') {
        handler_path.push('/');
    }
    handler_path.push_str(&function_name);

    if !handler_path.ends_with(".js") && !handler_path.ends_with(".mjs") {
        handler_path.push_str(".js");
    }

    handler_path
}

// RAII guard that flushes the output buffer of libc when dropped.
// This is used to make sure we flush all output after running a handler, without needing to manually call it in every code path.
struct FlushGuard;

impl Drop for FlushGuard {
    fn drop(&mut self) {
        modules::io::io::flush();
    }
}

trait CatchJsErrorExt {
    type Ok;
    fn catch(self, ctx: &Ctx<'_>) -> anyhow::Result<Self::Ok>;
}

impl<T> CatchJsErrorExt for rquickjs::Result<T> {
    type Ok = T;
    fn catch(self, ctx: &Ctx<'_>) -> anyhow::Result<T> {
        match rquickjs::CatchResultExt::catch(self, ctx) {
            Ok(s) => Ok(s),
            Err(e) => Err(anyhow!("Runtime error: {e:#?}")),
        }
    }
}

// RAII guard that runs a GC cycle when dropped if `run_gc` is true.
// This is used to make sure we run a GC cycle after running a handler if requested, without needing to manually call it in every code path.
struct MaybeRunGcGuard<'a> {
    run_gc: bool,
    ctx: Ctx<'a>,
}

impl<'a> MaybeRunGcGuard<'a> {
    fn new(run_gc: bool, ctx: &Ctx<'a>) -> Self {
        Self {
            run_gc,
            ctx: ctx.clone(),
        }
    }
}

impl Drop for MaybeRunGcGuard<'_> {
    fn drop(&mut self) {
        if self.run_gc {
            // safety: we are in the same context
            self.ctx.run_gc();
        }
    }
}
