#!/usr/bin/env node

// ── Hyperlight JS MCP Server ─────────────────────────────────────────
//
// An MCP (Model Context Protocol) server that allows AI agents to
// execute JavaScript code inside a Hyperlight sandbox with strict
// CPU time bounding.
//
// "In the sandbox, no one can hear you scream." — Alien (1979), adapted
//
// Features:
//   - Isolated execution via Hyperlight (no filesystem, no network)
//   - CPU time limit: configurable (default 1000ms) via HYPERLIGHT_CPU_TIMEOUT_MS
//   - Wall-clock backstop: configurable (default 5000ms) via HYPERLIGHT_WALL_TIMEOUT_MS
//   - Automatic snapshot/restore recovery after timeouts
//   - Sandbox reuse across invocations for performance
//   - Optional timing log (HYPERLIGHT_TIMING_LOG) for performance analysis
//
// Transport: stdio (standard for local MCP integrations)
// Tool:      execute_javascript
//
// Usage:
//   node server.js
//
// ─────────────────────────────────────────────────────────────────────

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { createRequire } from 'node:module';
import { appendFileSync } from 'node:fs';

const require = createRequire(import.meta.url);
const { SandboxBuilder } = require('../../lib.js');

// ── Defaults ─────────────────────────────────────────────────────────

/** Default maximum CPU time per execution (milliseconds). */
const DEFAULT_CPU_TIMEOUT_MS = 1000;

/** Default maximum wall-clock time per execution (milliseconds). */
const DEFAULT_WALL_CLOCK_TIMEOUT_MS = 5000;

/** Default guest heap size in megabytes. */
const DEFAULT_HEAP_SIZE_MB = 16;

/** Default guest stack size in megabytes. */
const DEFAULT_STACK_SIZE_MB = 1;

// ── Configuration ────────────────────────────────────────────────────
//
// All sandbox limits are configurable via environment variables.
// The server operator sets the ceiling — the calling agent cannot
// override them. "The power is yours!" — Captain Planet (1990… close enough)

/**
 * Parse a positive integer from an environment variable, falling back
 * to `defaultVal` when the variable is unset, empty, or not a valid
 * positive integer.
 *
 * @param {string|undefined} raw  — raw env-var value
 * @param {number}           defaultVal — fallback
 * @returns {number}
 */
function parsePositiveInt(raw, defaultVal) {
    if (raw === undefined || raw === '') return defaultVal;
    const n = Number(raw);
    if (!Number.isFinite(n) || n <= 0 || !Number.isInteger(n)) {
        console.error(
            `[hyperlight] Warning: ignoring invalid value "${raw}", using default ${defaultVal}`
        );
        return defaultVal;
    }
    return n;
}

/** Maximum CPU time per execution (milliseconds).
 *  Override with HYPERLIGHT_CPU_TIMEOUT_MS. */
const CPU_TIMEOUT_MS = parsePositiveInt(
    process.env.HYPERLIGHT_CPU_TIMEOUT_MS,
    DEFAULT_CPU_TIMEOUT_MS
);

/** Maximum wall-clock time per execution (milliseconds). Backstop for
 *  edge cases where CPU time alone doesn't catch the issue.
 *  Override with HYPERLIGHT_WALL_TIMEOUT_MS. */
const WALL_CLOCK_TIMEOUT_MS = parsePositiveInt(
    process.env.HYPERLIGHT_WALL_TIMEOUT_MS,
    DEFAULT_WALL_CLOCK_TIMEOUT_MS
);

/** Guest heap size in bytes. Override with HYPERLIGHT_HEAP_SIZE_MB (megabytes). */
const HEAP_SIZE_BYTES =
    parsePositiveInt(process.env.HYPERLIGHT_HEAP_SIZE_MB, DEFAULT_HEAP_SIZE_MB) * 1024 * 1024;

/** Guest stack size in bytes. Override with HYPERLIGHT_STACK_SIZE_MB (megabytes). */
const STACK_SIZE_BYTES =
    parsePositiveInt(process.env.HYPERLIGHT_STACK_SIZE_MB, DEFAULT_STACK_SIZE_MB) * 1024 * 1024;

/**
 * Path to a timing log file. When set (via the HYPERLIGHT_TIMING_LOG
 * environment variable), the server appends one JSON line per tool
 * invocation with a breakdown of sandbox init, setup, compile, and
 * execution times. The demo script reads this to show where time went.
 */
const TIMING_LOG_PATH = process.env.HYPERLIGHT_TIMING_LOG || null;

/**
 * Path to a code log file. When set (via the HYPERLIGHT_CODE_LOG
 * environment variable), the server writes the received JavaScript
 * source code to this file on each tool invocation. The demo script
 * reads it back to show what the model generated.
 */
const CODE_LOG_PATH = process.env.HYPERLIGHT_CODE_LOG || null;

