import { afterAll, describe, expect, it } from 'vitest';
import { trackSandbox } from './test-helpers.js';

describe('test sandbox cleanup', () => {
    const disposed = [];
    let asyncDisposed = false;
    const disposeLoaded = async () => {
        await new Promise((resolve) => setTimeout(resolve, 10));
        asyncDisposed = true;
    };

    afterAll(() => {
        expect(disposed).toEqual(['runtime', 'consumed', 'failed']);
        expect(asyncDisposed).toBe(true);
    });

    it('returns the original runtime wrapper', async () => {
        const sandbox = { dispose: () => disposed.push('runtime') };
        expect(await trackSandbox(Promise.resolve(sandbox))).toBe(sandbox);
    });

    it('can clean up an already-consumed wrapper', async () => {
        await trackSandbox(Promise.resolve({ dispose: () => disposed.push('consumed') }));
    });

    it('preserves failures during sandbox creation', async () => {
        const error = new Error('sandbox creation failed');
        await expect(trackSandbox(Promise.reject(error))).rejects.toBe(error);
    });

    it.fails('cleans up even when a test fails', async () => {
        await trackSandbox(Promise.resolve({ dispose: () => disposed.push('failed') }));
        throw new Error('deliberate test failure');
    });

    it('awaits asynchronous disposal', async () => {
        await trackSandbox(Promise.resolve({ dispose: disposeLoaded }));
    });
});
