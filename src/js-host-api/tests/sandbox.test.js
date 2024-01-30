// Basic sandbox functionality tests
import { describe, it, expect, beforeEach } from 'vitest';
import { SandboxBuilder } from '../lib.js';
import { expectThrowsWithCode, expectRejectsWithCode } from './test-helpers.js';

// ── SandboxBuilder ───────────────────────────────────────────────────

describe('SandboxBuilder', () => {
    it('should create a builder with defaults', () => {
        const builder = new SandboxBuilder();
        expect(builder).toBeInstanceOf(SandboxBuilder);
    });

    it('should support method chaining on setters', () => {
        const builder = new SandboxBuilder();
        const returned = builder.setHeapSize(8 * 1024 * 1024);
        // Builder setters should return `this` for chaining
        expect(returned).toBe(builder);
    });

    it('should chain multiple setters', () => {
        const builder = new SandboxBuilder();
        const result = builder
            .setHeapSize(8 * 1024 * 1024)
            .setStackSize(512 * 1024)
            .setInputBufferSize(4096)
            .setOutputBufferSize(4096);
        expect(result).toBe(builder);
    });

    it('should build a proto sandbox', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        expect(proto).toBeDefined();
    });

    it('should throw CONSUMED on double build()', async () => {
        const builder = new SandboxBuilder();
        await builder.build();
        await expectRejectsWithCode(builder.build(), 'ERR_CONSUMED');
    });

    it('should throw CONSUMED on setters after build()', async () => {
        const builder = new SandboxBuilder();
        await builder.build();
        expectThrowsWithCode(() => builder.setHeapSize(1024), 'ERR_CONSUMED');
    });

    // ── Validation ───────────────────────────────────────────────────

    it('should reject zero heap size', () => {
        const builder = new SandboxBuilder();
        expectThrowsWithCode(() => builder.setHeapSize(0), 'ERR_INVALID_ARG');
    });

    it('should reject zero stack size', () => {
        const builder = new SandboxBuilder();
        expectThrowsWithCode(() => builder.setStackSize(0), 'ERR_INVALID_ARG');
    });

    it('should reject zero input buffer size', () => {
        const builder = new SandboxBuilder();
        expectThrowsWithCode(() => builder.setInputBufferSize(0), 'ERR_INVALID_ARG');
    });

    it('should reject zero output buffer size', () => {
        const builder = new SandboxBuilder();
        expectThrowsWithCode(() => builder.setOutputBufferSize(0), 'ERR_INVALID_ARG');
    });
});

// ── ProtoJSSandbox ───────────────────────────────────────────────────

describe('ProtoJSSandbox', () => {
    it('should load the JavaScript runtime', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        expect(sandbox).toBeDefined();
    });

    it('should throw CONSUMED on double loadRuntime()', async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        await proto.loadRuntime();
        await expectRejectsWithCode(proto.loadRuntime(), 'ERR_CONSUMED');
    });
});

// ── JSSandbox ────────────────────────────────────────────────────────

describe('JSSandbox', () => {
    let sandbox;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        sandbox = await proto.loadRuntime();
    });

    it('should add a handler without throwing', () => {
        // addHandler is sync and returns void; no throw means success
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
    });

    it('should reject empty handler name on add', () => {
        expectThrowsWithCode(
            () => sandbox.addHandler('', 'function handler(e) { return e; }'),
            'ERR_INVALID_ARG'
        );
    });

    it('should remove a handler without throwing', () => {
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        sandbox.removeHandler('handler');
    });

    it('should reject empty handler name on remove', () => {
        expectThrowsWithCode(() => sandbox.removeHandler(''), 'ERR_INVALID_ARG');
    });

    it('should clear handlers without throwing', () => {
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        sandbox.clearHandlers();
    });

    it('should get a loaded sandbox', async () => {
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        const loaded = await sandbox.getLoadedSandbox();
        expect(loaded).toBeDefined();
    });

    it('should report poisoned state', () => {
        expect(sandbox.poisoned).toBe(false);
    });

    it('should throw CONSUMED on double getLoadedSandbox()', async () => {
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        await sandbox.getLoadedSandbox();
        await expectRejectsWithCode(sandbox.getLoadedSandbox(), 'ERR_CONSUMED');
    });

    it('should throw CONSUMED on operations after getLoadedSandbox()', async () => {
        sandbox.addHandler('handler', 'function handler(e) { return e; }');
        await sandbox.getLoadedSandbox();
        expectThrowsWithCode(
            () => sandbox.addHandler('another', 'function handler(e) { return e; }'),
            'ERR_CONSUMED'
        );
    });
});

