#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

#[test]
fn builtin_globals_should_be_defined() {
    let handler = Script::from_content(
        r#"
        function assert(condition, message) {
            if (!condition) {
                throw new Error(message);
            }
        }

        function handler(event) {
            assert(typeof console.log === "function", "console.log should be defined");
            assert(typeof print === "function", "print should be defined");
            assert(typeof require === "function", "require should be defined");
            assert(typeof String.bytesFrom === "function", "String.bytesFrom should be defined");

            return 0;
        }
        "#,
    );

    let event = r#"{}"#;

    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler", event.to_string(), None)
        .unwrap();

    assert_eq!(res, "0");
}
