# Extending the Runtime with Custom Native Modules

This document describes how to extend `hyperlight-js-runtime` with custom
native (Rust-implemented) modules that run alongside the built-in modules
inside the Hyperlight guest VM.

## Why Native Modules? 🤔

Some operations are too slow in pure JavaScript. For example, DEFLATE
compression can be 50–100× slower than native Rust, which may trigger CPU
timeouts on large inputs. Native modules let you add high-performance Rust
code that JavaScript handlers can `import` — without forking the runtime.

## How It Works

1. **`hyperlight-js-runtime` as a library** — the runtime crate exposes a
   `[lib]` target so your crate can depend on it.
2. **`native_modules!` macro** — registers custom modules into a global
   registry. The runtime's `NativeModuleLoader` checks custom modules
   first, then falls back to built-ins (io, crypto, console, require).
3. **`HYPERLIGHT_JS_RUNTIME_PATH`** — a build-time env var that tells
   `hyperlight-js` to embed your custom runtime binary instead of the
   default one.

## Quick Start

### 1. Create your custom runtime crate

```bash
cargo init --bin my-custom-runtime
```

```toml
[dependencies]
hyperlight-js-runtime = { git = "https://github.com/hyperlight-dev/hyperlight-js" }
rquickjs = { version = "0.11", default-features = false, features = ["bindgen", "futures", "macro", "loader"] }

# Only needed for native CLI testing, not the hyperlight guest
[target.'cfg(not(hyperlight))'.dependencies]
anyhow = "1.0"

[lints.rust]
unexpected_cfgs = { level = "allow", check-cfg = ['cfg(hyperlight)'] }
```

> **Note:** The `rquickjs` version and features must match what
> `hyperlight-js-runtime` uses. Check its `Cargo.toml` for the exact spec.

### 2. Define your module and register it

```rust
#![cfg_attr(hyperlight, no_std)]
#![cfg_attr(hyperlight, no_main)]

#[rquickjs::module(rename_vars = "camelCase")]
mod math {
    #[rquickjs::function]
    pub fn add(a: f64, b: f64) -> f64 { a + b }

    #[rquickjs::function]
    pub fn multiply(a: f64, b: f64) -> f64 { a * b }
}

hyperlight_js_runtime::native_modules! {
    "math" => js_math,
}
```

That's all the Rust you write for the Hyperlight guest. The macro generates
an `init_native_modules()` function that the `NativeModuleLoader` calls
automatically on first use. Built-in modules are inherited. The lib provides
all hyperlight guest infrastructure (entry point, host function dispatch,
libc stubs) — no copying files or build scripts needed.

### 3. Build and embed in hyperlight-js

Build your custom runtime for the Hyperlight target and embed it:

```bash
# Build for the hyperlight target
cargo hyperlight build --manifest-path my-custom-runtime/Cargo.toml

# Build hyperlight-js with your custom runtime embedded
HYPERLIGHT_JS_RUNTIME_PATH=/path/to/my-custom-runtime \
    cargo build -p hyperlight-js
```

### 4. Use from the host

The host-side code is **identical** to any other `hyperlight-js` usage.
Custom native modules are transparent — they're baked into the guest
binary. Your handlers just `import` from them:

```rust
use hyperlight_js::{SandboxBuilder, Script};

fn main() -> anyhow::Result<()> {
    let proto = SandboxBuilder::new().build()?;
    let mut sandbox = proto.load_runtime()?;

    let handler = Script::from_content(r#"
        import { add, multiply } from "math";
        export function handler(event) {
            return {
                sum: add(event.a, event.b),
                product: multiply(event.a, event.b),
            };
        }
    "#);
    sandbox.add_handler("compute", handler)?;

    let mut loaded = sandbox.get_loaded_sandbox()?;
    let result = loaded.handle_event("compute", r#"{"a":6,"b":7}"#.to_string(), None)?;

    println!("{result}");
    // {"sum":13,"product":42}

    Ok(())
}
```

### 5. Test natively (optional)

For local development you can run your custom runtime as a native CLI
without building for Hyperlight. Add a `main()` to your `main.rs`.

Since your custom modules are registered via the macro (and built-ins are
handled by the runtime), you don't need filesystem module resolution (But you can have it if you want it).
A no-op `Host` is all that's needed — it only gets called for `.js` file
imports, which native modules don't use:

```rust
struct NoOpHost;
impl hyperlight_js_runtime::host::Host for NoOpHost {
    fn resolve_module(&self, _base: String, name: String) -> anyhow::Result<String> {
        anyhow::bail!("Module '{name}' not found")
    }
    fn load_module(&self, name: String) -> anyhow::Result<String> {
        anyhow::bail!("Module '{name}' not found")
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let script = std::fs::read_to_string(&args[1])?;

    let mut runtime = hyperlight_js_runtime::JsRuntime::new(NoOpHost)?;
    runtime.register_handler("handler", script, ".")?;
    let result = runtime.run_handler("handler".into(), args[2].clone(), false)?;
    println!("{result}");
    Ok(())
}
```

```bash
# handler.js
cat > handler.js << 'EOF'
import { add, multiply } from "math";
export function handler(event) {
    return { sum: add(event.a, event.b), product: multiply(event.a, event.b) };
}
EOF

cargo run -- handler.js '{"a":6,"b":7}'
# {"sum":13,"product":42}
```

## Complete Example

See the [extended_runtime fixture](../src/hyperlight-js-runtime/tests/fixtures/extended_runtime/)
for a working example with end-to-end tests.

Run `just test-native-modules` to build the fixture for the Hyperlight
target and run the full integration tests.

## API Reference

### `native_modules!`

```rust
hyperlight_js_runtime::native_modules! {
    "module_name" => ModuleDefType,
    "another"     => AnotherModuleDefType,
}
```

Generates an `init_native_modules()` function that registers the listed
modules into the global native module registry. Called automatically by the
`NativeModuleLoader` on first use — you never need to call it yourself.
Built-in modules are inherited automatically.

**Restrictions:**
- Custom module names **cannot** shadow built-in modules (`io`, `crypto`,
  `console`, `require`). Attempting to register a built-in name panics.

### `register_native_module`

```rust
hyperlight_js_runtime::modules::register_native_module(name, declaration_fn)
```

Register a single custom native module by name. Typically called via the
`native_modules!` macro rather than directly.

### `JsRuntime::new`

```rust
hyperlight_js_runtime::JsRuntime::new(host)
```
