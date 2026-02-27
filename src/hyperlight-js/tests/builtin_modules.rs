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
