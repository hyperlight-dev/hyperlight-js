//! Interrupt Example: Demonstrates timeout-based handler termination and poisoned state recovery
//!
//! This example shows how to:
//! 1. Use `interrupt_handle().kill()` to terminate long-running handlers
//! 2. Check the `poisoned()` state after interruption
//! 3. Use `snapshot()` and `restore()` to recover from poisoned state
//!
//! Run with: cargo run --example interrupt

#![allow(clippy::disallowed_macros)]

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use hyperlight_js::{SandboxBuilder, Script};

fn main() -> Result<()> {
    println!("⏱️  Interrupt Example: Timeout-based handler termination\n");

    // Create sandbox
    let proto_js_sandbox = SandboxBuilder::new().build()?;
    let mut sandbox = proto_js_sandbox.load_runtime()?;

    // Handler that runs for 4 seconds (will be interrupted)
    let slow_handler = Script::from_content(
        r#"
        function handler(event) {
            const start = Date.now();
            let now = start;
            while (now - start < 4000) {
                now = Date.now();
            }
            event.message = "Handler completed";
            return event;
        }
        "#,
    );

    sandbox.add_handler("handler", slow_handler)?;
    let mut loaded_sandbox = sandbox.get_loaded_sandbox()?;

    // Verify sandbox is not poisoned initially
    println!("🔒 Initial poisoned state: {}", loaded_sandbox.poisoned());
    assert!(
        !loaded_sandbox.poisoned(),
        "Sandbox should not be poisoned initially"
    );

    // Take a snapshot before continuing
    println!("📸 Taking snapshot for recovery...\n");
    let snapshot = loaded_sandbox.snapshot()?;

    // Get interrupt handle for killing the handler
    let interrupt_handle = loaded_sandbox.interrupt_handle();

    // Use a barrier to synchronize the main thread and the kill thread
    let barrier1 = Arc::new(Barrier::new(2));
    let barrier2 = barrier1.clone();

    // Spawn a thread that will kill the handler after 1 second
    let kill_thread = thread::spawn(move || {
        barrier1.wait();
        println!("⏱️  Kill thread: Waiting 1 second before sending interrupt...");
        thread::sleep(Duration::from_secs(1));
        println!("💀 Kill thread: Sending interrupt!");
        interrupt_handle.kill();
        println!("✅ Kill thread: Interrupt sent!");
    });

    // Start handling the event (this will be interrupted)
    barrier2.wait();
    println!("🚀 Main thread: Starting handler (4-second busy loop)...");

    let result = loaded_sandbox.handle_event("handler", "{}".to_string(), None);

    // Wait for the kill thread to finish
    kill_thread.join().expect("kill thread panicked");

    // Check the result
    match result {
        Ok(output) => {
            println!("❌ Unexpected: Handler completed with output: {}", output);
        }
        Err(hyperlight_js::HyperlightError::ExecutionCanceledByHost()) => {
            println!("\n✅ Handler was properly interrupted!");
            println!("🔒 Poisoned after interrupt: {}", loaded_sandbox.poisoned());
            assert!(
                loaded_sandbox.poisoned(),
                "Sandbox should be poisoned after interruption"
            );
        }
        Err(e) => {
            println!("❌ Unexpected error: {:?}", e);
            return Err(e.into());
        }
    }

    // Demonstrate recovery from poisoned state
    println!("\n📸 Restoring sandbox from snapshot...");
    loaded_sandbox.restore(&snapshot)?;

    println!("🔒 Poisoned after restore: {}", loaded_sandbox.poisoned());
    assert!(
        !loaded_sandbox.poisoned(),
        "Sandbox should not be poisoned after restore"
    );

    println!("✅ Sandbox recovered and ready for use!\n");

    // Demonstrate the sandbox works after recovery
    println!("🧪 Testing recovered sandbox with a simple handler...");

    // Need to unload and add a new handler
    let mut sandbox = loaded_sandbox.unload()?;
    sandbox.clear_handlers();

    let simple_handler = Script::from_content(
        r#"
        function handler(event) {
            event.message = "Hello from recovered sandbox!";
            return event;
        }
        "#,
    );
    sandbox.add_handler("handler", simple_handler)?;
    let mut loaded_sandbox = sandbox.get_loaded_sandbox()?;

    let result = loaded_sandbox.handle_event("handler", "{}".to_string(), None)?;
    println!("📤 Output: {}", result);

    println!("\n💡 How it works:");
    println!("   - interrupt_handle.kill() terminates running guest code");
    println!("   - Sandbox becomes poisoned after interruption");
    println!("   - Use snapshot/restore to recover from poisoned state");
    println!("   - unload() also works to recover (resets to clean state)");

    Ok(())
}
