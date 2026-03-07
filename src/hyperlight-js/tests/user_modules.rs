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
//! Integration tests for user module registration via `add_module` / `add_module_ns`.
//!
//! These tests exercise the full lifecycle: host-side registration → guest-side
//! lazy compilation → handler import → execution.

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

// ── Basic import ─────────────────────────────────────────────────────

#[test]
fn handler_imports_user_module_with_default_namespace() {
    let math_module = Script::from_content(
        r#"
        export function add(a, b) { return a + b; }
        export function multiply(a, b) { return a * b; }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { add, multiply } from 'user:math';
        export function handler(event) {
            event.sum = add(event.a, event.b);
            event.product = multiply(event.a, event.b);
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("math", math_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"a": 5, "b": 3}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["sum"], 8);
    assert_eq!(json["product"], 15);
}

#[test]
fn handler_imports_user_module_with_custom_namespace() {
    let math_module = Script::from_content(
        r#"
        export function add(a, b) { return a + b; }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { add } from 'mylib:math';
        export function handler(event) {
            event.result = add(event.a, event.b);
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module_ns("math", math_module, "mylib").unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"a": 10, "b": 20}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["result"], 30);
}

// ── Inter-module dependencies ────────────────────────────────────────

#[test]
fn module_imports_another_module() {
    let constants_module = Script::from_content(
        r#"
        export const PI = 3.14159;
        "#,
    );

    let geometry_module = Script::from_content(
        r#"
        import { PI } from 'user:constants';
        export function circleArea(r) { return PI * r * r; }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { circleArea } from 'user:geometry';
        export function handler(event) {
            event.area = circleArea(event.radius);
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    // Registration order doesn't matter — modules are lazily compiled
    sandbox.add_module("geometry", geometry_module).unwrap();
    sandbox.add_module("constants", constants_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"radius": 5}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let area = json["area"].as_f64().unwrap();
    // PI * 5 * 5 = 78.53975
    assert!(
        (area - 78.53975).abs() < 0.001,
        "Expected ~78.53975, got {area}"
    );
}

// ── State retention ──────────────────────────────────────────────────

#[test]
fn module_state_persists_between_handler_calls() {
    let counter_module = Script::from_content(
        r#"
        let count = 0;
        export function increment() { return ++count; }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { increment } from 'user:counter';
        export function handler(event) {
            event.count = increment();
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("counter", counter_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result1 = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let json1: serde_json::Value = serde_json::from_str(&result1).unwrap();
    assert_eq!(json1["count"], 1);

    let result2 = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let json2: serde_json::Value = serde_json::from_str(&result2).unwrap();
    assert_eq!(json2["count"], 2);
}

// ── Unload / reload ──────────────────────────────────────────────────

#[test]
fn unload_and_reload_with_different_module_version() {
    let math_v1 = Script::from_content(r#"export function compute(x) { return x + 1; }"#);
    let math_v2 = Script::from_content(r#"export function compute(x) { return x * 2; }"#);

    let handler_src = r#"
        import { compute } from 'user:math';
        export function handler(event) {
            event.result = compute(event.x);
            return event;
        }
    "#;

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("math", math_v1).unwrap();
    sandbox
        .add_handler("handler", Script::from_content(handler_src))
        .unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"x": 5}"#.to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["result"], 6); // 5 + 1

    // Unload, swap module, reload
    let mut sandbox = loaded.unload().unwrap();
    sandbox.add_module("math", math_v2).unwrap();
    sandbox
        .add_handler("handler", Script::from_content(handler_src))
        .unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"x": 5}"#.to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["result"], 10); // 5 * 2
}

// ── Validation ───────────────────────────────────────────────────────

#[test]
fn add_module_rejects_empty_name() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result = sandbox.add_module("", Script::from_content("export const x = 1;"));
    assert!(result.is_err(), "Empty module name should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty name, got: {err}"
    );
}

#[test]
fn add_module_rejects_reserved_namespace() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result =
        sandbox.add_module_ns("utils", Script::from_content("export const x = 1;"), "host");
    assert!(result.is_err(), "Reserved namespace should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("reserved"),
        "Error should mention reserved namespace, got: {err}"
    );
}

#[test]
fn add_module_rejects_duplicate() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox
        .add_module("utils", Script::from_content("export const x = 1;"))
        .unwrap();
    let result = sandbox.add_module("utils", Script::from_content("export const y = 2;"));
    assert!(result.is_err(), "Duplicate module should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("already exists"),
        "Error should mention duplicate, got: {err}"
    );
}

#[test]
fn remove_module_rejects_empty_name() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result = sandbox.remove_module("");
    assert!(result.is_err(), "Empty module name should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty name, got: {err}"
    );
}

#[test]
fn remove_module_rejects_nonexistent_module() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result = sandbox.remove_module("nonexistent");
    assert!(result.is_err(), "Non-existent module removal should fail");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("does not exist"),
        "Error should mention non-existence, got: {err}"
    );
}

