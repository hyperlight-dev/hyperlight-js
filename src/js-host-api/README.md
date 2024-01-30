# Hyperlight JS Host API

Node.js bindings for hyperlight-js

## Installation

```bash
npm install @hyperlight/js-host-api
```

## Quick Start

```javascript
import { SandboxBuilder } from '@hyperlight/js-host-api';

// Create and build a sandbox
const builder = new SandboxBuilder();
const protoSandbox = await builder.build();

// Load the JavaScript runtime
const jsSandbox = await protoSandbox.loadRuntime();

// Add a handler function (sync — no await needed)
// First arg is a routing key; the function must be named 'handler'
jsSandbox.addHandler('greet', `
  function handler(event) {
    event.message = 'Hello, ' + event.name + '!';
    return event;
  }
`);

// Get the loaded sandbox
const loadedSandbox = await jsSandbox.getLoadedSandbox();

// Call the handler using the routing key
const result = await loadedSandbox.callHandler('greet', { name: 'World' });
console.log(result); // { name: 'World', message: 'Hello, World!' }
```

> **Note:** All sandbox operations that touch the hypervisor (`build`, `loadRuntime`,
> `getLoadedSandbox`, `callHandler`, `unload`, `snapshot`,
> `restore`) return Promises. This means the Node.js event loop stays free while
> the hypervisor does its work — no blocking!

## API

### SandboxBuilder

Creates and configures a new sandbox.

**Methods:**
- `setHeapSize(bytes: number)` → `this` — Set guest heap size (must be > 0, chainable)
- `setStackSize(bytes: number)` → `this` — Set guest stack size (must be > 0, chainable)
- `setInputBufferSize(bytes: number)` → `this` — Set guest input buffer size (must be > 0, chainable)
- `setOutputBufferSize(bytes: number)` → `this` — Set guest output buffer size (must be > 0, chainable)
- `build()` → `Promise<ProtoJSSandbox>` — Builds a proto sandbox ready to load the JavaScript runtime

```javascript
const builder = new SandboxBuilder()
    .setHeapSize(8 * 1024 * 1024)
    .setStackSize(512 * 1024);
const protoSandbox = await builder.build();
```

### ProtoJSSandbox

A proto sandbox ready to load the JavaScript runtime.

**Methods:**
- `loadRuntime()` → `Promise<JSSandbox>` — Loads the JavaScript runtime into the sandbox

```javascript
const jsSandbox = await protoSandbox.loadRuntime();
```

### JSSandbox

A sandbox with the JavaScript runtime loaded, ready for handlers.

**Methods:**
- `addHandler(name: string, code: string)` — Adds a JavaScript handler function (sync)
- `getLoadedSandbox()` → `Promise<LoadedJSSandbox>` — Gets the loaded sandbox ready to call handlers
- `clearHandlers()` — Clears all registered handlers (sync)
- `removeHandler(name: string)` — Removes a specific handler by name (sync)

```javascript
// Add a handler (sync) — routing key can be any name, but the function must be named 'handler'
sandbox.addHandler('myHandler', 'function handler(input) { return input; }');

// Get loaded sandbox (async)
const loaded = await sandbox.getLoadedSandbox();

// Clear all handlers (sync)
sandbox.clearHandlers();

// Remove specific handler by routing key (sync)
sandbox.removeHandler('myHandler');
```

### LoadedJSSandbox

A sandbox with handlers loaded, ready to process events.

**Methods:**
- `callHandler(handlerName: string, eventData: any, options?: CallHandlerOptions)` → `Promise<any>` — Calls a handler with event data (any JSON-serializable value). Pass options with `gc: false` to skip post-call garbage collection, or with `wallClockTimeoutMs`/`cpuTimeoutMs` to enforce resource limits ⏱️
- `unload()` → `Promise<JSSandbox>` — Unloads all handlers and returns to JSSandbox state
- `snapshot()` → `Promise<Snapshot>` — Takes a snapshot of the sandbox state
- `restore(snapshot: Snapshot)` → `Promise<void>` — Restores sandbox state from a snapshot

**Properties:**
- `interruptHandle` → `InterruptHandle` — Gets a handle to interrupt/kill handler execution (getter, not a method)
- `poisoned` → `boolean` — Whether the sandbox is in a poisoned (inconsistent) state

```javascript
// Call a handler with event data — pass objects directly, get objects back
const result = await loaded.callHandler('handler', { data: "value" });

// Call with wall-clock timeout only
try {
    const result = await loaded.callHandler('handler', {}, {
        wallClockTimeoutMs: 1000,
    });
} catch (error) {
    if (error.code === 'ERR_CANCELLED') {
        console.log('Handler exceeded 1s wall-clock timeout');
    } else {
        throw error; // unexpected — don't swallow it
    }
}

// Call with CPU time timeout only (better for pure computation)
try {
    const result = await loaded.callHandler('handler', {}, {
        cpuTimeoutMs: 500,
    });
} catch (error) {
    if (error.code === 'ERR_CANCELLED') {
        console.log('Handler exceeded 500ms CPU time');
    } else {
        throw error;
    }
}

// Recommended: Both monitors (OR semantics — first to fire terminates)
try {
    const result = await loaded.callHandler('handler', {}, {
        wallClockTimeoutMs: 5000,
        cpuTimeoutMs: 500,
    });
} catch (error) {
    if (error.code === 'ERR_CANCELLED') {
        console.log('Handler exceeded a resource limit');
    } else {
        throw error;
    }
}

// Unload all handlers to reset state
const sandbox = await loaded.unload();

// Get interrupt handle (property getter, not a method call)
const handle = loaded.interruptHandle;

// Snapshot and restore
const snapshot = await loaded.snapshot();
// ... do something that poisons the sandbox ...
await loaded.restore(snapshot);
```

