# Persistent snapshots

hyperlight-js sandboxes can be captured to hyperlight's OCI image-layout
snapshot format and restored after the original sandbox or process has exited.
Snapshots are immutable and may be addressed by a human-readable tag or their
`sha256:` manifest digest.

Two lifecycle states are supported:

- `JSSandbox` snapshots contain the initialized JavaScript runtime together with
  handler and module definitions added to `JSSandbox` but not yet registered in
  the guest.
- `LoadedJSSandbox` snapshots additionally contain evaluated guest handlers,
  closures, and current mutable guest state, including module-scoped state.

An OCI image layout is one directory on disk. The same layout can contain any
number of independently tagged `JSSandbox` and `LoadedJSSandbox` snapshots;
each tag or manifest digest identifies one snapshot.

In-memory `restore()` remains the fastest way to roll back an existing sandbox.
`Snapshot::save()` and `Snapshot::load()` provide a durable, cross-process
workflow.

## Rust

```rust
use hyperlight_js::{LoadedJSSandbox, OciTag, SandboxBuilder, Script, Snapshot};

fn main() -> hyperlight_js::Result<()> {
    let mut sandbox = SandboxBuilder::new().build()?.load_runtime()?;
    sandbox.add_handler(
        "handler",
        Script::from_content("function handler(event) { return event; }"),
    )?;
    let mut loaded = sandbox.get_loaded_sandbox()?;

    let snapshot = loaded.snapshot()?;
    let digest = snapshot.save("./snapshots", &OciTag::new("application-ready")?)?;

    // Later, potentially in another process.
    // The tag can be used instead of the digest.
    let snapshot = Snapshot::<LoadedJSSandbox>::checked_load("./snapshots", digest)?;
    let _loaded = SandboxBuilder::from_snapshot(snapshot).build()?;
    Ok(())
}
```

`Snapshot<T>` names the sandbox type the snapshot captured. Loading fails if the
saved snapshot holds the other type. `build()` returns `T`.

## Node.js

```javascript
const { SandboxBuilder, Snapshot } = require('@hyperlight-dev/js-host-api');

const snapshot = await loaded.snapshot();
const digest = await snapshot.save('./snapshots', 'application-ready');

// Later, potentially in another process.
// 'application-ready' can be used instead of digest to load by tag.
const persisted = await Snapshot.checkedLoad('./snapshots', digest);
const restored = await SandboxBuilder.fromSnapshot(persisted).build();
```

`build()` returns a `JSSandbox` or `LoadedJSSandbox`, matching the saved
snapshot. TypeScript types the result as `JSSandbox | LoadedJSSandbox`. Narrow it
with `instanceof`.

Configure process-local host resources on the builder before calling `build()`.
A snapshot's memory layout overrides heap, scratch, and transport settings.

`Snapshot::checked_load()` in Rust and `Snapshot.checkedLoad()` in Node check
every OCI blob against its recorded digest. `Snapshot::load()` and
`Snapshot.load()` skip the check and reduce I/O. Use them only for trusted local
layouts. All four accept a tag or a manifest digest.

The loaded blobs stay memory-mapped while the snapshot or any sandbox built from
it is alive. Overwriting, truncating, or deleting them during that time can
corrupt guest memory.

## Modules and host resources

hyperlight-js has four mechanisms that are all visible to JavaScript as modules
but have different ownership and restore behavior.

| Category | How it is provided | Where code executes | Snapshot behavior | Restore action |
| --- | --- | --- | --- | --- |
| Host-function module | `HostFunctionModule`s containing Rust closures or Node callbacks | Host process through `CallHostJsFunction` | Records the supplied module and function names, not implementations | Supply fresh modules to `SandboxBuilder` |
| Embedded JavaScript module | `JSSandbox::add_module()` / `addModule()` | Guest QuickJS runtime | A `JSSandbox` snapshot stores staged source in hyperlight-js metadata. A `LoadedJSSandbox` snapshot captures the guest module-loader map and any evaluated module state. | None; hyperlight-js restores it automatically |
| External JavaScript module | Rust `SandboxBuilder::with_module_loader()` backed by `ResolveModule` and `LoadModule` | Guest QuickJS runtime after source is returned by the host | Loaded/evaluated state is captured, but source returned by the loader is not currently archived | Supply a compatible loader to the restoring builder when the underlying snapshot requires it |
| Native guest module | Rust compiled into `hyperlight-js-runtime` with `rquickjs` | Guest runtime | Native module code and state are already resident in the captured guest VM | None |

