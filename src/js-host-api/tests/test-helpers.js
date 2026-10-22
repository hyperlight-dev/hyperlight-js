// ── Test helpers for sandbox cleanup and error code assertions ────────
//
// Vitest's built-in `.toThrow()` only matches on error messages, not on
// the `error.code` property that our lib.js wrapper sets. These helpers
// provide a clean way to assert that a function throws (or a promise
// rejects) with a specific `error.code` value.

import { afterEach, expect } from 'vitest';

const sandboxes = [];

// V8 garbage collection need not run between tests. Dispose every runtime/loaded
// wrapper explicitly so live VMs do not accumulate past WHP's ARM64 limit of 64.
// Track each ownership transition, including unload(); consumed wrappers have
// idempotent dispose(), so cleanup is safe even after a test consumes one.
export async function trackSandbox(promise) {
    const sandbox = await promise;
    sandboxes.push(sandbox);
    return sandbox;
}

afterEach(async () => {
    const results = await Promise.allSettled(
        sandboxes.splice(0).map(async (sandbox) => sandbox.dispose())
    );
    const errors = results.filter((result) => result.status === 'rejected');
    if (errors.length) {
        throw new AggregateError(
            errors.map((result) => result.reason),
            'Failed to dispose test sandboxes'
        );
    }
});

/**
 * Assert that `fn` throws synchronously with the given `error.code`.
 *
 * @param {Function} fn     — the function expected to throw
 * @param {string}   code   — the expected `error.code` (e.g. 'ERR_INVALID_ARG')
 */
export function expectThrowsWithCode(fn, code) {
    let caught;
    try {
        fn();
    } catch (e) {
        caught = e;
    }
    expect(caught, `Expected function to throw with code ${code}`).toBeDefined();
    expect(caught).toBeInstanceOf(Error);
    expect(caught.code).toBe(code);
}

/**
 * Assert that a promise rejects with the given `error.code`.
 *
 * @param {Promise}  promise — the promise expected to reject
 * @param {string}   code    — the expected `error.code` (e.g. 'ERR_CANCELLED')
 */
export async function expectRejectsWithCode(promise, code) {
    let caught;
    try {
        await promise;
    } catch (e) {
        caught = e;
    }
    expect(caught, `Expected promise to reject with code ${code}`).toBeDefined();
    expect(caught).toBeInstanceOf(Error);
    expect(caught.code).toBe(code);
}
