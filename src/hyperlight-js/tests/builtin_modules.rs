//! Tests for the built-in (native) modules

#![allow(clippy::disallowed_macros)]

use std::collections::{HashMap, HashSet};

use hyperlight_js::{SandboxBuilder, Script};

#[test]
fn modules_exist_and_contains_expected_exports() {
    let handler = Script::from_content(
        r#"
        import * as crypto from "crypto";
        import * as console from "console";
        import * as io from "io";
        import * as require from "require";

        function handler(event) {
            return {
                crypto: Object.keys(crypto),
                console: Object.keys(console),
                io: Object.keys(io),
                require: Object.keys(require),
            };
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

    let res: HashMap<String, HashSet<String>> = serde_json::from_str(&res).unwrap();

    assert_eq!(
        res,
        HashMap::from([
            (
                "crypto".to_string(),
                HashSet::from(["Hmac".to_string(), "createHmac".to_string()])
            ),
            ("console".to_string(), HashSet::from(["log".to_string()])),
            (
                "io".to_string(),
                HashSet::from(["print".to_string(), "flush".to_string()])
            ),
            (
                "require".to_string(),
                HashSet::from(["default".to_string(), "require".to_string()])
            ),
        ])
    );
}
