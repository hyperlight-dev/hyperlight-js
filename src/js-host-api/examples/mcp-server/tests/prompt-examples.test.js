// ── README Example Prompts — Validation Test Suite ──────────────────
//
// Exercises every example prompt from the README by generating the
// JavaScript code an AI agent would produce, executing it through
// the MCP server, and asserting the results are correct.
//
// "If you build it, they will come." — Field of Dreams (1989)
// "If you prompt it, it better work." — Us (2026)
//
// ─────────────────────────────────────────────────────────────────────

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SERVER_PATH = join(__dirname, '..', 'server.js');
const PROTOCOL_VERSION = '2025-11-25';

// ── NDJSON Utilities (MCP stdio transport framing) ──────────────────

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

// ── Code Implementations for Each README Prompt ─────────────────────
//
// Each constant is the JavaScript code an AI agent would generate
// in response to the corresponding README prompt. The code runs as
// the body of `function handler(event) { ... }` inside the sandbox.

// ── Mathematics ─────────────────────────────────────────────────────

/** Prompt: "Calculate π to 50 decimal places using the Bailey–Borwein–Plouffe formula" */
const PI_50_DIGITS_CODE = `
// Machin's formula: π/4 = 4·arctan(1/5) - arctan(1/239)
// (BBP naturally produces hex digits; Machin is better for decimal output)
// Using BigInt for arbitrary-precision fixed-point arithmetic.
const DIGITS = 50;
const SCALE = 10n ** BigInt(DIGITS + 10); // extra precision buffer

function arccot(x) {
    const bx = BigInt(x);
    const x2 = bx * bx;
    let power = SCALE / bx; // 1/x at our scale
    let sum = power;
    for (let n = 1; n < 120; n++) {
        power = -power / x2;
        const term = power / BigInt(2 * n + 1);
        if (term === 0n) break;
        sum += term;
    }
    return sum;
}

// π = 4 × (4·arccot(5) - arccot(239))
const pi = 4n * (4n * arccot(5) - arccot(239));
const s = pi.toString();
const formatted = s[0] + '.' + s.slice(1, DIGITS + 1);
return { pi: formatted, digits: DIGITS, method: 'Machin formula with BigInt' };
`;

/** Prompt: "Find all prime numbers below 10,000 using the Sieve of Eratosthenes" */
const SIEVE_CODE = `
const limit = 10000;
const sieve = new Array(limit).fill(true);
sieve[0] = sieve[1] = false;
for (let i = 2; i * i < limit; i++) {
    if (sieve[i]) {
        for (let j = i * i; j < limit; j += i) sieve[j] = false;
    }
}
const primes = [];
for (let i = 0; i < limit; i++) {
    if (sieve[i]) primes.push(i);
}
return { count: primes.length, last10: primes.slice(-10) };
`;

/** Prompt: "Compute the first 100 digits of Euler's number (e) using the Taylor series" */
const EULER_100_DIGITS_CODE = `
// e = Σ 1/n! for n = 0, 1, 2, ...
// Using BigInt fixed-point arithmetic for 100+ digits of precision.
const DIGITS = 100;
const SCALE = 10n ** BigInt(DIGITS + 15); // extra precision for rounding

let sum = 0n;
let factorial = 1n;
for (let n = 0; n < 200; n++) {
    sum += SCALE / factorial;
    factorial *= BigInt(n + 1);
}

const s = sum.toString();
const formatted = s[0] + '.' + s.slice(1, DIGITS + 1);
return { e: formatted, digits: DIGITS, method: 'Taylor series with BigInt' };
`;

/** Prompt: "Run a Monte Carlo simulation with 100,000 random dart throws to estimate π" */
const MONTE_CARLO_CODE = `
let inside = 0;
const N = 100000;
for (let i = 0; i < N; i++) {
    const x = Math.random();
    const y = Math.random();
    if (x * x + y * y <= 1) inside++;
}
const piEstimate = 4 * inside / N;
return {
    pi: piEstimate,
    throws: N,
    inside,
    error: Math.abs(piEstimate - Math.PI),
};
`;

