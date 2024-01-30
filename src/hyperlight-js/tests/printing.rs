//! Tests for output printing from the sandbox

#![allow(clippy::disallowed_macros)]

use std::sync::mpsc::channel;

use hyperlight_js::{SandboxBuilder, Script};

fn host_print_fn() -> (
    impl Fn(String) -> i32 + Send + Sync + Clone + 'static,
    impl Fn() -> String,
) {
    let (tx, rx) = channel();
    let tx_fn = move |msg: String| {
        tx.send(msg).unwrap();
        0i32
    };

    let rx_fn = move || -> String {
        let mut result = String::new();
        while let Ok(msg) = rx.try_recv() {
            result.push_str(&msg);
        }
        result
    };

    (tx_fn, rx_fn)
}

#[test]
fn console_log_writes_to_host_print_function() {
    let handler_1 = Script::from_content(
        r#"
    function handler(event) {
        console.log("Hello, World!!");
        return event
    }
    "#,
    );

    let handler_2 = Script::from_content(
        r#"
    function handler(event) {
        console.log("");
        return event
    }   
    "#,
    );

    let event = r#"
    {
    }"#;

    let (fn_writer, output) = host_print_fn();

    let proto_js_sandbox = SandboxBuilder::new()
        .with_host_print_fn(fn_writer.clone().into())
        .build()
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler_1).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());
    assert_eq!(output(), "Hello, World!!\n");

    let proto_js_sandbox = SandboxBuilder::new()
        .with_host_print_fn(fn_writer.into())
        .build()
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler_2).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());
    assert_eq!(output(), "\n");
}

#[test]
fn print_writes_to_host_print_function() {
    let handler_1 = Script::from_content(
        r#"
    function handler(event) {
        print("Hello, World!!");
        return event
    }
    "#,
    );

    let handler_2 = Script::from_content(
        r#"
    function handler(event) {
        print("");
        return event
    }
    "#,
    );

    let event = r#"
    {
    }"#;

    let (fn_writer, output) = host_print_fn();

    let proto_js_sandbox = SandboxBuilder::new()
        .with_host_print_fn(fn_writer.clone().into())
        .build()
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler_1).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());
    assert_eq!(output(), "Hello, World!!");

    let proto_js_sandbox = SandboxBuilder::new()
        .with_host_print_fn(fn_writer.into())
        .build()
        .unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler_2).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());
    assert_eq!(output(), "");
}
