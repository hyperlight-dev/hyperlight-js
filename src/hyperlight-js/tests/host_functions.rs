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

use hyperlight_js::{SandboxBuilder, Script};

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

// ── Binary data (register_js) tests ──────────────────────────────────
//
// These test the binary sidecar round-trip through the hypervisor using
// register_js directly. register_js is #[doc(hidden)] but still pub —
// it's the foundation of the NAPI bridge and needs integration coverage.

#[test]
fn register_js_binary_arg_round_trip() {
    // Guest sends Uint8Array → host receives blobs → returns length as JSON
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const data = new Uint8Array([72, 101, 108, 108, 111]);
            return { len: host.byte_length(data) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "byte_length",
        |_args: serde_json::Value, blobs: Vec<Vec<u8>>| {
            // The first arg should be a placeholder {"__bin__": 0}
            // and blobs should contain the Uint8Array bytes
            let len = if let Some(blob) = blobs.first() {
                blob.len()
            } else {
                // Fallback: try to read from the JSON args
                0
            };
            let result = serde_json::to_string(&len)
                .map_err(|e| hyperlight_js::HyperlightError::Error(format!("JSON error: {e}")))?;
            Ok(hyperlight_js::FnReturn::Json(result))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"len":5}"#);
}

#[test]
fn register_js_binary_return() {
    // Host returns FnReturn::Binary → guest sees Uint8Array
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const data = host.get_bytes();
            return { len: data.length, first: data[0], last: data[4] };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "get_bytes",
        |_args: serde_json::Value, _blobs: Vec<Vec<u8>>| {
            Ok(hyperlight_js::FnReturn::Binary(vec![10, 20, 30, 40, 50]))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"len":5,"first":10,"last":50}"#);
}

#[test]
fn register_js_mixed_args() {
    // Guest sends string + Uint8Array + number → host receives all correctly
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const data = new Uint8Array([1, 2, 3]);
            return { result: host.describe("pfx", data, 42) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "describe",
        |args: serde_json::Value, blobs: Vec<Vec<u8>>| {
            // args is [{"__bin__": 0}, "pfx", 42] or ["pfx", {"__bin__": 0}, 42]
            // depending on arg order. Extract what we need.
            let arr = args.as_array().unwrap();
            let mut prefix = String::new();
            let mut num = 0i64;
            let blob_len = blobs.first().map(|b| b.len()).unwrap_or(0);

            for val in arr {
                if let Some(s) = val.as_str() {
                    prefix = s.to_string();
                } else if let Some(n) = val.as_i64() {
                    num = n;
                }
            }

            let result = format!("{prefix}-{blob_len}-{num}");
            let json = serde_json::to_string(&result)
                .map_err(|e| hyperlight_js::HyperlightError::Error(format!("JSON error: {e}")))?;
            Ok(hyperlight_js::FnReturn::Json(json))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"result":"pfx-3-42"}"#);
}

#[test]
fn register_typed_rejects_binary_args_e2e() {
    // Guest sends Uint8Array to a typed register() function — should error
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            try {
                host.add(new Uint8Array([1, 2]), 3);
                return { error: "should have thrown" };
            } catch (e) {
                return { caught: true };
            }
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

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    // The guest should catch the error from the typed function rejecting binary
    assert_eq!(res, r#"{"caught":true}"#);
}

#[test]
fn register_js_empty_uint8array() {
    // Guest sends empty Uint8Array — should work, blobs[0] is empty vec
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const data = new Uint8Array(0);
            return { len: host.byte_length(data) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "byte_length",
        |_args: serde_json::Value, blobs: Vec<Vec<u8>>| {
            let len = blobs.first().map(|b| b.len()).unwrap_or(0);
            let result = serde_json::to_string(&len)
                .map_err(|e| hyperlight_js::HyperlightError::Error(format!("{e}")))?;
            Ok(hyperlight_js::FnReturn::Json(result))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"len":0}"#);
}

#[test]
fn register_js_multiple_binary_args() {
    // Guest sends two separate Uint8Arrays as args
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const a = new Uint8Array([1, 2, 3]);
            const b = new Uint8Array([4, 5]);
            return { total: host.total_length(a, b) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "total_length",
        |_args: serde_json::Value, blobs: Vec<Vec<u8>>| {
            let total: usize = blobs.iter().map(|b| b.len()).sum();
            let result = serde_json::to_string(&total)
                .map_err(|e| hyperlight_js::HyperlightError::Error(format!("{e}")))?;
            Ok(hyperlight_js::FnReturn::Json(result))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"total":5}"#);
}

#[test]
fn register_js_binary_in_nested_object() {
    // Guest sends an object containing a Uint8Array as a property
    let handler = Script::from_content(
        r#"
        import * as host from "host";
        function handler(event) {
            const payload = {
                name: "test",
                data: new Uint8Array([10, 20, 30]),
                count: 3,
            };
            return { result: host.process(payload) };
        }
        "#,
    );

    let event = r#"{}"#;

    let mut proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    proto_js_sandbox.host_module("host").register_js(
        "process",
        |args: serde_json::Value, blobs: Vec<Vec<u8>>| {
            // The object should have {"name": "test", "data": {"__bin__": 0}, "count": 3}
            // with blobs containing [10, 20, 30]
            let arr = args.as_array().unwrap();
            let obj = arr[0].as_object().unwrap();
            let name = obj.get("name").unwrap().as_str().unwrap();
            let blob_len = blobs.first().map(|b| b.len()).unwrap_or(0);
            let result = format!("{name}-{blob_len}");
            let json = serde_json::to_string(&result)
                .map_err(|e| hyperlight_js::HyperlightError::Error(format!("{e}")))?;
            Ok(hyperlight_js::FnReturn::Json(json))
        },
    );

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, r#"{"result":"test-3"}"#);
}

// ── Numeric type tests ───────────────────────────────────────────────
// QuickJS stores JSON-parsed numbers as doubles internally. The binary
// host function path (extract_binaries → value_to_json_with_binaries)
// must serialize whole-number floats as integers to preserve serde
// deserialization on the host side.

#[test]
fn host_fn_with_i32_arg_from_event_data() {
    // event.x is parsed from JSON → stored as f64 in QuickJS → must
    // arrive at the host as an integer, not 42.0
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.double(event.x) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("math", "double", |x: i32| x * 2).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"x": 42}"#.to_string(), None)
        .unwrap();
    assert_eq!(res, r#"{"result":84}"#);
}

#[test]
fn host_fn_with_i64_arg_from_event_data() {
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.negate(event.x) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("math", "negate", |x: i64| -x).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"x": 100}"#.to_string(), None)
        .unwrap();
    assert_eq!(res, r#"{"result":-100}"#);
}

#[test]
fn host_fn_with_f64_arg_preserves_fractional() {
    // Actual floats (3.14) must remain as floats, not be truncated
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.half(event.x) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("math", "half", |x: f64| x / 2.0).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"x": 3.14}"#.to_string(), None)
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&res).unwrap();
    let result = json["result"].as_f64().unwrap();
    assert!(
        (result - 1.57).abs() < 0.001,
        "Expected ~1.57, got {result}"
    );
}

#[test]
fn host_fn_with_bool_arg() {
    let handler = Script::from_content(
        r#"
        import * as logic from "logic";
        function handler(event) {
            return { result: logic.flip(event.flag) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("logic", "flip", |b: bool| !b).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"flag": true}"#.to_string(), None)
        .unwrap();
    assert_eq!(res, r#"{"result":false}"#);
}

#[test]
fn host_fn_with_mixed_numeric_types() {
    // i32 + f64 mix in the same call
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.weighted_add(event.a, event.b, event.weight) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto
        .register("math", "weighted_add", |a: i32, b: i32, w: f64| {
            (a as f64 * w + b as f64 * (1.0 - w)) as i32
        })
        .unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event(
            "handler",
            r#"{"a": 100, "b": 200, "weight": 0.75}"#.to_string(),
            None,
        )
        .unwrap();
    assert_eq!(res, r#"{"result":125}"#);
}

#[test]
fn host_fn_with_negative_integer() {
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.abs(event.x) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("math", "abs", |x: i32| x.abs()).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"x": -42}"#.to_string(), None)
        .unwrap();
    assert_eq!(res, r#"{"result":42}"#);
}

#[test]
fn host_fn_with_zero() {
    let handler = Script::from_content(
        r#"
        import * as math from "math";
        function handler(event) {
            return { result: math.inc(event.x) };
        }
        "#,
    );

    let mut proto = SandboxBuilder::new().build().unwrap();
    proto.register("math", "inc", |x: i32| x + 1).unwrap();

    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", r#"{"x": 0}"#.to_string(), None)
        .unwrap();
    assert_eq!(res, r#"{"result":1}"#);
}
