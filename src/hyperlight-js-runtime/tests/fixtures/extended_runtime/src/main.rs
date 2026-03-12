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

//! An extended runtime binary that demonstrates adding custom native modules
//! to hyperlight-js-runtime using the `native_modules!` macro.
//!
//! This binary works for both native (CLI) testing and as a Hyperlight guest.
//! The lib provides all hyperlight guest infrastructure — no copying needed.

#![cfg_attr(hyperlight, no_std)]
#![cfg_attr(hyperlight, no_main)]

// Use the shared math module
use native_math::js_math;

// Register "math" into the global native module registry.
// Built-in modules (io, crypto, console, require) are inherited automatically.
hyperlight_js_runtime::native_modules! {
    "math" => js_math,
}

// ── Native CLI entry point (for dev/testing) ───────────────────────────────

#[cfg(not(hyperlight))]
fn main() -> anyhow::Result<()> {
    use std::path::Path;
    use std::{env, fs};

    let args: Vec<String> = env::args().collect();
    let file = std::path::PathBuf::from(&args[1]);
    let event = &args[2];

    let handler_script = fs::read_to_string(&file)?;
    let handler_pwd = file.parent().unwrap_or_else(|| Path::new("."));
    env::set_current_dir(handler_pwd)?;

    struct NoOpHost;
    impl hyperlight_js_runtime::host::Host for NoOpHost {
        fn resolve_module(&self, _base: String, name: String) -> anyhow::Result<String> {
            anyhow::bail!("Module '{name}' not found")
        }
        fn load_module(&self, name: String) -> anyhow::Result<String> {
            anyhow::bail!("Module '{name}' not found")
        }
    }

    let mut runtime = hyperlight_js_runtime::JsRuntime::new(NoOpHost)?;
    runtime.register_handler("handler", handler_script, ".")?;

    let result = runtime.run_handler("handler".into(), event.clone(), false)?;
    println!("Handler result: {result}");
    Ok(())
}

// For hyperlight builds: the lib's `guest` module provides hyperlight_main,
// guest_dispatch_function, and all plumbing. Nothing else needed here.
