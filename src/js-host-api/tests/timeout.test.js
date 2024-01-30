// Timeout and interrupt tests
import { describe, it, expect, beforeEach } from 'vitest';
import { SandboxBuilder } from '../lib.js';
import { expectRejectsWithCode } from './test-helpers.js';

describe('Wall Clock Timeout', () => {
    let loaded;
    let snapshot;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                const startTime = Date.now();
                const runtime = event.runtime || 100;
                
                let counter = 0;
                while (Date.now() - startTime < runtime) {
                    counter++;
                }
                
                event.counter = counter;
                event.actualRuntime = Date.now() - startTime;
                return event;
            }
        `
        );
        loaded = await sandbox.getLoadedSandbox();
        snapshot = await loaded.snapshot();
    });

    it('should complete fast handler before timeout', async () => {
        const result = await loaded.callHandler(
            'handler',
            { runtime: 100 },
            {
                wallClockTimeoutMs: 1000,
            }
        );

        // Result is now a JS object, not a string
        expect(typeof result).toBe('object');
        expect(result.counter).toBeGreaterThan(0);
        expect(result.actualRuntime).toBeLessThan(500);
        expect(loaded.poisoned).toBe(false);
    });

    it('should kill slow handler after timeout', async () => {
        const startTime = Date.now();

        // Handler tries to run for 4 seconds, timeout is 500ms
        // Should reject when terminated
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                { runtime: 4000 },
                {
                    wallClockTimeoutMs: 500,
                }
            ),
            'ERR_CANCELLED'
        );

        const elapsed = Date.now() - startTime;

        // Should have been killed around 500ms, not 4000ms
        expect(elapsed).toBeLessThan(2000);
        expect(elapsed).toBeGreaterThan(300);
        expect(loaded.poisoned).toBe(true);
    });

    it('should recover from poisoned state with restore', async () => {
        // Kill the handler - should reject
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                { runtime: 4000 },
                {
                    wallClockTimeoutMs: 500,
                }
            ),
            'ERR_CANCELLED'
        );

        expect(loaded.poisoned).toBe(true);

        // Restore from snapshot
        await loaded.restore(snapshot);

        expect(loaded.poisoned).toBe(false);

        // Should be able to use the sandbox again
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                wallClockTimeoutMs: 1000,
            }
        );
        expect(typeof result).toBe('object');
    });
});

describe('CPU Time Timeout', () => {
    let loaded;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                const startTime = Date.now();
                const runtime = event.runtime || 100;
                
                let counter = 0;
                while (Date.now() - startTime < runtime) {
                    counter++;
                }
                
                event.counter = counter;
                return event;
            }
        `
        );
        loaded = await sandbox.getLoadedSandbox();
    });

    it('should complete fast handler before CPU timeout', async () => {
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                cpuTimeoutMs: 500,
            }
        );

        // Result is now a JS object, not a string
        expect(typeof result).toBe('object');
        expect(result.counter).toBeGreaterThan(0);
        expect(loaded.poisoned).toBe(false);
    });

    it('should kill CPU-intensive handler after CPU timeout', async () => {
        const startTime = Date.now();

        // Handler tries to run for 3 seconds of CPU, timeout is 500ms
        // Should reject when terminated
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                { runtime: 3000 },
                {
                    cpuTimeoutMs: 500,
                }
            ),
            'ERR_CANCELLED'
        );

        const elapsed = Date.now() - startTime;

        // Should have been killed well before 3 seconds
        // CPU time is close to wall time for busy loops
        expect(elapsed).toBeLessThan(2000);
        expect(loaded.poisoned).toBe(true);
    });
});

