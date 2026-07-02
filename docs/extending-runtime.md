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
```

That's all the Rust you write for the Hyperlight guest. The macro generates
an `init_native_modules()` function that the `NativeModuleLoader` calls
automatically on first use. Built-in modules are inherited. The lib provides
all hyperlight guest infrastructure (entry point, host function dispatch,
libc stubs) — no copying files or build scripts needed.

### 3. Build and embed in hyperlight-js

The hyperlight target has no libc, so QuickJS needs stub headers from
`hyperlight-js-runtime/include/` and `-D__wasi__=1` to disable pthreads.
Set `HYPERLIGHT_CFLAGS` before building — the one-liner below uses
`cargo metadata` to resolve the include path from your dependency tree:

```bash
# Resolve CFLAGS from hyperlight-js-runtime's include/ directory
export HYPERLIGHT_CFLAGS=$(node -e "
  var m=JSON.parse(require('child_process').execSync(
    'cargo metadata --format-version 1 --manifest-path my-custom-runtime/Cargo.toml',
    {encoding:'utf8',stdio:['pipe','pipe','pipe'],maxBuffer:20*1024*1024}));
  var p=m.packages.find(function(p){return p.name==='hyperlight-js-runtime'});
  if(p)console.log('-I'+require('path').join(
    require('path').dirname(p.manifest_path),'include')+' -D__wasi__=1');
")

# Build the custom runtime for the hyperlight target
cargo hyperlight build --manifest-path my-custom-runtime/Cargo.toml --release

# Tell hyperlight-js to embed the custom runtime (not the default one)
export HYPERLIGHT_JS_RUNTIME_PATH=my-custom-runtime/target/x86_64-hyperlight-none/release/my-custom-runtime

# Rebuild hyperlight-js so the embedded runtime is updated
cargo build -p hyperlight-js --release
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

## Using js-host-api from a Downstream Node.js Project

If your downstream project depends on `@hyperlight/js-host-api` (the
Node.js NAPI addon) and uses a custom runtime, you **cannot** use a
published version of the addon — the published binary has the default
runtime baked in via `include_bytes!()`. You need to build the NAPI
addon from source with your custom runtime embedded.

### Why not just `npm install`?

The `js-host-api` NAPI addon links against the `hyperlight-js` Rust crate,
which embeds the runtime binary at compile time. A published npm package
would contain a `.node` binary with the **default** runtime — your custom
native modules wouldn't be present.

### The pattern: reuse Cargo's git checkout

Your custom runtime crate already has a Cargo dependency on
`hyperlight-js-runtime`, which causes Cargo to clone the full
`hyperlight-js` workspace into `~/.cargo/git/checkouts/`. The
`js-host-api` NAPI source is included in that checkout — no separate
git clone needed.

#### 1. Discover the checkout path

Use `cargo metadata` to find where Cargo placed the hyperlight-js
workspace:

```bash
HYPERLIGHT_DIR=$(node -e "
  var m=JSON.parse(require('child_process').execSync(
    'cargo metadata --format-version 1 --manifest-path my-custom-runtime/Cargo.toml',
    {encoding:'utf8',stdio:['pipe','pipe','pipe'],maxBuffer:20*1024*1024}));
  var p=m.packages.find(function(p){return p.name==='hyperlight-js-runtime'});
  if(p)console.log(require('path').resolve(
    require('path').dirname(p.manifest_path),'..','..'));
")
echo "$HYPERLIGHT_DIR"
# e.g. /home/you/.cargo/git/checkouts/hyperlight-js-abc123/def456
```

#### 2. Build the NAPI addon with your custom runtime

```bash
# Set HYPERLIGHT_CFLAGS for the guest build
export HYPERLIGHT_CFLAGS=$(node -e "
  var m=JSON.parse(require('child_process').execSync(
    'cargo metadata --format-version 1 --manifest-path my-custom-runtime/Cargo.toml',
    {encoding:'utf8',stdio:['pipe','pipe','pipe'],maxBuffer:20*1024*1024}));
  var p=m.packages.find(function(p){return p.name==='hyperlight-js-runtime'});
  if(p)console.log('-I'+require('path').join(
    require('path').dirname(p.manifest_path),'include')+' -D__wasi__=1');
")

# Build your custom runtime for the hyperlight target
cargo hyperlight build --manifest-path my-custom-runtime/Cargo.toml --release

# Point hyperlight-js at your custom runtime binary
export HYPERLIGHT_JS_RUNTIME_PATH=my-custom-runtime/target/x86_64-hyperlight-none/release/my-custom-runtime

# Clean stale builds so build.rs re-embeds the runtime
cd "${HYPERLIGHT_DIR}/src/hyperlight-js" && cargo clean -p hyperlight-js

# Build the NAPI addon from the Cargo checkout
cd "${HYPERLIGHT_DIR}" && just build release
```

#### 3. Symlink for npm dependency resolution

Create a symlink so npm can resolve the addon via a stable path:

```bash
mkdir -p deps
ln -sfn "${HYPERLIGHT_DIR}/src/js-host-api" deps/js-host-api
```

In your package.json, point to js-host-api via the symlink:

```json
{
  "dependencies": {
    "@hyperlight/js-host-api": "file:deps/js-host-api"
  }
}
```
Make sure to add `deps` to your `.gitignore` since it's a symlink to a local Cargo checkout.

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