// ── Sandbox Lifecycle ────────────────────────────────────────────────
//
// The sandbox follows a state machine:
//
//   [null] ──build──▶ [ProtoJSSandbox] ──loadRuntime──▶ [JSSandbox]
//                                                          │
//                                              addHandler + getLoadedSandbox
//                                                          │
//                                                          ▼
//                                                  [LoadedJSSandbox]
//                                                     │        │
//                                              callHandler   unload
//                                                     │        │
//                                                     ▼        ▼
//                                               (result)  [JSSandbox]
//
// Between invocations, we keep the sandbox in [JSSandbox] state so we
// can register new handler code for each execution request. After a
// timeout or unrecoverable error, jsSandbox is set to null and
// rebuilt on the next call.

/** @type {import('../../index.d.ts').JSSandbox | null} */
let jsSandbox = null;

/**
 * Build a fresh sandbox from scratch.
 * Called once on first invocation, and again after unrecoverable errors.
 */
async function initializeSandbox() {
    const builder = new SandboxBuilder();
    builder.setHeapSize(HEAP_SIZE_BYTES);
    builder.setStackSize(STACK_SIZE_BYTES);

    const proto = await builder.build();
    jsSandbox = await proto.loadRuntime();

    // Log to stderr — stdout is reserved for MCP protocol messages
    console.error('[hyperlight] Sandbox initialized');
}

/**
 * Execute arbitrary JavaScript code inside the Hyperlight sandbox.
 *
 * The code is wrapped as the body of a `handler(event)` function.
 * Use `return` to produce a JSON-serializable result. The `event`
 * object is an empty `{}` — provided for API consistency but usually
 * not needed.
 *
 * @param {string} code — JavaScript source to execute
 * @returns {Promise<{success: boolean, result?: any, error?: string}>}
 */
async function executeJavaScript(code) {
    /** Timing record — filled in progressively, written at the end. */
    const timing = {
        initMs: 0,
        setupMs: 0,
        compileMs: 0,
        executeMs: 0,
        snapshotMs: 0,
        totalMs: 0,
    };
    const totalStart = performance.now();

    // Lazy initialization — build sandbox on first call
    if (jsSandbox === null) {
        const initStart = performance.now();
        await initializeSandbox();
        timing.initMs = Math.round(performance.now() - initStart);
    }

    // Wrap user code as a handler function body.
    // The user writes code as if inside a function:
    //   let x = 2 + 2;
    //   return { answer: x };
    //
    // We wrap it as:
    //   function handler(event) {
    //     let x = 2 + 2;
    //     return { answer: x };
    //   }
    const wrappedCode = `function handler(event) {\n${code}\n}`;

    // ── Register handler ─────────────────────────────────────────
    const setupStart = performance.now();
    try {
        jsSandbox.clearHandlers();
        jsSandbox.addHandler('execute', wrappedCode);
    } catch (err) {
        // Sandbox in bad state — force reinit on next call
        jsSandbox = null;
        return { success: false, error: `Setup error: ${err.message}` };
    }
    timing.setupMs = Math.round(performance.now() - setupStart);

    // ── Compile & load ───────────────────────────────────────────
    const compileStart = performance.now();
    let loaded;
    try {
        loaded = await jsSandbox.getLoadedSandbox();
    } catch (err) {
        // Compilation failed (syntax error in user code, or sandbox consumed)
        // The JSSandbox may be consumed — reinitialize on next call
        jsSandbox = null;
        return { success: false, error: `Compilation error: ${err.message}` };
    }
    timing.compileMs = Math.round(performance.now() - compileStart);

    // ── Snapshot for timeout recovery ────────────────────────────
    const snapStart = performance.now();
    let snapshot;
    try {
        snapshot = await loaded.snapshot();
    } catch (err) {
        jsSandbox = null;
        return { success: false, error: `Snapshot error: ${err.message}` };
    }
    timing.snapshotMs = Math.round(performance.now() - snapStart);

    // ── Execute with CPU + wall-clock guards ─────────────────────
    const execStart = performance.now();
    try {
        const result = await loaded.callHandler(
            'execute',
            {},
            {
                cpuTimeoutMs: CPU_TIMEOUT_MS,
                wallClockTimeoutMs: WALL_CLOCK_TIMEOUT_MS,
            }
        );
        timing.executeMs = Math.round(performance.now() - execStart);
        timing.totalMs = Math.round(performance.now() - totalStart);
        writeTiming(timing);

        // Success — return to JSSandbox state for the next invocation
        jsSandbox = await loaded.unload();
        return { success: true, result };
    } catch (err) {
        // ── Build a user-friendly error message ──────────────────
        let errorMessage;
        if (err.code === 'ERR_CANCELLED') {
            errorMessage =
                `Execution timed out — CPU limit: ${CPU_TIMEOUT_MS}ms, ` +
                `wall-clock limit: ${WALL_CLOCK_TIMEOUT_MS}ms. ` +
                'Try a less expensive computation or reduce iteration count.';
        } else {
            errorMessage = `Runtime error: ${err.message}`;
        }

        // ── Attempt recovery ─────────────────────────────────────
        try {
            if (loaded.poisoned) {
                await loaded.restore(snapshot);
            }
            jsSandbox = await loaded.unload();
        } catch {
            // Recovery failed — sandbox will be rebuilt on next call
            jsSandbox = null;
        }

        timing.executeMs = Math.round(performance.now() - execStart);
        timing.totalMs = Math.round(performance.now() - totalStart);
        writeTiming(timing);

        return { success: false, error: errorMessage };
    }
}

