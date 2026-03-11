// Host function registration and invocation tests
//
// Tests the NAPI bridge for registering host-side JS callbacks that guest
// sandboxed code can call via `import * as <module> from "host:<module>"`.
import { describe, it, expect, beforeEach } from 'vitest';
import { SandboxBuilder } from '../lib.js';
import { expectThrowsWithCode } from './test-helpers.js';

// ── Helpers ──────────────────────────────────────────────────────────

/**
 * Build a full pipeline: proto → register host fns → runtime → add handler → loaded.
 *
 * @param {(proto: import('../lib.js').ProtoJSSandbox) => void} registerFns
 *   — callback to register host functions on the proto sandbox
 * @param {string} handlerScript
 *   — guest JS source defining a `handler(event)` function
 * @returns {Promise<import('../lib.js').LoadedJSSandbox>}
 */
async function buildLoadedSandbox(registerFns, handlerScript) {
    const proto = await new SandboxBuilder().build();
    registerFns(proto);
    const sandbox = await proto.loadRuntime();
    sandbox.addHandler('handler', handlerScript);
    return sandbox.getLoadedSandbox();
}

// ── HostModule ────────────────────────────────────────────────

describe('HostModule', () => {
    let proto;

    beforeEach(async () => {
        proto = await new SandboxBuilder().build();
    });

    it('should return a HostModule from hostModule()', () => {
        const builder = proto.hostModule('math');
        expect(builder).toBeDefined();
        expect(typeof builder.register).toBe('function');
    });

    it('should throw on empty module name', () => {
        expectThrowsWithCode(() => proto.hostModule(''), 'ERR_INVALID_ARG');
    });

    it('should throw on empty function name in register()', () => {
        const builder = proto.hostModule('math');
        expectThrowsWithCode(() => builder.register('', () => 42), 'ERR_INVALID_ARG');
    });
});

// ── ProtoJSSandbox.register() convenience method ─────────────────────

describe('ProtoJSSandbox.register()', () => {
    let proto;

    beforeEach(async () => {
        proto = await new SandboxBuilder().build();
    });

    it('should throw on empty module name', () => {
        expectThrowsWithCode(() => proto.register('', 'add', () => 42), 'ERR_INVALID_ARG');
    });

    it('should throw on empty function name', () => {
        expectThrowsWithCode(() => proto.register('math', '', () => 42), 'ERR_INVALID_ARG');
    });
});

// ── Host function invocation (end-to-end) ────────────────────────────

