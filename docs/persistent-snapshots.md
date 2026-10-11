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
use hyperlight_js::{LoadedJSSandbox, SandboxBuilder, Script, Snapshot};

fn main() -> hyperlight_js::Result<()> {
    let mut sandbox = SandboxBuilder::new().build()?.load_runtime()?;
    sandbox.add_handler(
        "handler",
        Script::from_content("function handler(event) { return event; }"),
    )?;
    let mut loaded = sandbox.get_loaded_sandbox()?;

    let snapshot = loaded.snapshot()?;
    let digest = snapshot.save("./snapshots", "application-ready")?;

    // Later, potentially in another process.
    // "application-ready" can be used instead of digest to load by tag.
    let snapshot = Snapshot::load("./snapshots", digest)?;
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot)?;
    let _loaded = restorer.restore::<LoadedJSSandbox>()?;
    Ok(())
}
```

Use `restore::<JSSandbox>()` for a snapshot whose `kind()` is
`SnapshotKind::JsSandbox`. The requested type must match the snapshot kind.

## Node.js

```javascript
const { SandboxBuilder, Snapshot } = require('@hyperlight-dev/js-host-api');

const snapshot = await loaded.snapshot();
const digest = await snapshot.save('./snapshots', 'application-ready');

// Later, potentially in another process.
// 'application-ready' can be used instead of digest to load by tag.
const persisted = await Snapshot.load('./snapshots', digest);
const restorer = await new SandboxBuilder().buildFromSnapshot(persisted);
const restored = await restorer.restoreLoadedSandbox();
```

`build_from_snapshot()` in Rust and `buildFromSnapshot()` in Node create a
`SandboxRestorer`. Configure process-local host resources on `SandboxBuilder`
before creating the restorer, then restore the matching lifecycle type. Rust uses
`restore::<JSSandbox>()` or `restore::<LoadedJSSandbox>()`; Node uses
`restoreJsSandbox()` or `restoreLoadedSandbox()`. A restorer does not expose
`load_runtime()` or `loadRuntime()`; those methods belong to a fresh
`ProtoJSSandbox`.

Both bindings verify every OCI blob when loading a snapshot. Use
`Snapshot::load()` in Rust or `Snapshot.load()` in Node; both accept either a
tag or manifest digest.

For trusted local layouts, `Snapshot::load_unverified()` in Rust and
`Snapshot.loadUnverified()` in Node skip digest verification and reduce I/O.
Do not use them with untrusted snapshot data.

## Modules and host resources

hyperlight-js has four mechanisms that are all visible to JavaScript as modules
but have different ownership and restore behavior.

| Category | How it is provided | Where code executes | Snapshot behavior | Restore action |
| --- | --- | --- | --- | --- |
| Host-function module | A factory that creates `HostFunctionModule`s containing Rust closures or Node callbacks | Host process through `CallHostJsFunction` | Records the builder-selected module and function names, not implementations | Supply the factory to `SandboxBuilder` |
| Embedded JavaScript module | `JSSandbox::add_module()` / `addModule()` | Guest QuickJS runtime | A `JSSandbox` snapshot stores staged source in hyperlight-js metadata. A `LoadedJSSandbox` snapshot captures the guest module-loader map and any evaluated module state. | None; hyperlight-js restores it automatically |
| External JavaScript module | Rust `SandboxBuilder::with_module_loader()` backed by `ResolveModule` and `LoadModule` | Guest QuickJS runtime after source is returned by the host | Loaded/evaluated state is captured, but source returned by the loader is not currently archived | Supply a compatible loader to the restoring builder when the underlying snapshot requires it |
| Native guest module | Rust compiled into `hyperlight-js-runtime` with `rquickjs` | Guest runtime | Native module code and state are already resident in the captured guest VM | None |

### Reusable host-function definitions

Define a factory once and supply it to fresh and restored builders. Each builder
invokes the factory once, creating fresh module objects, callback implementations,
and state. Create mutable state inside the factory to isolate it per sandbox.
Capturing an external `Arc`, static, or JavaScript variable deliberately shares
that state between sandboxes, which breaks the Sandbox isolation model.

Rust:

```rust
use hyperlight_js::{
    HostFunctionFilter, HostFunctionModule, JSSandbox, SandboxBuilder, Snapshot,
};

