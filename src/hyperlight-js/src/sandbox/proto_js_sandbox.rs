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
use std::fmt::Debug;
use std::time::SystemTime;

use anyhow::Context;
use hyperlight_host::sandbox::SandboxConfiguration;
use hyperlight_host::{GuestBinary, Result, UninitializedSandbox};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::{instrument, Level};

use super::js_sandbox::JSSandbox;
use super::sandbox_builder::SandboxBuilder;
use crate::sandbox::host_fn::{
    host_function_dispatch, host_function_manifest, Function, HostModule,
};
use crate::sandbox::metrics::SandboxMetricsGuard;
use crate::sandbox::module_loader::ExternalModuleLoader;
use crate::HostPrintFn;

pub(super) fn current_time_micros() -> hyperlight_host::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .with_context(|| "Unable to get duration since epoch")
        .map(|duration| duration.as_micros() as u64)?)
}

/// A fresh Hyperlight sandbox before the JavaScript runtime is loaded.
///
/// Register host resources, then call [`Self::load_runtime`] to create a
/// [`JSSandbox`]. Snapshot restoration uses [`crate::SandboxRestorer`] instead.
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
        host_modules: HashMap<String, HostModule>,
        module_loader: Option<ExternalModuleLoader>,
    ) -> Result<Self> {
        let mut usbox: UninitializedSandbox = UninitializedSandbox::new(guest_binary, cfg)?;

        // Set the host print function
        if let Some(host_print_writer) = host_print_writer {
            usbox.register_print(host_print_writer)?;
        }

        usbox.register("CurrentTimeMicros", current_time_micros)?;
        if let Some(module_loader) = module_loader {
            module_loader.register(&mut usbox)?;
        }

        Ok(Self {
            inner: usbox,
            host_modules,
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
        ExternalModuleLoader::new(file_system).register(&mut self.inner)?;
        Ok(self)
    }

    /// Load the JavaScript runtime into the sandbox.
    #[instrument(err(Debug), skip(self), level=Level::INFO)]
    pub fn load_runtime(mut self) -> Result<JSSandbox> {
        let host_functions = host_function_manifest(&self.host_modules);
        let host_modules_json = serde_json::to_string(&self.host_modules)?;
        let host_modules = self.host_modules;

        // Register the host function that the guest calls for all host
        // function invocations. Binary data (if any) is carried in a
        // length-prefixed sidecar alongside the JSON args.
        self.inner
            .register("CallHostJsFunction", host_function_dispatch(host_modules))?;

        let mut multi_use_sandbox = self.inner.evolve()?;

        let _: () = multi_use_sandbox.call("RegisterHostModules", host_modules_json)?;

        JSSandbox::new(multi_use_sandbox, host_functions)
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