describe('Host function invocation', () => {
    it('should call a sync host function from guest code', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('math').register('add', (a, b) => a + b);
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.add(event.a, event.b) };
            }
            `
        );

        const result = await loaded.callHandler('handler', { a: 10, b: 32 });
        expect(result).toEqual({ result: 42 });
    });

    it('should call an async host function from guest code', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('utils').register('greet', async (name) => {
                    // Simulate async work (e.g., a database lookup)
                    await new Promise((resolve) => setTimeout(resolve, 10));
                    return `Hello, ${name}!`;
                });
            },
            `
            import * as utils from "host:utils";
            function handler(event) {
                return { greeting: utils.greet(event.name) };
            }
            `
        );

        const result = await loaded.callHandler('handler', { name: 'World' });
        expect(result).toEqual({ greeting: 'Hello, World!' });
    });

    it('should support multiple functions in one module', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                const math = proto.hostModule('math');
                math.register('add', (a, b) => a + b);
                math.register('multiply', (a, b) => a * b);
            },
            `
            import * as math from "host:math";
            function handler(event) {
                let sum = math.add(event.a, event.b);
                let product = math.multiply(event.a, event.b);
                return { sum, product };
            }
            `
        );

        const result = await loaded.callHandler('handler', { a: 6, b: 7 });
        expect(result).toEqual({ sum: 13, product: 42 });
    });

    it('should support multiple modules', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('math').register('add', (a, b) => a + b);
                proto.hostModule('strings').register('upper', (s) => s.toUpperCase());
            },
            `
            import * as math from "host:math";
            import * as strings from "host:strings";
            function handler(event) {
                let sum = math.add(event.a, event.b);
                let upper = strings.upper(event.name);
                return { sum, upper };
            }
            `
        );

        const result = await loaded.callHandler('handler', { a: 1, b: 2, name: 'hello' });
        expect(result).toEqual({ sum: 3, upper: 'HELLO' });
    });

    it('should support the convenience register() method', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.register('math', 'add', (a, b) => a + b);
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.add(3, 4) };
            }
            `
        );

        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ result: 7 });
    });

    it('should propagate errors from host function callbacks', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('explode', () => {
                    throw new Error('💥 kaboom from host');
                });
            },
            `
            import * as host from "host:host";
            function handler(event) {
                return host.explode();
            }
            `
        );

        // The guest catches host errors as HostFunctionError
        await expect(loaded.callHandler('handler', {})).rejects.toThrow();
    });

    it('should handle host function returning complex objects', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('db').register('query', (table) => ({
                    rows: [
                        { id: 1, name: 'Alice' },
                        { id: 2, name: 'Bob' },
                    ],
                    table,
                }));
            },
            `
            import * as db from "host:db";
            function handler(event) {
                let result = db.query(event.table);
                return { count: result.rows.length, first: result.rows[0].name };
            }
            `
        );

        const result = await loaded.callHandler('handler', { table: 'users' });
        expect(result).toEqual({ count: 2, first: 'Alice' });
    });

    it('should work with snapshot/restore cycle', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                let counter = 0;
                proto.hostModule('state').register('increment', () => {
                    counter++;
                    return counter;
                });
            },
            `
            import * as state from "host:state";
            function handler(event) {
                return { count: state.increment() };
            }
            `
        );

        // Take snapshot, call handler, restore, call again
        const snapshot = await loaded.snapshot();
        const r1 = await loaded.callHandler('handler', {});
        expect(r1.count).toBe(1);

        await loaded.restore(snapshot);
        const r2 = await loaded.callHandler('handler', {});
        // Host-side counter keeps incrementing (it's outside the sandbox)
        // but guest state was restored
        expect(r2.count).toBe(2);
    });

    // ── register() — host function registration ─────────────────

    it('should support register() on HostModule', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('math').register('add', (a, b) => {
                    return a + b;
                });
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.add(event.a, event.b) };
            }
            `
        );

        const result = await loaded.callHandler('handler', { a: 10, b: 32 });
        expect(result).toEqual({ result: 42 });
    });

    it('should support register() convenience method', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.register('math', 'add', (a, b) => {
                    return a + b;
                });
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.add(3, 4) };
            }
            `
        );

        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ result: 7 });
    });

    it('should support async register() callbacks', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('utils').register('greet', async (name) => {
                    await new Promise((resolve) => setTimeout(resolve, 10));
                    return `Hello, ${name}!`;
                });
            },
            `
            import * as utils from "host:utils";
            function handler(event) {
                return { greeting: utils.greet(event.name) };
            }
            `
        );

        const result = await loaded.callHandler('handler', { name: 'World' });
        expect(result).toEqual({ greeting: 'Hello, World!' });
    });

    it('should propagate errors from register() callbacks', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('explode', () => {
                    throw new Error('💥 async kaboom from host');
                });
            },
            `
            import * as host from "host:host";
            function handler(event) {
                try {
                    host.explode();
                    return { success: true };
                } catch (err) {
                    return { success: false, error: err.message };
                }
            }
            `
        );

        // The guest catches host errors as HostFunctionError
        let result = await loaded.callHandler('handler', {});

        expect(result.success).toBe(false);
        expect(result.error).toContain('💥 async kaboom from host');
    });

    it('should propagate errors from register() async callbacks', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('explode', async () => {
                    await new Promise((resolve) => setTimeout(resolve, 10));
                    throw new Error('💥 async kaboom from host');
                });
            },
            `
            import * as host from "host:host";
            function handler(event) {
                try {
                    host.explode();
                    return { success: true };
                } catch (err) {
                    return { success: false, error: err.message };
                }
            }
            `
        );

        // The guest catches host errors as HostFunctionError
        let result = await loaded.callHandler('handler', {});

        expect(result.success).toBe(false);
        expect(result.error).toContain('💥 async kaboom from host');
    });
});

// ── Multi-sandbox isolation ──────────────────────────────────────────
//
// These tests verify that two sandboxes with the same host function names
// use independent resolver maps and don't cross-contaminate. This is the
// "Two sandboxes enter, no data leaves" guarantee.

