//! Test the behaviour of JavaScript handlers

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

#[test]
fn handle_event() {
    let handler = Script::from_content(
        r#"
        function handler(event) {
            event.result = "Hello, " + event.name + "!";
            return event
        }
        "#,
    );

    let event = r#"
    {
        "name": "world",
        "result": ""
    }"#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res, r#"{"name":"world","result":"Hello, world!"}"#);
}

#[test]
fn check_javascript_handler_returns_value() {
    let handler = Script::from_content(
        r#"
        function handler(event) {
            1
        }
        "#,
    );

    let event = r#"{}"#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);

    assert!(res.is_err());

    let err = res.unwrap_err();

    assert_eq!(
        err.to_string(),
        "Guest error occurred GuestError: Error: The handler function did not return a value"
    );
}

#[test]
fn add_handler_rejects_empty_name() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let script = Script::from_content("function handler(e) { return e; }");
    let result = sandbox.add_handler("", script);
    assert!(result.is_err(), "Empty handler name should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty name, got: {err}"
    );
}

#[test]
fn remove_handler_rejects_empty_name() {
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();

    let result = sandbox.remove_handler("");
    assert!(result.is_err(), "Empty handler name should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty name, got: {err}"
    );
}

#[test]
fn handle_event_rejects_empty_name() {
    let handler = Script::from_content("function handler(e) { return e; }");
    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result = loaded.handle_event("", "{}".to_string(), None);
    assert!(result.is_err(), "Empty handler name should be rejected");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("must not be empty"),
        "Error should mention empty name, got: {err}"
    );
}
