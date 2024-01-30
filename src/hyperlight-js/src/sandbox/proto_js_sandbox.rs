use std::collections::HashMap;
use std::fmt::Debug;
use std::time::SystemTime;

use anyhow::Context;
use hyperlight_host::sandbox::SandboxConfiguration;
use hyperlight_host::{new_error, GuestBinary, Result, UninitializedSandbox};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::{instrument, Level};

use super::js_sandbox::JSSandbox;
use super::sandbox_builder::SandboxBuilder;
use crate::sandbox::host_fn::{Function, HostModule};
use crate::sandbox::metrics::SandboxMetricsGuard;
use crate::HostPrintFn;

/// A Hyperlight Sandbox with no JavaScript run time loaded and no guest code.
/// This is used to register new host functions prior to loading the JavaScript run time.
pub struct ProtoJSSandbox {
    inner: UninitializedSandbox,
    host_modules: HashMap<String, HostModule>,
    // metric drop guard to manage sandbox metric
    _metric_guard: SandboxMetricsGuard<ProtoJSSandbox>,
}

impl ProtoJSSandbox {
    #[instrument(err(Debug), skip_all, level=Level::INFO, fields(version= env!("CARGO_PKG_VERSION")))]
    pub(super) fn new(
        guest_binary: GuestBinary,
        cfg: Option<SandboxConfiguration>,
        host_print_writer: Option<HostPrintFn>,
    ) -> Result<Self> {
        let mut usbox: UninitializedSandbox = UninitializedSandbox::new(guest_binary, cfg)?;

        // Set the host print function
        if let Some(host_print_writer) = host_print_writer {
            usbox.register_print(host_print_writer)?;
        }

        // host function used by rquickjs for Date.now()
        fn current_time_micros() -> hyperlight_host::Result<u64> {
            Ok(SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .with_context(|| "Unable to get duration since epoch")
                .map(|d| d.as_micros() as u64)?)
        }

        usbox.register("CurrentTimeMicros", current_time_micros)?;

        Ok(Self {
            inner: usbox,
            host_modules: HashMap::new(),
            _metric_guard: SandboxMetricsGuard::new(),
        })
    }

