// ── Hyperlight JS MCP Server — Integration Tests ────────────────────
//
// Tests the MCP server end-to-end by spawning it as a child process
// and communicating via stdio using NDJSON (newline-delimited JSON),
// which is the framing format used by the MCP stdio transport.
//
// "Trust, but verify." — Reagan (1987), also good advice for sandboxes
//
// ─────────────────────────────────────────────────────────────────────

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

/** Path to server.js — one directory up from tests/ */
const SERVER_PATH = join(__dirname, '..', 'server.js');

/**
 * The protocol version the MCP SDK (v1.26.0) expects.
 * Must match LATEST_PROTOCOL_VERSION in the SDK.
 */
const PROTOCOL_VERSION = '2025-11-25';

// ── NDJSON Framing ──────────────────────────────────────────────────
//
// MCP stdio transport uses newline-delimited JSON (NDJSON):
//   - Send: JSON.stringify(message) + '\n'
//   - Receive: read lines, parse each as JSON
//
// NOT Content-Length / LSP framing.

/**
 * Send a JSON-RPC message to the server via stdin (NDJSON framing).
 *
 * @param {import('node:child_process').ChildProcess} proc
 * @param {object} message — JSON-RPC message object
 */
function send(proc, message) {
    proc.stdin.write(JSON.stringify(message) + '\n');
}

// Shared per-process line reader state: buffer, queued lines, and waiters.
// Uses a WeakMap so the reader survives across multiple waitForResponse calls
// without dropping lines that arrived in the same stdout chunk.
const procLineState = new WeakMap();

function ensureLineReader(proc) {
    let state = procLineState.get(proc);
    if (state) return state;

    state = {
        buffer: '',
        lines: [],
        waiters: [],
    };

    const onData = (chunk) => {
        state.buffer += chunk.toString();
        let idx;
        while ((idx = state.buffer.indexOf('\n')) !== -1) {
            let line = state.buffer.slice(0, idx).replace(/\r$/, '');
            state.buffer = state.buffer.slice(idx + 1);
            if (line.length === 0) {
                continue;
            }

            if (state.waiters.length > 0) {
                const { resolve, reject } = state.waiters.shift();
                try {
                    resolve(JSON.parse(line));
                } catch (_err) {
                    reject(new Error(`Invalid JSON from server: ${line}`));
                }
            } else {
                state.lines.push(line);
            }
        }
    };

    proc.stdout.on('data', onData);
    procLineState.set(proc, state);
    return state;
}

/**
 * Wait for the next JSON-RPC response from the server's stdout.
 * Reads newline-delimited JSON via a persistent per-process line buffer.
 *
 * @param {import('node:child_process').ChildProcess} proc
 * @returns {Promise<object>} — parsed JSON-RPC response
 */
function waitForResponse(proc) {
    return new Promise((resolve, reject) => {
        const state = ensureLineReader(proc);

        if (state.lines.length > 0) {
            const line = state.lines.shift();
            try {
                resolve(JSON.parse(line));
            } catch (_err) {
                reject(new Error(`Invalid JSON from server: ${line}`));
            }
            return;
        }

        state.waiters.push({ resolve, reject });
    });
}

// ── Test Suite ──────────────────────────────────────────────────────

describe('MCP Server', () => {
    let server;
    let messageId = 1;

    /**
     * Call the execute_javascript MCP tool and return the parsed response.
     *
     * @param {string} code — JavaScript code to execute
     * @returns {Promise<object>} — full JSON-RPC response
     */
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

    beforeAll(async () => {
        // Start the MCP server as a child process
        server = spawn('node', [SERVER_PATH], {
            stdio: ['pipe', 'pipe', 'pipe'],
        });

        // Surface server stderr for debugging (vitest captures it)
        server.stderr.on('data', (d) => {
            process.stderr.write(`[mcp-server] ${d}`);
        });

        // MCP handshake — initialize
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'initialize',
            params: {
                protocolVersion: PROTOCOL_VERSION,
                capabilities: {},
                clientInfo: { name: 'vitest-mcp-client', version: '1.0.0' },
            },
        });

        const initResponse = await waitForResponse(server);
        expect(initResponse.result).toBeDefined();
        expect(initResponse.result.serverInfo?.name).toBe('hyperlight-js-sandbox');

        // MCP handshake — send initialized notification (no response expected)
        send(server, {
            jsonrpc: '2.0',
            method: 'notifications/initialized',
        });

        // Let the notification process
        await new Promise((r) => setTimeout(r, 200));
    });

    afterAll(() => {
        if (server) {
            server.kill();
        }
    });

    // ── Tool Discovery ───────────────────────────────────────────

    it('should list execute_javascript tool', async () => {
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'tools/list',
        });
        const response = await waitForResponse(server);

        const tools = response.result?.tools;
        expect(tools).toBeInstanceOf(Array);

        const jsTool = tools.find((t) => t.name === 'execute_javascript');
        expect(jsTool).toBeDefined();
        expect(jsTool.description).toContain('Hyperlight');
        expect(jsTool.inputSchema.properties.code).toBeDefined();
    });

    // ── Successful Execution ─────────────────────────────────────

    it('should execute simple arithmetic', async () => {
        const response = await callExecuteJavaScript('return { result: 2 + 2 };');
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.result).toBe(4);
    });

    it('should compute Fibonacci sequence', async () => {
        const code = [
            'const fib = [0, 1];',
            'for (let i = 2; i < 10; i++) {',
            '    fib.push(fib[i - 1] + fib[i - 2]);',
            '}',
            'return { fibonacci: fib };',
        ].join('\n');

        const response = await callExecuteJavaScript(code);
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.fibonacci).toEqual([0, 1, 1, 2, 3, 5, 8, 13, 21, 34]);
    });

    it('should handle string operations', async () => {
        const code = `
            const msg = 'Hello, Hyperlight!';
            return { upper: msg.toUpperCase(), length: msg.length };
        `;
        const response = await callExecuteJavaScript(code);
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.upper).toBe('HELLO, HYPERLIGHT!');
        expect(parsed.length).toBe(18);
    });

    it('should handle array operations', async () => {
        const code = `
            const nums = [5, 3, 8, 1, 9, 2, 7, 4, 6];
            return {
                sorted: nums.slice().sort((a, b) => a - b),
                sum: nums.reduce((a, b) => a + b, 0),
                max: Math.max(...nums),
            };
        `;
        const response = await callExecuteJavaScript(code);
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.sorted).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
        expect(parsed.sum).toBe(45);
        expect(parsed.max).toBe(9);
    });

    // ── Timeout Enforcement ──────────────────────────────────────

    it('should kill infinite loops with CPU timeout', async () => {
        const response = await callExecuteJavaScript('while (true) {}');

        expect(response.result.isError).toBe(true);
        expect(response.result.content[0].text).toContain('timed out');
    });

    // ── Recovery ─────────────────────────────────────────────────

    it('should recover and execute after a timeout', async () => {
        // Previous test caused a timeout — this verifies recovery
        const response = await callExecuteJavaScript('return { result: 3 * 7 };');
        const parsed = JSON.parse(response.result.content[0].text);
        expect(parsed.result).toBe(21);
    });

    // ── Error Handling ───────────────────────────────────────────

    it('should report syntax errors gracefully', async () => {
        const response = await callExecuteJavaScript('this is not valid javascript ???');
        expect(response.result.isError).toBe(true);
    });

    it('should report runtime errors gracefully', async () => {
        const response = await callExecuteJavaScript('throw new Error("deliberate failure");');
        expect(response.result.isError).toBe(true);
        expect(response.result.content[0].text).toContain('deliberate failure');
    });
});
