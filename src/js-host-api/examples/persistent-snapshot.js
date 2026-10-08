/*
Copyright 2026  The Hyperlight Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

const { HostFunctionModule, SandboxBuilder, Snapshot } = require('../lib.js');

const command = process.argv[2] ?? 'write';
const path = process.argv[3] ?? '../../target/persistent-node-snapshot';

function hostFunctionModules() {
    const counter = new HostFunctionModule('counter');
    counter.register('step', (value) => value + 1);
    return [counter];
}

async function writeSnapshot() {
    const proto = await new SandboxBuilder().setHostFunctionModules(hostFunctionModules).build();
    const sandbox = await proto.loadRuntime();
    sandbox.addHandler(
        'counter',
        `
        import * as counter from "host:counter";
        let count = 0;
        function handler(event) {
            count = counter.step(count);
            event.count = count;
            return event;
        }
        `
    );
    const loaded = await sandbox.getLoadedSandbox();
    console.log('Before snapshot:', await loaded.callHandler('counter', {}));

    const snapshot = await loaded.snapshot();
    const digest = await snapshot.save(path, 'application-ready');
    console.log(`Saved application-ready as ${digest}`);
    await loaded.dispose();
}

async function readSnapshot() {
    const snapshot = await Snapshot.load(path, 'application-ready');
    const restorer = await new SandboxBuilder()
        .setHostFunctionModules(hostFunctionModules)
        .setHostFunctionFilter('snapshotRequirements')
        .buildFromSnapshot(snapshot);
    const loaded = await restorer.restoreLoadedSandbox();
    console.log('After restore:', await loaded.callHandler('counter', {}));
    await loaded.dispose();
}

if (command === 'write') {
    writeSnapshot().catch(fail);
} else if (command === 'read') {
    readSnapshot().catch(fail);
} else {
    fail(new Error(`Unknown command '${command}'; expected 'write' or 'read'`));
}

function fail(error) {
    console.error(error);
    process.exitCode = 1;
}
