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
