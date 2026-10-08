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

import { afterEach, describe, expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { URL } from 'node:url';

import { HostFunctionModule, SandboxBuilder, Snapshot } from '../lib.js';

const directories = [];

async function snapshotDirectory() {
    const directory = await mkdtemp(join(tmpdir(), 'hyperlight-js-snapshot-'));
    directories.push(directory);
    return directory;
}

function stateHostFunctionModules() {
    const state = new HostFunctionModule('state');
    state.register('next', () => 1);
    return [state];
}

afterEach(async () => {
    await Promise.all(
        directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true }))
    );
});

describe('Persistent snapshots', () => {
    it('exports the restoration API through ESM', async () => {
        const esm = await import('../lib.mjs');

        expect(esm.HostFunctionModule).toBe(HostFunctionModule);
        expect(Object.keys(esm).some((name) => name.endsWith('Wrapper'))).toBe(false);
        expect(typeof esm.SandboxRestorer).toBe('function');
        expect(esm.SandboxStatus).toBeDefined();
        expect(esm.SnapshotRequirementStatus).toBeDefined();
    });

    it('generates self-contained TypeScript declarations', async () => {
        const declarations = await readFile(new URL('../index.d.ts', import.meta.url), 'utf8');

        expect(declarations).not.toContain('JsHostFunction');
        expect(declarations).not.toMatch(/\b\w+Wrapper\b/);
        expect(declarations).toContain(
            "setHostFunctionFilter(filter: 'all' | 'snapshotRequirements'): this"
        );
    });

    it('enriches reusable host-function configuration errors', async () => {
        expect(() => new HostFunctionModule('')).toThrow(
            expect.objectContaining({ code: 'ERR_INVALID_ARG' })
        );

        const builder = new SandboxBuilder();
        expect(() => builder.setHostFunctionFilter('invalid')).toThrow(
            expect.objectContaining({ code: 'ERR_INVALID_ARG' })
        );
        const proto = await builder.build();
        expect(() => builder.setHostFunctionModules(() => [])).toThrow(
            expect.objectContaining({ code: 'ERR_CONSUMED' })
        );
        const sandbox = await proto.loadRuntime();
        sandbox.dispose();
    });

    it('prevents sharing a host-function module across sandbox builders', () => {
        const [state] = stateHostFunctionModules();
        const modules = () => [state];

        new SandboxBuilder().setHostFunctionModules(modules);

        expect(() => new SandboxBuilder().setHostFunctionModules(modules)).toThrow(
            expect.objectContaining({ code: 'ERR_CONSUMED' })
        );
        expect(() => state.register('another', () => 2)).toThrow(
            expect.objectContaining({ code: 'ERR_CONSUMED' })
        );
    });

    it('does not consume modules when a host-function batch is rejected', () => {
        const available = new HostFunctionModule('available');
        available.register('value', () => 1);
        const consumed = new HostFunctionModule('consumed');
        consumed.register('value', () => 2);
        new SandboxBuilder().setHostFunctionModules(() => [consumed]);

        const builder = new SandboxBuilder();
        expect(() => builder.setHostFunctionModules(() => [available, consumed])).toThrow(
            expect.objectContaining({ code: 'ERR_CONSUMED' })
        );

        available.register('stillAvailable', () => true);
        expect(builder.setHeapSize(8 * 1024 * 1024)).toBe(builder);
    });

    it('rejects duplicate module objects without consuming them', () => {
        const duplicate = new HostFunctionModule('duplicate');
        duplicate.register('value', () => 1);

        expect(() =>
            new SandboxBuilder().setHostFunctionModules(() => [duplicate, duplicate])
        ).toThrow(expect.objectContaining({ code: 'ERR_INVALID_ARG' }));

        duplicate.register('stillAvailable', () => true);
    });

    it('does not consume modules when the sandbox builder is consumed', async () => {
        const available = new HostFunctionModule('available');
        available.register('value', () => 1);
        const builder = new SandboxBuilder();
        const proto = await builder.build();

        expect(() => builder.setHostFunctionModules(() => [available])).toThrow(
            expect.objectContaining({ code: 'ERR_CONSUMED' })
        );
        available.register('stillAvailable', () => true);

        const sandbox = await proto.loadRuntime();
        sandbox.dispose();
    });

    it('enriches static snapshot load errors', async () => {
        await expect(Snapshot.load('missing-snapshot-layout', 'missing')).rejects.toMatchObject({
            code: 'ERR_INTERNAL',
        });
    });

    it('persists and restores a JSSandbox with handlers not yet registered in the guest', async () => {
        const directory = await snapshotDirectory();
        const proto = await new SandboxBuilder().build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'counter',
            `
            let count = 0;
            function handler(event) {
                event.count = ++count;
                return event;
            }
            `
        );

        const snapshot = await sandbox.snapshot();
        expect(snapshot.kind).toBe('jsSandbox');
        const digest = await snapshot.save(directory, 'runtime-ready');
        expect(digest).toMatch(/^sha256:/);
        sandbox.dispose();

        const persisted = await Snapshot.load(directory, 'runtime-ready');
        const restorer = await new SandboxBuilder().buildFromSnapshot(persisted);
        expect(restorer.loadRuntime).toBeUndefined();
        expect(typeof restorer.restoreJsSandbox).toBe('function');
        const restoredSandbox = await restorer.restoreJsSandbox();
        const loaded = await restoredSandbox.getLoadedSandbox();

        await expect(loaded.callHandler('counter', {})).resolves.toEqual({ count: 1 });
        await loaded.dispose();
    });

    it('persists loaded guest state and restores it in a new sandbox', async () => {
        const directory = await snapshotDirectory();
        const proto = await new SandboxBuilder().build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'counter',
            `
            let count = 0;
            function handler(event) {
                event.count = ++count;
                return event;
            }
            `
        );
        const loaded = await sandbox.getLoadedSandbox();
        await expect(loaded.callHandler('counter', {})).resolves.toEqual({ count: 1 });

        const snapshot = await loaded.snapshot();
        expect(snapshot.kind).toBe('loadedJsSandbox');
        await snapshot.save(directory, 'application-ready');
        await loaded.dispose();

        const persisted = await Snapshot.load(directory, 'application-ready');
        const restorer = await new SandboxBuilder().buildFromSnapshot(persisted);
        const restored = await restorer.restoreLoadedSandbox();

        await expect(restored.callHandler('counter', {})).resolves.toEqual({ count: 2 });
        const unloaded = await restored.unload();
        unloaded.addHandler('replacement', 'function handler(event) { return event; }');
        const reloaded = await unloaded.getLoadedSandbox();
        await expect(reloaded.callHandler('replacement', { ok: true })).resolves.toEqual({
            ok: true,
        });
        await reloaded.dispose();
    });

    it('requires host functions used by the persisted sandbox', async () => {
        const directory = await snapshotDirectory();
        const proto = await new SandboxBuilder()
            .setHostFunctionModules(stateHostFunctionModules)
            .build();
        const sandbox = await proto.loadRuntime();
        sandbox.addHandler(
            'handler',
            `
            import * as state from "host:state";
            function handler(event) {
                event.value = state.next();
                return event;
            }
            `
        );
        const loaded = await sandbox.getLoadedSandbox();
        const snapshot = await loaded.snapshot();
        await snapshot.save(directory, 'host-functions');
        await loaded.dispose();

        const persisted = await Snapshot.load(directory, 'host-functions');
        expect(persisted.requirements).toEqual({
            hostFunctions: [{ module: 'host:state', functions: ['next'] }],
            moduleLoader: 'unknown',
        });
        const missingRestorer = await new SandboxBuilder().buildFromSnapshot(persisted);
        await expect(missingRestorer.restoreLoadedSandbox()).rejects.toThrow(
            /requires host module 'host:state'/
        );

        const restorer = await new SandboxBuilder()
            .setHostFunctionModules(stateHostFunctionModules)
            .setHostFunctionFilter('snapshotRequirements')
            .buildFromSnapshot(persisted);
        const restored = await restorer.restoreLoadedSandbox();
        await expect(restored.callHandler('handler', {})).resolves.toEqual({ value: 1 });
        await restored.dispose();
    });
});