describe('Combined Monitors (CPU + Wall Clock)', () => {
    let loaded;
    let snapshot;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                const startTime = Date.now();
                const runtime = event.runtime || 100;
                
                let counter = 0;
                while (Date.now() - startTime < runtime) {
                    counter++;
                }
                
                event.counter = counter;
                return event;
            }
        `
        );
        loaded = await sandbox.getLoadedSandbox();
        snapshot = await loaded.snapshot();
    });

    it('should complete fast handler with both monitors', async () => {
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                wallClockTimeoutMs: 5000,
                cpuTimeoutMs: 2000,
            }
        );

        expect(result.counter).toBeGreaterThan(0);
        expect(loaded.poisoned).toBe(false);
    });

    it('should kill CPU-intensive handler (CPU fires first)', async () => {
        const startTime = Date.now();

        // CPU timeout (500ms) should fire before wall-clock (5s) for a tight loop
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                { runtime: 10000 },
                {
                    wallClockTimeoutMs: 5000,
                    cpuTimeoutMs: 500,
                }
            ),
            'ERR_CANCELLED'
        );

        const elapsed = Date.now() - startTime;

        // CPU monitor should fire well before the wall-clock timeout
        expect(elapsed).toBeLessThan(3000);
        expect(loaded.poisoned).toBe(true);
    });

    it('should recover after combined monitor kill', async () => {
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                { runtime: 10000 },
                {
                    wallClockTimeoutMs: 5000,
                    cpuTimeoutMs: 500,
                }
            ),
            'ERR_CANCELLED'
        );

        expect(loaded.poisoned).toBe(true);

        // Restore from snapshot
        await loaded.restore(snapshot);
        expect(loaded.poisoned).toBe(false);

        // Should work again
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                wallClockTimeoutMs: 5000,
                cpuTimeoutMs: 2000,
            }
        );
        expect(typeof result).toBe('object');
    });

    it('should call handler without monitors when no timeout is specified', async () => {
        // Passing empty options (no timeouts) runs via the fast path without monitors
        const result = await loaded.callHandler('handler', { runtime: 50 }, {});
        expect(typeof result).toBe('object');
        expect(result.counter).toBeGreaterThan(0);
        expect(loaded.poisoned).toBe(false);
    });

    it('should call handler with gc: false and no monitors', async () => {
        const result = await loaded.callHandler('handler', { runtime: 50 }, { gc: false });
        expect(typeof result).toBe('object');
        expect(result.counter).toBeGreaterThan(0);
        expect(loaded.poisoned).toBe(false);
    });

    it('should call handler with gc: true and no monitors', async () => {
        const result = await loaded.callHandler('handler', { runtime: 50 }, { gc: true });
        expect(typeof result).toBe('object');
        expect(result.counter).toBeGreaterThan(0);
        expect(loaded.poisoned).toBe(false);
    });

    it('should throw INVALID_ARG for timeout exceeding maximum', async () => {
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                {},
                {
                    wallClockTimeoutMs: 4_000_000,
                }
            ),
            'ERR_INVALID_ARG'
        );
    });

    it('should throw INVALID_ARG for zero wallClockTimeoutMs', async () => {
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                {},
                {
                    wallClockTimeoutMs: 0,
                }
            ),
            'ERR_INVALID_ARG'
        );
    });

    it('should throw INVALID_ARG for zero cpuTimeoutMs', async () => {
        await expectRejectsWithCode(
            loaded.callHandler(
                'handler',
                {},
                {
                    cpuTimeoutMs: 0,
                }
            ),
            'ERR_INVALID_ARG'
        );
    });

    it('should accept gc option', async () => {
        // gc: false should still work — handler completes normally
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                wallClockTimeoutMs: 5000,
                cpuTimeoutMs: 2000,
                gc: false,
            }
        );
        expect(typeof result).toBe('object');
        expect(loaded.poisoned).toBe(false);
    });

    it('should default gc to true when not specified', async () => {
        // Not passing gc should behave identically to gc: true
        const result = await loaded.callHandler(
            'handler',
            { runtime: 50 },
            {
                wallClockTimeoutMs: 5000,
            }
        );
        expect(typeof result).toBe('object');
        expect(loaded.poisoned).toBe(false);
    });
});

describe('Interrupt Handle', () => {
    it('should provide an interrupt handle', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        const loaded = await sandbox.getLoadedSandbox();

        // interruptHandle is now a getter, not a method
        const handle = loaded.interruptHandle;
        expect(handle).toBeDefined();
        expect(typeof handle.kill).toBe('function');
    });

    it('should kill a running handler via interruptHandle', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        // Handler that runs for 10 seconds — plenty of time for kill() to fire
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                const startTime = Date.now();
                while (Date.now() - startTime < 10000) { /* busy loop */ }
                return event;
            }
        `
        );
        const loaded = await sandbox.getLoadedSandbox();
        const handle = loaded.interruptHandle;

        // Start the handler, then kill it from a timer
        const promise = loaded.callHandler('handler', {});
        const timer = setTimeout(() => handle.kill(), 200);

        await expectRejectsWithCode(promise, 'ERR_CANCELLED');
        clearTimeout(timer);
        expect(loaded.poisoned).toBe(true);
    });

    it('should reject empty handler name on callHandler', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        const loaded = await sandbox.getLoadedSandbox();

        await expectRejectsWithCode(
            loaded.callHandler('', {}, { wallClockTimeoutMs: 1000 }),
            'ERR_INVALID_ARG'
        );
    });
});
