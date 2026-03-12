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

//! Tests for the native module extension system.
//!
//! Verifies that:
//! - `register_native_module` adds custom modules to the loader
//! - Built-in modules always work
//! - Custom modules can be imported from JS handlers
//! - Built-in module names cannot be overridden (panics)
//! - The `native_modules!` macro generates correct `init_native_modules`
//! - Full pipeline tests with the extended_runtime fixture binary

#![cfg(not(hyperlight))]

use rquickjs::loader::{Loader, Resolver};

/// Helper: create a QuickJS runtime + context and run a closure within it.
fn with_qjs_context(f: impl FnOnce(rquickjs::Ctx<'_>)) {
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    ctx.with(f);
}

/// A minimal Host that doesn't support loading external modules.
struct NoOpHost;

impl hyperlight_js_runtime::host::Host for NoOpHost {
    fn resolve_module(&self, _base: String, name: String) -> anyhow::Result<String> {
        anyhow::bail!("NoOpHost does not support resolving module '{name}'")
    }

    fn load_module(&self, name: String) -> anyhow::Result<String> {
        anyhow::bail!("NoOpHost does not support loading module '{name}'")
    }
}

// ── NativeModuleLoader: built-in modules ───────────────────────────────────

#[test]
fn loader_resolves_all_builtin_modules() {
    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;
    let builtins = hyperlight_js_runtime::modules::builtin_module_names();
    assert!(
        !builtins.is_empty(),
        "Should have at least one built-in module"
    );

    with_qjs_context(|ctx| {
        for name in &builtins {
            let result = loader.resolve(&ctx, ".", name);
            assert!(result.is_ok(), "Should resolve built-in module '{name}'");
            assert_eq!(result.unwrap(), *name);
        }
    });
}

#[test]
fn loader_rejects_unknown_modules() {
    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;

    with_qjs_context(|ctx| {
        let result = loader.resolve(&ctx, ".", "nonexistent");
        assert!(result.is_err(), "Should reject unknown modules");
    });
}

#[test]
fn loader_loads_all_builtin_modules() {
    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;
    let builtins = hyperlight_js_runtime::modules::builtin_module_names();

    with_qjs_context(|ctx| {
        for name in &builtins {
            let result = loader.load(&ctx, name);
            assert!(
                result.is_ok(),
                "Should load built-in module '{name}', got: {:?}",
                result.err()
            );
        }
    });
}

// ── register_native_module ─────────────────────────────────────────────────

/// A trivial test module that exports a single `greet` function.
#[rquickjs::module(rename_vars = "camelCase")]
mod test_greeting {
    #[rquickjs::function]
    pub fn greet() -> String {
        String::from("hello from test module")
    }
}

#[test]
fn registered_custom_module_resolves_and_loads() {
    hyperlight_js_runtime::modules::register_native_module(
        "greeting",
        hyperlight_js_runtime::modules::declaration::<js_test_greeting>(),
    );

    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;

    with_qjs_context(|ctx| {
        let result = loader.resolve(&ctx, ".", "greeting");
        assert!(
            result.is_ok(),
            "Should resolve registered 'greeting' module"
        );

        let result = loader.load(&ctx, "greeting");
        assert!(
            result.is_ok(),
            "Should load registered 'greeting' module, got: {:?}",
            result.err()
        );
    });
}

#[test]
fn builtins_still_work_after_custom_registration() {
    // Register a custom module first, then verify builtins still work
    hyperlight_js_runtime::modules::register_native_module(
        "greeting",
        hyperlight_js_runtime::modules::declaration::<js_test_greeting>(),
    );

    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;
    let builtins = hyperlight_js_runtime::modules::builtin_module_names();

    with_qjs_context(|ctx| {
        for name in &builtins {
            let result = loader.resolve(&ctx, ".", name);
            assert!(
                result.is_ok(),
                "Built-in '{name}' should still resolve after custom registration"
            );
        }
    });
}

// ── Override prevention ────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "conflicts with a built-in module")]
fn registering_builtin_name_panics() {
    #[rquickjs::module(rename_vars = "camelCase")]
    mod fake_io {
        #[rquickjs::function]
        pub fn print(_txt: String) {}
    }

    hyperlight_js_runtime::modules::register_native_module(
        "io",
        hyperlight_js_runtime::modules::declaration::<js_fake_io>(),
    );
}

// ── native_modules! macro ──────────────────────────────────────────────────

#[rquickjs::module(rename_vars = "camelCase")]
mod test_math {
    #[rquickjs::function]
    pub fn add(a: f64, b: f64) -> f64 {
        a + b
    }
}

// The macro generates init_native_modules() which calls register_native_module
hyperlight_js_runtime::native_modules! {
    "test_math_macro" => js_test_math,
}

#[test]
fn macro_generated_init_registers_modules() {
    init_native_modules();

    let mut loader = hyperlight_js_runtime::modules::NativeModuleLoader;

    with_qjs_context(|ctx| {
        let result = loader.resolve(&ctx, ".", "test_math_macro");
        assert!(
            result.is_ok(),
            "Module registered via native_modules! macro should resolve"
        );
    });
}

// ── End-to-end JsRuntime tests ─────────────────────────────────────────────

#[test]
fn e2e_handler_imports_custom_native_module() {
    // Register our test module (idempotent — HashMap insert is safe to repeat)
    hyperlight_js_runtime::modules::register_native_module(
        "greeting",
        hyperlight_js_runtime::modules::declaration::<js_test_greeting>(),
    );

    let mut runtime =
        hyperlight_js_runtime::JsRuntime::new(NoOpHost).expect("Failed to create JsRuntime");

    let handler_script = r#"
        import { greet } from "greeting";
        export function handler(event) {
            return greet();
        }
    "#;

    runtime
        .register_handler("test_handler", handler_script, ".")
        .expect("Failed to register handler");

    let result = runtime
        .run_handler("test_handler".to_string(), "{}".to_string(), false)
        .expect("Failed to run handler");

    assert_eq!(result, "\"hello from test module\"");
}

#[test]
fn e2e_handler_uses_builtin_and_custom_modules_together() {
    hyperlight_js_runtime::modules::register_native_module(
        "greeting",
        hyperlight_js_runtime::modules::declaration::<js_test_greeting>(),
    );

    let mut runtime =
        hyperlight_js_runtime::JsRuntime::new(NoOpHost).expect("Failed to create JsRuntime");

    let handler_script = r#"
        import { greet } from "greeting";
        import { createHmac } from "crypto";
        export function handler(event) {
            const greeting = greet();
            const hmac = createHmac("sha256", "key");
            hmac.update("data");
            const digest = hmac.digest("hex");
            return { greeting, hasDigest: digest.length > 0 };
        }
    "#;

    runtime
        .register_handler("combo_handler", handler_script, ".")
        .expect("Failed to register handler");

    let result = runtime
        .run_handler("combo_handler".to_string(), "{}".to_string(), false)
        .expect("Failed to run handler");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Invalid JSON result");
    assert_eq!(parsed["greeting"], "hello from test module");
    assert_eq!(parsed["hasDigest"], true);
}

#[test]
fn e2e_default_runtime_builtins_work() {
    let mut runtime =
        hyperlight_js_runtime::JsRuntime::new(NoOpHost).expect("Failed to create JsRuntime");

    let handler_script = r#"
        import { createHmac } from "crypto";
        export function handler(event) {
            const hmac = createHmac("sha256", "secret");
            hmac.update(event.data);
            return hmac.digest("hex");
        }
    "#;

    runtime
        .register_handler("default_handler", handler_script, ".")
        .expect("Failed to register handler");

    let result = runtime
        .run_handler(
            "default_handler".to_string(),
            r#"{"data":"test"}"#.to_string(),
            false,
        )
        .expect("Failed to run handler");

    let hex_str = result.trim_matches('"');
    assert_eq!(
        hex_str.len(),
        64,
        "Expected 64-char hex digest, got: {result}"
    );
    assert!(
        hex_str.chars().all(|c| c.is_ascii_hexdigit()),
        "Expected hex string"
    );
}

// ── Full pipeline E2E tests ────────────────────────────────────────────────

use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

const EXTENDED_RUNTIME_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/extended_runtime/Cargo.toml"
);

