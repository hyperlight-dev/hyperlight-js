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
use hyperlight_js::{Result, SandboxBuilder, Script};

fn builder() -> SandboxBuilder {
    #[cfg(all(feature = "gdb", debug_assertions))]
    {
        SandboxBuilder::new()
            .with_guest_input_buffer_size(2 * 1024 * 1024) // 2 MiB
            .with_guest_heap_size(10 * 1024 * 1024) // 10 MiB
            .with_debugging_enabled(8080) // debugging on port 8080
    }
    #[cfg(not(all(feature = "gdb", debug_assertions)))]
    SandboxBuilder::new()
}

fn main() -> Result<()> {
    println!("🕵️  Runtime Debugging Example: Debugging the guest runtime with GDB\n");

    let proto_js_sandbox = builder().build()?;

    #[cfg(all(feature = "gdb", debug_assertions))]
    println!("🪳  You can now connect to the GDB server from another terminal and set breakpoints in the hyperlight-js-runtime code to debug it.
\x1b[1m     $ gdb target/hyperlight-js-runtime/x86_64-hyperlight-none/debug/hyperlight-js-runtime -ex \"target remote localhost:8080\"\x1b[0m
");

    #[cfg(all(feature = "gdb", debug_assertions))]
    println!("ℹ️  Execution will resume once you have connected to the GDB server and continued execution.");

    #[cfg(not(all(feature = "gdb", debug_assertions)))]
    println!("⚠️  The GDB feature is not enabled, build with `--features=gdb` and in debug mode.");

    let mut sandbox = proto_js_sandbox.load_runtime()?;

    sandbox.add_handler(
        "handler",
        Script::from_content(
            r#"
                function handler(event) {
                    console.log("hello, world!");
                    return { "n": 42 };
                }
            "#,
        ),
    )?;

    let mut loaded_sandbox = sandbox.get_loaded_sandbox()?;

    match loaded_sandbox.handle_event("handler", "{}".to_string(), Some(false)) {
        Ok(result) => println!("✅ Handler executed successfully with result: {}", result),
        Err(e) => println!("❌ Handler execution failed with error: {}", e),
    }

    Ok(())
}
