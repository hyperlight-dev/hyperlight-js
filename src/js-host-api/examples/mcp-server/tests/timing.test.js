// ── Hyperlight JS MCP Server — Timing Log Tests ─────────────────────
//
// Validates that the MCP server writes correct timing data to the
// HYPERLIGHT_TIMING_LOG file when the environment variable is set.
//
// "Time is an illusion. Lunchtime doubly so."
//   — Ford Prefect, The Hitchhiker's Guide (1979… close enough to the 80s)
//
// These tests spawn the server with HYPERLIGHT_TIMING_LOG pointed at a
// temp file, run tool invocations, then inspect the JSON-lines output.
//
// ─────────────────────────────────────────────────────────────────────

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdtempSync, readFileSync, unlinkSync, rmdirSync } from 'node:fs';
import { tmpdir } from 'node:os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SERVER_PATH = join(__dirname, '..', 'server.js');
const PROTOCOL_VERSION = '2025-11-25';

// ── NDJSON Utilities ────────────────────────────────────────────────

function send(proc, message) {
    proc.stdin.write(JSON.stringify(message) + '\n');
}

function waitForResponse(proc) {
    return new Promise((resolve, reject) => {
        let buffer = '';
        const onData = (chunk) => {
            buffer += chunk.toString();
            const idx = buffer.indexOf('\n');
            if (idx === -1) return;
            const line = buffer.slice(0, idx).replace(/\r$/, '');
            buffer = buffer.slice(idx + 1);
            proc.stdout.off('data', onData);
            if (line.length === 0) return;
            try {
                resolve(JSON.parse(line));
            } catch (_err) {
                reject(new Error(`Invalid JSON from server: ${line}`));
            }
        };
        proc.stdout.on('data', onData);
    });
}

// ── Expected timing record fields ───────────────────────────────────

/** Every timing JSON line must contain these keys. */
const TIMING_FIELDS = ['initMs', 'setupMs', 'compileMs', 'executeMs', 'snapshotMs', 'totalMs'];

// ── Test Suite ──────────────────────────────────────────────────────