describe('Multi-sandbox isolation', () => {
    /**
     * Build a loaded sandbox with a single host function and handler.
     * Returns { loaded, tag } so callers can identify which sandbox responded.
     */
    async function buildTaggedSandbox(tag, hostFnImpl, handlerScript) {
        const proto = await new SandboxBuilder().build();
        hostFnImpl(proto);
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler('handler', handlerScript);
        const loaded = await sandbox.getLoadedSandbox();
        return { loaded, tag };
    }

    it('should isolate two sandboxes with same-named host functions (different impls)', async () => {
        // Sandbox A: math.compute returns a + b
        const { loaded: loadedA } = await buildTaggedSandbox(
            'A',
            (proto) => proto.hostModule('math').register('compute', (a, b) => a + b),
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.compute(event.a, event.b), source: "A" };
            }
            `
        );

        // Sandbox B: math.compute returns a * b (same module, same fn name!)
        const { loaded: loadedB } = await buildTaggedSandbox(
            'B',
            (proto) => proto.hostModule('math').register('compute', (a, b) => a * b),
            `
            import * as math from "host:math";
            function handler(event) {
                return { result: math.compute(event.a, event.b), source: "B" };
            }
            `
        );

        // Call both — each should hit its own host function
        const resultA = await loadedA.callHandler('handler', { a: 3, b: 7 });
        const resultB = await loadedB.callHandler('handler', { a: 3, b: 7 });

        expect(resultA).toEqual({ result: 10, source: 'A' }); // 3 + 7
        expect(resultB).toEqual({ result: 21, source: 'B' }); // 3 * 7
    });

    it('should isolate per-sandbox state in host function closures', async () => {
        // Each sandbox gets its own counter — closures capture different state
        let counterA = 0;
        let counterB = 0;

        const { loaded: loadedA } = await buildTaggedSandbox(
            'A',
            (proto) =>
                proto.hostModule('stats').register('hit', () => {
                    counterA++;
                    return counterA;
                }),
            `
            import * as stats from "host:stats";
            function handler() { return { count: stats.hit() }; }
            `
        );

        const { loaded: loadedB } = await buildTaggedSandbox(
            'B',
            (proto) =>
                proto.hostModule('stats').register('hit', () => {
                    counterB++;
                    return counterB;
                }),
            `
            import * as stats from "host:stats";
            function handler() { return { count: stats.hit() }; }
            `
        );

        // Interleave calls: A, B, A, B, A
        const a1 = await loadedA.callHandler('handler', {});
        const b1 = await loadedB.callHandler('handler', {});
        const a2 = await loadedA.callHandler('handler', {});
        const b2 = await loadedB.callHandler('handler', {});
        const a3 = await loadedA.callHandler('handler', {});

        // Each counter should be independent
        expect(a1).toEqual({ count: 1 });
        expect(a2).toEqual({ count: 2 });
        expect(a3).toEqual({ count: 3 });
        expect(b1).toEqual({ count: 1 });
        expect(b2).toEqual({ count: 2 });

        // Final state: counterA=3, counterB=2
        expect(counterA).toBe(3);
        expect(counterB).toBe(2);
    });

    it('should isolate async host functions across sandboxes with interleaved calls', async () => {
        const { loaded: loadedA } = await buildTaggedSandbox(
            'A',
            (proto) =>
                proto.hostModule('net').register('fetch', async (url) => {
                    await new Promise((resolve) => setTimeout(resolve, 20));
                    return `A:${url}`;
                }),
            `
            import * as net from "host:net";
            function handler(event) { return { data: net.fetch(event.url) }; }
            `
        );

        const { loaded: loadedB } = await buildTaggedSandbox(
            'B',
            (proto) =>
                proto.hostModule('net').register('fetch', async (url) => {
                    await new Promise((resolve) => setTimeout(resolve, 20));
                    return `B:${url}`;
                }),
            `
            import * as net from "host:net";
            function handler(event) { return { data: net.fetch(event.url) }; }
            `
        );

        // Interleave calls — each sandbox's async host fn must resolve
        // through its own per-sandbox resolver map, not the other's
        const resultA1 = await loadedA.callHandler('handler', { url: '/api' });
        const resultB1 = await loadedB.callHandler('handler', { url: '/api' });
        const resultA2 = await loadedA.callHandler('handler', { url: '/data' });
        const resultB2 = await loadedB.callHandler('handler', { url: '/data' });

        expect(resultA1).toEqual({ data: 'A:/api' });
        expect(resultB1).toEqual({ data: 'B:/api' });
        expect(resultA2).toEqual({ data: 'A:/data' });
        expect(resultB2).toEqual({ data: 'B:/data' });
    });

    it('should isolate multiple host functions per sandbox across two sandboxes', async () => {
        // Both sandboxes register math.add AND math.multiply — but with
        // different implementations. Exercises multiple TSFNs per sandbox
        // sharing the same resolver map, across two independent maps.
        const { loaded: loadedA } = await buildTaggedSandbox(
            'A',
            (proto) => {
                const math = proto.hostModule('math');
                math.register('add', (a, b) => a + b);
                math.register('multiply', (a, b) => a * b);
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { sum: math.add(event.a, event.b), product: math.multiply(event.a, event.b) };
            }
            `
        );

        const { loaded: loadedB } = await buildTaggedSandbox(
            'B',
            (proto) => {
                const math = proto.hostModule('math');
                // Reversed! add does multiply, multiply does add
                math.register('add', (a, b) => a * b);
                math.register('multiply', (a, b) => a + b);
            },
            `
            import * as math from "host:math";
            function handler(event) {
                return { sum: math.add(event.a, event.b), product: math.multiply(event.a, event.b) };
            }
            `
        );

        const resultA = await loadedA.callHandler('handler', { a: 3, b: 7 });
        const resultB = await loadedB.callHandler('handler', { a: 3, b: 7 });

        // A: normal — add=10, multiply=21
        expect(resultA).toEqual({ sum: 10, product: 21 });
        // B: reversed — add=21, multiply=10
        expect(resultB).toEqual({ sum: 21, product: 10 });
    });
});