// ── LoadedJSSandbox ──────────────────────────────────────────────────

describe('LoadedJSSandbox', () => {
    let loaded;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                event.message = 'Hello, ' + event.name + '!';
                return event;
            }
        `
        );
        loaded = await sandbox.getLoadedSandbox();
    });

    it('should handle events and return correct data', async () => {
        const result = await loaded.callHandler('handler', { name: 'World' }, { gc: false });

        expect(result.message).toBe('Hello, World!');
        expect(result.name).toBe('World');
    });

    it('should handle events with default options (no third arg)', async () => {
        const result = await loaded.callHandler('handler', { name: 'Default' });

        expect(result.message).toBe('Hello, Default!');
        expect(result.name).toBe('Default');
        expect(loaded.poisoned).toBe(false);
    });

    it('should handle events with explicit gc: true', async () => {
        const result = await loaded.callHandler('handler', { name: 'GC' }, { gc: true });

        expect(result.message).toBe('Hello, GC!');
        expect(result.name).toBe('GC');
        expect(loaded.poisoned).toBe(false);
    });

    it('should not be poisoned initially', () => {
        expect(loaded.poisoned).toBe(false);
    });

    it('should take and restore snapshots', async () => {
        await loaded.callHandler('handler', { name: 'Test' }, { gc: false });

        const snapshot = await loaded.snapshot();
        expect(snapshot).toBeDefined();

        await loaded.restore(snapshot);
        expect(loaded.poisoned).toBe(false);
    });

    it('should unload back to JSSandbox', async () => {
        const jsSandbox = await loaded.unload();
        expect(jsSandbox).toBeDefined();
    });

    it('should throw CONSUMED on double unload()', async () => {
        await loaded.unload();
        await expectRejectsWithCode(loaded.unload(), 'ERR_CONSUMED');
    });

    it('should throw CONSUMED on callHandler after unload', async () => {
        await loaded.unload();
        await expectRejectsWithCode(
            loaded.callHandler('handler', {}, { gc: false }),
            'ERR_CONSUMED'
        );
    });

    it('should reject empty handler name on callHandler', async () => {
        await expectRejectsWithCode(loaded.callHandler('', {}, { gc: false }), 'ERR_INVALID_ARG');
    });

    it('should provide an interrupt handle', () => {
        // interruptHandle is now a getter, not a method
        const handle = loaded.interruptHandle;
        expect(handle).toBeDefined();
        expect(typeof handle.kill).toBe('function');
    });
});

// ── Calculator (functional test) ─────────────────────────────────────

describe('Calculator example', () => {
    let loaded;

    beforeEach(async () => {
        const builder = new SandboxBuilder();
        const proto = await builder.build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            function handler(event) {
                const a = event.a;
                const b = event.b;
                const op = event.operation;
                
                let result;
                switch(op) {
                    case 'add': result = a + b; break;
                    case 'subtract': result = a - b; break;
                    case 'multiply': result = a * b; break;
                    case 'divide': result = b !== 0 ? a / b : 'Error: Division by zero'; break;
                    default: result = 'Error: Unknown operation';
                }
                
                event.result = result;
                return event;
            }
        `
        );
        loaded = await sandbox.getLoadedSandbox();
    });

    it('should add numbers', async () => {
        const result = await loaded.callHandler(
            'handler',
            { a: 10, b: 5, operation: 'add' },
            { gc: false }
        );
        expect(result.result).toBe(15);
    });

    it('should subtract numbers', async () => {
        const result = await loaded.callHandler(
            'handler',
            { a: 50, b: 30, operation: 'subtract' },
            { gc: false }
        );
        expect(result.result).toBe(20);
    });

    it('should multiply numbers', async () => {
        const result = await loaded.callHandler(
            'handler',
            { a: 20, b: 4, operation: 'multiply' },
            { gc: false }
        );
        expect(result.result).toBe(80);
    });

    it('should divide numbers', async () => {
        const result = await loaded.callHandler(
            'handler',
            { a: 100, b: 25, operation: 'divide' },
            { gc: false }
        );
        expect(result.result).toBe(4);
    });
});
