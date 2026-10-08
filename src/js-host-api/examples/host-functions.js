// Host Functions example — register host-side callbacks that guest JS can call
//
// Demonstrates:
// - Sync host functions (immediate return)
// - Async host functions (Promise-returning)
// - Multiple modules and functions
// - Reusable host-function module definitions with per-sandbox state

const { HostFunctionModule, SandboxBuilder } = require('../lib.js');

function hostFunctionModules() {
    // ── Define fresh host-function modules ───────────────────────────
    // Guest JS imports these as: import * as math from "host:math"
    // Args are auto-parsed from JSON and spread; return value auto-stringified.
    const math = new HostFunctionModule('math');
    math.register('add', (a, b) => a + b);
    math.register('multiply', (a, b) => a * b);

    // ── Register an async host function ──────────────────────────────
    // Async callbacks are automatically awaited by the bridge.
    // The guest call still blocks (Hyperlight is sync), but the host
    // can do async work (DB queries, HTTP, file I/O, etc.)
    const greetings = new HostFunctionModule('greetings');
    greetings.register('hello', async (name) => {
        // Simulate async work (e.g. looking up a name in a database)
        await new Promise((resolve) => setTimeout(resolve, 50));
        return `Hello, ${name}! 👋`;
    });

    const strings = new HostFunctionModule('strings');
    strings.register('upper', (s) => s.toUpperCase());

    return [math, greetings, strings];
}

async function main() {
    console.log('=== Hyperlight JS Host Functions ===\n');
    console.log('2. Registering host functions...');
    console.log('   ✓ Host functions registered\n');

    // Reuse the factory for another builder; each invocation creates fresh
    // module objects, callbacks, and captured state.
    const proto = await new SandboxBuilder()
        .setHeapSize(8 * 1024 * 1024)
        .setScratchSize(1024 * 1024)
        .setHostFunctionModules(hostFunctionModules)
        .build();

    // ── Load runtime and add a handler ───────────────────────────────
    console.log('3. Loading runtime...');
    const sandbox = await proto.loadRuntime();
    console.log('   ✓ Runtime loaded\n');

    console.log('4. Adding handler...');
    sandbox.addHandler(
        'handler',
        `
        import * as math from "host:math";
        import * as greetings from "host:greetings";
        import * as strings from "host:strings";

        function handler(event) {
            const sum = math.add(event.a, event.b);
            const product = math.multiply(event.a, event.b);
            const greeting = greetings.hello(event.name);
            const shout = strings.upper(event.message);

            return { sum, product, greeting, shout };
        }
        `
    );
    const loaded = await sandbox.getLoadedSandbox();
    console.log('   ✓ Handler ready\n');

    // ── Call the handler ─────────────────────────────────────────────
    console.log('5. Calling handler...');
    const result = await loaded.callHandler('handler', {
        a: 6,
        b: 7,
        name: 'World',
        message: 'hyperlight is rad',
    });

    console.log('   Result:', JSON.stringify(result, null, 2));
    console.log('\n✅ Host functions example complete!');
}

main().catch((err) => {
    console.error('Error:', err);
    process.exit(1);
});