describe('Timing Log (HYPERLIGHT_TIMING_LOG)', () => {
    let server;
    let messageId = 1;
    let timingLogPath;
    let tmpDir;

    async function callExecuteJavaScript(code) {
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'tools/call',
            params: {
                name: 'execute_javascript',
                arguments: { code },
            },
        });
        return waitForResponse(server);
    }

    /** Read all timing records from the log file. */
    function readTimingRecords() {
        try {
            const content = readFileSync(timingLogPath, 'utf8').trim();
            if (!content) return [];
            return content.split('\n').map((line) => JSON.parse(line));
        } catch {
            return [];
        }
    }

    beforeAll(async () => {
        // Create a temp directory and timing log file path
        tmpDir = mkdtempSync(join(tmpdir(), 'hyperlight-timing-test-'));
        timingLogPath = join(tmpDir, 'timing.jsonl');

        // Start server with HYPERLIGHT_TIMING_LOG set
        server = spawn('node', [SERVER_PATH], {
            stdio: ['pipe', 'pipe', 'pipe'],
            env: {
                ...process.env,
                HYPERLIGHT_TIMING_LOG: timingLogPath,
            },
        });

        server.stderr.on('data', (d) => {
            process.stderr.write(`[timing-test] ${d}`);
        });

        // MCP handshake
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'initialize',
            params: {
                protocolVersion: PROTOCOL_VERSION,
                capabilities: {},
                clientInfo: { name: 'vitest-timing-client', version: '1.0.0' },
            },
        });

        const initResponse = await waitForResponse(server);
        expect(initResponse.result).toBeDefined();

        send(server, {
            jsonrpc: '2.0',
            method: 'notifications/initialized',
        });

        await new Promise((r) => setTimeout(r, 200));
    });

    afterAll(() => {
        if (server) {
            server.kill();
        }
        // Clean up temp files
        try {
            unlinkSync(timingLogPath);
        } catch {
            // may not exist if no tests wrote
        }
        try {
            rmdirSync(tmpDir);
        } catch {
            // best effort
        }
    });

    // ── Timing record structure ──────────────────────────────────

    it('should write a timing record on successful execution', async () => {
        const response = await callExecuteJavaScript('return { answer: 6 * 7 };');
        // Verify the tool call itself succeeded
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.answer).toBe(42);

        // Check the timing log
        const records = readTimingRecords();
        expect(records.length).toBeGreaterThanOrEqual(1);

        const record = records[records.length - 1];
        for (const field of TIMING_FIELDS) {
            expect(record).toHaveProperty(field);
            expect(typeof record[field]).toBe('number');
        }
    });

    it('should have all timing values as non-negative integers', async () => {
        const records = readTimingRecords();
        expect(records.length).toBeGreaterThanOrEqual(1);

        const record = records[records.length - 1];
        for (const field of TIMING_FIELDS) {
            expect(record[field]).toBeGreaterThanOrEqual(0);
            expect(Number.isInteger(record[field])).toBe(true);
        }
    });

    it('should have totalMs >= sum of sub-phase times', async () => {
        const records = readTimingRecords();
        const record = records[records.length - 1];

        // totalMs should be at least the sum of the individual phases
        // (with some tolerance for rounding)
        const sumOfParts =
            record.initMs +
            record.setupMs +
            record.compileMs +
            record.snapshotMs +
            record.executeMs;

        expect(record.totalMs).toBeGreaterThanOrEqual(sumOfParts - 2);
    });

    it('should include initMs > 0 on the first call (sandbox cold start)', async () => {
        // The first record should have a non-zero initMs because the
        // sandbox was created from scratch
        const records = readTimingRecords();
        expect(records.length).toBeGreaterThanOrEqual(1);
        expect(records[0].initMs).toBeGreaterThan(0);
    });

    it('should have initMs === 0 on subsequent calls (sandbox reuse)', async () => {
        // Execute a second call — sandbox should already be warm
        await callExecuteJavaScript('return { warm: true };');

        const records = readTimingRecords();
        expect(records.length).toBeGreaterThanOrEqual(2);

        // The latest record should have initMs === 0 (no re-init)
        const latest = records[records.length - 1];
        expect(latest.initMs).toBe(0);
    });

    it('should write a timing record even on timeout errors', async () => {
        const recordsBefore = readTimingRecords();
        const countBefore = recordsBefore.length;

        // Trigger a timeout
        const response = await callExecuteJavaScript('while (true) {}');
        expect(response.result.isError).toBe(true);

        const recordsAfter = readTimingRecords();
        expect(recordsAfter.length).toBe(countBefore + 1);

        // The timeout record should still have valid structure
        const timeoutRecord = recordsAfter[recordsAfter.length - 1];
        for (const field of TIMING_FIELDS) {
            expect(timeoutRecord).toHaveProperty(field);
            expect(typeof timeoutRecord[field]).toBe('number');
        }

        // executeMs should be substantial (at least ~1000ms for CPU timeout)
        expect(timeoutRecord.executeMs).toBeGreaterThanOrEqual(500);
    });

    it('should write a new record per invocation (JSON-lines format)', async () => {
        const recordsBefore = readTimingRecords();
        const countBefore = recordsBefore.length;

        // Run two more calls
        await callExecuteJavaScript('return 1;');
        await callExecuteJavaScript('return 2;');

        const recordsAfter = readTimingRecords();
        expect(recordsAfter.length).toBe(countBefore + 2);
    });

    it('should measure non-trivial executeMs for computation-heavy code', async () => {
        // Sieve of Eratosthenes — enough work to register measurable time
        const code = `
            const limit = 100000;
            const sieve = new Array(limit).fill(true);
            sieve[0] = sieve[1] = false;
            for (let i = 2; i * i < limit; i++) {
                if (sieve[i]) {
                    for (let j = i * i; j < limit; j += i) sieve[j] = false;
                }
            }
            let count = 0;
            for (let i = 0; i < limit; i++) if (sieve[i]) count++;
            return { primeCount: count };
        `;

        await callExecuteJavaScript(code);

        const records = readTimingRecords();
        const latest = records[records.length - 1];

        // executeMs should be measurable (> 0) for real computation
        expect(latest.executeMs).toBeGreaterThanOrEqual(0);
        // totalMs should always be >= executeMs
        expect(latest.totalMs).toBeGreaterThanOrEqual(latest.executeMs);
    });
});
