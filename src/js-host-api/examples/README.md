# Examples

This directory contains example code showing how to use the Hyperlight JS Host API.

## Prerequisites

### Option 1: Using Just (Recommended)

From the repository root:
```bash
# Build
just build-js-host-api release
```

This builds everything you need: the runtime binary (hyperlight-js-runtime) and the Node.js native module.

### Option 2: Manual Build

1. Build the native module:
   ```bash
   cd ..
   npm install
   npm run build
   ```

2. Make sure you have the runtime binary built:
   ```bash
   cd ../../..
   just build release
   ```

## Running Examples

### Quick Test All (Recommended)

From the repository root:
```bash
# Run all examples as a test suite
just test-js-host-api release

# OR just run the examples directly
just run-js-host-api-examples release
```

This builds everything and runs all examples!

### Individual Examples

From this directory (`src/js-host-api/examples/`):

### Simple Hello World

A basic example showing the core workflow of creating a sandbox and calling a function:

```bash
node simple.js
```

**What it does:**
- Creates and configures a sandbox
- Loads the JavaScript runtime
- Adds a simple greeting function
- Calls the function from the host

Expected output:
```
=== Hyperlight JS Hello World ===

1. Creating sandbox builder...
   ✓ Builder configured

2. Building proto sandbox...
   ✓ Proto sandbox created

3. Loading JavaScript runtime...
   ✓ Runtime loaded

4. Adding handler function...
   ✓ Handler added

5. Getting loaded sandbox...
   ✓ Sandbox ready

6. Calling guest function...
   ✓ Function executed

Result: Hello, World! Welcome to Hyperlight JS.

=== Success! ===
```

### Calculator Example

Shows multiple operations and JSON data processing:

```bash
node calculator.js
```

**What it does:**
- Creates a sandbox with a calculator handler
- Demonstrates math operations (add, multiply, divide, subtract)
- Uses object-in/object-out for input/output

Expected output:
```
=== Hyperlight JS Advanced Example ===

Adding calculator handler...

Testing calculator operations:
  10 add 5 = 15
  20 multiply 4 = 80
  100 divide 25 = 4
  50 subtract 30 = 20

=== All tests passed! ===
```

## API Overview

### SandboxBuilder
```javascript
const { SandboxBuilder } = require('../lib.js');

async function main() {
    const builder = new SandboxBuilder();
    builder.setHeapSize(8 * 1024 * 1024);   // Set heap size
    builder.setStackSize(512 * 1024);        // Set stack size
    const protoSandbox = await builder.build(); // Build sandbox
}
main();
```

### ProtoJSSandbox
```javascript
const jsSandbox = await protoSandbox.loadRuntime();  // Load JavaScript runtime
```

### JSSandbox
```javascript
// Add handler functions — first arg is a routing key, function must be named 'handler'
jsSandbox.addHandler('echo', 'function handler(event) { return event; }');

// Remove a handler by its routing key
jsSandbox.removeHandler('echo');

// Clear all handlers
jsSandbox.clearHandlers();

// Get loaded sandbox
const loadedSandbox = await jsSandbox.getLoadedSandbox();
```

### Interrupt Handler Execution ⏱️

A powerful example showing how to timeout/kill long-running handlers:

```bash
node interrupt.js
```

**What it does:**
- Creates a handler that runs for 4 seconds in a busy loop
- Calls `callHandler()` with a 1000ms wall-clock timeout
- Native Rust monitor kills execution after 1 second
- Shows proper error handling when interrupted

Expected output:
```
⏱️  Interrupt Example: Timeout-based handler termination

📊 Test 1: Fast Handler (completes before timeout)
   ✅ SUCCESS: Handler completed in ~200ms

📊 Test 2: Slow Handler (exceeds timeout)
   💀 Handler killed after ~1000ms
   🔒 Poisoned: true (sandbox is in inconsistent state)
   ✅ SUCCESS: Handler was properly interrupted!
```

**Key concepts:**
- `callHandler()` accepts `{ wallClockTimeoutMs?, cpuTimeoutMs? }` in options
- Set one or both — when both set, OR semantics (first to fire terminates)
- Useful for preventing runaway scripts
- Essential for production timeout enforcement

