# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This file records significant changes since [v0.5.0](https://github.com/hyperlight-dev/hyperlight-js/releases/tag/v0.5.0).
Release notes also include GitHub's automatically generated list of merged pull requests.

## [Prerelease] - Unreleased

### Changed

- **Breaking:** Upgrade Hyperlight from 0.18 to 0.19 and remove the separate guest
  heap setting: Rust `SandboxBuilder::with_guest_heap_size` and Node.js
  `SandboxBuilder.setHeapSize` no longer exist. Use `with_guest_scratch_size` /
  `setScratchSize` instead. Scratch now shares one budget across the heap, stack,
  transport pools, page tables, and copy-on-write pages, with a default and minimum
  of 5 MiB. It is not a JavaScript-only heap allowance; re-evaluate memory budgets
  when migrating. Exhaustion aborts and poisons the sandbox; restore a snapshot
  before reuse. ([#429](https://github.com/hyperlight-dev/hyperlight-js/pull/429))
- **Breaking:** Snapshot metadata now uses schema version 2. Snapshots produced by
  v0.5.0 are rejected because their base may not have registered the host functions
  recorded in metadata. Recreate persisted snapshots with the new runtime rather
  than editing their metadata. The Hyperlight 0.19 upgrade also changes the
  underlying snapshot ABI.
  ([#422](https://github.com/hyperlight-dev/hyperlight-js/pull/422),
  [#429](https://github.com/hyperlight-dev/hyperlight-js/pull/429))
- **Breaking for custom native runtimes:** Upgrade `rquickjs` from 0.12 to 0.14,
  including its QuickJS engine update. Update custom runtime dependencies to
  `rquickjs = "0.14"` and rebuild guests. Upstream buffer/typed-array byte accessors
  are now unsafe, their safe `AsRef` implementations are removed, and raw buffer
  accessors return slice pointers. Review custom modules against the
  [upstream migration notes](https://github.com/DelSkayn/rquickjs/blob/v0.14.0/CHANGELOG.md).
  ([#326](https://github.com/hyperlight-dev/hyperlight-js/pull/326))
- Host and guest builds now use separate pinned Rust toolchains. The standard
  `just` build recipes install both, allowing host dependencies to advance without
  changing the guest compiler. Update the module resolver to `oxc_resolver`
  11.24.3.
  ([#389](https://github.com/hyperlight-dev/hyperlight-js/pull/389),
  [#390](https://github.com/hyperlight-dev/hyperlight-js/pull/390))
- Update observability dependencies to `tracing-opentelemetry` 0.34 and
  `tracing-tracy` 0.12.
  ([#356](https://github.com/hyperlight-dev/hyperlight-js/pull/356),
  [#413](https://github.com/hyperlight-dev/hyperlight-js/pull/413))
- Update the MCP server example to `@modelcontextprotocol/sdk` 1.32.2 and Hono
  4.13.18.
  ([#387](https://github.com/hyperlight-dev/hyperlight-js/pull/387),
  [#439](https://github.com/hyperlight-dev/hyperlight-js/pull/439))
- CI now separates code checks from builds, restores primed Rust and guest sysroot
  caches, uses shorter benchmarks, and skips expensive jobs for documentation-only
  PRs. Group Hyperlight dependency updates so host and guest versions stay aligned.
  ([#399](https://github.com/hyperlight-dev/hyperlight-js/pull/399),
  [#438](https://github.com/hyperlight-dev/hyperlight-js/pull/438))

### Removed

- **Breaking (Rust):** Remove `ProtoJSSandbox::set_module_loader`, `host_module`,
  and `register`. Configure resources before `build()` using
  `SandboxBuilder::with_module_loader` and `with_host_function_modules`, supplying
  `HostFunctionModule` values. `HostModule` is now internal.
  ([#448](https://github.com/hyperlight-dev/hyperlight-js/pull/448))
- **Breaking (Node.js):** Remove `ProtoJSSandbox.hostModule`, `register`, and the
  `HostModule` export. Create `HostFunctionModule` instances and pass them to
  `SandboxBuilder.setHostFunctionModules` before `build()`.
  ([#448](https://github.com/hyperlight-dev/hyperlight-js/pull/448))
- **Breaking:** Remove deprecated Rust `with_guest_input_buffer_size` /
  `with_guest_output_buffer_size` and Node.js `setInputBufferSize` /
  `setOutputBufferSize`. Use `with_input_transport_pool_pages` /
  `with_output_transport_pool_pages` or `setInputTransportPoolPages` /
  `setOutputTransportPoolPages` for explicit pool budgets, and the corresponding
  transport-buffer-size setters for individual buffers. Pool budgets are in pages,
  not bytes; this is not a direct rename.
  ([#448](https://github.com/hyperlight-dev/hyperlight-js/pull/448))
- **Breaking:** Remove `JSSandbox::poisoned()` and `LoadedJSSandbox::poisoned()`;
  use `status().is_poisoned()`. In Node.js, replace the removed `poisoned` getter
  with `status === 'poisoned'`.
  ([#448](https://github.com/hyperlight-dev/hyperlight-js/pull/448))

### Fixed

- Avoid redundant host-function registration during load/unload cycles. Restoring
  a snapshot with different host functions registers them once and updates the
  base snapshot, so subsequent cycles avoid unnecessary guest calls.
  ([#422](https://github.com/hyperlight-dev/hyperlight-js/pull/422))
- Fix Windows rebuild failures caused by losing the Hyperlight surrogate
  executable from the build output directory.
  ([#404](https://github.com/hyperlight-dev/hyperlight-js/pull/404),
  [#429](https://github.com/hyperlight-dev/hyperlight-js/pull/429))
- Publish all existing npm platform packages, including ARM64, using trusted
  publishing. Reruns after a partial release skip already-published versions,
  and successful publishes no longer fail a premature registry visibility check.
  ([#401](https://github.com/hyperlight-dev/hyperlight-js/pull/401))
- Remove an unsupported toolchain input from CI setup.
  ([#405](https://github.com/hyperlight-dev/hyperlight-js/pull/405))

[Prerelease]: https://github.com/hyperlight-dev/hyperlight-js/compare/v0.5.0...HEAD