// ── Binary data (Buffer/Uint8Array) ──────────────────────────────────

describe('Binary data support', () => {
    it('should pass Buffer args from guest Uint8Array to host', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('byte_length', (data) => {
                    expect(Buffer.isBuffer(data)).toBe(true);
                    return data.length;
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                const data = new Uint8Array([72, 101, 108, 108, 111]);
                return { len: host.byte_length(data) };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ len: 5 });
    });

    it('should return Buffer from host as Uint8Array on guest', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('get_bytes', () => {
                    return Buffer.from([1, 2, 3, 4, 5]);
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                const data = host.get_bytes();
                return { len: data.length, first: data[0], last: data[4] };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ len: 5, first: 1, last: 5 });
    });

    it('should handle mixed Buffer and JSON args', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('describe', (prefix, data, num) => {
                    expect(typeof prefix).toBe('string');
                    expect(Buffer.isBuffer(data)).toBe(true);
                    expect(typeof num).toBe('number');
                    return `${prefix}-${data.length}-${num}`;
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                const data = new Uint8Array([10, 20, 30]);
                return { result: host.describe("pfx", data, 42) };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ result: 'pfx-3-42' });
    });

    it('should handle empty Uint8Array', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('check_empty', (data) => {
                    expect(Buffer.isBuffer(data)).toBe(true);
                    return data.length;
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                return { len: host.check_empty(new Uint8Array(0)) };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ len: 0 });
    });

    it('should handle host returning empty Buffer', async () => {
        // Regression: napi_get_buffer_info returns data=null, len=0 for
        // empty buffers. JsReturn::from_napi_value must not panic on the
        // null pointer — it should return an empty Vec instead.
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('empty_response', () => {
                    return Buffer.alloc(0);
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                const data = host.empty_response();
                return { len: data.length, isUint8: data instanceof Uint8Array };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ len: 0, isUint8: true });
    });

    it('should round-trip binary data (send and receive)', async () => {
        const loaded = await buildLoadedSandbox(
            (proto) => {
                proto.hostModule('host').register('echo_bytes', (data) => {
                    // Return the same Buffer back
                    return data;
                });
            },
            `
            import * as host from "host:host";
            function handler() {
                const input = new Uint8Array([0, 127, 128, 255]);
                const output = host.echo_bytes(input);
                // Verify round-trip preserves all byte values
                return {
                    len: output.length,
                    b0: output[0],
                    b1: output[1],
                    b2: output[2],
                    b3: output[3],
                };
            }
            `
        );
        const result = await loaded.callHandler('handler', {});
        expect(result).toEqual({ len: 4, b0: 0, b1: 127, b2: 128, b3: 255 });
    });
});
