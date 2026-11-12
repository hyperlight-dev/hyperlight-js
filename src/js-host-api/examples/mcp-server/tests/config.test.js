// ── Hyperlight JS MCP Server — Configuration Tests ──────────────────
//
// Validates that sandbox limits (CPU timeout, wall-clock timeout, scratch
// budget) are configurable via environment variables, that
// invalid values fall back to defaults gracefully, and that the tool
// description dynamically reflects the configured limits.
//
// "You can tune a piano, but you can't tuna fish."
//   — REO Speedwagon (1978… close enough to the 80s)
//
// Each describe block spawns a separate server process with different
// env var configurations to test the behaviour in isolation.
//
// ─────────────────────────────────────────────────────────────────────

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawnServer } from './helpers.js';

// ── Custom CPU Timeout ──────────────────────────────────────────────

describe('Custom CPU timeout (HYPERLIGHT_CPU_TIMEOUT_MS)', () => {
    let ctx;

    beforeAll(async () => {
        // Set a very short CPU timeout — 100ms. Computations that take
        // ~500ms of CPU should be killed. "Short fuse!" — Rambo (1982)
        ctx = await spawnServer({ HYPERLIGHT_CPU_TIMEOUT_MS: '100' });
    });

    afterAll(() => {
        if (ctx?.server) ctx.server.kill();
    });

    it('should timeout a computation that exceeds the custom limit', async () => {
        // Infinite loop — deterministic timeout trigger, no machine-speed dependency.
        const code = `while (true) {}`;

        const response = await ctx.callTool(code);
        expect(response.result.isError).toBe(true);
        expect(response.result.content[0].text).toContain('timed out');
        // Error message should reflect the custom 100ms limit
        expect(response.result.content[0].text).toContain('100ms');
    });

    it('should still execute fast code successfully', async () => {
        // Simple arithmetic — well under 100ms
        const response = await ctx.callTool('return { answer: 6 * 7 };');
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.answer).toBe(42);
    });
});

// ── Tool Description Reflects Config ────────────────────────────────

describe('Tool description reflects configured limits', () => {
    let ctx;

    beforeAll(async () => {
        ctx = await spawnServer({
            HYPERLIGHT_CPU_TIMEOUT_MS: '2000',
            HYPERLIGHT_WALL_TIMEOUT_MS: '8000',
            HYPERLIGHT_SCRATCH_SIZE_MB: '2',
        });
    });

    afterAll(() => {
        if (ctx?.server) ctx.server.kill();
    });

    it('should include custom CPU timeout in tool description', async () => {
        const response = await ctx.listTools();
        const jsTool = response.result.tools.find((t) => t.name === 'execute_javascript');
        expect(jsTool).toBeDefined();
        expect(jsTool.description).toContain('2000ms');
    });

    it('should include custom wall-clock timeout in tool description', async () => {
        const response = await ctx.listTools();
        const jsTool = response.result.tools.find((t) => t.name === 'execute_javascript');
        expect(jsTool.description).toContain('8000ms');
    });

    it('should include custom scratch size in tool description', async () => {
        const response = await ctx.listTools();
        const jsTool = response.result.tools.find((t) => t.name === 'execute_javascript');
        expect(jsTool.description).toContain('5MiB scratch');
    });
});

// ── Invalid Env Vars Fallback ───────────────────────────────────────

describe('Invalid env vars fall back to defaults', () => {
    let ctx;

    beforeAll(async () => {
        // "Garbage in, defaults out" — every sysadmin ever
        ctx = await spawnServer({
            HYPERLIGHT_CPU_TIMEOUT_MS: 'banana',
            HYPERLIGHT_WALL_TIMEOUT_MS: '-999',
            HYPERLIGHT_SCRATCH_SIZE_MB: '3.14',
        });
    });

    afterAll(() => {
        if (ctx?.server) ctx.server.kill();
    });

    it('should start successfully despite invalid config', async () => {
        // If we got here, the server started and completed the MCP
        // handshake — that's the main assertion.
        const response = await ctx.callTool('return { ok: true };');
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.ok).toBe(true);
    });

    it('should use default CPU timeout (code that runs under 1000ms succeeds)', async () => {
        // This light computation should succeed with the default
        // 1000ms timeout but would fail if the server had somehow
        // parsed 'banana' as 0 or some tiny value.
        const code = `
            const primes = [];
            for (let n = 2; primes.length < 100; n++) {
                let ok = true;
                for (let d = 2; d * d <= n; d++) {
                    if (n % d === 0) { ok = false; break; }
                }
                if (ok) primes.push(n);
            }
            return { count: primes.length };
        `;
        const response = await ctx.callTool(code);
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.count).toBe(100);
    });

    it('should show default values in tool description', async () => {
        const response = await ctx.listTools();
        const jsTool = response.result.tools.find((t) => t.name === 'execute_javascript');
        // Default values should appear since the invalid ones were rejected
        expect(jsTool.description).toContain('1000ms');
        expect(jsTool.description).toContain('5000ms');
        expect(jsTool.description).toContain('16MiB scratch');
    });

    it('should log warnings to stderr about invalid values', () => {
        const stderr = ctx.stderrChunks.join('');
        expect(stderr).toContain('ignoring invalid value "banana"');
        expect(stderr).toContain('ignoring invalid value "-999"');
        expect(stderr).toContain('ignoring invalid value "3.14"');
    });
});

describe('Scratch budget controls guest allocations', () => {
    let constrained;
    let larger;
    const allocation =
        'const bytes = new Uint8Array(8 * 1024 * 1024); bytes.fill(7); return bytes[bytes.length - 1];';

    beforeAll(async () => {
        constrained = await spawnServer({ HYPERLIGHT_SCRATCH_SIZE_MB: '1' });
        larger = await spawnServer({ HYPERLIGHT_SCRATCH_SIZE_MB: '32' });
    });

    afterAll(() => {
        constrained?.server.kill();
        larger?.server.kill();
    });

    it('advertises the effective minimum rather than the smaller request', async () => {
        const response = await constrained.listTools();
        const tool = response.result.tools.find((entry) => entry.name === 'execute_javascript');
        expect(tool.description).toContain('5MiB scratch');
    });

    it('fails when scratch is exhausted and rebuilds for the next request', async () => {
        const failed = await constrained.callTool(allocation);
        expect(failed.result.isError).toBe(true);
        const recovered = await constrained.callTool('return { ok: true };');
        expect(JSON.parse(recovered.result.content[0].text)).toEqual({ ok: true });
    });

    it('runs the same allocation with a larger scratch budget', async () => {
        const response = await larger.callTool(allocation);
        expect(response.result.isError).not.toBe(true);
        expect(JSON.parse(response.result.content[0].text)).toBe(7);
    });
});
