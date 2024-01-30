// ── Test helpers for structured error code assertions ─────────────────
//
// Vitest's built-in `.toThrow()` only matches on error messages, not on
// the `error.code` property that our lib.js wrapper sets. These helpers
// provide a clean way to assert that a function throws (or a promise
// rejects) with a specific `error.code` value.

import { expect } from 'vitest';

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
