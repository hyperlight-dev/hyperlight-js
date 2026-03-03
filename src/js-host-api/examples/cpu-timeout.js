const { SandboxBuilder } = require('../lib.js');

// This example demonstrates combined CPU + wall-clock monitoring ⏱️
// Uses callHandler() with both cpuTimeoutMs and wallClockTimeoutMs
// The recommended pattern for comprehensive resource protection

async function main() {
    console.log('⏱️  Combined Monitor Example: CPU time + Wall Clock time\n');

    // Create sandbox
    const builder = new SandboxBuilder();
    const proto = await builder.build();
    const sandbox = await proto.loadRuntime();

    // Handler 1: Fast handler (completes before either timeout) ✅
    const fastCode = `
        function handler(event) {
            const startTime = Date.now();
            const RUNTIME = 100; // Run for 100ms
            
            let counter = 0;
            while (Date.now() - startTime < RUNTIME) {
                counter++;
            }
            
            event.message = "Fast handler completed";
            event.counter = counter;
            return event;
        }
    `;

    sandbox.addHandler('handler', fastCode);
    let loaded = await sandbox.getLoadedSandbox();

    console.log('📊 Test 1: Fast Handler (completes before either timeout)');
    console.log('   Handler: 100ms busy loop');
    console.log('   Timeout: 500ms CPU + 5s wall-clock\n');

    try {
        const result = await loaded.callHandler(
            'handler',
            {},
            {
                cpuTimeoutMs: 500,
                wallClockTimeoutMs: 5000,
            }
        );
        console.log(`   ✅ SUCCESS: Handler completed!`);
        console.log(`   📊 Counter: ${result.counter.toLocaleString()}`);
        printCallStats(loaded);
        console.log(`   🔒 Poisoned: ${loaded.poisoned}\n`);
    } catch (err) {
        console.log(`   ❌ Unexpected timeout: ${err.message}\n`);
    }

    // Handler 2: Slow handler (exceeds CPU timeout) 💀
    const slowCode = `
        function handler(event) {
            const startTime = Date.now();
            const RUNTIME = 3000; // Try to run for 3 seconds
            
            let counter = 0;
            while (Date.now() - startTime < RUNTIME) {
                counter++;
            }
            
            event.message = "Slow handler completed";
            event.counter = counter;
            return event;
        }
    `;

    // Unload and reload with slow handler
    let jsbox = await loaded.unload();
    jsbox.clearHandlers();
    jsbox.addHandler('handler', slowCode);
    loaded = await jsbox.getLoadedSandbox();

    // Take a snapshot before proceeding
    const snapshot = await loaded.snapshot();

    console.log('📊 Test 2: Slow Handler (CPU monitor fires first)');
    console.log('   Handler: 3-second busy loop');
    console.log('   Timeout: 500ms CPU + 5s wall-clock\n');

    const startTime = Date.now();
    try {
        await loaded.callHandler(
            'handler',
            {},
            {
                cpuTimeoutMs: 500,
                wallClockTimeoutMs: 5000,
            }
        );
        console.log(`   ❌ Unexpected: Handler completed without timeout\n`);
    } catch (err) {
        const elapsed = Date.now() - startTime;
        if (err.code === 'ERR_CANCELLED') {
            console.log(`   💀 Handler killed after ~${elapsed}ms`);
            console.log(`   ⚡ CPU time limit: 500ms (fired first for compute-bound work)`);
            console.log(`   ⏱️  Wall-clock limit: 5000ms (backstop, not reached)`);
            printCallStats(loaded);
            console.log(`   🔒 Poisoned: ${loaded.poisoned} (sandbox is in inconsistent state)`);
            console.log(`   ✅ SUCCESS: Timeout enforced correctly!\n`);

            // Demonstrate recovery from poisoned state
            console.log('📸 Restoring sandbox from snapshot...');
            await loaded.restore(snapshot);
            console.log(`   🔒 Poisoned after restore: ${loaded.poisoned}`);
            console.log('   ✅ Sandbox recovered and ready for use!\n');
        } else {
            console.log(`   ❌ Unexpected error: ${err.message}\n`);
        }
    }

    console.log('💡 Combined Monitors (Recommended Pattern):');
    console.log('   - cpuTimeoutMs: Catches compute-bound abuse (tight loops, crypto mining)');
    console.log('   - wallClockTimeoutMs: Catches resource exhaustion (blocking, holding FDs)');
    console.log('   - When both set: OR semantics — whichever fires first terminates execution');
    console.log('   - Neither alone is sufficient for comprehensive protection');
    console.log('   - Sandbox becomes poisoned after timeout, use snapshot/restore to recover\n');

    console.log('🔍 Use Cases:');
    console.log('   Wall Clock Only: { wallClockTimeoutMs: 5000 }');
    console.log('   CPU Time Only:   { cpuTimeoutMs: 500 }');
    console.log('   Combined (best): { wallClockTimeoutMs: 5000, cpuTimeoutMs: 500 }');
    console.log('\n✅ Combined monitor demonstration complete! ⏱️');
}

main().catch((error) => {
    console.error('\n❌ Error:', error.message);
    console.error('\nStack trace:', error.stack);
    process.exit(1);
});

/// Print last call stats from the loaded sandbox.
function printCallStats(loaded) {
    const stats = loaded.lastCallStats;
    if (stats) {
        console.log(
            `   📊 Stats: wall=${stats.wallClockMs.toFixed(1)}ms` +
                (stats.cpuTimeMs != null ? `, cpu=${stats.cpuTimeMs.toFixed(1)}ms` : '') +
                (stats.terminatedBy ? `, terminated_by=${stats.terminatedBy}` : '')
        );
    }
}
