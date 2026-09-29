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

//! Hyperlight guest entry point and infrastructure.
//!
//! This module provides the guest-side plumbing needed to run the JS runtime
//! inside a Hyperlight VM. It includes:
//! - The `Host` implementation that calls out to hyperlight host functions
//! - The `hyperlight_main` entry point
//! - Guest function registrations (register_handler, RegisterHostModules)
//! - The `guest_dispatch_function` fallback for handler calls
//! - Libc stub implementations required by QuickJS
//!
//! This is all `cfg(hyperlight)` — compiled out entirely for native builds.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use anyhow::{anyhow, Context as _};
use hashbrown::HashMap;
use hyperlight_guest_bin::error::{ErrorCode, HyperlightGuestError, Result};
use hyperlight_guest_bin::{guest_function, host_function, main};
use hyperlight_js_common::Bytes;
use spin::Mutex;
use tracing::instrument;

use crate::host_fn::HostFunction;
use crate::JsRuntime;

mod stubs;

struct Host;

trait CatchGuestErrorExt {
    type Ok;
    fn catch(self) -> anyhow::Result<Self::Ok>;
}

impl<T> CatchGuestErrorExt for Result<T> {
    type Ok = T;
    fn catch(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow!("{}: {}", String::from(e.kind), e.message))
    }
}

/// Validates an external text payload while retaining its owned allocation.
fn decode_utf8(bytes: Vec<u8>, label: &str) -> Result<String> {
    String::from_utf8(bytes).map_err(|error| {
        HyperlightGuestError::new(
            ErrorCode::GuestError,
            format!("Invalid UTF-8 in {label}: {error}"),
        )
    })
}

impl crate::host::Host for Host {
    fn resolve_module(&self, base: String, name: String) -> anyhow::Result<String> {
        #[host_function("ResolveModule")]
        fn resolve_module(base: String, name: String) -> Result<String>;

        resolve_module(base.clone(), name.clone())
            .catch()
            .with_context(|| format!("Resolving module {name:?} from {base:?}"))
    }

    fn load_module(&self, name: String) -> anyhow::Result<String> {
        #[host_function("LoadModule")]
        fn load_module(name: String) -> Result<Vec<u8>>;

        load_module(name.clone())
            .and_then(|bytes| decode_utf8(bytes, "module source"))
            .catch()
            .with_context(|| format!("Loading module {name:?}"))
    }
}

static RUNTIME: spin::LazyLock<Mutex<JsRuntime>> = spin::LazyLock::new(|| {
    Mutex::new(JsRuntime::new(Host).unwrap_or_else(|e| {
        panic!("Failed to initialize JS runtime: {e:#?}");
    }))
});

#[main]
#[instrument(skip_all, level = "info")]
pub extern "C" fn hyperlight_main() {
    // Initialise the runtime (custom modules are registered lazily on first use)
    let _ = &*RUNTIME;
}

/// Registers a handler from an externally transported UTF-8 source buffer.
#[guest_function("register_handler")]
#[instrument(skip_all, level = "info")]
fn register_handler(
    function_name: String,
    handler_script: Vec<u8>,
    handler_pwd: String,
) -> Result<()> {
    let handler_script = decode_utf8(handler_script, "handler source")?;

    RUNTIME
        .lock()
        .register_handler(function_name, handler_script, handler_pwd)?;
    Ok(())
}

/// Registers a user module from an externally transported UTF-8 source buffer.
#[guest_function("register_module")]
#[instrument(skip_all, level = "info")]
fn register_module(module_name: String, module_source: Vec<u8>) -> Result<()> {
    let module_source = decode_utf8(module_source, "module source")?;

    RUNTIME.lock().register_module(module_name, module_source)?;
    Ok(())
}

#[host_function("CallHostJsFunction")]
fn call_host_js_function(
    module_name: String,
    func_name: String,
    args_json: Vec<u8>,
    binaries: Vec<Bytes>,
) -> Result<Vec<u8>>;

#[guest_function("RegisterHostModules")]
fn register_host_modules(host_modules_json: String) -> Result<()> {
    // The serialization in here has to match the serialization of
    // HostModule in src/hyperlight_js/src/sandbox/host_fn.rs
    let host_modules: HashMap<String, Vec<String>> = serde_json::from_str(&host_modules_json)
        .map_err(|e| {
            HyperlightGuestError::new(
                ErrorCode::GuestError,
                format!("Failed to parse host modules JSON: {e:#?}"),
            )
        })?;

    let mut runtime = RUNTIME.lock();

    for (module_name, functions) in host_modules {
        for function_name in functions {
            let module_name = module_name.clone();
            runtime.add_host_function(
                module_name.clone(),
                function_name.clone(),
                HostFunction::new_bin_chunks(
                    move |args_json: String, binaries: Vec<Bytes>| -> anyhow::Result<Vec<u8>> {
                        call_host_js_function(
                            module_name.clone(),
                            function_name.clone(),
                            args_json.into_bytes(),
                            binaries,
                        )
                        .map_err(|e| {
                            // Use e.message directly — {e:#?} would expand into a
                            // huge Debug struct that exceeds the hyperlight
                            // guest↔host error buffer and gets truncated.
                            // Include the error kind for diagnostics.
                            anyhow!(
                                "Calling host function {module_name:?} {function_name:?} failed ({:?}): {}",
                                e.kind,
                                e.message
                            )
                        })
                    },
                ),
            )?;
        }
    }
    Ok(())
}

/// Runs a handler with external UTF-8 JSON input and returns external JSON bytes.
#[guest_function("RunHandler")]
fn run_handler(function_name: String, event: Vec<u8>, run_gc: bool) -> Result<Vec<u8>> {
    let event = decode_utf8(event, "event JSON")?;

    Ok(RUNTIME
        .lock()
        .run_handler(function_name, event, run_gc)?
        .into_bytes())
}
