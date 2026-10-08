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

use hyperlight_host::{new_error, Result, SandboxBuilder as HyperlightSandboxBuilder};

use super::host_fn::{host_function_dispatch, host_function_manifest, HostModule};
use super::js_sandbox::JSSandbox;
use super::loaded_js_sandbox::LoadedJSSandbox;
use super::module_loader::{
    ExternalModuleLoader, LOAD_MODULE_HOST_FUNCTION, RESOLVE_MODULE_HOST_FUNCTION,
};
use super::proto_js_sandbox::current_time_micros;
use super::snapshot::{HostFunctionManifest, Snapshot, SnapshotKind, SnapshotRequirements};
use crate::HostPrintFn;

mod private {
    pub trait Sealed {}
}

/// A sandbox type that can be restored by [`SandboxRestorer::restore`].
///
/// This trait is sealed and implemented only for [`JSSandbox`] and
/// [`LoadedJSSandbox`].
pub trait RestoreTarget: private::Sealed + Sized {
    /// Restore this concrete sandbox type from the configured restorer.
    #[doc(hidden)]
    fn restore_from(restorer: SandboxRestorer) -> Result<Self>;
}

/// Configures process-local host resources before restoring a snapshot.
pub struct SandboxRestorer {
    builder: HyperlightSandboxBuilder,
    snapshot: Snapshot,
    host_modules: HashMap<String, HostModule>,
}

impl SandboxRestorer {
    pub(super) fn new(
        snapshot: Snapshot,
        host_print_writer: Option<HostPrintFn>,
        host_modules: HashMap<String, HostModule>,
        module_loader: Option<ExternalModuleLoader>,
    ) -> Result<Self> {
        let mut builder = HyperlightSandboxBuilder::from_snapshot(snapshot.inner());
        if let Some(host_print_writer) = host_print_writer {
            builder = builder.host_print(host_print_writer);
        }
        builder = builder.host_function("CurrentTimeMicros", current_time_micros);
        if let Some(module_loader) = module_loader {
            builder = module_loader.apply(builder);
        }

        Ok(Self {
            builder,
            snapshot,
            host_modules,
        })
    }

    /// Return the lifecycle kind captured by the snapshot.
    pub fn kind(&self) -> SnapshotKind {
        self.snapshot.kind()
    }

    /// Return the host resources required by the snapshot.
    pub fn requirements(&self) -> SnapshotRequirements {
        self.snapshot.requirements()
    }

    /// Restore the snapshot as the requested sandbox type.
    pub fn restore<T: RestoreTarget>(self) -> Result<T> {
        T::restore_from(self)
    }

    fn build(self) -> Result<(hyperlight_host::MultiUseSandbox, HostFunctionManifest)> {
        let available_host_functions = host_function_manifest(&self.host_modules);
        self.snapshot
            .metadata()
            .validate_host_functions(&available_host_functions)?;

        let builder = self.builder.host_function(
            "CallHostJsFunction",
            host_function_dispatch(self.host_modules),
        );
        let sandbox = builder.build().map_err(|error| match &error {
            hyperlight_host::HyperlightError::SnapshotHostFunctionMismatch { missing, .. }
                if missing.iter().any(|name| {
                    name == RESOLVE_MODULE_HOST_FUNCTION || name == LOAD_MODULE_HOST_FUNCTION
                }) =>
            {
                new_error!(
                    "Snapshot requires a module loader; call with_module_loader() on SandboxBuilder before build_from_snapshot() ({})",
                    error
                )
            }
            _ => error,
        })?;
        Ok((sandbox, available_host_functions))
    }
}

impl private::Sealed for JSSandbox {}

impl RestoreTarget for JSSandbox {
    fn restore_from(restorer: SandboxRestorer) -> Result<Self> {
        if restorer.snapshot.kind() != SnapshotKind::JsSandbox {
            return Err(new_error!(
                "Cannot restore a {:?} snapshot as a JSSandbox",
                restorer.snapshot.kind()
            ));
        }
        let snapshot = restorer.snapshot.clone();
        let (sandbox, available_host_functions) = restorer.build()?;
        JSSandbox::from_snapshot(sandbox, &snapshot, available_host_functions)
    }
}

impl private::Sealed for LoadedJSSandbox {}

impl RestoreTarget for LoadedJSSandbox {
    fn restore_from(restorer: SandboxRestorer) -> Result<Self> {
        if restorer.snapshot.kind() != SnapshotKind::LoadedJsSandbox {
            return Err(new_error!(
                "Cannot restore a {:?} snapshot as a LoadedJSSandbox",
                restorer.snapshot.kind()
            ));
        }
        let snapshot = restorer.snapshot.clone();
        let (sandbox, available_host_functions) = restorer.build()?;
        LoadedJSSandbox::from_snapshot(sandbox, &snapshot, available_host_functions)
    }
}