### CallHandlerOptions

Configuration for execution monitors (optional). When no timeouts are specified,
the handler runs without any monitors.

| Property | Type | Description |
|----------|------|-------------|
| `wallClockTimeoutMs` | `number?` | Wall-clock timeout in ms.  |
| `cpuTimeoutMs` | `number?` | CPU time timeout in ms. Catches compute-bound abuse (tight loops, etc) |
| `gc` | `boolean?` | Whether to run GC after the handler call. Defaults to `true` |

When both timeouts are set, monitors race with **OR semantics** — whichever fires first terminates execution. This is the **recommended** pattern for comprehensive protection.

### InterruptHandle ⏱️

Handle for interrupting/killing handler execution. Because all hypervisor calls return Promises, the Node.js event loop stays free during execution — you can call `kill()` from a timer, a signal handler, or any async callback.

**Methods:**
- `kill()` — Immediately stops the currently executing handler in the sandbox

```javascript
// Get interrupt handle 
const handle = loaded.interruptHandle;

// Kill from a timer 
const timer = setTimeout(() => handle.kill(), 2000);
const result = await loaded.callHandler('handler', {});
clearTimeout(timer);
```

**Recommended:** Pass timeout options to `callHandler()` instead for built-in timeout support:

```javascript
// Combined monitors — the recommended pattern 🛡️
try {
    const result = await loaded.callHandler('handler', {}, {
        wallClockTimeoutMs: 5000,
        cpuTimeoutMs: 500,
    });
} catch (error) {
    console.log('Handler killed by monitor');
}
```

**CPU Time vs Wall Clock:**
- **Wall Clock** (`wallClockTimeoutMs`): Measures real-world elapsed time. Catches resource exhaustion where the guest holds host resources without burning CPU. (Not really possible today unless the guest calls a host function that blocks)
- **CPU Time** (`cpuTimeoutMs`): Measures only actual CPU execution time. Catches compute-bound abuse. Supported on Linux and Windows.
- **Combined** (both set): Best protection — neither alone is sufficient.

### Snapshot

An opaque handle representing a point-in-time snapshot of the sandbox state. Use `snapshot()` to capture and `restore()` to roll back after a poisoned state or any other reason.

```javascript
const snapshot = await loaded.snapshot();

// ... handler gets killed, sandbox is poisoned ...

await loaded.restore(snapshot);
console.log(loaded.poisoned); // false — back to normal
```

### Error Codes

All errors thrown by the API include a `code` property for programmatic handling:

| Code | Meaning |
|------|---------|
| `ERR_INVALID_ARG` | Bad argument (empty handler name, zero timeout, etc.) |
| `ERR_CONSUMED` | Object already consumed (e.g., calling `loadRuntime()` twice) |
| `ERR_POISONED` | Sandbox is in an inconsistent state (after timeout kill, guest abort, etc.) — restore from snapshot or unload |
| `ERR_CANCELLED` | Execution was cancelled (by monitor timeout or manual `kill()`) |
| `ERR_STACK_OVERFLOW` | Guest code caused a stack overflow |
| `ERR_GUEST_ABORT` | Guest code aborted |
| `ERR_INTERNAL` | Unexpected internal error |

```javascript
try {
    await loaded.callHandler('handler', {});
} catch (error) {
    switch (error.code) {
        case 'ERR_CANCELLED':
            console.log('Execution was cancelled');
            break;
        case 'ERR_STACK_OVERFLOW':
            console.log('Stack overflow in guest code');
            break;
        default:
            console.log(`Unexpected error [${error.code}]: ${error.message}`);
    }
}
```

## Examples

See the `examples/` directory for complete examples:

### Simple Usage (`simple.js`)
Basic "Hello World" demonstrating the sandbox lifecycle.

### Calculator (`calculator.js`)
JSON event processing with multiple operations.

### Unload/Reload (`unload.js`)
Handler lifecycle management — unload handlers, reset state, and load new handlers.

### Interrupt/Timeout (`interrupt.js`) ⏱️
Timeout-based handler termination using wall-clock timeout. Demonstrates killing a 4-second handler after 1 second using `callHandler()`.

### CPU Timeout (`cpu-timeout.js`) 🚀
Combined CPU + wall-clock monitoring — the recommended pattern for comprehensive resource protection. Demonstrates OR semantics where the CPU monitor fires first for compute-bound work, with wall-clock as backstop.

## Requirements

- **Node.js** >= 18

## Building from Source

### Build Commands

```bash
# Install dependencies
npm install

# Release builds (optimized)
npm run build

# Debug builds (with symbols)
npm run build:debug

# Run tests
npm test
```

### Using Just (Build Automation)

From the repository root:

```bash
# Build js-host-api
just build-js-host-api release

# Build with debug symbols
just build-js-host-api debug

# Run js-host-api examples
just run-js-host-api-examples release

# Run js-host-api tests
just test-js-host-api release

# Build and test everything (all runtimes and targets)
just build-all
just test-all release
```
