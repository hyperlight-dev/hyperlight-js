# How to create and process crash dumps for Hyperlight guests

This document explains how to create and process crash dumps for hyperlight-js guests.

## Dumping the guest state to an ELF core dump 

When a guest crashes because of an unknown VmExit or unhandled exception, the vCPU state can be optionally dumped to an `ELF` core dump file.
This can be used to inspect the state of the guest at the time of the crash.

To make Hyperlight dump the state of the vCPU (general purpose registers, registers) to an `ELF` core dump file, enable the `crashdump` feature and run.
The feature enables the creation of core dump files for both debug and release builds of Hyperlight hosts.
By default, Hyperlight places the core dumps in the temporary directory (platform specific).
To change this, use the `HYPERLIGHT_CORE_DUMP_DIR` environment variable to specify a directory.
The name and location of the dump file will be printed to the console and logged as an error message.

**NOTE**: If the directory provided by `HYPERLIGHT_CORE_DUMP_DIR` does not exist, Hyperlight places the file in the temporary directory.
**NOTE**: By enabling the `crashdump` feature, you instruct Hyperlight to create core dump files for all sandboxes when an unhandled crash occurs.
To selectively disable this feature for a specific sandbox, you can set `with_crashdump_enabled` to `false` in the `SandboxBuilder`.
```rust
let mut js_sandbox = SandboxBuilder::new()
    .with_crashdump_enabled(false) // Disable core dump for this sandbox
    .build()?
    .load_runtime()?;
```

## Creating a dump on demand

You can also create a core dump of the current state of the guest on demand by calling the `generate_crashdump` method on a `JSSandbox` or `LoadedJSSandbox` instance. This can be useful for debugging issues in the guest that do not cause crashes (e.g., a guest function that does not return).

This is only available when the `crashdump` feature is enabled and then only if the sandbox
is also configured to allow core dumps (which is the default behavior).

### Example

Attach to your running process with gdb and call this function:

```shell
sudo gdb -p <pid_of_your_process>
(gdb) info threads
# find the thread that is running the guest function you want to debug
(gdb) thread <thread_number>
# switch to the frame where you have access to your sandbox instance
(gdb) backtrace
(gdb) frame <frame_number>
# get the pointer to your sandbox instance
# Get the sandbox pointer
(gdb) print sandbox
# Call the crashdump function with the pointer
    # Call the crashdump function 
call sandbox.generate_crashdump()
```
The crashdump should be available `/tmp` or in the crash dump directory (see `HYPERLIGHT_CORE_DUMP_DIR` env var). To make this process easier, you can also create a gdb script that automates these steps. You can find an example script [here](https://github.com/hyperlight-dev/hyperlight/blob/main/src/hyperlight_host/scripts/dump_all_sandboxes.gdb). This script will try and generate a crashdump for every active thread except thread 1 , it assumes that the variable sandbox exists in frame 15 on every thread. You can edit it to fit your needs. Then use it like this:

```shell
(gdb) source scripts/dump_all_sandboxes.gdb
(gdb) dump_all_sandboxes
```

### Inspecting the core dump

After the core dump has been created, to inspect the state of the guest, load the core dump file using `gdb` or `lldb`.
**NOTE: This feature has been tested with version `15.0` of `gdb` and version `17` of `lldb`, earlier versions may not work, it is recommended to use these versions or later.**

To do this in vscode, the following configuration can be used to add debug configurations:

```vscode
{
    "version": "0.2.0",
    "inputs": [
        {
            "id": "core_dump",
            "type": "promptString",
            "description": "Path to the core dump file",
        },
        {
            "id": "program",
            "type": "promptString",
            "description": "Path to the program to debug",
        }
    ],
    "configurations": [
        {
            "name": "[GDB] Load core dump file",
            "type": "cppdbg",
            "request": "launch",
            "program": "${input:program}",
            "coreDumpPath": "${input:core_dump}",
            "cwd": "${workspaceFolder}",
            "MIMode": "gdb",
            "externalConsole": false,
            "miDebuggerPath": "/usr/bin/gdb",
            "setupCommands": [
            {
                "description": "Enable pretty-printing for gdb",
                "text": "-enable-pretty-printing",
                "ignoreFailures": true
            },
            {
                "description": "Set Disassembly Flavor to Intel",
                "text": "-gdb-set disassembly-flavor intel",
                "ignoreFailures": true
            }
            ]
        },
        {
        "name": "[LLDB] Load core dump file",
        "type": "lldb",
        "request": "launch",
        "stopOnEntry": true,
        "processCreateCommands": [],
        "targetCreateCommands": [
            "target create -c ${input:core_dump} ${input:program}",
        ],
        },
    ]
}
```
**NOTE: The `CodeLldb` debug session does not stop after launching. To see the code, stack frames and registers you need to
press the `pause` button. This is a known issue with the `CodeLldb` extension [#1245](https://github.com/vadimcn/codelldb/issues/1245).
The `cppdbg` extension works as expected and stops at the entry point of the program.**
