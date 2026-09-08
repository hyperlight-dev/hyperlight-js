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
3. **Build with `cargo hyperlight`** — it discovers Hyperlight's libc
   headers and configures the guest compiler and sysroot.
4. **Build and embed with the host** — point `hyperlight-js` at your custom
   runtime manifest. Its build script builds the guest and embeds it at
   compile time.

Custom guests must use a compatible `hyperlight-js-runtime` and Hyperlight
version with the host library/addon. Pin the guest and host to the same
release (or git revision).

## Quick Start

### 1. Create your custom runtime crate

```bash
cargo init --bin my-custom-runtime
```

```toml
[dependencies]
hyperlight-js-runtime = { git = "https://github.com/hyperlight-dev/hyperlight-js" }
rquickjs = { version = "0.12", default-features = false, features = ["bindgen", "futures", "macro", "loader"] }

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

hyperlight_js_runtime::custom_globals! {}
```

That's the guest application code. The macro generates
an `init_native_modules()` function that the `NativeModuleLoader` calls
automatically on first use. Built-in modules are inherited. The lib provides
the guest entry point and host function dispatch. Invoke both registration
macros, even when one is empty.

### 3. Build and embed in hyperlight-js

Set `HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH` to the custom crate's **absolute**
`Cargo.toml` path, then build your host project normally:

```bash
export HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH="$(realpath my-custom-runtime/Cargo.toml)"
cargo build --release
```

PowerShell:

```powershell
$env:HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH = (Resolve-Path .\my-custom-runtime\Cargo.toml).Path
cargo build --release
```

This builds your custom runtime with `cargo-hyperlight` and embeds it in the
host. No additional compiler flags or include paths need to be configured.
The custom runtime manifest must define exactly one binary target.
Re-run the host build after changing your runtime.

### 4. Use from the Rust host

The host-side API is unchanged. Your custom runtime is already embedded,
and handlers simply import your modules:

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
#[cfg(not(hyperlight))]
struct NoOpHost;
#[cfg(not(hyperlight))]
impl hyperlight_js_runtime::host::Host for NoOpHost {
    fn resolve_module(&self, _base: String, name: String) -> anyhow::Result<String> {
        anyhow::bail!("Module '{name}' not found")
    }
    fn load_module(&self, name: String) -> anyhow::Result<String> {
        anyhow::bail!("Module '{name}' not found")
    }
}

#[cfg(not(hyperlight))]
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

Run `just test-native-modules` to build and embed the fixture.
These VM tests cover custom modules, custom globals, built-ins, and the
host-backed clock. They require a supported hypervisor. Build selection
regressions are also covered by
`cargo test -p hyperlight-js --test runtime_build`.

## Using js-host-api from a Downstream Node.js Project

**If you use a custom runtime, you must build the Node.js addon from source
instead of using the published `@hyperlight-dev/js-host-api` binary.**

### Why the published addon cannot be used

The NAPI addon links against the `hyperlight-js` Rust crate, which embeds
the guest runtime using `include_bytes!()` at compile time. The published
package's `.node` binary therefore already contains the **default** runtime.
Your custom native modules are not in that binary.

Running `npm install` to get the published package does not rebuild it with
your guest. Neither building your custom runtime separately nor setting
`HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH` when starting Node.js changes the
runtime inside an already-compiled addon. That variable is read during the
Rust host build, not when JavaScript creates a sandbox.

### Build the addon with your custom runtime

Use a `hyperlight-js` checkout matching the release or git revision used by
your custom runtime. From the checkout root, set the custom manifest's
absolute path and build the addon:

```powershell
$env:HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH = (Resolve-Path C:\path\to\my-custom-runtime\Cargo.toml).Path
just build-js-host-api release
```

On Bash, use `export HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH=/absolute/path/to/my-custom-runtime/Cargo.toml`
before the same `just` command. This builds and embeds the custom guest as
part of the addon build.

### Use the locally built addon

Point your downstream project's npm dependency at the built checkout's
`src/js-host-api` directory, rather than a published version:

```json
{
  "dependencies": {
    "@hyperlight-dev/js-host-api": "file:../hyperlight-js/src/js-host-api"
  }
}
```

Adjust the path for your layout, then run `npm install` in the downstream
project to update its dependency and lockfile. The application must use this
locally built addon, not a previously installed published copy.

The JavaScript API is unchanged: use the usual `SandboxBuilder`, and handlers
can import the custom modules embedded in your guest. After changing the
custom runtime, rebuild the addon and refresh the downstream installation
before restarting the application.

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

Custom modules with the same name as a built-in (`io`, `crypto`, `console`)
take priority, allowing extender crates to replace built-in implementations
when needed.

**Restriction:** The `require` module cannot be overridden — it is part of
the runtime's core module loading infrastructure. Attempting to register
a module named `"require"` will panic.

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

## Custom Globals

Register global objects (constructors, polyfills, constants) available
to all JavaScript code without `import`:

```rust
fn setup_my_globals(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval::<(), _>("globalThis.MY_CONSTANT = 42;")?;
    Ok(())
}

hyperlight_js_runtime::custom_globals! {
    setup_my_globals,
}
```

Custom globals are set up after built-in globals (console, require, print)
during `JsRuntime::new()`. Both Rust-implemented classes (via
`#[rquickjs::class]`) and JavaScript polyfills (via `ctx.eval()`) are
supported.

### Rust class example

For things like `TextEncoder` / `TextDecoder` where you need a proper
constructor accessible as `new TextEncoder()`:

```rust
use rquickjs::{Ctx, class::Trace, JsLifetime, TypedArray};

#[rquickjs::class]
#[derive(Trace, JsLifetime)]
pub struct TextEncoder {}

#[rquickjs::methods]
impl TextEncoder {
    #[qjs(constructor)]
    pub fn new() -> Self { TextEncoder {} }

    pub fn encode<'js>(&self, ctx: Ctx<'js>, input: String)
        -> rquickjs::Result<TypedArray<'js, u8>> {
        TypedArray::new(ctx, input.into_bytes())
    }
}

fn setup_text_encoding(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    // `Class::define` builds the class constructor and installs it on the
    // target object under the class's name ("TextEncoder"), so handlers can
    // call `new TextEncoder()` with no import.
    rquickjs::Class::<TextEncoder>::define(&ctx.globals())?;
    Ok(())
}

hyperlight_js_runtime::custom_globals! {
    setup_text_encoding,
}
```

### Combined with native modules

Both macros can be used together — the binary just needs to invoke both:

```rust
hyperlight_js_runtime::native_modules! {
    "math" => js_math,
}

hyperlight_js_runtime::custom_globals! {
    setup_text_encoding,
}
```

### `custom_globals!`

```rust
hyperlight_js_runtime::custom_globals! {
    setup_fn_a,
    setup_fn_b,
}
```

Generates an `init_custom_globals(ctx)` function that calls each setup
function in order. Called automatically by `JsRuntime::new()` after
built-in globals are installed. Each setup function receives `&Ctx` and
can register constructors, objects, or values on `ctx.globals()`.

**Important:** Every binary that links `hyperlight-js-runtime` must invoke
this macro (even if empty). The base runtime's `main.rs` already does this
with `custom_globals! {}` — same pattern as `native_modules!`.
