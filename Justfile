set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]
default-target:= "debug"
set-env-command := if os() == "windows" { "$env:" } else { "export " }
latest-release:= if os() == "windows" {"$(git tag -l --sort=v:refname | select -last 2 | select -first 1)"} else {`git tag -l --sort=v:refname | tail -n 2 | head -n 1`}

# in windows we need to replace the backslashes with forward slashes
# otherwise clang will misinterpret the paths
PWD := replace(justfile_dir(), "\\", "/")

# Set the HYPERLIGHT_CFLAGS so cargo-hyperlight applies them when building the runtimes:
# * include the stubs required by hyperlight-js-runtime
# * define __wasi__ as this disables threading support in quickjs
export HYPERLIGHT_CFLAGS := \
    "-I" + PWD + "/src/hyperlight-js-runtime/stubs/include " + \
    "-D__wasi__=1 "

# On Windows, use Ninja generator for CMake to avoid aws-lc-sys build issues with Visual Studio generator
export CMAKE_GENERATOR := if os() == "windows" { "Ninja" } else { "" }

ensure-tools:
    cargo install cargo-hyperlight --locked --version 0.1.3

# Check if npm is installed, install automatically if missing (Linux)
[private]
[unix]
check-npm:
    @bash dev/check-npm.sh

# Check if npm is installed, install automatically if missing (Windows)
[private]
[windows]
check-npm:
    @pwsh.exe -NoLogo -File dev/check-npm.ps1

check:
    cargo check

clippy target=default-target features="": (ensure-tools)
    cd src/hyperlight-js-runtime && \
        cargo hyperlight clippy \
            --profile={{ if target == "debug" {"dev"} else { target } }} \
            -- -D warnings
    cargo clippy --all-targets \
        --profile={{ if target == "debug" {"dev"} else { target } }} \
        {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} \
        -- -D warnings
    cargo clippy --manifest-path src/js-host-api/Cargo.toml \
        --profile={{ if target == "debug" {"dev"} else { target } }} \
        {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} \
        -- -D warnings

clippy-apply-fix-unix:
    cd src/hyperlight-js-runtime && \
        cargo clippy --fix --all
    cargo clippy --fix --all

clippy-apply-fix-windows:
    cargo clippy --target x86_64-pc-windows-msvc --fix --all

fmt-check: fmt-check-rust fmt-check-js
    @echo "✅ All format checks passed!"

fmt-check-rust:
    -rustup component add --toolchain nightly rustfmt
    cargo +nightly fmt --all -- --check
    cargo +nightly fmt --manifest-path src/hyperlight-js-runtime/Cargo.toml -- --check
    cargo +nightly fmt --manifest-path src/js-host-api/Cargo.toml -- --check

fmt-check-js: check-npm
    cd src/js-host-api && npm install
    cd src/js-host-api && npm run fmt:check

fmt-apply: fmt-apply-rust fmt-apply-js
    @echo "✅ All formatting applied!"

fmt-apply-rust:
    -rustup component add --toolchain nightly rustfmt
    cargo +nightly fmt --all
    cargo +nightly fmt --manifest-path src/hyperlight-js-runtime/Cargo.toml
    cargo +nightly fmt --manifest-path src/js-host-api/Cargo.toml

fmt-apply-js: check-npm
    cd src/js-host-api && npm install
    cd src/js-host-api && npm run fmt

# Lint everything (Rust + JS)
lint target=default-target features="": (clippy target features) (lint-js)
    @echo "✅ All linting passed!"

# Lint JS only (eslint)
lint-js: check-npm
    cd src/js-host-api && npm install
    cd src/js-host-api && npm run lint

# Fix JS lint errors
lint-js-fix: check-npm
    cd src/js-host-api && npm install
    cd src/js-host-api && npm run lint:fix

build target=default-target features="": (build-rust target features) (build-js-host-api target features) (build-native-runtime)
build-trace target=default-target features="": (build-rust-trace target features)

build-native-runtime target=default-target:
    cargo build --manifest-path=./src/hyperlight-js-runtime/Cargo.toml --profile={{ if target == "debug" {"dev"} else { target } }}