// ── Algorithms & Data Structures ────────────────────────────────────

/** Prompt: "Implement quicksort and mergesort, sort an array of 5,000 random numbers" */
const SORT_COMPARISON_CODE = `
function quicksort(arr) {
    if (arr.length <= 1) return arr;
    const pivot = arr[Math.floor(arr.length / 2)];
    const left = arr.filter(x => x < pivot);
    const mid  = arr.filter(x => x === pivot);
    const right = arr.filter(x => x > pivot);
    return [...quicksort(left), ...mid, ...quicksort(right)];
}

function mergesort(arr) {
    if (arr.length <= 1) return arr;
    const m = Math.floor(arr.length / 2);
    const left = mergesort(arr.slice(0, m));
    const right = mergesort(arr.slice(m));
    const out = [];
    let i = 0, j = 0;
    while (i < left.length && j < right.length) {
        out.push(left[i] <= right[j] ? left[i++] : right[j++]);
    }
    while (i < left.length) out.push(left[i++]);
    while (j < right.length) out.push(right[j++]);
    return out;
}

const N = 5000;
const arr = Array.from({ length: N }, () => Math.floor(Math.random() * 1000000));
const qsorted = quicksort(arr.slice());
const msorted = mergesort(arr.slice());

return {
    size: N,
    quicksortCorrect: qsorted.every((v, i, a) => i === 0 || a[i - 1] <= v),
    mergesortCorrect: msorted.every((v, i, a) => i === 0 || a[i - 1] <= v),
    match: JSON.stringify(qsorted) === JSON.stringify(msorted),
    first5: qsorted.slice(0, 5),
    last5: qsorted.slice(-5),
};
`;

/** Prompt: "Solve the Tower of Hanoi for 15 disks" */
const TOWER_OF_HANOI_CODE = `
const N = 15;
let moveCount = 0;
const firstMoves = [];

function hanoi(n, from, to, via) {
    if (n === 0) return;
    hanoi(n - 1, from, via, to);
    moveCount++;
    if (firstMoves.length < 10) {
        firstMoves.push({ move: moveCount, disk: n, from, to });
    }
    hanoi(n - 1, via, to, from);
}

hanoi(N, 'A', 'C', 'B');
return { disks: N, totalMoves: moveCount, firstMoves };
`;

/** Prompt: "Find the longest common subsequence of 'AGGTAB' and 'GXTXAYB'" */
const LCS_CODE = `
const s1 = 'AGGTAB';
const s2 = 'GXTXAYB';
const m = s1.length, n = s2.length;

// Build DP table
const dp = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
        if (s1[i - 1] === s2[j - 1]) {
            dp[i][j] = dp[i - 1][j - 1] + 1;
        } else {
            dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
        }
    }
}

// Backtrack to recover the subsequence
let lcs = '';
let i = m, j = n;
while (i > 0 && j > 0) {
    if (s1[i - 1] === s2[j - 1]) {
        lcs = s1[i - 1] + lcs;
        i--; j--;
    } else if (dp[i - 1][j] > dp[i][j - 1]) {
        i--;
    } else {
        j--;
    }
}

return { s1, s2, lcs, length: lcs.length };
`;

/** Prompt: "Implement a trie data structure, insert 1000 random 8-letter words" */
const TRIE_CODE = `
class TrieNode {
    constructor() { this.children = {}; this.isEnd = false; }
}

class Trie {
    constructor() { this.root = new TrieNode(); }
    insert(word) {
        let node = this.root;
        for (const ch of word) {
            if (!node.children[ch]) node.children[ch] = new TrieNode();
            node = node.children[ch];
        }
        node.isEnd = true;
    }
    search(word) {
        let node = this.root;
        for (const ch of word) {
            if (!node.children[ch]) return false;
            node = node.children[ch];
        }
        return node.isEnd;
    }
}

function randomWord() {
    const chars = 'abcdefghijklmnopqrstuvwxyz';
    let w = '';
    for (let i = 0; i < 8; i++) w += chars[Math.floor(Math.random() * 26)];
    return w;
}

const trie = new Trie();
const words = [];
for (let i = 0; i < 1000; i++) {
    const w = randomWord();
    words.push(w);
    trie.insert(w);
}

// Search for 100 known words — all should be found
let found = 0;
for (let i = 0; i < 100; i++) {
    if (trie.search(words[i])) found++;
}

// Search for words that almost certainly don't exist
let falsePositives = 0;
for (let i = 0; i < 100; i++) {
    if (trie.search('zz' + randomWord().slice(2))) falsePositives++;
}

return { inserted: words.length, searched: 100, found, falsePositives };
`;

