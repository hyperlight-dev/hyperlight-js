# Hyperlight-js

Provides a capability to run JavaScript inside of Hyperlight using quickjs as the JavaScript engine.

## Documentation

- [Execution Monitors](docs/execution-monitors.md) - Timeout and resource limit enforcement for handler execution
- [Observability](docs/observability.md) - Metrics and tracing
- [Crashdumps](docs/create-and-analyse-guest-crashdumps.md) - Creating and analyzing guest crash dumps
- [Debugging the guest runtime](docs/guest-runtime-debugging.md) - Debugging the guest runtime using GDB or LLDB
- [JS Host API](src/js-host-api/README.md) - Node.js bindings

## Build prerequisites  

1. Install [Rust](https://www.rust-lang.org/tools/install)
1. Install [just](https://github.com/casey/just). -  `cargo install just`.
1. Install clang:

For Windows [see here](https://learn.microsoft.com/en-us/cpp/build/clang-support-msbuild?view=msvc-170#install-1).

For Ubuntu:

```bash
    wget https://apt.llvm.org/llvm.sh 
    chmod +x ./llvm.sh
    sudo ./llvm.sh 16 all
    sudo ln -s /usr/lib/llvm-16/bin/clang-cl /usr/bin/clang-cl
    sudo ln -s /usr/lib/llvm-16/bin/llvm-lib /usr/bin/llvm-lib
```

For Azure Linux:

```bash
    sudo dnf remove clang -y|| true
    sudo dnf install clang16 -y
    sudo dnf install clang16-tools-extra -y
```

In addition on Linux you will need to install the `x86_64-unknown-none` target:

```bash
    rustup target add x86_64-unknown-none
```

## Building

To build the project, run:

```console
# Build the project
just build
```

## Testing

To run the tests, run:

```console
just test
```

## Running the examples

### run_handler example

The run_handler example demonstrates how to process a json formatted event using a JavaScript handler function. 

```console
cargo run --example run_handler 
```

Once you see ```Enter the name of the example to run or 'exit' to quit:``` you can enter the name of the example you want to run, valid names are the names of directories in `src/hyperlight-js/examples/data` (e.g., `echo`, `fibonacci`, `regex`) or `exit` to quit.

Alternatively you can pass the name of the sample that you want to run on the command line:

```console
cargo run --example run_handler <name_of_sample>
```

### Metrics example

The metrics example demonstrates how to use the prometheus to collect metrics from the guest.

```console
cargo run --example metrics 
```

### Tracing examples

The tracing example demonstrates how to use to configure tracing subscribers

- tracing_forest subscriber

    ```console
    cargo run --example tracing forest
    ```

- fmt subscriber

    ```console
    cargo run --example tracing fmt
    ```

## Debugging the guest runtime `hyperlight-js-runtime`

Hyperlight-js supports debugging the guest runtime using GDB or LLDB through the `gdb` feature.
For instructions on how to set up debugging, see the [Debugging the guest runtime](docs/guest-runtime-debugging.md) guide.
