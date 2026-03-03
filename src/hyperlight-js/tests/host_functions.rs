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
//! Test for host modules / functions.

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{new_error, SandboxBuilder, Script};

#[test]
fn can_call_host_functions() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        import * as utils from "utils";
        function handler(event) {
            let a = host.print("Hello, World!!");
            let b = 24;
            let c = utils.add(a, b);
            return { c };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox
        .register("host", "print", |msg: String| {
            assert_eq!(msg, "Hello, World!!");
            42
        })
        .unwrap();
    proto_js_sandbox
        .register("utils", "add", |a: i32, b: i32| {
            assert_eq!(a, 42);
            assert_eq!(b, 24);
            66
        })
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"c":66}"#);
}

#[test]
fn should_fail_for_invalid_host_function() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            host.print2("Hello, World!!");
            return { };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox
        .register("host", "print", |msg: String| {
            assert_eq!(msg, "Hello, World!!");
            42
        })
        .unwrap();
    proto_js_sandbox
        .register("utils", "add", |a: i32, b: i32| {
            assert_eq!(a, 42);
            assert_eq!(b, 24);
            66
        })
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap_err();

    println!("Error: {:?}", res);
}

#[test]
fn host_modules_should_contain_all_host_functions() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {;
            return Object.keys(host);
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.register("host", "life", || 42).unwrap();
    proto_js_sandbox
        .register("host", "add", |a: i32, b: i32| a + b)
        .unwrap();
    proto_js_sandbox
        .register("host", "hello", || String::from("Hello, World!"))
        .unwrap();
    proto_js_sandbox
        .register("host", "no-op", || String::from("Hello, World!"))
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    let mut res: Vec<String> = serde_json::from_str(&res).unwrap();
    res.sort();

    assert_eq!(res, &["add", "hello", "life", "no-op"]);
}

#[test]
fn host_fn_call_should_fail_if_wrong_arg_types() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {;
            return host.add("10", "32");
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox
        .register("host", "add", |a: i32, b: i32| a + b)
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let err = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap_err();

    assert!(err.to_string().contains("HostFunctionError"));
}

#[test]
fn host_fn_with_unusual_names() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {;
            return host["scoped/add/😊"](10, 32);
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox
        .register("host", "scoped/add/😊", |a: i32, b: i32| a + b)
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert!(res == "42");
}

#[test]
fn register_raw_basic() {
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.add(10, 32) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    // register_raw receives the guest args as a JSON string "[10,32]"
    // and must return a JSON string result.
    proto_js_sandbox
        .register_raw("math", "add", |args: String| {
            let parsed: Vec<i64> = serde_json::from_str(&args)?;
            let sum: i64 = parsed.iter().sum();
            Ok(serde_json::to_string(&sum)?)
        })
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"result":42}"#);
}

#[test]
fn register_raw_mixed_with_typed() {
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            let sum = math.add(10, 32);
            let doubled = math.double(sum);
            return { result: doubled };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    // Typed registration via the Function trait
    proto_js_sandbox
        .register("math", "add", |a: i32, b: i32| a + b)
        .unwrap();

    // Raw registration alongside typed — both in the same module
    proto_js_sandbox
        .register_raw("math", "double", |args: String| {
            let parsed: Vec<i64> = serde_json::from_str(&args)?;
            let val = parsed.first().copied().unwrap_or(0);
            Ok(serde_json::to_string(&(val * 2))?)
        })
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"result":84}"#);
}

#[test]
fn register_raw_error_propagation() {
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            return host.fail();
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox
        .register_raw("host", "fail", |_args: String| {
            Err(new_error!("intentional failure from raw host fn"))
        })
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let err = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap_err();

    assert!(err.to_string().contains("intentional failure"));
}

#[test]
fn register_raw_via_host_module() {
    let handler = Script::from_content(
        r#"
        import * as utils from "utils";
        function handler(event) {
            let greeting = utils.greet("World");
            return { greeting };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    // Use host_module() accessor + register_raw() directly on HostModule
    proto_js_sandbox
        .host_module("utils")
        .register_raw("greet", |args: String| {
            let parsed: Vec<String> = serde_json::from_str(&args)?;
            let name = parsed.first().cloned().unwrap_or_default();
            Ok(serde_json::to_string(&format!("Hello, {}!", name))?)
        });

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"greeting":"Hello, World!"}"#);
}
