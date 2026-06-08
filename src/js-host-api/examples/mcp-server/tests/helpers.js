// ── Shared Test Utilities — NDJSON Transport Helpers ─────────────────
//
// Provides a persistent NDJSON line reader for MCP stdio transport tests.
// Each test file imports from here instead of duplicating the reader logic.
//
// The reader maintains a per-process buffer via WeakMap, correctly handles
// multiple messages arriving in a single stdout chunk, and queues complete
// lines for consumption by waitForResponse callers.
//
// ─────────────────────────────────────────────────────────────────────

import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { expect } from 'vitest';

const __dirname = dirname(fileURLToPath(import.meta.url));

/** Path to server.js — one directory up from tests/ */
export const SERVER_PATH = join(__dirname, '..', 'server.js');

/**
 * The protocol version the MCP SDK (v1.26.0) expects.
 * Must match LATEST_PROTOCOL_VERSION in the SDK.
 */
export const PROTOCOL_VERSION = '2025-11-25';

// ── NDJSON Framing ──────────────────────────────────────────────────
//
// MCP stdio transport uses newline-delimited JSON (NDJSON):
//   - Send: JSON.stringify(message) + '\n'
//   - Receive: read lines, parse each as JSON

/**
 * Send a JSON-RPC message to the server via stdin (NDJSON framing).
 *
 * @param {import('node:child_process').ChildProcess} proc
 * @param {object} message — JSON-RPC message object
 */
export function send(proc, message) {
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
export function waitForResponse(proc) {
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

/**
 * Spawn a server with the given env overrides, perform the MCP
 * handshake, and return a context object with helper methods.
 *
 * @param {Record<string, string>} envOverrides
 * @param {object} [options]
 * @param {string} [options.clientName] — client name for the handshake
 * @param {string} [options.stderrPrefix] — prefix for stderr debug output
 * @returns {Promise<{server: import('node:child_process').ChildProcess, messageId: {value: number}, callTool: (code: string) => Promise<object>, listTools: () => Promise<object>, stderrChunks: string[]}>}
 */
export async function spawnServer(envOverrides = {}, options = {}) {
    const { clientName = 'vitest-client', stderrPrefix = '[mcp-server]' } = options;
    const messageId = { value: 1 };
    const server = spawn('node', [SERVER_PATH], {
        stdio: ['pipe', 'pipe', 'pipe'],
        env: { ...process.env, ...envOverrides },
    });

    const stderrChunks = [];
    server.stderr.on('data', (d) => {
        stderrChunks.push(d.toString());
        process.stderr.write(`${stderrPrefix} ${d}`);
    });

    // MCP handshake — initialize
    send(server, {
        jsonrpc: '2.0',
        id: messageId.value++,
        method: 'initialize',
        params: {
            protocolVersion: PROTOCOL_VERSION,
            capabilities: {},
            clientInfo: { name: clientName, version: '1.0.0' },
        },
    });

    const initResponse = await waitForResponse(server);
    expect(initResponse.result).toBeDefined();

    // MCP handshake — initialized notification
    send(server, {
        jsonrpc: '2.0',
        method: 'notifications/initialized',
    });
    await new Promise((r) => setTimeout(r, 200));

    /** Call execute_javascript and return the full response. */
    const callTool = async (code) => {
        send(server, {
            jsonrpc: '2.0',
            id: messageId.value++,
            method: 'tools/call',
            params: {
                name: 'execute_javascript',
                arguments: { code },
            },
        });
        return waitForResponse(server);
    };

    /** Call tools/list and return the full response. */
    const listTools = async () => {
        send(server, {
            jsonrpc: '2.0',
            id: messageId.value++,
            method: 'tools/list',
        });
        return waitForResponse(server);
    };

    return { server, messageId, callTool, listTools, stderrChunks };
}