build-rust target=default-target features="":
    cd src/hyperlight-js && \
      cargo build {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --profile={{ if target == "debug" {"dev"} else { target } }}

build-rust-trace target=default-target features="":
    cd src/hyperlight-js && \
      cargo build {{ if features =="" {'-F trace_guest'} else if features=="no-default-features" {"--no-default-features -F trace_guest" } else {"--no-default-features -F trace_guest," + features } }} --profile={{ if target == "debug" {"dev"} else { target } }}

build-js-host-api target=default-target features="": check-npm (build-rust target features)
    cd src/js-host-api && npm install
    cd src/js-host-api && npx napi build --platform {{ if target == "release" { "--release" } else { "" } }} {{ if features == "" { "" } else { "--features=" + features } }}

build-all: (build "debug") (build "release")
    @echo "✅ All builds complete!"

run-examples target=default-target features="": (build target)
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example run_handler echo
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example run_handler fibonacci
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example run_handler regex
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F function_call_metrics," + features } }} --example metrics
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example metrics

run-examples-tracing target=default-target features="": (build target)
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {'--features function_call_metrics'} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example tracing fmt
    cargo run --profile={{ if target == "debug" {"dev"} else { target } }} {{ if features =="" {'--features function_call_metrics'} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --example tracing forest

clean:
    cargo clean
    cd src/hyperlight-js-runtime && cargo clean
    cd src/js-host-api && cargo clean
    -rm -rf src/js-host-api/node_modules
    -rm -f src/js-host-api/*.node
    -rm -f src/js-host-api/index.js
    -rm -f src/js-host-api/index.d.ts

# TESTING
# Metrics tests cannot run with other tests they are marked as ignored so that cargo test works
# There may be tests that we really want to ignore so we cant just use --ignored and run then we have to
# specify the test name of the ignored tests that we want to run
# NOTE: If features is non-empty, the test will be ran with no-default-features + the given features
test target=default-target features="": (build target)
    cd src/hyperlight-js && cargo test {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --profile={{ if target == "debug" {"dev"} else { target } }}
    cd src/hyperlight-js && cargo test {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} handle_termination --profile={{ if target == "debug" {"dev"} else { target } }} -- --ignored --nocapture
    cd src/hyperlight-js && cargo test {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} test_metrics --profile={{ if target == "debug" {"dev"} else { target } }} -- --ignored --nocapture
    cargo test --manifest-path=./src/hyperlight-js-runtime/Cargo.toml --test=native_cli --profile={{ if target == "debug" {"dev"} else { target } }}

# Test with monitor features enabled (wall-clock and CPU time monitors)
# Note: We exclude test_metrics as it requires process isolation and is already run by `test` recipe
test-monitors target=default-target:
    cd src/hyperlight-js && cargo test --features monitor-wall-clock,monitor-cpu-time --profile={{ if target == "debug" {"dev"} else { target } }} -- --include-ignored --skip test_metrics

test-js-host-api target=default-target features="": (build-js-host-api target features)
    cd src/js-host-api && npm test

# Run js-host-api examples (simple.js, calculator.js, unload.js, interrupt.js, cpu-timeout.js)
run-js-host-api-examples target=default-target features="": (build-js-host-api target features)
    @echo "Running js-host-api examples..."
    @echo ""
    cd src/js-host-api && node examples/simple.js
    @echo ""
    cd src/js-host-api && node examples/calculator.js
    @echo ""
    cd src/js-host-api && node examples/unload.js
    @echo ""
    cd src/js-host-api && node examples/interrupt.js
    @echo ""
    cd src/js-host-api && node examples/cpu-timeout.js
    @echo ""
    @echo "✅ All examples completed successfully!"

test-all target=default-target features="": (test target features) (test-monitors target) (test-js-host-api target features)
    @echo "✅ All tests passed!"

# warning, compares to and then OVERWRITES the given baseline
bench-ci baseline target=default-target features="":
    cd src/hyperlight-js && cargo bench --bench benchmarks --features monitor-wall-clock,monitor-cpu-time {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --profile={{ if target == "debug" {"dev"} else { target } }} -- --verbose --save-baseline {{baseline}}
bench target=default-target features="":
    cd src/hyperlight-js && cargo bench --bench benchmarks --features monitor-wall-clock,monitor-cpu-time {{ if features =="" {''} else if features=="no-default-features" {"--no-default-features" } else {"--no-default-features -F " + features } }} --profile={{ if target == "debug" {"dev"} else { target } }} -- --verbose
bench-download os hypervisor:
    gh release download -D target/ -p benchmarks_{{ os }}_{{ hypervisor }}.tar.gz
    mkdir -p target/criterion {{ if os() == "windows" { "-Force" } else { "" } }}
    tar -zxvf target/benchmarks_{{ os }}_{{ hypervisor }}.tar.gz -C target/criterion/ --strip-components=1