### Reusable host-function definitions

Write one function that builds the modules, and call it for each fresh or
restored builder. A builder consumes its modules, so each call must create fresh
module objects, callbacks, and state. Create mutable state inside the function
to isolate it per sandbox. Capturing an external `Arc`, static, or JavaScript
variable shares that state between sandboxes, which breaks the Sandbox isolation
model.

Rust:

```rust
use hyperlight_js::{HostFunctionModule, JSSandbox, OciTag, SandboxBuilder, Snapshot};

fn host_function_modules() -> Vec<HostFunctionModule> {
    let mut database = HostFunctionModule::new("host:database");
    database.register("query", |id: u64| format!("record-{id}"));
    vec![database]
}

let fresh = SandboxBuilder::new()
    .with_host_function_modules(host_function_modules())
    .build()?
    .load_runtime()?;

let snapshot =
    Snapshot::<JSSandbox>::checked_load("./snapshots", OciTag::new("with-host-functions")?)?;
let restored = SandboxBuilder::from_snapshot(snapshot)
    .with_host_function_modules(host_function_modules())
    .build()?;
```

Node.js:

```javascript
const {
  HostFunctionModule,
  SandboxBuilder,
  Snapshot,
} = require('@hyperlight-dev/js-host-api');

function hostFunctionModules() {
  const database = new HostFunctionModule('database');
  database.register('query', query);
  return [database];
}

const freshProto = await new SandboxBuilder()
  .setHostFunctionModules(hostFunctionModules())
  .build();
const fresh = await freshProto.loadRuntime();

const snapshot = await Snapshot.checkedLoad('./snapshots', 'with-host-functions');
const restored = await SandboxBuilder.fromSnapshot(snapshot)
  .setHostFunctionModules(hostFunctionModules())
  .build();
```

A snapshot records the host functions its guest state needs. `build()` fails
before running the guest if a recorded module or function is not supplied.

The guest can call every supplied host function, including ones the snapshot
does not record. Handlers loaded after `unload()` can import them too. Supply
only the modules the guest should reach.

A loaded snapshot records the host functions its evaluated handlers need. Saving
it again after a restore keeps that record, even when the restoring builder
supplied more functions. A `JSSandbox` snapshot records every function supplied
to its builder, because its staged handlers are not compiled yet.

