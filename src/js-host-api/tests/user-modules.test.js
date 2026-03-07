// User module registration and import tests
//
// Tests the `addModule()` / `removeModule()` / `clearModules()` NAPI API
// for registering ES modules that handlers (and other modules) can import
// using the `<namespace>:<name>` convention.

import { describe, it, expect, beforeEach } from 'vitest';
import { SandboxBuilder } from '../lib.js';
import { expectThrowsWithCode, expectRejectsWithCode } from './test-helpers.js';

// ── Helpers ──────────────────────────────────────────────────────────

/**
 * Create a JSSandbox (runtime loaded, ready for handlers/modules).
 * @returns {Promise<import('../lib.js').JSSandbox>}
 */
async function createSandbox() {
    const proto = await new SandboxBuilder().build();
    return proto.loadRuntime();
}

// ── Module registration ──────────────────────────────────────────────

describe('Module registration', () => {
    let sandbox;

    beforeEach(async () => {
        sandbox = await createSandbox();
    });

    it('should add a module without throwing', () => {
        sandbox.addModule('utils', 'export function greet() { return "hi"; }');
    });

    it('should add a module with explicit namespace', () => {
        sandbox.addModule('utils', 'export function greet() { return "hi"; }', 'mylib');
    });

    it('should remove a module without throwing', () => {
        sandbox.addModule('utils', 'export const x = 1;');
        sandbox.removeModule('utils');
    });

    it('should remove a module with explicit namespace', () => {
        sandbox.addModule('utils', 'export const x = 1;', 'mylib');
        sandbox.removeModule('utils', 'mylib');
    });

    it('should clear all modules without throwing', () => {
        sandbox.addModule('a', 'export const x = 1;');
        sandbox.addModule('b', 'export const y = 2;');
        sandbox.clearModules();
    });

    // ── Validation ───────────────────────────────────────────────────

    it('should reject empty module name on add', () => {
        expectThrowsWithCode(() => sandbox.addModule('', 'export const x = 1;'), 'ERR_INVALID_ARG');
    });

    it('should reject empty module name on remove', () => {
        expectThrowsWithCode(() => sandbox.removeModule(''), 'ERR_INVALID_ARG');
    });

    it('should reject reserved "host" namespace', () => {
        expectThrowsWithCode(
            () => sandbox.addModule('utils', 'export const x = 1;', 'host'),
            'ERR_INVALID_ARG'
        );
    });

    it('should reject duplicate module', () => {
        sandbox.addModule('utils', 'export const x = 1;');
        expectThrowsWithCode(
            () => sandbox.addModule('utils', 'export const y = 2;'),
            'ERR_INTERNAL' // duplicate check is in the inner Rust layer
        );
    });

    it('should throw CONSUMED after getLoadedSandbox()', async () => {
        sandbox.addModule('utils', 'export const x = 1;');
        sandbox.addHandler(
            'handler',
            `
            import { x } from 'user:utils';
            function handler(e) { e.x = x; return e; }
        `
        );
        await sandbox.getLoadedSandbox();
        expectThrowsWithCode(
            () => sandbox.addModule('another', 'export const y = 2;'),
            'ERR_CONSUMED'
        );
    });
});

// ── Handler importing a user module ──────────────────────────────────

