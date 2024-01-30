#![cfg(not(hyperlight))]

use std::fs::write;
use std::process::Command;

use escargot::CargoBuild;
use tempfile::tempdir;

#[test]
fn smoke_test() {
    let dir = tempdir().unwrap();

    write(
        dir.path().join("index.js"),
        r#"
            import * as math from './math.js';
            function handler(event) {
                console.log(JSON.stringify(event));
                return math.add(event.a, 41);
            }
        "#,
    )
    .unwrap();

    write(
        dir.path().join("math.js"),
        r#"
            const add = (a, b) => a + b;
            const subtract = (a, b) => a - b;
            export { add, subtract };
        "#,
    )
    .unwrap();

    let output = js_runtime_cli()
        .arg(dir.path().join("./index.js"))
        .arg(r#"{"a":1,"b":[1,2,3]}"#)
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.trim().lines().collect::<Vec<_>>();

    assert_eq!(lines, [r#"{"a":1,"b":[1,2,3]}"#, "Handler result: 42",]);
}

fn js_runtime_cli() -> Command {
    CargoBuild::new()
        .manifest_path(env!("CARGO_MANIFEST_PATH"))
        .bin("hyperlight-js-runtime")
        .current_release()
        .current_target()
        .run()
        .unwrap()
        .command()
}