    /// Install a custom file system for module resolution and loading.
    ///
    /// Enables JavaScript module imports using the provided ~FileSystem~ implementation.
    #[instrument(err(Debug), skip_all, level=Level::INFO)]
    pub fn set_module_loader<Fs: crate::resolver::FileSystem + Clone + 'static>(
        mut self,
        file_system: Fs,
    ) -> Result<Self> {
        use std::path::PathBuf;

        use oxc_resolver::{ResolveOptions, ResolverGeneric};

        let resolver = ResolverGeneric::new_with_file_system(
            file_system.clone(),
            ResolveOptions {
                extensions: vec![".js".into(), ".mjs".into()],
                condition_names: vec!["import".into(), "module".into()],
                ..Default::default()
            },
        );

        self.inner.register(
            "ResolveModule",
            move |base: String, specifier: String| -> hyperlight_host::Result<String> {
                tracing::debug!(
                    base = %base,
                    specifier = %specifier,
                    "Resolving module"
                );

                let resolved = resolver.resolve(&base, &specifier).map_err(|e| {
                    new_error!(
                        "Failed to resolve module '{}' from '{}': {:?}",
                        specifier,
                        base,
                        e
                    )
                })?;

                Ok(resolved.path().to_string_lossy().to_string())
            },
        )?;

        self.inner.register(
            "LoadModule",
            move |path: String| -> hyperlight_host::Result<String> {
                tracing::debug!(path = %path, "Loading module");
                let path_buf = PathBuf::from(&path);
                let source = file_system
                    .read_to_string(&path_buf)
                    .map_err(|e| new_error!("Failed to read module '{}': {}", path, e))?;

                Ok(source)
            },
        )?;

        Ok(self)
    }

    /// Load the JavaScript runtime into the sandbox.
    #[instrument(err(Debug), skip(self), level=Level::INFO)]
    pub fn load_runtime(mut self) -> Result<JSSandbox> {
        let host_modules = self.host_modules;

        let host_modules_json = serde_json::to_string(&host_modules)?;

        self.inner.register(
            "CallHostJsFunction",
            move |module_name: String, func_name: String, args: String| -> Result<String> {
                let module = host_modules
                    .get(&module_name)
                    .ok_or_else(|| new_error!("Host module '{}' not found", module_name))?;
                let func = module.get(&func_name).ok_or_else(|| {
                    new_error!(
                        "Host function '{}' not found in module '{}'",
                        func_name,
                        module_name
                    )
                })?;
                func(args)
            },
        )?;

        let mut multi_use_sandbox = self.inner.evolve()?;

        let _: () = multi_use_sandbox.call("RegisterHostModules", host_modules_json)?;

        JSSandbox::new(multi_use_sandbox)
    }

    /// Register a host module that can be called from the guest JavaScript code.
    ///
    /// This method should be called **before** [`ProtoJSSandbox::load_runtime`], while
    /// the sandbox is still in its "proto" (uninitialized) state. After
    /// [`load_runtime`](Self::load_runtime) is called, the set of host modules and
    /// functions is fixed for the resulting [`JSSandbox`].
    ///
    /// Calling this method multiple times with the same `name` refers to the same
    /// module; additional calls will reuse the existing module instance and allow
    /// you to register more functions on it. The first call creates the module and
    /// subsequent calls return the previously created module.
    ///
    /// Module names are matched by exact string equality from the guest
    /// JavaScript environment. They should be valid UTF‑8 strings and while there is
    /// no explicit restriction on special characters, using simple, ASCII identifiers
    /// (e.g. `"fs"`, `"net"`, `"my_module"`) is recommended for portability and clarity.
    ///
    /// # Example
    ///
    /// ```
    /// use hyperlight_js::SandboxBuilder;
    ///
    /// // Create a proto sandbox and register a host function.
    /// let mut sbox = SandboxBuilder::new().build()?;
    ///
    /// // Register a module and a function on it before loading the runtime.
    /// sbox.host_module("math").register("add", |a: i32, b: i32| a + b);
    ///
    /// // Once all host modules/functions are registered, load the JS runtime.
    /// let js_sandbox = sbox.load_runtime()?;
    /// # Ok::<(), hyperlight_host::HyperlightError>(())
    /// ```
    #[instrument(skip(self), level=Level::INFO)]
    pub fn host_module(&mut self, name: impl Into<String> + Debug) -> &mut HostModule {
        self.host_modules.entry(name.into()).or_default()
    }

    /// Register a host function that can be called from the guest JavaScript code.
    /// This is equivalent to calling `sbox.host_module(module).register(name, func)`.
    ///
    /// Registering a function with the same `module` and `name` as an existing function
    /// overwrites the previous registration.
    #[instrument(err(Debug), skip(self, func), level=Level::INFO)]
    pub fn register<Output: Serialize, Args: DeserializeOwned>(
        &mut self,
        module: impl Into<String> + Debug,
        name: impl Into<String> + Debug,
        func: impl Function<Output, Args> + Send + Sync + 'static,
    ) -> Result<()> {
        self.host_module(module).register(name, func);
        Ok(())
    }
}

impl std::fmt::Debug for ProtoJSSandbox {
    #[instrument(skip_all, level=Level::TRACE)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtoJsSandbox").finish()
    }
}

impl Default for ProtoJSSandbox {
    #[instrument(skip_all, level=Level::INFO)]
    fn default() -> Self {
        // This should not fail so we unwrap it.
        // If it does fail then it is a fundamental bug.
        #[allow(clippy::unwrap_used)]
        SandboxBuilder::new().build().unwrap()
    }
}