// ── Creative & Visual ───────────────────────────────────────────────

/** Prompt: "Generate a Sierpinski triangle as ASCII art with depth 5" */
const SIERPINSKI_CODE = `
const depth = 5;
const rows = Math.pow(2, depth); // 32 rows

const lines = [];
for (let y = 0; y < rows; y++) {
    let line = ' '.repeat(rows - y - 1); // leading spaces for centering
    for (let x = 0; x <= y; x++) {
        // Pascal's triangle mod 2: (x & y) === x iff C(y,x) is odd
        line += (x & y) === x ? '* ' : '  ';
    }
    lines.push(line.trimEnd());
}

return { art: lines.join('\\n'), rows: lines.length, depth };
`;

/** Prompt: "Create a text-based Mandelbrot set visualization (60×30 ASCII)" */
const MANDELBROT_CODE = `
const WIDTH = 60, HEIGHT = 30, MAX_ITER = 100;
const chars = ' .:-=+*#%@';

const lines = [];
for (let y = 0; y < HEIGHT; y++) {
    let line = '';
    for (let x = 0; x < WIDTH; x++) {
        const cx = (x / WIDTH) * 3.5 - 2.5;
        const cy = (y / HEIGHT) * 2 - 1;
        let zx = 0, zy = 0, iter = 0;
        while (zx * zx + zy * zy < 4 && iter < MAX_ITER) {
            const tmp = zx * zx - zy * zy + cx;
            zy = 2 * zx * zy + cy;
            zx = tmp;
            iter++;
        }
        line += chars[Math.floor(iter / MAX_ITER * (chars.length - 1))];
    }
    lines.push(line);
}

return { art: lines.join('\\n'), width: WIDTH, height: HEIGHT };
`;

/** Prompt: "Generate a maze using recursive backtracking on a 21×21 grid" */
const MAZE_CODE = `
const SIZE = 21;
const WALL = '#', PATH = ' ';
const grid = Array.from({ length: SIZE }, () => new Array(SIZE).fill(WALL));

function carve(x, y) {
    grid[y][x] = PATH;
    const dirs = [[2,0],[0,2],[-2,0],[0,-2]];
    // Fisher-Yates shuffle
    for (let i = dirs.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [dirs[i], dirs[j]] = [dirs[j], dirs[i]];
    }
    for (const [dx, dy] of dirs) {
        const nx = x + dx, ny = y + dy;
        if (nx > 0 && nx < SIZE && ny > 0 && ny < SIZE && grid[ny][nx] === WALL) {
            grid[y + dy / 2][x + dx / 2] = PATH; // carve wall between
            carve(nx, ny);
        }
    }
}

carve(1, 1);
grid[0][1] = PATH;                   // entrance
grid[SIZE - 1][SIZE - 2] = PATH;     // exit

const art = grid.map(r => r.join('')).join('\\n');
return { art, size: SIZE, entrance: [0, 1], exit: [SIZE - 1, SIZE - 2] };
`;

// ── Cryptography & Encoding ─────────────────────────────────────────

/** Prompt: "Implement a Caesar cipher, ROT13 'HELLO WORLD', then decrypt" */
const ROT13_CODE = `
function caesarShift(text, shift) {
    return text.split('').map(ch => {
        const code = ch.charCodeAt(0);
        if (code >= 65 && code <= 90) {
            return String.fromCharCode(((code - 65 + shift) % 26 + 26) % 26 + 65);
        }
        if (code >= 97 && code <= 122) {
            return String.fromCharCode(((code - 97 + shift) % 26 + 26) % 26 + 97);
        }
        return ch;
    }).join('');
}

const original  = 'HELLO WORLD';
const encrypted = caesarShift(original, 13);
const decrypted = caesarShift(encrypted, 13);
return { original, encrypted, decrypted, roundTrip: original === decrypted };
`;