fn host_function_modules() -> Vec<HostFunctionModule> {
  let mut database = HostFunctionModule::new("host:database");
  database.register("query", |id: u64| format!("record-{id}"));
  vec![database]
}

let fresh = SandboxBuilder::new()
  .with_host_function_modules(host_function_modules)
    .build()?
    .load_runtime()?;

let snapshot = Snapshot::load("./snapshots", "with-host-functions")?;
let restorer = SandboxBuilder::new()
  .with_host_function_modules(host_function_modules)
    .with_host_function_filter(HostFunctionFilter::snapshot_requirements())
    .build_from_snapshot(snapshot)?;
let restored: JSSandbox = restorer.restore()?;
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
  .setHostFunctionModules(hostFunctionModules)
  .build();
const fresh = await freshProto.loadRuntime();

const snapshot = await Snapshot.load('./snapshots', 'with-host-functions');
const restorer = await new SandboxBuilder()
  .setHostFunctionModules(hostFunctionModules)
  .setHostFunctionFilter('snapshotRequirements')
  .buildFromSnapshot(snapshot);
const restored = await restorer.restoreJsSandbox();
```

Loaded sandboxes track two host-function manifests:

- **Required** functions belong to the currently captured guest state and are
  reported by `Snapshot::requirements()` / `snapshot.requirements`.
- **Available** functions are callbacks retained by the host for dispatch and
  possible future handlers after `unload()`.

Restoring a loaded snapshot never inflates its required manifest. With the
default `HostFunctionFilter::All` / `"all"`, every supplied callback remains
available; unloading promotes that complete set into the resulting `JSSandbox`,
where newly staged handlers may import it. With `SnapshotRequirements` /
`"snapshotRequirements"`, callbacks not declared by the snapshot are discarded,
so they remain unavailable after unload. The requirements filter is valid only
with `build_from_snapshot()` / `buildFromSnapshot()`.

For `JSSandbox`, no handlers are executing in the guest, but its snapshot still
records the host capabilities configured for the next load. It therefore records
the available manifest rather than an empty requirement set.

hyperlight validates only the typed `CallHostJsFunction` bridge. hyperlight-js
therefore validates individual logical module and function names itself. It
cannot verify argument types, return types, or behavior; [hyperlight-js#339]
[hyperlight-js-host-function-signatures] tracks stable semantic signatures.

### Embedded JavaScript modules

Modules added with `add_module()` / `addModule()` are guest application state.

For a `JSSandbox` snapshot, handlers and modules have not yet been transferred
to the guest. hyperlight-js therefore stores their source in snapshot metadata
and reconstructs the staged definitions automatically during restoration.

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
let snapshot = Snapshot::load("./snapshots", "with-module-loader")?;
let restorer = SandboxBuilder::new()
  .with_module_loader(file_system)
  .build_from_snapshot(snapshot)?;
let sandbox = restorer.restore::<JSSandbox>()?;
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

### What hyperlight #1870 blocks

hyperlight already enforces low-level host-function requirements during restore.
[hyperlight#1870][hyperlight-snapshot-host-functions] is not needed for safety;
it is needed to inspect those requirements before attempting to build.

Until #1870 is available, `Snapshot::requirements().module_loader()` returns
`RequirementStatus::Unknown` and `snapshot.requirements.moduleLoader` returns
`"unknown"`. hyperlight-js cannot determine in advance whether the underlying
snapshot requires `ResolveModule` and `LoadModule`. It must rely on the
application's contract or attempt restoration and translate the mismatch error.
This prevents automatic preflight selection between a captured-source loader, a
caller-supplied live loader, and no loader. It also prevents a complete public
view of future low-level hyperlight host-function requirements without
duplicating hyperlight's canonical ABI metadata.

Logical host-function names remain inspectable because hyperlight-js owns that
metadata:

```rust
for (module, functions) in snapshot.requirements().host_functions() {
    println!("{module}: {functions:?}");
}
```

`HostPrint` is handled separately. hyperlight provides a default stdout printer
when no custom handler is supplied. Reinstall a custom print handler only when
the same output routing is required.

## Compatibility and trust

- Snapshots are tied to their CPU architecture and hyperlight snapshot format.
- hyperlight-js metadata is schema-versioned and records the producing package
  version for diagnostics. Unsupported metadata schema versions are rejected
  before starting guest execution.
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
`JSSandbox` snapshot: it has no `SnapshotKind::JsSandbox` lifecycle metadata or
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