### Combined CPU + Wall-Clock Monitoring (`cpu-timeout.js`) 🚀

The recommended pattern for comprehensive resource protection:

```bash
node cpu-timeout.js
```

**What it does:**
- Creates handler with 3-second CPU busy loop
- Uses `callHandler()` with **both** CPU + wall-clock timeout options
- CPU monitor fires first for compute-bound work
- Wall-clock acts as backstop for resource exhaustion attacks

Expected output:
```
⏱️  Combined Monitor Example: CPU time + Wall Clock time

📊 Test 1: Fast Handler (completes before either timeout)
   ✅ SUCCESS: Handler completed!

📊 Test 2: Slow Handler (CPU monitor fires first)
   💀 Handler killed after ~500ms
   ⚡ CPU time limit: 500ms (fired first for compute-bound work)
   ⏱️  Wall-clock limit: 5000ms (backstop, not reached)
   ✅ SUCCESS: Timeout enforced correctly!
```

**Key concepts:**
- Combined monitors race with OR semantics — first to fire wins
- **CPU** (`cpuTimeoutMs`): Catches tight loops, crypto mining
- **Wall-clock** (`wallClockTimeoutMs`): Catches resource exhaustion (holding FDs, sleeping)
- Neither alone is sufficient; combined provides comprehensive protection
- Platform-native CPU monitoring:
  - **Linux**: `pthread_getcpuclockid` + `clock_gettime`
  - **Windows**: `QueryThreadCycleTime` with registry-based frequency

### LoadedJSSandbox
```javascript
const { SandboxBuilder } = require('../lib.js');

async function main() {
    // ... build, loadRuntime, addHandler, getLoadedSandbox ...

    // Call a handler function with event data — pass objects directly
    const result = await loadedSandbox.callHandler(
        'handler',        // Name of the handler function
        { key: 'value' }, // Event data (any JSON-serializable value)
        { gc: false }     // Skip post-call garbage collection (optional)
    );
    console.log(result); // Result is already a JS object

    // Call with wall-clock timeout only
    const wallResult = await loadedSandbox.callHandler('handler', {}, {
        wallClockTimeoutMs: 1000,
    });

    // Call with CPU time timeout only
    const cpuResult = await loadedSandbox.callHandler('handler', {}, {
        cpuTimeoutMs: 500,
    });

    // Recommended: Both monitors (OR semantics — first to fire terminates)
    try {
        const result = await loadedSandbox.callHandler('handler', {}, {
            wallClockTimeoutMs: 5000,
            cpuTimeoutMs: 500,
        });
        console.log('Handler completed:', result);
    } catch (error) {
        console.log('Handler exceeded a resource limit');
    }

    // Get interrupt handle for advanced timeout control
    const interruptHandle = loadedSandbox.interruptHandle;

    // Manual kill
    interruptHandle.kill();
}
main();
```

## Handler Function Pattern

Handler functions in Hyperlight JS follow this pattern:
- The function in the script **must** be named `handler` (or explicitly `export { yourFn as handler }`)
- The first argument to `addHandler()` is a **routing key** used by `callHandler()` to dispatch calls
- They receive a JavaScript event object as input
- They modify and return the event object
- Input/output is automatic — pass objects in, get objects back

Example:
```javascript
function handler(event) {
    // Read from event
    const input = event.inputField;
    
    // Process
    const result = input.toUpperCase();
    
    // Add to event and return
    event.outputField = result;
    return event;
}
```

## Troubleshooting

### Build errors
Build everything using just:
```bash
# From repository root
just build-js-host-api release
```

### Module loading errors
Make sure the native module is built:
```bash
cd ..
npm run build
```

## Creating Your Own Examples

1. Create a new .js file in this directory
2. Import the API: `const { SandboxBuilder } = require('../lib.js');`
3. Wrap your code in `async function main() { ... } main();`
4. Follow the pattern: build → load runtime → add handlers → get loaded sandbox → call handlers
5. Run with: `node your-example.js`
