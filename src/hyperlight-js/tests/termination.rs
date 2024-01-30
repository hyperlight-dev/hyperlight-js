//! Test manual termination of the sandbox (i.e., without using a monitor)

#![allow(clippy::disallowed_macros)]

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use hyperlight_js::{HyperlightError, Result, SandboxBuilder, Script};

#[ignore]
#[test]
fn handle_termination() -> Result<()> {
    let handler = Script::from_content(
        r#"
    function handler(event) {
        const start = Date.now();
        let now = start;
        while (now - start < 4000) {
            now = Date.now();
        }
        return event
    }
    "#,
    );

    let empty_event = "{}";

    let proto_js_sandbox = SandboxBuilder::new().build()?;

    let mut sandbox = proto_js_sandbox.load_runtime()?;

    sandbox.add_handler("handler", handler)?;

    let mut loaded_sandbox = sandbox.get_loaded_sandbox()?;

    // Verify sandbox is not poisoned before we start
    assert!(
        !loaded_sandbox.poisoned(),
        "Sandbox should not be poisoned initially"
    );

    // Take a snapshot
    let snapshot = loaded_sandbox.snapshot()?;

    let interrupt_handle = loaded_sandbox.interrupt_handle();
    let barrier1 = Arc::new(Barrier::new(2));
    let barrier2 = barrier1.clone();

    let thread = std::thread::spawn(move || {
        barrier1.wait();
        println!(
            "{} - Waiting for 1 sec before sending interrupts...",
            chrono::Local::now().format("%H:%M:%S%.6f")
        );
        thread::sleep(Duration::from_secs(1));
        println!(
            "{} - Sending interrupts...",
            chrono::Local::now().format("%H:%M:%S%.6f")
        );
        interrupt_handle.kill();
        println!(
            "{} - Interrupts sent",
            chrono::Local::now().format("%H:%M:%S%.6f")
        );
    });

    let res = {
        barrier2.wait();
        println!(
            "{} - Starting to handle event",
            chrono::Local::now().format("%H:%M:%S%.6f")
        );
        let res = loaded_sandbox
            .handle_event("handler", empty_event.to_string(), None)
            .unwrap_err();
        println!(
            "{} - Finished handling event",
            chrono::Local::now().format("%H:%M:%S%.6f")
        );
        res
    };

    thread.join().expect("kill thread panicked");

    assert!(matches!(res, HyperlightError::ExecutionCanceledByHost()));

    // Verify sandbox is poisoned after interruption
    assert!(
        loaded_sandbox.poisoned(),
        "Sandbox should be poisoned after interruption"
    );

    // Restore the sandbox from snapshot
    loaded_sandbox.restore(&snapshot)?;

    // Verify sandbox is no longer poisoned after restore
    assert!(
        !loaded_sandbox.poisoned(),
        "Sandbox should not be poisoned after restore"
    );

    Ok(())
}
