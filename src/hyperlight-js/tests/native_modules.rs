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

//! Integration tests for custom native modules in the Hyperlight VM.
//!
//! These tests require a custom runtime (the `extended_runtime` fixture)
//! built for `x86_64-hyperlight-none` and embedded in `hyperlight-js` via
//! `HYPERLIGHT_JS_RUNTIME_PATH`. They are marked `#[ignore]` because they
//! cannot run with a normal `cargo test`.
//!
//! To run them, use:
//! ```bash
//! just test-native-modules
//! ```
//!
//! This recipe builds the fixture with `cargo hyperlight build`, sets the
//! env var, rebuilds `hyperlight-js` with the custom guest, and runs these
//! tests.

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

/// Test that a custom native module ("math") can be imported and used
/// from a handler running inside the Hyperlight VM.
#[test]
#[ignore]
fn custom_native_module_works_in_vm() {
    let handler = Script::from_content(
        r#"
        import { add, multiply } from "math";
        export function handler(event) {
            return {
                sum: add(event.a, event.b),
                product: multiply(event.a, event.b),
            };
        }
        "#,
    );

    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();

    sandbox.add_handler("compute", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("compute", r#"{"a":6,"b":7}"#.to_string(), None)
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["sum"], 13.0);
    assert_eq!(parsed["product"], 42.0);
}

/// Test that built-in modules still work alongside custom native modules.
#[test]
#[ignore]
fn builtin_modules_work_with_custom_native_module() {
    let handler = Script::from_content(
        r#"
        import { add } from "math";
        import { createHmac } from "crypto";
        export function handler(event) {
            const sum = add(event.a, event.b);
            const hmac = createHmac("sha256", "key");
            hmac.update("data");
            const digest = hmac.digest("hex");
            return { sum, digestLength: digest.length };
        }
        "#,
    );

    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();

    sandbox.add_handler("combo", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("combo", r#"{"a":10,"b":32}"#.to_string(), None)
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["sum"], 42.0);
    assert_eq!(parsed["digestLength"], 64);
}

/// Test that console.log works alongside custom native modules.
#[test]
#[ignore]
fn console_log_works_with_custom_native_module() {
    let handler = Script::from_content(
        r#"
        import { multiply } from "math";
        export function handler(event) {
            const result = multiply(event.x, event.y);
            console.log("computed: " + result);
            return result;
        }
        "#,
    );

    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();

    sandbox.add_handler("log_test", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("log_test", r#"{"x":6,"y":9}"#.to_string(), None)
        .unwrap();

    assert_eq!(result, "54");
}
