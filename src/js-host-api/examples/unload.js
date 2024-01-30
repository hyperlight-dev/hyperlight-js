// Example showing how to unload and reload handlers

const { SandboxBuilder } = require('../lib.js');

async function main() {
    console.log('=== Hyperlight JS Unload/Reload Example ===\n');

    // Create sandbox
    const builder = new SandboxBuilder();
    builder.setHeapSize(8 * 1024 * 1024);

    const protoSandbox = await builder.build();
    let jsSandbox = await protoSandbox.loadRuntime();

    console.log('Adding initial handler...');
    jsSandbox.addHandler(
        'handler',
        `function handler(event) {
            return { message: 'Hello, ' + event.name + '!' };
        }`
    );

    let loadedSandbox = await jsSandbox.getLoadedSandbox();

    console.log('Calling initial handler...');
    let result = await loadedSandbox.callHandler('handler', { name: 'Alice' }, { gc: false });
    console.log('Result:', result);

    // Unload the handlers (async — returns a Promise<JSSandbox>)
    console.log('\nUnloading handlers...');
    jsSandbox = await loadedSandbox.unload();
    console.log('Handlers unloaded successfully!');

    // Add a different handler
    console.log('\nAdding new handler...');
    jsSandbox.addHandler(
        'handler',
        `function handler(event) {
            return { message: 'Goodbye, ' + event.name + '! See you later.' };
        }`
    );

    loadedSandbox = await jsSandbox.getLoadedSandbox();

    console.log('Calling new handler...');
    result = await loadedSandbox.callHandler('handler', { name: 'Bob' }, { gc: false });
    console.log('Result:', result);

    console.log('\n=== Success! ===');
    console.log('We successfully unloaded the first handler and loaded a new one!');
}

main().catch((error) => {
    console.error('\n❌ Error:', error.message);
    console.error('\nStack trace:', error.stack);
    process.exit(1);
});