#[test]
fn remove_module_ns_rejects_empty_namespace() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result = sandbox.remove_module_ns("utils", "");
    assert!(result.is_err(), "Empty namespace should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty namespace, got: {err}"
    );
}

// ── Multiple handlers sharing mutable state via a module ─────────────

/// Proves that two handlers importing the same module see the **same** mutable
/// state. Handler A mutates module-level state and Handler B reads it,
/// confirming ESM singleton semantics deliver cross-handler state sharing.
#[test]
fn multiple_handlers_share_mutable_module_state() {
    // Shared module with mutable state: a simple counter.
    let counter_module = Script::from_content(
        r#"
        let count = 0;
        export function increment() { return ++count; }
        export function getCount() { return count; }
        "#,
    );

    // Handler A: mutates state by calling increment()
    let writer_handler = Script::from_content(
        r#"
        import { increment } from 'user:counter';
        export function handler(event) {
            event.count = increment();
            return event;
        }
        "#,
    );

    // Handler B: reads state without mutating it
    let reader_handler = Script::from_content(
        r#"
        import { getCount } from 'user:counter';
        export function handler(event) {
            event.count = getCount();
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("counter", counter_module).unwrap();
    sandbox.add_handler("writer", writer_handler).unwrap();
    sandbox.add_handler("reader", reader_handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    // writer increments → count=1
    let result = loaded
        .handle_event("writer", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        json["count"], 1,
        "writer should see count=1 after first increment"
    );

    // reader sees the mutation made by writer → count=1
    let result = loaded
        .handle_event("reader", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        json["count"], 1,
        "reader should see count=1 written by writer"
    );

    // writer increments again → count=2
    let result = loaded
        .handle_event("writer", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        json["count"], 2,
        "writer should see count=2 after second increment"
    );

    // reader sees the updated state → count=2
    let result = loaded
        .handle_event("reader", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        json["count"], 2,
        "reader should see count=2 written by writer"
    );
}

// ── Multiple handlers sharing a module (pure functions) ──────────────

#[test]
fn multiple_handlers_can_share_a_module() {
    let utils_module = Script::from_content(
        r#"
        export function double(x) { return x * 2; }
        export function triple(x) { return x * 3; }
        "#,
    );

    let double_handler = Script::from_content(
        r#"
        import { double } from 'user:utils';
        export function handler(event) {
            event.result = double(event.x);
            return event;
        }
        "#,
    );

    let triple_handler = Script::from_content(
        r#"
        import { triple } from 'user:utils';
        export function handler(event) {
            event.result = triple(event.x);
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("utils", utils_module).unwrap();
    sandbox.add_handler("doubler", double_handler).unwrap();
    sandbox.add_handler("tripler", triple_handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result = loaded
        .handle_event("doubler", r#"{"x": 7}"#.to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["result"], 14);

    let result = loaded
        .handle_event("tripler", r#"{"x": 7}"#.to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["result"], 21);
}

// ── User module importing a built-in module ──────────────────────────

#[test]
fn user_module_can_import_builtin_module() {
    let hasher_module = Script::from_content(
        r#"
        import { createHmac } from 'crypto';
        export function hmac(data) {
            return createHmac('sha256', 'secret').update(data).digest('hex');
        }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { hmac } from 'user:hasher';
        export function handler(event) {
            event.hash = hmac(event.data);
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("hasher", hasher_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"data": "hello"}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let hash = json["hash"].as_str().unwrap();
    assert!(!hash.is_empty(), "Hash should not be empty");
    // SHA-256 HMAC hex output is always 64 characters
    assert_eq!(hash.len(), 64, "SHA-256 HMAC hex should be 64 chars");
}

// ── User module importing a host function ────────────────────────────

#[test]
fn user_module_can_import_host_function() {
    let enricher_module = Script::from_content(
        r#"
        import * as db from 'host:db';
        export function enrich(event) {
            const user = db.lookup(event.userId);
            event.userName = user.name;
            return event;
        }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { enrich } from 'user:enricher';
        export function handler(event) {
            return enrich(event);
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.host_module("host:db").register_raw(
        "lookup",
        |args: String| -> hyperlight_js::Result<String> {
            let parsed: serde_json::Value = serde_json::from_str(&args).unwrap();
            let id = parsed[0].as_i64().unwrap();
            let result = serde_json::json!({ "id": id, "name": format!("User {}", id) });
            Ok(serde_json::to_string(&result).unwrap())
        },
    );

    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("enricher", enricher_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", r#"{"userId": 42}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["userName"], "User 42");
}

// ── Error on missing module ──────────────────────────────────────────

#[test]
fn handler_fails_when_importing_nonexistent_module() {
    let handler = Script::from_content(
        r#"
        import { foo } from 'user:nonexistent';
        export function handler(event) {
            event.result = foo();
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    // Loading should fail because the module doesn't exist
    let result = sandbox.get_loaded_sandbox();
    assert!(
        result.is_err(),
        "Should fail when handler imports a non-existent module"
    );
}

// ── Circular module dependencies ─────────────────────────────────────

#[test]
fn circular_module_imports_work() {
    // ESM circular imports are well-defined: live bindings resolve after evaluation.
    let module_a = Script::from_content(
        r#"
        import { getY } from 'user:moduleB';
        export function getX() { return 'X'; }
        export function getXY() { return getX() + getY(); }
        "#,
    );

    let module_b = Script::from_content(
        r#"
        import { getX } from 'user:moduleA';
        export function getY() { return 'Y'; }
        export function getYX() { return getY() + getX(); }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { getXY } from 'user:moduleA';
        import { getYX } from 'user:moduleB';
        export function handler(event) {
            return { xy: getXY(), yx: getYX() };
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("moduleA", module_a).unwrap();
    sandbox.add_module("moduleB", module_b).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["xy"], "XY");
    assert_eq!(json["yx"], "YX");
}

// ── Snapshot / restore interaction with modules ──────────────────────

#[test]
fn snapshot_restore_resets_module_state() {
    let counter_module = Script::from_content(
        r#"
        let count = 0;
        export function increment() { return ++count; }
        "#,
    );

    let handler = Script::from_content(
        r#"
        import { increment } from 'user:counter';
        export function handler(event) {
            event.count = increment();
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("counter", counter_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    // Call once → count=1
    let result = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["count"], 1);

    // Snapshot after count=1
    let snapshot = loaded.snapshot().unwrap();

    // Call again → count=2
    let result = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["count"], 2);

    // Restore → back to count=1
    loaded.restore(snapshot).unwrap();

    // Call again → count=2 (restored to post-snapshot state, so next call is 2)
    let result = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["count"], 2);
}

// ── Module with syntax error ─────────────────────────────────────────

#[test]
fn module_with_syntax_error_fails_at_load() {
    let bad_module = Script::from_content("export function broken( { NOPE }");

    let handler = Script::from_content(
        r#"
        import { broken } from 'user:bad';
        export function handler(event) { return event; }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox.add_module("bad", bad_module).unwrap();
    sandbox.add_handler("handler", handler).unwrap();

    let result = sandbox.get_loaded_sandbox();
    assert!(
        result.is_err(),
        "Should fail when module has a syntax error"
    );
}

// ── Remove then load lifecycle ───────────────────────────────────────

#[test]
fn removed_module_is_unavailable_on_load() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    sandbox
        .add_module(
            "utils",
            Script::from_content("export function greet() { return 'hi'; }"),
        )
        .unwrap();
    sandbox.remove_module("utils").unwrap();

    sandbox
        .add_handler(
            "handler",
            Script::from_content(
                r#"
                import { greet } from 'user:utils';
                export function handler(event) { event.msg = greet(); return event; }
                "#,
            ),
        )
        .unwrap();

    let result = sandbox.get_loaded_sandbox();
    assert!(
        result.is_err(),
        "Should fail when handler imports a removed module"
    );
}
