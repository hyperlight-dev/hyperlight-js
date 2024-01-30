//! Tests for the module loader that import files from the embedded filesystem.

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{embed_modules, SandboxBuilder, Script};

#[test]
fn test_handler_with_multiple_imports() {
    let fs = embed_modules! {
        "math.js" => "fixtures/math.js",
        "strings.js" => "fixtures/strings.js",
    };

    // Create handler that imports both modules
    let handler_content = r#"
    import { add, multiply } from './math.js';
    import { toUpperCase, concat } from './strings.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        event.message = toUpperCase(concat('Result: ', event.sum));
        return event;
    }
    "#;

    let event = r#"{"a": 5, "b": 3}"#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let proto_js_sandbox = proto_js_sandbox.set_module_loader(fs).unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();
    let res = loaded_sandbox
        .handle_event("calculator", event.to_string(), None)
        .unwrap();

    assert!(res.contains(r#""sum":8"#));
    assert!(res.contains(r#""product":15"#));
    assert!(res.contains(r#""message":"RESULT: 8"#));
}

#[test]
fn test_handler_import_restrictions() {
    let fs = embed_modules! {
        "math.js" => "fixtures/math.js",
        // strings.js not loaded
    };

    // Create handler that imports both modules
    let handler_content = r#"
    import { add, multiply } from './math.js';
    import { toUpperCase, concat } from './strings.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        event.message = toUpperCase(concat('Result: ', event.sum));
        return event;
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let proto_js_sandbox = proto_js_sandbox.set_module_loader(fs).unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    let res = sandbox.get_loaded_sandbox();
    assert!(
        res.is_err(),
        "Expected module not found error for strings.js, got: {:?}",
        res
    );
}

#[test]
fn test_resolve_module_without_resolver_set() {
    let handler_content = r#"
    import { add, multiply } from './math.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        return event;
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    // This should fail because we haven't set module loader
    let res = sandbox.get_loaded_sandbox();
    assert!(res.is_err());
}

#[test]
fn test_handler_import_from_a_subfolder() {
    let fs = embed_modules! {
        "hitchhiker.js" => "fixtures/hitchhiker.js",
        "galaxy/deepThought.js" => "fixtures/galaxy/deepThought.js",
        "galaxy/index.js" => "fixtures/galaxy/index.js",
    };

    // Create handler that imports a module which itself imports another module
    let handler_content = r#"
    import { ultimateQuestionOfEverything } from './galaxy/index.js';

    function handler(event) {
        return ultimateQuestionOfEverything;
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let proto_js_sandbox = proto_js_sandbox.set_module_loader(fs).unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("hitchhiker", handler).unwrap();

    let event = r#"{}"#;
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();
    let res = loaded_sandbox
        .handle_event("hitchhiker", event.to_string(), None)
        .unwrap();

    assert_eq!(res, "42");
}
