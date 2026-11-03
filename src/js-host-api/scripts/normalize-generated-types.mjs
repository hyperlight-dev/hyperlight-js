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

import { readFile, writeFile } from 'node:fs/promises';
import { URL } from 'node:url';

const declarationsPath = new URL('../index.d.ts', import.meta.url);
const publicTypes = [
    'HostFunctionModule',
    'HostModule',
    'InterruptHandle',
    'JSSandbox',
    'LoadedJSSandbox',
    'ProtoJSSandbox',
    'SandboxBuilder',
    'Snapshot',
];

let declarations = await readFile(declarationsPath, 'utf8');
// Remove every alias first. Renaming `JSSandboxWrapper` would otherwise also
// rewrite `LoadedJSSandboxWrapper` before its own alias is removed.
for (const publicType of publicTypes) {
    declarations = declarations.replace(
        new RegExp(`^export type ${publicType}Wrapper = ${publicType}\\r?\\n`, 'm'),
        ''
    );
}
for (const publicType of publicTypes) {
    declarations = declarations.replace(new RegExp(`\\b${publicType}Wrapper\\b`, 'g'), publicType);
}

// `build()` returns the sandbox type selected by `new SandboxBuilder()` or
// `SandboxBuilder.fromSnapshot()`. napi cannot declare class type parameters.
const builderClass = /^export declare class SandboxBuilder \{/m;
if (!builderClass.test(declarations)) {
    throw new Error('Generated declarations do not declare class SandboxBuilder');
}
declarations = declarations.replace(
    builderClass,
    'export declare class SandboxBuilder<T extends ProtoJSSandbox | JSSandbox | LoadedJSSandbox = ProtoJSSandbox> {'
);
await writeFile(declarationsPath, declarations);