describe('Handler importing user module', () => {
    it('should import module with default namespace', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule(
            'math',
            `
            export function add(a, b) { return a + b; }
            export function multiply(a, b) { return a * b; }
        `
        );
        sandbox.addHandler(
            'handler',
            `
            import { add, multiply } from 'user:math';
            function handler(event) {
                event.sum = add(event.a, event.b);
                event.product = multiply(event.a, event.b);
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', { a: 6, b: 7 });
        expect(result.sum).toBe(13);
        expect(result.product).toBe(42);
    });

    it('should import module with custom namespace', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule('math', 'export function add(a, b) { return a + b; }', 'mylib');
        sandbox.addHandler(
            'handler',
            `
            import { add } from 'mylib:math';
            function handler(event) {
                event.result = add(event.a, event.b);
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', { a: 10, b: 20 });
        expect(result.result).toBe(30);
    });
});

// ── Module importing another module ──────────────────────────────────

describe('Inter-module imports', () => {
    it('should allow a user module to import another user module', async () => {
        const sandbox = await createSandbox();

        // constants is imported by geometry — registration order doesn't matter
        sandbox.addModule(
            'geometry',
            `
            import { PI } from 'user:constants';
            export function circleArea(r) { return PI * r * r; }
        `
        );
        sandbox.addModule(
            'constants',
            `
            export const PI = 3.14159;
        `
        );

        sandbox.addHandler(
            'handler',
            `
            import { circleArea } from 'user:geometry';
            function handler(event) {
                event.area = circleArea(event.radius);
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', { radius: 5 });
        expect(result.area).toBeCloseTo(78.53975, 3);
    });

    it('should allow a user module to import a built-in module', async () => {
        const sandbox = await createSandbox();

        sandbox.addModule(
            'hasher',
            `
            import { createHmac } from 'crypto';
            export function hmac(data) {
                return createHmac('sha256', 'secret').update(data).digest('hex');
            }
        `
        );

        sandbox.addHandler(
            'handler',
            `
            import { hmac } from 'user:hasher';
            function handler(event) {
                event.hash = hmac(event.data);
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', { data: 'hello' });
        expect(result.hash).toBeTruthy();
        expect(typeof result.hash).toBe('string');
        expect(result.hash.length).toBeGreaterThan(0);
    });
});

// ── Module importing host functions ──────────────────────────────────

describe('User module importing host functions', () => {
    it('should allow a user module to call a host function', async () => {
        const proto = await new SandboxBuilder().build();
        proto.hostModule('db').register('lookup', (id) => ({ id, name: `User ${id}` }));

        const sandbox = await proto.loadRuntime();

        sandbox.addModule(
            'enricher',
            `
            import * as db from 'host:db';
            export function enrich(event) {
                const user = db.lookup(event.userId);
                event.userName = user.name;
                return event;
            }
        `
        );

        sandbox.addHandler(
            'handler',
            `
            import { enrich } from 'user:enricher';
            function handler(event) {
                return enrich(event);
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', { userId: 42 });
        expect(result.userName).toBe('User 42');
    });
});

// ── State retention ──────────────────────────────────────────────────

describe('Module state retention', () => {
    it('should retain module state between handler calls', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule(
            'counter',
            `
            let count = 0;
            export function increment() { return ++count; }
        `
        );
        sandbox.addHandler(
            'handler',
            `
            import { increment } from 'user:counter';
            function handler(event) {
                event.count = increment();
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const r1 = await loaded.callHandler('handler', {});
        expect(r1.count).toBe(1);
        const r2 = await loaded.callHandler('handler', {});
        expect(r2.count).toBe(2);
    });
});

// ── Multiple handlers sharing a module ───────────────────────────────

describe('Multiple handlers sharing a module', () => {
    it('should allow two handlers to import the same module', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule(
            'utils',
            `
            export function double(x) { return x * 2; }
            export function triple(x) { return x * 3; }
        `
        );
        sandbox.addHandler(
            'doubler',
            `
            import { double } from 'user:utils';
            function handler(event) { event.result = double(event.x); return event; }
        `
        );
        sandbox.addHandler(
            'tripler',
            `
            import { triple } from 'user:utils';
            function handler(event) { event.result = triple(event.x); return event; }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const r1 = await loaded.callHandler('doubler', { x: 5 });
        expect(r1.result).toBe(10);
        const r2 = await loaded.callHandler('tripler', { x: 5 });
        expect(r2.result).toBe(15);
    });
});

// ── Cross-handler mutable state sharing ──────────────────────────────

describe('Cross-handler mutable state sharing', () => {
    it('should allow handler B to see mutable state written by handler A', async () => {
        const sandbox = await createSandbox();

        // Shared module with mutable state: a simple counter.
        sandbox.addModule(
            'counter',
            `
            let count = 0;
            export function increment() { return ++count; }
            export function getCount() { return count; }
        `
        );

        // Handler A: mutates state by calling increment()
        sandbox.addHandler(
            'writer',
            `
            import { increment } from 'user:counter';
            function handler(event) {
                event.count = increment();
                return event;
            }
        `
        );

        // Handler B: reads state without mutating it
        sandbox.addHandler(
            'reader',
            `
            import { getCount } from 'user:counter';
            function handler(event) {
                event.count = getCount();
                return event;
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();

        // writer increments → count=1
        const r1 = await loaded.callHandler('writer', {});
        expect(r1.count).toBe(1);

        // reader sees the mutation made by writer → count=1
        const r2 = await loaded.callHandler('reader', {});
        expect(r2.count).toBe(1);

        // writer increments again → count=2
        const r3 = await loaded.callHandler('writer', {});
        expect(r3.count).toBe(2);

        // reader sees the updated state → count=2
        const r4 = await loaded.callHandler('reader', {});
        expect(r4.count).toBe(2);
    });

    it('should share complex mutable state between multiple handlers', async () => {
        const sandbox = await createSandbox();

        // Shared module with a richer state store (key-value map).
        sandbox.addModule(
            'store',
            `
            const data = new Map();
            export function set(key, value) { data.set(key, value); }
            export function get(key) { return data.get(key); }
            export function size() { return data.size; }
        `
        );

        // Handler that writes to the store
        sandbox.addHandler(
            'put',
            `
            import { set } from 'user:store';
            function handler(event) {
                set(event.key, event.value);
                return { ok: true };
            }
        `
        );

        // Handler that reads from the store
        sandbox.addHandler(
            'fetch',
            `
            import { get, size } from 'user:store';
            function handler(event) {
                return { value: get(event.key), size: size() };
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();

        // Write two entries via the 'put' handler
        await loaded.callHandler('put', { key: 'name', value: 'Hyperlight' });
        await loaded.callHandler('put', { key: 'year', value: 1985 });

        // Read them back via the 'fetch' handler
        const r1 = await loaded.callHandler('fetch', { key: 'name' });
        expect(r1.value).toBe('Hyperlight');
        expect(r1.size).toBe(2);

        const r2 = await loaded.callHandler('fetch', { key: 'year' });
        expect(r2.value).toBe(1985);
    });
});

// ── Unload / reload cycle ────────────────────────────────────────────

describe('Unload and reload with modules', () => {
    it('should allow swapping module versions after unload', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule('math', 'export function compute(x) { return x + 1; }');
        sandbox.addHandler(
            'handler',
            `
            import { compute } from 'user:math';
            function handler(event) { event.result = compute(event.x); return event; }
        `
        );

        let loaded = await sandbox.getLoadedSandbox();
        let result = await loaded.callHandler('handler', { x: 5 });
        expect(result.result).toBe(6); // 5 + 1

        // Unload, swap module, reload
        const sandbox2 = await loaded.unload();
        sandbox2.addModule('math', 'export function compute(x) { return x * 2; }');
        sandbox2.addHandler(
            'handler',
            `
            import { compute } from 'user:math';
            function handler(event) { event.result = compute(event.x); return event; }
        `
        );

        loaded = await sandbox2.getLoadedSandbox();
        result = await loaded.callHandler('handler', { x: 5 });
        expect(result.result).toBe(10); // 5 * 2
    });
});

// ── Error on missing module import ───────────────────────────────────

describe('Missing module import', () => {
    it('should fail at getLoadedSandbox when handler imports non-existent module', async () => {
        const sandbox = await createSandbox();
        sandbox.addHandler(
            'handler',
            `
            import { foo } from 'user:nonexistent';
            function handler(event) { return event; }
        `
        );

        // Loading should fail because the module doesn't exist
        await expectRejectsWithCode(sandbox.getLoadedSandbox(), 'ERR_INTERNAL');
    });
});

// ── clearModules behaviour verification ──────────────────────────────

describe('clearModules behaviour', () => {
    it('should make cleared modules unavailable on next load', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule('utils', 'export function greet() { return "hi"; }');
        sandbox.addHandler(
            'handler',
            `
            import { greet } from 'user:utils';
            function handler(event) { event.msg = greet(); return event; }
        `
        );

        // Clear the module before loading
        sandbox.clearModules();

        // Loading should fail — handler imports a now-missing module
        await expectRejectsWithCode(sandbox.getLoadedSandbox(), 'ERR_INTERNAL');
    });
});

// ── Remove → load lifecycle ──────────────────────────────────────────

describe('Remove then load lifecycle', () => {
    it('should fail when handler imports a removed module', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule('utils', 'export function greet() { return "hi"; }');
        sandbox.removeModule('utils');
        sandbox.addHandler(
            'handler',
            `
            import { greet } from 'user:utils';
            function handler(event) { event.msg = greet(); return event; }
        `
        );

        await expectRejectsWithCode(sandbox.getLoadedSandbox(), 'ERR_INTERNAL');
    });
});

// ── Snapshot / restore with modules ──────────────────────────────────

describe('Snapshot and restore with modules', () => {
    it('should restore module state from snapshot', async () => {
        const sandbox = await createSandbox();
        sandbox.addModule(
            'counter',
            `
            let count = 0;
            export function increment() { return ++count; }
        `
        );
        sandbox.addHandler(
            'handler',
            `
            import { increment } from 'user:counter';
            function handler(event) { event.count = increment(); return event; }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const r1 = await loaded.callHandler('handler', {});
        expect(r1.count).toBe(1);

        const snapshot = await loaded.snapshot();

        const r2 = await loaded.callHandler('handler', {});
        expect(r2.count).toBe(2);

        await loaded.restore(snapshot);

        // After restore, counter should be back to snapshot state (count=1)
        const r3 = await loaded.callHandler('handler', {});
        expect(r3.count).toBe(2); // post-snapshot call → 2 again
    });
});

// ── Circular module dependencies ─────────────────────────────────────

describe('Circular module dependencies', () => {
    it('should handle circular imports between two modules', async () => {
        const sandbox = await createSandbox();

        // Module A imports B, module B imports A — ESM circular imports
        // are well-defined: live bindings resolve after evaluation
        sandbox.addModule(
            'moduleA',
            `
            import { getY } from 'user:moduleB';
            export function getX() { return 'X'; }
            export function getXY() { return getX() + getY(); }
        `
        );
        sandbox.addModule(
            'moduleB',
            `
            import { getX } from 'user:moduleA';
            export function getY() { return 'Y'; }
            export function getYX() { return getY() + getX(); }
        `
        );

        sandbox.addHandler(
            'handler',
            `
            import { getXY } from 'user:moduleA';
            import { getYX } from 'user:moduleB';
            function handler(event) {
                return { xy: getXY(), yx: getYX() };
            }
        `
        );

        const loaded = await sandbox.getLoadedSandbox();
        const result = await loaded.callHandler('handler', {});
        expect(result.xy).toBe('XY');
        expect(result.yx).toBe('YX');
    });
});

// ── Additional validation ────────────────────────────────────────────

describe('Additional validation', () => {
    it('should reject colon in module name', async () => {
        const sandbox = await createSandbox();
        expectThrowsWithCode(
            () => sandbox.addModule('bad:name', 'export const x = 1;'),
            'ERR_INVALID_ARG'
        );
    });

    it('should reject control characters in module name', async () => {
        const sandbox = await createSandbox();
        expectThrowsWithCode(
            () => sandbox.addModule('bad\nname', 'export const x = 1;'),
            'ERR_INVALID_ARG'
        );
    });

    it('should reject whitespace-only module name', async () => {
        const sandbox = await createSandbox();
        expectThrowsWithCode(
            () => sandbox.addModule('   ', 'export const x = 1;'),
            'ERR_INVALID_ARG'
        );
    });
});
