# Debugging the guest runtime

This guide provides instructions on how to debug `hyperlight-js-runtime` using GDB or LLDB.

## Getting started

To enable debugging, you need to build the `hyperlight-js` library in **debug mode** and with the `gdb` feature enabled. This will include the necessary debug symbols and enable the GDB server in the runtime.

When the `gdb` feature is enabled, you can specify a port for the GDB server to listen on when creating the sandbox.
```rust
let proto_sandbox = SandboxBuilder::new()
    .with_debugging_enabled(8080) // debugging on port 8080
    .build()?;
```
This will start the GDB server on port 8080 when the sandbox is created.

A breakpoint is inserted on the VM entry point, inside the VM on the `load_runtime()` method.
This will allow you to connect to the GDB server before the guest code starts executing, and set additional breakpoints as needed.
```rust
let sandbox = proto_sandbox.load_runtime()?; // Breakpoint is hit when the guest code starts executing here
// ℹ️ This thread will be paused at the breakpoint until you connect to the GDB server and continue execution.
```

### Connecting with GDB

You can connect to the GDB server by running the following command in the terminal:
```bash
gdb \
    target/hyperlight-js-runtime/x86_64-hyperlight-none/debug/hyperlight-js-runtime \
    -ex "target remote localhost:8080"
```

### Connecting with LLDB

Alternatively, you can connect using LLDB:
```bash
lldb \
    target/hyperlight-js-runtime/x86_64-hyperlight-none/debug/hyperlight-js-runtime \
    -o "gdb-remote localhost:8080"
```

## Example

You can find an example of how to set up the sandbox for debugging in `src/hyperlight-js/examples/runtime_debugging/main.rs`. This example creates a sandbox with debugging enabled and loads the runtime, allowing you to connect to it with GDB or LLDB as described above.

You can also check the GDB and LLDB configurations in `.vscode/launch.json` for examples of how to set up debugging configurations in Visual Studio Code.

You can run the example with the following command:
```bash
cargo run --example runtime_debugging --features gdb
```

And then from the "Run and Debug" tab in Visual Studio Code, you can select the "Remote GDB attach" or "Remote LLDB attach" configuration to start debugging the guest runtime.