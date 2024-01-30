// Advanced example showing object-based event processing

const { SandboxBuilder } = require('../lib.js');

async function main() {
    console.log('=== Hyperlight JS Advanced Example ===\n');

    // Create and configure sandbox
    const builder = new SandboxBuilder();
    builder.setHeapSize(16 * 1024 * 1024); // 16MB heap

    const protoSandbox = await builder.build();
    const jsSandbox = await protoSandbox.loadRuntime();

    // Add a calculator handler
    console.log('Adding calculator handler...\n');

    jsSandbox.addHandler(
        'handler',
        `function handler(event) {
            const a = event.a;
            const b = event.b;
            const op = event.operation;
            
            let result;
            switch(op) {
                case 'add': result = a + b; break;
                case 'subtract': result = a - b; break;
                case 'multiply': result = a * b; break;
                case 'divide': result = b !== 0 ? a / b : 'Error: Division by zero'; break;
                default: result = 'Error: Unknown operation';
            }
            
            event.result = result;
            return event;
        }`
    );

    const loadedSandbox = await jsSandbox.getLoadedSandbox();

    // Test the calculator — callHandler is async, so we await each call
    console.log('Testing calculator operations:');
    const calcTests = [
        { a: 10, b: 5, operation: 'add' },
        { a: 20, b: 4, operation: 'multiply' },
        { a: 100, b: 25, operation: 'divide' },
        { a: 50, b: 30, operation: 'subtract' },
    ];

    for (const test of calcTests) {
        const result = await loadedSandbox.callHandler('handler', test, { gc: false });
        console.log(`  ${test.a} ${test.operation} ${test.b} = ${result.result}`);
    }

    console.log('\n=== All tests passed! ===');
}

main().catch((error) => {
    console.error('\n❌ Error:', error.message);
    console.error('\nStack trace:', error.stack);
    process.exit(1);
});
