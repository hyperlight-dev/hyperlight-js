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
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use anyhow::{anyhow, Context as _};
use hashbrown::HashMap;
use hyperlight_common::flatbuffer_wrappers::function_call::FunctionCall;
use hyperlight_common::flatbuffer_wrappers::guest_error::ErrorCode;
use hyperlight_common::flatbuffer_wrappers::util::get_flatbuffer_result;
use hyperlight_common::func::ParameterTuple;
use hyperlight_guest::error::{HyperlightGuestError, Result};
use hyperlight_guest_bin::{guest_function, host_function};
use spin::Mutex;
use tracing::instrument;

mod stubs;

struct Host;

pub trait CatchGuestErrorExt {
    type Ok;
    fn catch(self) -> anyhow::Result<Self::Ok>;
}

impl<T> CatchGuestErrorExt for hyperlight_guest::error::Result<T> {
    type Ok = T;
    fn catch(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow!("{}: {}", String::from(e.kind), e.message))
    }
}

impl hyperlight_js_runtime::host::Host for Host {
    fn resolve_module(&self, base: String, name: String) -> anyhow::Result<String> {
        #[host_function("ResolveModule")]
        fn resolve_module(base: String, name: String) -> Result<String>;

        resolve_module(base.clone(), name.clone())
            .catch()
            .with_context(|| format!("Resolving module {name:?} from {base:?}"))
    }

    fn load_module(&self, name: String) -> anyhow::Result<String> {
        #[host_function("LoadModule")]
        fn load_module(name: String) -> Result<String>;

        load_module(name.clone())
            .catch()
            .with_context(|| format!("Loading module {name:?}"))
    }
}

static RUNTIME: spin::Lazy<Mutex<hyperlight_js_runtime::JsRuntime>> = spin::Lazy::new(|| {
    Mutex::new(hyperlight_js_runtime::JsRuntime::new(Host).unwrap_or_else(|e| {
        panic!("Failed to initialize JS runtime: {e:#?}");
    }))
});

#[unsafe(no_mangle)]
#[instrument(skip_all, level = "info")]
pub extern "C" fn hyperlight_main() {
    // dereference RUNTIME to force its initialization
    // of the Lazy static
    let _ = &*RUNTIME;
}

#[guest_function("register_handler")]
#[instrument(skip_all, level = "info")]
fn register_handler(
    function_name: String,
    handler_script: String,
    handler_pwd: String,
) -> Result<()> {
    RUNTIME
        .lock()
        .register_handler(function_name, handler_script, handler_pwd)?;
    Ok(())
}

#[host_function("CallHostJsFunction")]
fn call_host_js_function(
    module_name: String,
    func_name: String,
    args_json: String,
    binaries: Vec<u8>,
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
            // Register binary-capable host function that can handle Uint8Array/Buffer
            runtime.register_binary_host_function(
                module_name.clone(),
                function_name.clone(),
                move |args_json: String, binaries: Vec<u8>| -> anyhow::Result<Vec<u8>> {
                    call_host_js_function(
                        module_name.clone(),
                        function_name.clone(),
                        args_json,
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
            )?;
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub fn guest_dispatch_function(function_call: FunctionCall) -> Result<Vec<u8>> {
    let params = function_call.parameters.unwrap_or_default();
    let function_name = function_call.function_name;
    let (event, run_gc) = ParameterTuple::from_value(params)?;
    let result = RUNTIME.lock().run_handler(function_name, event, run_gc)?;
    Ok(get_flatbuffer_result(result.as_str()))
}