/** Prompt: "Convert the first 20 Fibonacci numbers to different bases" */
const FIBONACCI_BASES_CODE = `
const fibs = [0, 1];
for (let i = 2; i < 20; i++) fibs.push(fibs[i - 1] + fibs[i - 2]);

const table = fibs.map((n, i) => ({
    index: i,
    decimal: n,
    binary: n.toString(2),
    octal: n.toString(8),
    hex: n.toString(16).toUpperCase(),
}));

return { table, count: table.length };
`;

// ── Simulations ─────────────────────────────────────────────────────

/** Prompt: "Simulate Conway's Game of Life on a 30×30 grid for 50 generations" */
const GAME_OF_LIFE_CODE = `
const ROWS = 30, COLS = 30, GENS = 50;
// Deterministic seed pattern (glider + block + blinker) for reproducibility
let grid = Array.from({ length: ROWS }, () => new Array(COLS).fill(0));

// Place a glider at (1,1)
grid[1][2] = 1; grid[2][3] = 1; grid[3][1] = 1; grid[3][2] = 1; grid[3][3] = 1;
// Place a block at (10,10)
grid[10][10] = 1; grid[10][11] = 1; grid[11][10] = 1; grid[11][11] = 1;
// Place a blinker at (20,15)
grid[20][15] = 1; grid[21][15] = 1; grid[22][15] = 1;
// Place an r-pentomino at (15,20) — chaotic evolution
grid[15][21] = 1; grid[15][22] = 1; grid[16][20] = 1; grid[16][21] = 1; grid[17][21] = 1;

function countNeighbors(g, r, c) {
    let count = 0;
    for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
            if (dr === 0 && dc === 0) continue;
            const nr = r + dr, nc = c + dc;
            if (nr >= 0 && nr < ROWS && nc >= 0 && nc < COLS) count += g[nr][nc];
        }
    }
    return count;
}

const popHistory = [];
for (let gen = 0; gen < GENS; gen++) {
    let pop = 0;
    for (let r = 0; r < ROWS; r++) for (let c = 0; c < COLS; c++) pop += grid[r][c];
    popHistory.push(pop);

    const next = Array.from({ length: ROWS }, () => new Array(COLS).fill(0));
    for (let r = 0; r < ROWS; r++) {
        for (let c = 0; c < COLS; c++) {
            const n = countNeighbors(grid, r, c);
            next[r][c] = grid[r][c] === 1 ? (n === 2 || n === 3 ? 1 : 0) : (n === 3 ? 1 : 0);
        }
    }
    grid = next;
}
// Final population
let finalPop = 0;
for (let r = 0; r < ROWS; r++) for (let c = 0; c < COLS; c++) finalPop += grid[r][c];

return { populationPerGen: popHistory, finalPopulation: finalPop, generations: GENS, gridSize: [ROWS, COLS] };
`;

/** Prompt: "Simulate a particle system: 100 particles bouncing in a 100×100 box" */
const PARTICLE_SYSTEM_CODE = `
const N = 100, BOX = 100, STEPS = 1000;
const particles = Array.from({ length: N }, () => ({
    x: Math.random() * BOX,
    y: Math.random() * BOX,
    vx: (Math.random() - 0.5) * 4,
    vy: (Math.random() - 0.5) * 4,
}));

let totalBounces = 0;
for (let step = 0; step < STEPS; step++) {
    for (const p of particles) {
        p.x += p.vx;
        p.y += p.vy;
        if (p.x < 0) { p.x = -p.x; p.vx = -p.vx; totalBounces++; }
        if (p.x > BOX) { p.x = 2 * BOX - p.x; p.vx = -p.vx; totalBounces++; }
        if (p.y < 0) { p.y = -p.y; p.vy = -p.vy; totalBounces++; }
        if (p.y > BOX) { p.y = 2 * BOX - p.y; p.vy = -p.vy; totalBounces++; }
    }
}

const allInBounds = particles.every(p =>
    p.x >= 0 && p.x <= BOX && p.y >= 0 && p.y <= BOX
);
return {
    particles: N, timesteps: STEPS, boxSize: BOX,
    totalBounces, allInBounds,
    samplePositions: particles.slice(0, 5).map(p => ({
        x: Math.round(p.x * 100) / 100,
        y: Math.round(p.y * 100) / 100,
    })),
};
`;

