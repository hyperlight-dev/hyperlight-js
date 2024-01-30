// Hello World example using the Hyperlight JS Host API

const { SandboxBuilder } = require('../lib.js');

async function main() {
    console.log('=== Hyperlight JS Hello World ===\n');

    // Step 1: Create and configure the sandbox builder
    console.log('1. Creating sandbox builder...');
    const builder = new SandboxBuilder();
    builder.setHeapSize(8 * 1024 * 1024); // 8MB heap
    builder.setStackSize(512 * 1024); // 512KB stack
    console.log('   ✓ Builder configured\n');

    // Step 2: Build the proto sandbox (async — returns a Promise)
    console.log('2. Building proto sandbox...');
    const protoSandbox = await builder.build();
    console.log('   ✓ Proto sandbox created\n');

    // Step 3: Load the JavaScript runtime (async — returns a Promise)
    console.log('3. Loading JavaScript runtime...');
    const jsSandbox = await protoSandbox.loadRuntime();
    console.log('   ✓ Runtime loaded\n');

    // Step 4: Add a simple "hello world" handler function (sync)
    console.log('4. Adding handler function...');
    jsSandbox.addHandler(
        'handler',
        `function handler(event) { 
            event.message = 'Hello, ' + event.name + '! Welcome to Hyperlight JS.';
            return event;
        }`
    );
    console.log('   ✓ Handler added\n');

    // Step 5: Get the loaded sandbox (async — returns a Promise)
    console.log('5. Getting loaded sandbox...');
    const loadedSandbox = await jsSandbox.getLoadedSandbox();
    console.log('   ✓ Sandbox ready\n');

    // Step 6: Call the function (async — returns a Promise)
    console.log('6. Calling guest function...');
    const result = await loadedSandbox.callHandler('handler', { name: 'World' }, { gc: false });
    console.log('   ✓ Function executed\n');

    console.log('Result:', result.message);
    console.log('\n=== Success! ===');
}

main().catch((error) => {
    console.error('\n❌ Error:', error.message);
    console.error('\nStack trace:', error.stack);
    process.exit(1);
});