static EXTENDED_RUNTIME_BINARY: LazyLock<PathBuf> = LazyLock::new(|| {
    let output = Command::new("cargo")
        .args(["build", "--manifest-path", EXTENDED_RUNTIME_MANIFEST])
        .output()
        .expect("Failed to run cargo build for extended-runtime fixture");

    assert!(
        output.status.success(),
        "Failed to build extended-runtime fixture:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let fixture_dir = PathBuf::from(EXTENDED_RUNTIME_MANIFEST)
        .parent()
        .unwrap()
        .to_path_buf();
    let binary_name = if cfg!(windows) {
        "extended-runtime.exe"
    } else {
        "extended-runtime"
    };
    let binary = fixture_dir.join("target/debug").join(binary_name);
    assert!(binary.exists(), "Binary not found at {binary:?}");
    binary
});

#[test]
fn full_pipeline_custom_native_module() {
    let binary = &*EXTENDED_RUNTIME_BINARY;
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("handler.js"),
        r#"
            import { add, multiply } from "math";
            export function handler(event) {
                return { sum: add(event.a, event.b), product: multiply(event.a, event.b) };
            }
        "#,
    )
    .unwrap();

    let output = Command::new(binary)
        .arg(dir.path().join("handler.js"))
        .arg(r#"{"a":6,"b":7}"#)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#"{"sum":13,"product":42}"#),
        "Got: {stdout}"
    );
}

#[test]
fn full_pipeline_custom_and_builtin_modules_together() {
    let binary = &*EXTENDED_RUNTIME_BINARY;
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("handler.js"),
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
    )
    .unwrap();

    let output = Command::new(binary)
        .arg(dir.path().join("handler.js"))
        .arg(r#"{"a":10,"b":32}"#)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#""sum":42"#), "Got: {stdout}");
    assert!(stdout.contains(r#""digestLength":64"#), "Got: {stdout}");
}

#[test]
fn full_pipeline_console_log_with_custom_modules() {
    let binary = &*EXTENDED_RUNTIME_BINARY;
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("handler.js"),
        r#"
            import { multiply } from "math";
            function handler(event) {
                const result = multiply(event.x, event.y);
                console.log("computed: " + result);
                return result;
            }
        "#,
    )
    .unwrap();

    let output = Command::new(binary)
        .arg(dir.path().join("handler.js"))
        .arg(r#"{"x":6,"y":9}"#)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines, ["computed: 54", "Handler result: 54"]);
}