/** Prompt: "Model a predator-prey ecosystem (Lotka-Volterra) with Euler's method" */
const LOTKA_VOLTERRA_CODE = `
// dx/dt = alpha*x - beta*x*y   (prey growth minus predation)
// dy/dt = delta*x*y - gamma*y  (predator growth minus death)
const alpha = 1.1, beta = 0.4, delta = 0.1, gamma = 0.4;
const dt = 0.01;
const STEPS = 1000;

let x = 10; // initial prey
let y = 5;  // initial predators
const prey = [x], pred = [y];

for (let i = 0; i < STEPS; i++) {
    const dx = (alpha * x - beta * x * y) * dt;
    const dy = (delta * x * y - gamma * y) * dt;
    x = Math.max(0, x + dx);
    y = Math.max(0, y + dy);
    prey.push(Math.round(x * 1000) / 1000);
    pred.push(Math.round(y * 1000) / 1000);
}

// Compute min/max manually (avoid spread on 1001-element array)
let preyMin = Infinity, preyMax = -Infinity;
let predMin = Infinity, predMax = -Infinity;
for (const v of prey) { if (v < preyMin) preyMin = v; if (v > preyMax) preyMax = v; }
for (const v of pred) { if (v < predMin) predMin = v; if (v > predMax) predMax = v; }

return {
    model: 'Lotka-Volterra',
    params: { alpha, beta, delta, gamma, dt },
    steps: STEPS,
    finalState: { prey: prey[STEPS], predators: pred[STEPS] },
    preyRange: { min: preyMin, max: preyMax },
    predRange: { min: predMin, max: predMax },
    preySample: prey.filter((_, i) => i % 100 === 0),
    predSample: pred.filter((_, i) => i % 100 === 0),
};
`;

// ── Brain Teasers ───────────────────────────────────────────────────

/** Prompt: "Solve the 8-queens problem and return all 92 unique solutions" */
const EIGHT_QUEENS_CODE = `
const N = 8;
const solutions = [];

function solve(board, row) {
    if (row === N) { solutions.push(board.slice()); return; }
    for (let col = 0; col < N; col++) {
        let safe = true;
        for (let r = 0; r < row; r++) {
            if (board[r] === col ||
                board[r] - r === col - row ||
                board[r] + r === col + row) {
                safe = false;
                break;
            }
        }
        if (safe) { board[row] = col; solve(board, row + 1); }
    }
}

solve(new Array(N), 0);

const firstBoard = solutions[0].map(col =>
    '.'.repeat(col) + 'Q' + '.'.repeat(N - col - 1)
).join('\\n');

return {
    n: N,
    totalSolutions: solutions.length,
    firstSolution: solutions[0],
    firstBoard,
    lastSolution: solutions[solutions.length - 1],
};
`;

/** Prompt: "Generate all valid balanced parentheses for n=8 (Catalan C₈ = 1430)" */
const BALANCED_PARENS_CODE = `
const N = 8;
const combos = [];

function generate(open, close, current) {
    if (current.length === 2 * N) { combos.push(current); return; }
    if (open < N) generate(open + 1, close, current + '(');
    if (close < open) generate(open, close + 1, current + ')');
}

generate(0, 0, '');
return {
    n: N,
    count: combos.length,
    first5: combos.slice(0, 5),
    last5: combos.slice(-5),
};
`;

