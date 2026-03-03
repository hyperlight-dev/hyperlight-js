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
//! Test some key aspects of the JavaScript runtime

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

#[test]
fn js_date_time_now_is_correct() {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::Deserialize;

    let handler = Script::from_content(
        r#"
        function handler(event) {
            let now = Date.now();
            console.log("now is:", now,"\n");
            event.now = now;
            return event
        }
        "#,
    );

    let event = r#"
    {
        "now":0
    }"#;

    #[derive(Deserialize, Debug)]
    struct Event {
        now: u64,
    }

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok(), "Error: {:?}", res);

    // Get the current system time in milliseconds since the Unix epoch
    let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let system_time_millis = system_time.as_secs() * 1000 + u64::from(system_time.subsec_millis());

    let res = res.unwrap();

    let result: Event = serde_json::from_str(&res).unwrap();
    println!("{:?}", result);

    // Allow a difference of less than 2 seconds
    assert!(system_time_millis - result.now < 2000);
}

#[test]
fn localtime_r_garbage_timezone_on_max_date() {
    let handler = Script::from_content(
        r#"
        function handler() {
            return JSON.stringify({
                valid: new Date(1735689600000).toString(),
                max: new Date(8640000000000000).toString(),
                min: new Date(-8640000000000000).toString(),
                overflow: new Date(8640000000000001).toString(),
                underflow: new Date(-8640000000000001).toString(),
            });
        }
        "#,
    );

    let sb = SandboxBuilder::new().build().unwrap();
    let mut rt = sb.load_runtime().unwrap();
    rt.add_handler("handler", handler).unwrap();
    let mut loaded = rt.get_loaded_sandbox().unwrap();

    let res = loaded
        .handle_event("handler", "{}".to_string(), None)
        .unwrap();
    let inner: String = serde_json::from_str(&res).unwrap();
    let result: serde_json::Value = serde_json::from_str(&inner).unwrap();

    let valid = result["valid"].as_str().unwrap();
    let max = result["max"].as_str().unwrap();
    let min = result["min"].as_str().unwrap();
    let overflow = result["overflow"].as_str().unwrap();
    let underflow = result["underflow"].as_str().unwrap();
    println!("Valid date:     {valid}");
    println!("Max date:       {max}");
    println!("Min date:       {min}");
    println!("Overflow date:  {overflow}");
    println!("Underflow date: {underflow}");

    // Check that we return the correct string representations for each date.
    assert_eq!(valid, "Wed Jan 01 2025 00:00:00 GMT+0000",);
    assert_eq!(max, "Sat Sep 13 275760 00:00:00 GMT+0000",);
    assert_eq!(min, "Tue Apr 20 -271821 00:00:00 GMT+0000",);
    assert_eq!(overflow, "Invalid Date",);
    assert_eq!(underflow, "Invalid Date",);
}

#[test]
fn async_support() {
    let handler = Script::from_content(
        r#"
        async function do_something() {
            return 1234;
        }

        async function handler(event) {
            const result = await do_something();
            return result;
        }
        "#,
    );

    let event = r#"{}"#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();

    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox
        .handle_event("handler".to_string(), event.to_string(), None)
        .unwrap();
    assert_eq!(res, "1234");
}
