//! Test the built-in crypto module

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{SandboxBuilder, Script};

#[test]
fn crypto_create_hmac() {
    let handler = Script::from_content(
        r#"
        function handler(event) {
            var crypto = require('crypto');
            var key = "TULWi2fOzLr9GcJeArpS4o135bEGmFhdUjpBSxUeJxXtIlx6qh";
            var data = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJUZXN0U3ViamVjdCIsIm5hbWUiOiJUZXN0IFVzZXIiLCJpYXQiOjE3MTg4OTE5MTksIm5iZiI6MTcxODg5MTkxOSwiZXhwIjoxODc2NjU4MzQ2fQ";
            var hmac = crypto.createHmac('sha256', key).update(data);
            event.signature_b64_url = hmac.digest('base64url');
            event.signature_b64 = hmac.digest('base64');
            event.signature_hex = hmac.digest('hex');
            String.bytesFrom("SGVsbG8gV29ybGQhIQo=", "base64url");
            String.bytesFrom("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9", "base64url");
            String.bytesFrom("eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyLCJuYmYiOjE1MTYyMzkwMjIsImV4cCI6MTcxNjIzOTAyMn0=","base64url");
            String.bytesFrom("eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyLCJuYmYiOjE1MTYyMzkwMjIsImV4cCI6MTcxNjIzOTAyMn0","base64url");
            return event;
        }
        "#,
    );

    let event = r#"
    {
        "signature_b64_url": "",
        "signature_b64": "",
        "signature_hex": ""
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    sandbox.add_handler("handler", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();

    let res = loaded_sandbox.handle_event("handler", event.to_string(), None);
    assert!(res.is_ok());

    let res = res.unwrap();

    assert_eq!(
        res,
        r#"{"signature_b64_url":"uRMcKIrmGTb0LDN0IxDF0kyS8zy2E5RZwV_L66XGHg8","signature_b64":"uRMcKIrmGTb0LDN0IxDF0kyS8zy2E5RZwV/L66XGHg8=","signature_hex":"b9131c288ae61936f42c33742310c5d24c92f33cb6139459c15fcbeba5c61e0f"}"#
    );
}