/** Prompt: "Find all Pythagorean triples where a² + b² = c² and c < 500" */
const PYTHAGOREAN_TRIPLES_CODE = `
const MAX_C = 500;
const triples = [];

for (let a = 1; a < MAX_C; a++) {
    for (let b = a; b < MAX_C; b++) {
        const c2 = a * a + b * b;
        if (c2 >= MAX_C * MAX_C) break;
        const c = Math.round(Math.sqrt(c2));
        if (c * c === c2) triples.push([a, b, c]);
    }
}

const allValid = triples.every(([a, b, c]) => a * a + b * b === c * c);
return {
    maxC: MAX_C,
    count: triples.length,
    allValid,
    first10: triples.slice(0, 10),
    last5: triples.slice(-5),
    smallest: triples[0],
    largest: triples[triples.length - 1],
};
`;

// ── Test Suite ──────────────────────────────────────────────────────

describe('README Example Prompts', () => {
    let server;
    let messageId = 1;

    /**
     * Call the execute_javascript MCP tool and parse the result.
     * Fails the test with a descriptive message if the tool returns an error.
     */
    async function executeAndParse(code) {
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'tools/call',
            params: { name: 'execute_javascript', arguments: { code } },
        });
        const response = await waitForResponse(server);

        // Fail fast with the server's error message if execution failed
        if (response.result?.isError) {
            expect.fail(`Tool returned error: ${response.result.content[0].text}`);
        }

        return JSON.parse(response.result.content[0].text);
    }

    beforeAll(async () => {
        server = spawn('node', [SERVER_PATH], {
            stdio: ['pipe', 'pipe', 'pipe'],
        });
        server.stderr.on('data', (d) => {
            process.stderr.write(`[mcp-server] ${d}`);
        });

        // MCP handshake
        send(server, {
            jsonrpc: '2.0',
            id: messageId++,
            method: 'initialize',
            params: {
                protocolVersion: PROTOCOL_VERSION,
                capabilities: {},
                clientInfo: { name: 'vitest-prompt-client', version: '1.0.0' },
            },
        });
        const init = await waitForResponse(server);
        expect(init.result).toBeDefined();

        send(server, {
            jsonrpc: '2.0',
            method: 'notifications/initialized',
        });
        await new Promise((r) => setTimeout(r, 200));
    });

    afterAll(() => {
        if (server) server.kill();
    });

    // ── Mathematics ──────────────────────────────────────────────

    describe('Mathematics', () => {
        it('π to 50 decimal places (Machin formula)', async () => {
            const result = await executeAndParse(PI_50_DIGITS_CODE);
            expect(result.digits).toBe(50);
            // Verify first 15 known digits of π
            expect(result.pi).toMatch(/^3\.14159265358979/);
            // Verify we got 50 digits after the decimal point
            const afterDot = result.pi.split('.')[1];
            expect(afterDot.length).toBe(50);
        });

        it('Sieve of Eratosthenes — primes below 10,000', async () => {
            const result = await executeAndParse(SIEVE_CODE);
            expect(result.count).toBe(1229);
            expect(result.last10).toEqual([
                9887, 9901, 9907, 9923, 9929, 9931, 9941, 9949, 9967, 9973,
            ]);
        });

        it("Euler's number (e) to 100 digits", async () => {
            const result = await executeAndParse(EULER_100_DIGITS_CODE);
            expect(result.digits).toBe(100);
            // First 15 known digits of e
            expect(result.e).toMatch(/^2\.71828182845904/);
            const afterDot = result.e.split('.')[1];
            expect(afterDot.length).toBe(100);
        });

        it('Monte Carlo estimation of π (100K throws)', async () => {
            const result = await executeAndParse(MONTE_CARLO_CODE);
            expect(result.throws).toBe(100000);
            // π estimate should be within 0.1 of actual π (reasonable for 100K throws)
            expect(result.pi).toBeGreaterThan(3.0);
            expect(result.pi).toBeLessThan(3.3);
            expect(result.error).toBeLessThan(0.1);
        });
    });

    // ── Algorithms & Data Structures ─────────────────────────────

    describe('Algorithms & Data Structures', () => {
        it('Quicksort vs Mergesort (5,000 elements)', async () => {
            const result = await executeAndParse(SORT_COMPARISON_CODE);
            expect(result.size).toBe(5000);
            expect(result.quicksortCorrect).toBe(true);
            expect(result.mergesortCorrect).toBe(true);
            expect(result.match).toBe(true);
            // First element should be smallest
            expect(result.first5[0]).toBeLessThanOrEqual(result.first5[1]);
            // Last element should be largest
            expect(result.last5[3]).toBeLessThanOrEqual(result.last5[4]);
        });

        it('Tower of Hanoi (15 disks)', async () => {
            const result = await executeAndParse(TOWER_OF_HANOI_CODE);
            expect(result.disks).toBe(15);
            // 2^15 - 1 = 32,767 moves
            expect(result.totalMoves).toBe(Math.pow(2, 15) - 1);
            expect(result.firstMoves).toHaveLength(10);
            // First move is always disk 1 (for odd n like 15: A→C)
            expect(result.firstMoves[0].disk).toBe(1);
        });

        it('Longest Common Subsequence', async () => {
            const result = await executeAndParse(LCS_CODE);
            expect(result.s1).toBe('AGGTAB');
            expect(result.s2).toBe('GXTXAYB');
            expect(result.lcs).toBe('GTAB');
            expect(result.length).toBe(4);
        });

        it('Trie data structure (1000 words)', async () => {
            const result = await executeAndParse(TRIE_CODE);
            expect(result.inserted).toBe(1000);
            expect(result.searched).toBe(100);
            // All 100 searched words were inserted, so all should be found
            expect(result.found).toBe(100);
            // Random "zz..." words almost certainly won't exist
            expect(result.falsePositives).toBeLessThan(5);
        });
    });

    // ── Creative & Visual ────────────────────────────────────────

    describe('Creative & Visual', () => {
        it('Sierpinski triangle (depth 5)', async () => {
            const result = await executeAndParse(SIERPINSKI_CODE);
            expect(result.depth).toBe(5);
            expect(result.rows).toBe(32); // 2^5
            // Art should contain asterisks and spaces
            expect(result.art).toContain('*');
            expect(result.art.split('\n')).toHaveLength(32);
        });

        it('Mandelbrot set (60×30 ASCII)', async () => {
            const result = await executeAndParse(MANDELBROT_CODE);
            expect(result.width).toBe(60);
            expect(result.height).toBe(30);
            const lines = result.art.split('\n');
            expect(lines).toHaveLength(30);
            // Each line should be 60 characters wide
            lines.forEach((line) => expect(line.length).toBe(60));
        });

        it('Maze generation (21×21)', async () => {
            const result = await executeAndParse(MAZE_CODE);
            expect(result.size).toBe(21);
            expect(result.entrance).toEqual([0, 1]);
            expect(result.exit).toEqual([20, 19]);
            const lines = result.art.split('\n');
            expect(lines).toHaveLength(21);
            // Each line should be 21 characters
            lines.forEach((line) => expect(line.length).toBe(21));
            // Should contain both walls and paths
            expect(result.art).toContain('#');
            expect(result.art).toContain(' ');
        });
    });

    // ── Cryptography & Encoding ──────────────────────────────────

    describe('Cryptography & Encoding', () => {
        it('Caesar cipher / ROT13', async () => {
            const result = await executeAndParse(ROT13_CODE);
            expect(result.original).toBe('HELLO WORLD');
            expect(result.encrypted).toBe('URYYB JBEYQ');
            expect(result.decrypted).toBe('HELLO WORLD');
            expect(result.roundTrip).toBe(true);
        });

        it('Fibonacci base conversion', async () => {
            const result = await executeAndParse(FIBONACCI_BASES_CODE);
            expect(result.count).toBe(20);
            // Verify first few Fibonacci numbers
            expect(result.table[0].decimal).toBe(0);
            expect(result.table[1].decimal).toBe(1);
            expect(result.table[2].decimal).toBe(1);
            expect(result.table[6].decimal).toBe(8);
            expect(result.table[6].binary).toBe('1000');
            expect(result.table[6].octal).toBe('10');
            expect(result.table[6].hex).toBe('8');
            // Verify 19th Fibonacci (F₁₉ = 4181)
            expect(result.table[19].decimal).toBe(4181);
            expect(result.table[19].hex).toBe('1055');
        });
    });

    // ── Simulations ──────────────────────────────────────────────

    describe('Simulations', () => {
        it("Conway's Game of Life (30×30, 50 gens)", async () => {
            const result = await executeAndParse(GAME_OF_LIFE_CODE);
            expect(result.generations).toBe(50);
            expect(result.gridSize).toEqual([30, 30]);
            // Should have 50 population entries (one per generation)
            expect(result.populationPerGen).toHaveLength(50);
            // All population values should be non-negative integers
            result.populationPerGen.forEach((pop) => {
                expect(pop).toBeGreaterThanOrEqual(0);
                expect(Number.isInteger(pop)).toBe(true);
            });
            // Initial population: glider(5) + block(4) + blinker(3) + r-pentomino(5) = 17
            expect(result.populationPerGen[0]).toBe(17);
        });

        it('Particle system (100 particles, 1000 steps)', async () => {
            const result = await executeAndParse(PARTICLE_SYSTEM_CODE);
            expect(result.particles).toBe(100);
            expect(result.timesteps).toBe(1000);
            expect(result.boxSize).toBe(100);
            expect(result.allInBounds).toBe(true);
            expect(result.totalBounces).toBeGreaterThan(0);
            expect(result.samplePositions).toHaveLength(5);
        });

        it('Lotka-Volterra predator-prey', async () => {
            const result = await executeAndParse(LOTKA_VOLTERRA_CODE);
            expect(result.model).toBe('Lotka-Volterra');
            expect(result.steps).toBe(1000);
            // Both populations should remain positive
            expect(result.finalState.prey).toBeGreaterThan(0);
            expect(result.finalState.predators).toBeGreaterThan(0);
            // Should show oscillatory behaviour — range should be non-trivial
            expect(result.preyRange.max).toBeGreaterThan(result.preyRange.min);
            expect(result.predRange.max).toBeGreaterThan(result.predRange.min);
            // Samples should have 11 entries (every 100 steps from 0..1000)
            expect(result.preySample).toHaveLength(11);
        });
    });

    // ── Brain Teasers ────────────────────────────────────────────

    describe('Brain Teasers', () => {
        it('8-queens (all 92 solutions)', async () => {
            const result = await executeAndParse(EIGHT_QUEENS_CODE);
            expect(result.n).toBe(8);
            // The 8-queens problem has exactly 92 distinct solutions
            expect(result.totalSolutions).toBe(92);
            // First solution should be a valid placement
            expect(result.firstSolution).toHaveLength(8);
            // Board visualization should have 8 lines with Q characters
            expect(result.firstBoard.split('\n')).toHaveLength(8);
            expect(result.firstBoard).toContain('Q');
        });

        it('Balanced parentheses (n=8, Catalan C₈=1430)', async () => {
            const result = await executeAndParse(BALANCED_PARENS_CODE);
            expect(result.n).toBe(8);
            // Catalan number C₈ = 1430
            expect(result.count).toBe(1430);
            // First combination should be all opens then all closes
            expect(result.first5[0]).toBe('(((((((())))))))');
            // Last combination should be alternating
            expect(result.last5[4]).toBe('()()()()()()()()');
        });

        it('Pythagorean triples (c < 500)', async () => {
            const result = await executeAndParse(PYTHAGOREAN_TRIPLES_CODE);
            expect(result.maxC).toBe(500);
            expect(result.allValid).toBe(true);
            expect(result.count).toBeGreaterThan(0);
            // The smallest triple is (3, 4, 5)
            expect(result.smallest).toEqual([3, 4, 5]);
            // All triples should have c < 500
            expect(result.largest[2]).toBeLessThan(500);
            // Verify a well-known triple is present
            expect(result.first10).toContainEqual([3, 4, 5]);
            expect(result.first10).toContainEqual([5, 12, 13]);
        });
    });
});
