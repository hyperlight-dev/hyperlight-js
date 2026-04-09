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

// ESM re-export of the CJS entry point (lib.js).
// This enables `import { SandboxBuilder } from '@hyperlight-dev/js-host-api'`
// while preserving the error-enrichment wrapper from lib.js.

import native from './lib.js';

export const {
    HostModule,
    HostModuleWrapper,
    InterruptHandle,
    InterruptHandleWrapper,
    JSSandbox,
    JSSandboxWrapper,
    LoadedJSSandbox,
    LoadedJSSandboxWrapper,
    ProtoJSSandbox,
    ProtoJSSandboxWrapper,
    SandboxBuilder,
    SandboxBuilderWrapper,
    Snapshot,
    SnapshotWrapper,
} = native;

export default native;
