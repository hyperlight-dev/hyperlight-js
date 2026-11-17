const { SandboxBuilder } = require('../lib.js');

// This example demonstrates using execution monitors to kill long-running handlers ⏱️
// Uses the callHandler() API with wall-clock timeout
// Also demonstrates poisoned state detection and recovery via snapshot/restore

async function main() {
    console.log('⏱️  Interrupt Example: Timeout-based handler termination\n');

    // Create sandbox
    const builder = new SandboxBuilder();
    const proto = await builder.build();
    const sandbox = await proto.loadRuntime();

    // Handler 1: Fast handler (completes before timeout) ✅
    const fastCode = `
        function handler(event) {
            const startTime = Date.now();
            const RUNTIME = 200; // Run for 200ms
            
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

    console.log('📊 Test 1: Fast Handler (completes before timeout)');
    console.log('   Handler: 200ms busy loop');
    console.log('   Timeout: 1000ms wall-clock\n');

    let startTime = Date.now();
    try {
        const result = await loaded.callHandler(
            'handler',
            {},
            {
                wallClockTimeoutMs: 1000,
            }
        );
        const elapsed = Date.now() - startTime;
        console.log(`   ✅ SUCCESS: Handler completed in ${elapsed}ms`);
        console.log(`   📊 Counter: ${result.counter.toLocaleString()}`);
        console.log(`   🎯 Timeout: 1000ms (not reached)`);
        console.log(`   🔒 Status: ${loaded.status}\n`);
    } catch (err) {
        console.log(`   ❌ Unexpected timeout: ${err.message}\n`);
    }

    // Handler 2: Slow handler (exceeds timeout) 💀
    const slowCode = `
        function handler(event) {
            const startTime = Date.now();
            const RUNTIME = 4000; // Run for 4 seconds
            
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

    console.log('📊 Test 2: Slow Handler (exceeds timeout)');
    console.log('   Handler: 4-second busy loop');
    console.log('   Timeout: 1000ms wall-clock\n');

    startTime = Date.now();
    try {
        await loaded.callHandler('handler', {}, { wallClockTimeoutMs: 1000 });
        const elapsed = Date.now() - startTime;
        console.log(`   ❌ Unexpected: Handler completed in ${elapsed}ms without timeout\n`);
    } catch (err) {
        const elapsed = Date.now() - startTime;
        if (err.code === 'ERR_CANCELLED') {
            console.log(`   💀 Handler killed after ~${elapsed}ms`);
            console.log(`   ⏱️  Expected: ~1000ms`);
            console.log(`   🔒 Status: ${loaded.status} (sandbox is in inconsistent state)`);
            console.log(`   ✅ SUCCESS: Handler was properly interrupted!\n`);

            // Demonstrate recovery from poisoned state
            console.log('📸 Restoring sandbox from snapshot...');
            await loaded.restore(snapshot);
            console.log(`   🔒 Status after restore: ${loaded.status}`);
            console.log('   ✅ Sandbox recovered and ready for use!\n');
        } else {
            console.log(`   ❌ Unexpected error: ${err.message}\n`);
        }
    }

    console.log('💡 How it works:');
    console.log('   - callHandler() accepts { wallClockTimeoutMs, cpuTimeoutMs } in options');
    console.log('   - Set one or both — when both are set they race (OR semantics)');
    console.log('   - After timeout, the monitor calls interruptHandle.kill()');
    console.log('   - Handler execution is interrupted and throws an error');
    console.log('   - Sandbox becomes poisoned after interruption');
    console.log('   - Use snapshot/restore to recover from poisoned state');
    console.log('   - Perfect for enforcing execution time limits! ⏱️');
}

main().catch((error) => {
    console.error('\n❌ Error:', error.message);
    console.error('\nStack trace:', error.stack);
    process.exit(1);
});