/**
 * Append a JSON timing record to the timing log file (if configured).
 * The demo script reads these to show model vs. tool time breakdown.
 *
 * @param {Record<string, number>} timing
 */
function writeTiming(timing) {
    if (!TIMING_LOG_PATH) return;
    try {
        appendFileSync(TIMING_LOG_PATH, JSON.stringify(timing) + '\n');
    } catch {
        // Best-effort — don't let logging failures break execution
        console.error('[hyperlight] Warning: failed to write timing log');
    }
}

// ── MCP Server Setup ────────────────────────────────────────────────

const mcpServer = new McpServer({
    name: 'hyperlight-js-sandbox',
    version: '0.1.0',
});

mcpServer.registerTool(
    'execute_javascript',
    {
        title: 'Execute JavaScript in Hyperlight Sandbox',
        description: [
            'Execute JavaScript code inside an isolated Hyperlight micro-VM.',
            '',
            'The code runs as the body of a function — use `return` to produce',
            `a JSON-serializable result. CPU time is hard-limited to ${CPU_TIMEOUT_MS}ms`,
            `with a ${WALL_CLOCK_TIMEOUT_MS}ms wall-clock backstop.`,
            `Memory: ${HEAP_SIZE_BYTES / (1024 * 1024)}MB heap, ${STACK_SIZE_BYTES / (1024 * 1024)}MB stack.`,
            '',
            'The sandbox has NO access to:',
            '  - Filesystem, network, or host environment',
            '  - Node.js APIs (require, process, fs, etc.)',
            '  - Browser APIs (fetch, DOM, setTimeout, etc.)',
            '',
            'The sandbox DOES support:',
            '  - Full ECMAScript (ES2023) — variables, functions, classes, closures',
            '  - Math, String, Array, Object, Map, Set, JSON, RegExp',
            '  - BigInt, Symbol, Proxy, Reflect, Promise (sync resolution)',
            '  - Typed arrays (Uint8Array, Float64Array, etc.)',
            '  - Structured algorithms, data processing, pure computation',
            '',
            'Tips:',
            '  - Use `return { ... }` to send back structured results',
            `  - Keep iteration counts reasonable (${CPU_TIMEOUT_MS}ms CPU limit)`,
            '  - No I/O — all data must be computed, not fetched',
            '  - Date.now() is available for timing within the sandbox',
        ].join('\n'),
        inputSchema: z.object({
            code: z
                .string()
                .describe(
                    'JavaScript code to execute as a function body. ' +
                        'Use `return` to produce output. ' +
                        'Example: `let x = 2 + 2; return { result: x };`'
                ),
        }),
    },
    async ({ code }) => {
        // Log the received code if configured (demo --show-code flag)
        if (CODE_LOG_PATH) {
            try {
                appendFileSync(CODE_LOG_PATH, code);
            } catch {
                console.error('[hyperlight] Warning: failed to write code log');
            }
        }

        const startTime = Date.now();
        const { success, result, error } = await executeJavaScript(code);
        const elapsed = Date.now() - startTime;

        if (success) {
            return {
                content: [
                    {
                        type: 'text',
                        text: JSON.stringify(result, null, 2),
                    },
                ],
            };
        } else {
            return {
                content: [
                    {
                        type: 'text',
                        text: `❌ ${error}\n\n(elapsed: ${elapsed}ms)`,
                    },
                ],
                isError: true,
            };
        }
    }
);

// ── Start Server ────────────────────────────────────────────────────

const transport = new StdioServerTransport();
await mcpServer.connect(transport);

// Log to stderr — stdout is reserved for MCP JSON-RPC messages
console.error('🔒 Hyperlight JS MCP Server running on stdio');
console.error(`   CPU timeout:       ${CPU_TIMEOUT_MS}ms`);
console.error(`   Wall-clock timeout: ${WALL_CLOCK_TIMEOUT_MS}ms`);
console.error(`   Heap size:          ${HEAP_SIZE_BYTES / (1024 * 1024)}MB`);
console.error(`   Stack size:         ${STACK_SIZE_BYTES / (1024 * 1024)}MB`);

// ── Graceful Shutdown ───────────────────────────────────────────────

const shutdown = async () => {
    console.error('[hyperlight] Shutting down...');
    await mcpServer.close();
    process.exit(0);
};

process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
