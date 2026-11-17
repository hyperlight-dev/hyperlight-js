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
use std::time::SystemTime;

use anyhow::Context;
use hyperlight_host::sandbox::SandboxConfiguration;
use hyperlight_host::{GuestBinary, Result, UninitializedSandbox};
use tracing::{instrument, Level};

use super::js_sandbox::JSSandbox;
use super::sandbox_builder::SandboxBuilder;
use crate::sandbox::host_fn::{host_function_dispatch, host_function_manifest, HostModule};
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
/// Configure host resources on [`SandboxBuilder`], then call [`Self::load_runtime`]
/// to create a [`JSSandbox`]. Restore snapshots with [`crate::SandboxBuilder::from_snapshot`].
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

    /// Load the JavaScript runtime into the sandbox.
    #[instrument(err(Debug), skip(self), level=Level::INFO)]
    pub fn load_runtime(mut self) -> Result<JSSandbox> {
        let host_functions = host_function_manifest(&self.host_modules);
        let host_modules_json = serde_json::to_string(&self.host_modules)?;
        let host_modules = self.host_modules;

        // Register the host function that the guest calls for all host
        // function invocations. Binary data (if any) is carried in a
        // chunked length-prefixed sidecar alongside the JSON args.
        self.inner
            .register("CallHostJsFunction", host_function_dispatch(host_modules))?;

        let mut multi_use_sandbox = self.inner.evolve()?;

        let _: () = multi_use_sandbox.call("RegisterHostModules", host_modules_json)?;

        JSSandbox::new(multi_use_sandbox, host_functions)
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
