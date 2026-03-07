// User Modules example — register reusable ES modules that handlers can import
//
// Demonstrates:
// - Registering user modules with `addModule()`
// - Handler importing a user module via `import { ... } from 'user:math'`
// - Inter-module dependencies (geometry imports constants)
// - Custom namespaces
// - Multiple handlers sharing a module
// - **Cross-handler mutable state sharing** via a shared module

const { SandboxBuilder } = require('../lib.js');

async function main() {
    console.log('=== Hyperlight JS User Modules ===\n');

    // ── Build sandbox ────────────────────────────────────────────────
    console.log('1. Creating sandbox...');
    const proto = await new SandboxBuilder()
        .setHeapSize(8 * 1024 * 1024)
        .setScratchSize(1024 * 1024)
        .build();
    const sandbox = await proto.loadRuntime();
    console.log('   ✓ Sandbox created\n');

    // ── Register user modules ────────────────────────────────────────
    // Modules use ES module syntax (export). They are compiled lazily
    // when first imported, so registration order doesn't matter.
    console.log('2. Registering user modules...');

    // A constants module — other modules can import from it
    sandbox.addModule(
        'constants',
        `
        export const PI = 3.14159;
        export const E = 2.71828;
    `
    );

    // A geometry module that depends on the constants module
    sandbox.addModule(
        'geometry',
        `
        import { PI } from 'user:constants';
        export function circleArea(radius) { return PI * radius * radius; }
        export function circleCircumference(radius) { return 2 * PI * radius; }
    `
    );

    // A string utils module with a custom namespace
    sandbox.addModule(
        'strings',
        `
        export function capitalize(s) {
            return s.charAt(0).toUpperCase() + s.slice(1);
        }
        export function reverse(s) {
            return s.split('').reverse().join('');
        }
    `,
        'mylib'
    ); // custom namespace → import from 'mylib:strings'

    console.log('   ✓ 3 modules registered (2 default namespace, 1 custom)\n');

    // ── Add handlers that import the modules ─────────────────────────
    console.log('3. Adding handlers...');

    // Handler 1: uses geometry module (which transitively imports constants)
    sandbox.addHandler(
        'circle',
        `
        import { circleArea, circleCircumference } from 'user:geometry';

        function handler(event) {
            return {
                radius: event.radius,
                area: circleArea(event.radius),
                circumference: circleCircumference(event.radius),
            };
        }
    `
    );

    // Handler 2: uses the custom-namespace strings module
    sandbox.addHandler(
        'strings',
        `
        import { capitalize, reverse } from 'mylib:strings';

        function handler(event) {
            return {
                original: event.text,
                capitalized: capitalize(event.text),
                reversed: reverse(event.text),
            };
        }
    `
    );

    const loaded = await sandbox.getLoadedSandbox();
    console.log('   ✓ Handlers loaded\n');

    // ── Call handlers ────────────────────────────────────────────────
    console.log('4. Calling handlers...\n');

    // Circle handler
    const circleResult = await loaded.callHandler('circle', { radius: 5 });
    console.log('   Circle (radius=5):');
    console.log(`     Area:          ${circleResult.area.toFixed(4)}`);
    console.log(`     Circumference: ${circleResult.circumference.toFixed(4)}`);

    // Strings handler
    const strResult = await loaded.callHandler('strings', { text: 'hyperlight' });
    console.log(`\n   Strings ("hyperlight"):`);
    console.log(`     Capitalized: ${strResult.capitalized}`);
    console.log(`     Reversed:    ${strResult.reversed}`);

    console.log('\n── Part 1 complete (pure-function modules) ──\n');

    // We need a fresh sandbox for the shared-state demo because
    // getLoadedSandbox() consumes the JSSandbox.
    console.log('5. Cross-handler shared mutable state...\n');
    const proto2 = await new SandboxBuilder()
        .setHeapSize(8 * 1024 * 1024)
        .setScratchSize(1024 * 1024)
        .build();
    const sandbox2 = await proto2.loadRuntime();

    // ── Shared mutable state module ──────────────────────────────────
    // A counter module with module-level mutable state. ESM singleton
    // semantics guarantee all importing handlers see the SAME instance.
    sandbox2.addModule(
        'counter',
        `
        let count = 0;
        export function increment() { return ++count; }
        export function getCount() { return count; }
    `
    );

    // Handler A: mutates state by calling increment()
    sandbox2.addHandler(
        'writer',
        `
        import { increment } from 'user:counter';
        function handler(event) {
            event.count = increment();
            return event;
        }
    `
    );

    // Handler B: reads state WITHOUT mutating it
    sandbox2.addHandler(
        'reader',
        `
        import { getCount } from 'user:counter';
        function handler(event) {
            event.count = getCount();
            return event;
        }
    `
    );

    const loaded2 = await sandbox2.getLoadedSandbox();

    // Writer increments → count=1
    const w1 = await loaded2.callHandler('writer', {});
    console.log(`   Writer call 1 → count=${w1.count}`);

    // Reader sees the mutation made by writer → count=1
    const r1 = await loaded2.callHandler('reader', {});
    console.log(`   Reader sees   → count=${r1.count}  (should be 1)`);

    // Writer increments again → count=2
    const w2 = await loaded2.callHandler('writer', {});
    console.log(`   Writer call 2 → count=${w2.count}`);

    // Reader sees the updated state → count=2
    const r2 = await loaded2.callHandler('reader', {});
    console.log(`   Reader sees   → count=${r2.count}  (should be 2)`);

    console.log(
        '\n✅ User modules example complete! — "Life moves pretty fast. If you don\'t stop and share state once in a while, you could miss it."'
    );
}

main().catch((err) => {
    console.error('Error:', err);
    process.exit(1);
});
