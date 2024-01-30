# Type Naming Convention 

## Overview

The `js-host-api` crate uses a clean naming convention where:
- **Rust types** are named `*Wrapper` to clearly indicate they wrap the core types
- **JavaScript types** use clean names matching the underlying Hyperlight types

This is achieved using the `#[napi(js_name = "...")]` attribute.

## Type Mapping

| Rust Type (Internal)        | JavaScript Type (Exported) | Wraps                      |
|-----------------------------|----------------------------|----------------------------|
| `SandboxBuilderWrapper`     | `SandboxBuilder`          | `SandboxBuilder`           |
| `ProtoJSSandboxWrapper`     | `ProtoJSSandbox`          | `ProtoJSSandbox`           |
| `JSSandboxWrapper`          | `JSSandbox`               | `JSSandbox`                |
| `LoadedJSSandboxWrapper`    | `LoadedJSSandbox`         | `LoadedJSSandbox`          |
| `SnapshotWrapper`           | `Snapshot`                | `Snapshot`                 |
| `InterruptHandleWrapper`    | `InterruptHandle`         | `Arc<dyn InterruptHandle>` |
| `CallHandlerOptions`        | `CallHandlerOptions`     | N/A (plain object)         |

## Example Implementation

```rust
/// JavaScript-friendly wrapper for SandboxBuilder
#[napi(js_name = "SandboxBuilder")]
pub struct SandboxBuilderWrapper {
    inner: Arc<Mutex<Option<SandboxBuilder>>>,
}

#[napi]
impl SandboxBuilderWrapper {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(SandboxBuilder::new()))),
        }
    }

    /// Builder is consumed on build — Arc<Mutex<Option>> enables
    /// one-shot consumption from &self (required by NAPI).
    #[napi]
    pub async fn build(&self) -> napi::Result<ProtoJSSandboxWrapper> {
        let builder = self.take_inner()?;
        // ...
    }
}
```

## Benefits

1. **Clear Rust Code**: The `Wrapper` suffix makes it obvious these are NAPI wrappers
2. **Clean JavaScript API**: Users see natural type names like `SandboxBuilder`, `JSSandbox`, etc.
3. **Matches Core Library**: JavaScript types mirror the underlying Hyperlight types
4. **Maintainability**: Easy to distinguish wrapper code from core library code
5. **Documentation**: Type names in docs match what users actually use

## JavaScript Usage

```javascript
const { SandboxBuilder } = require('@hyperlight/js-host-api');

// Build the sandbox pipeline
const builder = new SandboxBuilder();
const protoSandbox = await builder.build();
const jsSandbox = await protoSandbox.loadRuntime();

// Register handlers before loading
jsSandbox.addHandler('echo', 'function handler(event) { return event; }');
const loadedSandbox = await jsSandbox.getLoadedSandbox();

// Execute a handler using its routing key — pass objects in, get objects back
const result = await loadedSandbox.callHandler('echo', { message: 'hello' });

// Execute with monitoring (recommended)
const guarded = await loadedSandbox.callHandler('echo', { message: 'hello' }, {
  wallClockTimeoutMs: 5000,
  cpuTimeoutMs: 3000,
});
```

## Generated TypeScript Definitions

TypeScript definitions (`index.d.ts`) are **auto-generated** by `napi build --platform`
from the `#[napi]` attributes in [src/lib.rs](src/lib.rs). Do not edit `index.d.ts`
manually — it will be overwritten on the next build.

To regenerate after changing the Rust API:
```bash
cd src/js-host-api && npx napi build --platform
```

## Pattern for Future Types

When adding new wrapper types, follow this pattern:

```rust
#[napi(js_name = "YourTypeName")]
pub struct YourTypeNameWrapper {
    inner: Arc<Mutex<Option<YourTypeName>>>,
}
```

This ensures:
- Rust code is explicit about wrapper types
- JavaScript gets clean, idiomatic names
- `Arc<Mutex<Option<_>>>` enables one-shot consumption from `&self` (NAPI requirement)
- Consistency across the codebase
