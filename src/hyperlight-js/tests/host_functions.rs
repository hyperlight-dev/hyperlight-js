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