hyperlight validates only the typed `CallHostJsFunction` bridge. hyperlight-js
therefore validates individual logical module and function names itself. It
cannot verify argument types, return types, or behavior.
[hyperlight-js#339][hyperlight-js-host-function-signatures] tracks stable
semantic signatures.

### Embedded JavaScript modules

Modules added with `add_module()` / `addModule()` are guest application state.

For a `JSSandbox` snapshot, handlers and modules have not yet been transferred
to the guest. hyperlight-js therefore stores their source in snapshot metadata
and reconstructs the staged definitions automatically during restoration.
hyperlight limits that metadata to 1 MiB, so `save()` fails when the staged
sources exceed it. Load large sources into a `LoadedJSSandbox` before saving.

Creating a `LoadedJSSandbox` transfers every embedded module source into the
guest's module-loader map before handlers are registered. A loaded snapshot
therefore captures every registered source in guest memory, including modules
that have not yet been imported or evaluated. It also captures evaluated module
instances and mutable module-scoped state. Callers do not add embedded modules
again after restoring either lifecycle state.

### External JavaScript loaders

The external loader is lazy. `ResolveModule` and `LoadModule` run only when
QuickJS traverses an import that earlier loaders did not satisfy. Modules never
imported before the snapshot are not present in guest memory.

```rust
let snapshot =
    Snapshot::<JSSandbox>::checked_load("./snapshots", OciTag::new("with-module-loader")?)?;
let sandbox = SandboxBuilder::from_snapshot(snapshot)
    .with_module_loader(file_system)
    .build()?;
```

hyperlight records `ResolveModule` and `LoadModule`, including their exact type
signatures, in its snapshot. Missing or mismatched replacements produce
`SnapshotHostFunctionMismatch` before guest execution. hyperlight-js translates
that mismatch into a module-loader-specific error. Matching signatures do not
prove that a replacement loader resolves the same paths or returns the same
source.

hyperlight-js does not currently archive source returned by an arbitrary module
loader. A future capture API must distinguish sources observed through
`LoadModule` from a complete, explicitly supplied archive: observed sources do
not cover dynamic imports that have not executed yet.

### Native guest modules

Native modules execute inside the guest and are already resident in guest VM
state when a snapshot is taken. They are restored with that VM state and do not
require host-side registration. The `JSRUNTIME` embedded in the current host
binary is used only when constructing a fresh sandbox, not when restoring one.
CPU architecture and hyperlight snapshot-format compatibility still apply.

### Module-loader requirements

hyperlight checks `ResolveModule` and `LoadModule` during `build()`. It does not
yet expose a snapshot's host-function requirements for inspection.
[hyperlight#1870][hyperlight-snapshot-host-functions] tracks that. Until then,
hyperlight-js cannot tell before building whether a snapshot needs a module
loader. Supply one when the application's contract requires it, or handle the
build error.

`HostPrint` is handled separately. hyperlight provides a default stdout printer
when no custom handler is supplied. Reinstall a custom print handler only when
the same output routing is required.

## Compatibility and trust

- Snapshots are tied to their CPU architecture, CPU vendor, hypervisor, and
  hyperlight snapshot format. For example, a KVM snapshot taken on an Intel host
  does not load on AMD or under MSHV.
- hyperlight-js metadata is schema-versioned and records the producing package
  version for diagnostics. It also records the guest runtime ABI version.
  Unsupported metadata schema or guest ABI versions are rejected before starting
  guest execution.
- Digest verification detects accidental or malicious mutation, but does not
  authenticate who produced a snapshot. Authenticate snapshot artifacts through
  the distribution mechanism used by your application.

## Files in a loaded snapshot

A persisted `LoadedJSSandbox` consists of two hyperlight snapshots in the same
OCI layout:

- The loaded application state, including registered handlers, modules, and
  mutable guest state.
- A raw runtime-ready rollback state captured before user handlers and modules
  were registered.

The rollback state is not an independently restorable hyperlight-js
`JSSandbox` snapshot: it has no hyperlight-js lifecycle metadata or
staged handler and module definitions. The loaded snapshot's hyperlight-js
metadata stores its digest solely so `unload()` can discard evaluated handlers,
modules, and mutable guest state and return a clean `JSSandbox` after restore.

The digest is stored only in hyperlight-js metadata, not as an OCI descriptor.
Standard OCI tools therefore do not discover or copy the baseline
automatically. [hyperlight#1865][hyperlight-snapshot-bundles] tracks native OCI
bundle support for this relationship.

### Moving loaded snapshots with ORAS

Until [hyperlight supports OCI snapshot bundles][hyperlight-snapshot-bundles], a
persisted `LoadedJSSandbox` uses two OCI tags:

- `application-ready.hljs-base`: the runtime-ready baseline
- `application-ready`: the loaded guest state

`save()` rejects user tags ending in `.hljs-base`, so a user tag cannot replace a
baseline.

Copy both tags into the same repository or OCI layout. Push the baseline first
so the public application tag is never published without its dependency:

```console
oras cp --from-oci-layout \
  ./snapshots:application-ready.hljs-base \
  registry.example.com/snapshots:application-ready.hljs-base

oras cp --from-oci-layout \
  ./snapshots:application-ready \
  registry.example.com/snapshots:application-ready
```

Restore both tags into the same local OCI layout before loading the snapshot:

```console
oras cp --to-oci-layout \
  registry.example.com/snapshots:application-ready.hljs-base \
  ./snapshots:application-ready.hljs-base

oras cp --to-oci-layout \
  registry.example.com/snapshots:application-ready \
  ./snapshots:application-ready
```

The `oras cp --recursive` option copies OCI referrers. It does not discover the
hyperlight-js baseline because the relationship is currently stored in
application metadata rather than an OCI descriptor.

[hyperlight-snapshot-bundles]: https://github.com/hyperlight-dev/hyperlight/issues/1865
[hyperlight-snapshot-host-functions]: https://github.com/hyperlight-dev/hyperlight/issues/1870
[hyperlight-js-host-function-signatures]: https://github.com/hyperlight-dev/hyperlight-js/issues/339

## Examples and benchmarks

The Rust `persistent_snapshot` example and Node
`examples/persistent-snapshot.js` both have separate `write` and `read`
commands, demonstrating restoration in a new process.

Criterion benchmarks under `persistent_snapshots` measure save, verified load,
and construction of a new `LoadedJSSandbox` from disk.
