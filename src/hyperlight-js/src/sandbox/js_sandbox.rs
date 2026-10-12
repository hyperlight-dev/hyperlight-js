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
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use hyperlight_host::sandbox::snapshot::Snapshot as HyperlightSnapshot;
use hyperlight_host::{new_error, MultiUseSandbox, Result, SandboxStatus};
use tracing::{instrument, Level};

use super::host_fn::register_host_function_manifest;
use super::loaded_js_sandbox::LoadedJSSandbox;
use super::snapshot::{
    HostFunctionManifest, ScriptMetadata, Snapshot, SnapshotKind, SnapshotMetadata,
};
use crate::sandbox::metrics::SandboxMetricsGuard;
use crate::Script;

/// Default namespace for user modules when none is specified.
///
/// Guest JavaScript imports user modules as `import { ... } from "user:<name>"`.
/// This namespace is used by [`JSSandbox::add_module`] when no custom namespace
/// is provided.
///
/// Note: `"user"` is the default namespace but is **not** reserved — callers
/// can pass it explicitly to [`JSSandbox::add_module_ns`] with the same effect
/// as calling [`JSSandbox::add_module`].
pub const DEFAULT_MODULE_NAMESPACE: &str = "user";

/// Reserved namespaces that cannot be used for user modules.
///
/// The `host` namespace is reserved for host-function modules registered via
/// [`ProtoJSSandbox::host_module`].
pub const RESERVED_NAMESPACES: &[&str] = &["host"];

/// Validates a module identifier (name or namespace).
///
/// Rules:
/// - Must not be empty (or whitespace-only)
/// - Must not contain `':'`
///
/// Returns an error with a descriptive message using `label`
/// (e.g. "Module name", "Module namespace").
pub fn validate_module_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(new_error!("{} must not be empty", label));
    }
    if value.contains(':') {
        return Err(new_error!("{} must not contain ':'", label));
    }
    Ok(())
}

/// Validates that a namespace is not reserved.
///
/// Returns an error if the namespace is in [`RESERVED_NAMESPACES`].
pub fn validate_namespace_not_reserved(namespace: &str) -> Result<()> {
    if RESERVED_NAMESPACES.contains(&namespace) {
        return Err(new_error!("Module namespace '{}' is reserved", namespace));
    }
    Ok(())
}

/// A Hyperlight Sandbox with a JavaScript run time loaded but no guest code.
pub struct JSSandbox {
    pub(super) inner: MultiUseSandbox,
    // Mutable definitions staged for guest registration. Snapshot metadata is
    // built from these maps only when a snapshot or LoadedJSSandbox is created.
    handlers: HashMap<String, Script>,
    /// User modules keyed by qualified name (e.g. `user:utils`).
    modules: HashMap<String, Script>,
    // Host functions selected by SandboxBuilder.
    host_functions: HostFunctionManifest,
    // Snapshot of state before any handlers are added.
    // This is used to restore state back to a neutral JSSandbox.
    snapshot: Arc<HyperlightSnapshot>,
    // metric drop guard to manage sandbox metric
    _metric_guard: SandboxMetricsGuard<JSSandbox>,
}

impl JSSandbox {
    #[instrument(err(Debug), skip(inner), level=Level::INFO)]
    pub(super) fn new(
        mut inner: MultiUseSandbox,
        host_functions: HostFunctionManifest,
    ) -> Result<Self> {
        let snapshot = inner.snapshot()?;
        Ok(Self {
            inner,
            handlers: HashMap::new(),
            modules: HashMap::new(),
            host_functions,
            snapshot,
            _metric_guard: SandboxMetricsGuard::new(),
        })
    }

    /// Creates a new `JSSandbox` from a `MultiUseSandbox` and a `Snapshot` of state before any handlers were added.
    pub(crate) fn from_loaded(
        mut loaded: MultiUseSandbox,
        snapshot: Arc<HyperlightSnapshot>,
        host_functions: HostFunctionManifest,
    ) -> Result<Self> {
        loaded.restore(snapshot.clone())?;
        register_host_function_manifest(&mut loaded, &host_functions)?;
        Ok(Self {
            inner: loaded,
            handlers: HashMap::new(),
            modules: HashMap::new(),
            host_functions,
            snapshot,
            _metric_guard: SandboxMetricsGuard::new(),
        })
    }

    pub(crate) fn from_snapshot(
        inner: MultiUseSandbox,
        snapshot: &Snapshot,
        host_functions: HostFunctionManifest,
    ) -> Result<Self> {
        if snapshot.kind() != SnapshotKind::JsSandbox {
            return Err(new_error!(
                "Cannot restore a {:?} snapshot as a JSSandbox",
                snapshot.kind()
            ));
        }

        Ok(Self {
            inner,
            handlers: snapshot.metadata().handlers().collect(),
            modules: snapshot.metadata().modules().collect(),
            host_functions,
            snapshot: snapshot.inner(),
            _metric_guard: SandboxMetricsGuard::new(),
        })
    }

    /// Capture the current runtime state and handlers and modules added to this
    /// sandbox but not yet registered in the guest.
    #[instrument(err(Debug), skip_all, level=Level::DEBUG)]
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let inner = self.inner.snapshot()?;
        let metadata = SnapshotMetadata::new(
            SnapshotKind::JsSandbox,
            self.handlers
                .iter()
                .map(|(name, script)| (name.clone(), ScriptMetadata::from(script))),
            self.modules
                .iter()
                .map(|(name, script)| (name.clone(), ScriptMetadata::from(script))),
            self.host_functions.clone(),
        );
        Ok(Snapshot::new(inner, None, Arc::new(metadata)))
    }

    /// Restore the runtime state and handlers and modules that were added to the
    /// snapshotted sandbox but not yet registered in the guest.
    #[instrument(err(Debug), skip_all, level=Level::DEBUG)]
    pub fn restore(&mut self, snapshot: Snapshot) -> Result<()> {
        if snapshot.kind() != SnapshotKind::JsSandbox {
            return Err(new_error!(
                "Cannot restore a {:?} snapshot into a JSSandbox",
                snapshot.kind()
            ));
        }
        snapshot
            .metadata()
            .validate_host_functions(&self.host_functions)?;
        self.inner.restore(snapshot.inner())?;
        self.handlers = snapshot.metadata().handlers().collect();
        self.modules = snapshot.metadata().modules().collect();
        self.snapshot = snapshot.inner();
        Ok(())
    }

    /// Adds a new handler function to the sandboxes collection of handlers. This Handler will be
    /// available to the host to call once `get_loaded_sandbox` is called.
    #[instrument(err(Debug), skip(self, script), level=Level::DEBUG)]
    pub fn add_handler<F>(&mut self, function_name: F, script: Script) -> Result<()>
    where
        F: Into<String> + std::fmt::Debug,
    {
        let function_name = function_name.into();
        if function_name.is_empty() {
            return Err(new_error!("Handler name must not be empty"));
        }
        if self.handlers.contains_key(&function_name) {
            return Err(new_error!(
                "Handler already exists for function name: {}",
                function_name
            ));
        }

        self.handlers.insert(function_name, script);
        Ok(())
    }

    /// Removes a handler function from the sandboxes collection of handlers.
    #[instrument(err(Debug), skip(self), level=Level::DEBUG)]
    pub fn remove_handler(&mut self, function_name: &str) -> Result<()> {
        if function_name.is_empty() {
            return Err(new_error!("Handler name must not be empty"));
        }
        match self.handlers.remove(function_name) {
            Some(_) => Ok(()),
            None => Err(new_error!(
                "Handler does not exist for function name: {}",
                function_name
            )),
        }
    }

    /// Clears all handlers from the sandbox.
    #[instrument(skip_all, level=Level::TRACE)]
    pub fn clear_handlers(&mut self) {
        self.handlers.clear();
    }

    // ── Module management ────────────────────────────────────────────

    /// Adds a module to the sandbox with the default namespace (`user`).
    ///
    /// The module will be available for import by handlers (and other modules)
    /// using `import { ... } from 'user:<module_name>'`.
    ///
    /// Modules are compiled lazily when first imported, so inter-module
    /// dependencies are resolved automatically regardless of registration order.
    ///
    /// # Shared state between handlers
    ///
    /// ES modules are singletons — all importers share the **same** module
    /// instance. This means mutable module-level state (e.g. `let count = 0`)
    /// is visible to every handler that imports the module. Handler A can
    /// mutate module state, and Handler B will see those changes in
    /// subsequent calls.
    ///
    /// Module state persists across [`LoadedJSSandbox::handle_event()`] calls
    /// and is reset by [`LoadedJSSandbox::snapshot()`] / [`LoadedJSSandbox::restore()`]
    /// or [`LoadedJSSandbox::unload()`].
    ///
    /// # Example
    ///
    /// ```text
    /// // Register a utility module (pure functions)
    /// sandbox.add_module("utils", Script::from_content(
    ///     "export function greet(name) { return `hello ${name}`; }"
    /// ))?;
    /// // Handler can import it:
    /// // import { greet } from 'user:utils';
    ///
    /// // Register a module with mutable shared state
    /// sandbox.add_module("counter", Script::from_content(
    ///     "let count = 0;\nexport function increment() { return ++count; }\nexport function getCount() { return count; }"
    /// ))?;
    /// // Multiple handlers import the same module and share its state:
    /// // Handler A: import { increment } from 'user:counter';  → mutates count
    /// // Handler B: import { getCount } from 'user:counter';   → reads count
    /// ```
    #[instrument(err(Debug), skip(self, script), level=Level::DEBUG)]
    pub fn add_module<N: Into<String> + std::fmt::Debug>(
        &mut self,
        module_name: N,
        script: Script,
    ) -> Result<()> {
        self.add_module_ns(module_name, script, DEFAULT_MODULE_NAMESPACE)
    }

    /// Adds a module to the sandbox with a custom namespace.
    ///
    /// The module will be available for import by handlers (and other modules)
    /// using `import { ... } from '<namespace>:<module_name>'`.
    ///
    /// Like [`JSSandbox::add_module`], modules are ES module singletons —
    /// mutable state is shared across all importing handlers. See
    /// [`JSSandbox::add_module`] for details on state sharing and lifecycle.
    ///
    /// # Namespace restrictions
    ///
    /// - Must not be empty
    /// - Must not contain `':'`
    /// - Must not be a reserved namespace (e.g. `"host"`)
    ///
    /// # Example
    ///
    /// ```text
    /// sandbox.add_module_ns("math", script, "mylib")?;
    /// // Handler imports: import { add } from 'mylib:math';
    /// ```
    #[instrument(err(Debug), skip(self, script), level=Level::DEBUG)]
    pub fn add_module_ns<N, NS>(
        &mut self,
        module_name: N,
        script: Script,
        namespace: NS,
    ) -> Result<()>
    where
        N: Into<String> + std::fmt::Debug,
        NS: Into<String> + std::fmt::Debug,
    {
        let module_name = module_name.into();
        let namespace = namespace.into();

        validate_module_identifier(&module_name, "Module name")?;
        validate_module_identifier(&namespace, "Module namespace")?;
        validate_namespace_not_reserved(&namespace)?;

        let qualified_name = format!("{}:{}", namespace, module_name);
        if self.modules.contains_key(&qualified_name) {
            return Err(new_error!("Module already exists: {}", qualified_name));
        }

        self.modules.insert(qualified_name, script);
        Ok(())
    }

    /// Removes a module from the sandbox (using the default namespace).
    #[instrument(err(Debug), skip(self), level=Level::DEBUG)]
    pub fn remove_module(&mut self, module_name: &str) -> Result<()> {
        self.remove_module_ns(module_name, DEFAULT_MODULE_NAMESPACE)
    }

    /// Removes a module from the sandbox (using a custom namespace).
    #[instrument(err(Debug), skip(self), level=Level::DEBUG)]
    pub fn remove_module_ns(&mut self, module_name: &str, namespace: &str) -> Result<()> {
        validate_module_identifier(module_name, "Module name")?;
        validate_module_identifier(namespace, "Module namespace")?;
        validate_namespace_not_reserved(namespace)?;
        let qualified_name = format!("{namespace}:{module_name}");
        match self.modules.remove(&qualified_name) {
            Some(_) => Ok(()),
            None => Err(new_error!("Module does not exist: {}", qualified_name)),
        }
    }

    /// Clears all modules from the sandbox.
    #[instrument(skip_all, level=Level::TRACE)]
    pub fn clear_modules(&mut self) {
        self.modules.clear();
    }

    /// Returns whether the sandbox is currently poisoned.
    #[deprecated(since = "0.4.0", note = "use status().is_poisoned()")]
    pub fn poisoned(&self) -> bool {
        self.inner.status().is_poisoned()
    }

    /// Returns the sandbox lifecycle status.
    pub fn status(&self) -> SandboxStatus {
        self.inner.status()
    }

    #[cfg(test)]
    fn get_number_of_handlers(&self) -> usize {
        self.handlers.len()
    }

    #[cfg(test)]
    fn get_number_of_modules(&self) -> usize {
        self.modules.len()
    }

    /// Creates a new `LoadedJSSandbox` with the handlers that have been added to this `JSSandbox`.
    ///
    /// # Partial failure
    ///
    /// This method consumes `self`. If module registration succeeds but a handler
    /// fails to register, the `JSSandbox` is lost and the caller receives an error.
    /// To recover, create a new sandbox via `SandboxBuilder`. This is consistent with
    /// the existing handler-only behaviour and the one-shot consumption pattern.
    #[instrument(err(Debug), skip_all, level=Level::TRACE)]
    pub fn get_loaded_sandbox(mut self) -> Result<LoadedJSSandbox> {
        if self.handlers.is_empty() {
            return Err(new_error!("No handlers have been added to the sandbox"));
        }

        let metadata = Arc::new(SnapshotMetadata::new(
            SnapshotKind::LoadedJsSandbox,
            std::iter::empty(),
            std::iter::empty(),
            self.host_functions.clone(),
        ));

        // Publish the builder-selected host functions before compiling modules
        // and handlers that may import them.
        register_host_function_manifest(&mut self.inner, &self.host_functions)?;

        // Register user modules first so that handlers can import them.
        // NOTE: HashMap iteration order is non-deterministic, but this is safe
        // because modules are lazily compiled by the UserModuleLoader when first
        // imported — registration order does not affect resolution.
        for (qualified_name, script) in std::mem::take(&mut self.modules) {
            let content = script.content().to_owned();
            self.inner
                .call::<()>("register_module", (qualified_name, content))?;
        }

        for (function_name, script) in std::mem::take(&mut self.handlers) {
            let content = script.content().to_owned();

            let path = script
                .base_path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            self.inner
                .call::<()>("register_handler", (function_name, content, path))?;
        }

        LoadedJSSandbox::new(self.inner, self.snapshot, metadata, self.host_functions)
    }
    /// Generate a crash dump of the current state of the VM underlying this sandbox.
    ///
    /// Creates an ELF core dump file that can be used for debugging. The dump
    /// captures the current state of the sandbox including registers, memory regions,
    /// and other execution context.
    ///
    /// The location of the core dump file is determined by the `HYPERLIGHT_CORE_DUMP_DIR`
    /// environment variable. If not set, it defaults to the system's temporary directory.
    ///
    /// This is only available when the `crashdump` feature is enabled and then only if the sandbox
    /// is also configured to allow core dumps (which is the default behavior).
    ///
    /// hyperlight-host only implements crash dumps on x86_64, so this method is not compiled on
    /// other architectures (for example aarch64 macOS or Linux).
    ///
    ///
    /// This can be useful for generating a crash dump from gdb when trying to debug issues in the
    /// guest that dont cause crashes (e.g. a guest function that does not return)
    ///
    /// # Examples
    ///
    /// Attach to your running process with gdb and call this function:
    ///
    /// ```shell
    /// sudo gdb -p <pid_of_your_process>
    /// (gdb) info threads
    /// # find the thread that is running the guest function you want to debug
    /// (gdb) thread <thread_number>
    /// # switch to the frame where you have access to your MultiUseSandbox instance
    /// (gdb) backtrace
    /// (gdb) frame <frame_number>
    /// # get the pointer to your MultiUseSandbox instance
    /// # Get the sandbox pointer
    /// (gdb) print sandbox
    /// # Call the crashdump function
    /// call sandbox.generate_crashdump()
    /// ```
    /// The crashdump should be available in crash dump directory (see `HYPERLIGHT_CORE_DUMP_DIR` env var).
    ///
    #[cfg(crashdump)]
    pub fn generate_crashdump(&mut self) -> Result<()> {
        self.inner.generate_crashdump()
    }
}

impl Debug for JSSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSSandbox")
            .field("handlers", &self.handlers)
            .field("modules", &self.modules)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SandboxBuilder;

    #[test]
    fn test_add_handler() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox.add_handler("handler1", "script1".into()).unwrap();
        sandbox.add_handler("handler2", "script2".into()).unwrap();

        assert_eq!(sandbox.get_number_of_handlers(), 2);
    }

    #[test]
    fn test_remove_handler() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox.add_handler("handler1", "script1".into()).unwrap();
        sandbox.add_handler("handler2", "script2".into()).unwrap();

        sandbox.remove_handler("handler1").unwrap();

        assert_eq!(sandbox.get_number_of_handlers(), 1);
    }

    #[test]
    fn test_clear_handlers() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox.add_handler("handler1", "script1".into()).unwrap();
        sandbox.add_handler("handler2", "script2".into()).unwrap();

        sandbox.clear_handlers();

        assert_eq!(sandbox.get_number_of_handlers(), 0);
    }

    #[test]
    fn test_get_loaded_sandbox() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox
            .add_handler(
                "handler1",
                Script::from_content(
                    r#"function handler(event) {
                    event.request.uri = "/redirected.html";
                    return event
                }"#,
                ),
            )
            .unwrap();

        let res = sandbox.get_loaded_sandbox();
        assert!(res.is_ok());
    }

    // ── Auto-export heuristic tests (issue #39) ──────────────────────────
    // The auto-export logic must only detect actual ES export statements,
    // not the word "export" inside string literals, comments, or identifiers.

    #[test]
    fn handler_with_export_in_string_literal() {
        // "export" appears inside a string — auto-export should still fire
        let handler = Script::from_content(
            r#"
        function handler(event) {
            const xml = '<config mode="export">value</config>';
            return { result: xml };
        }
        "#,
        );

        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();
        sandbox.add_handler("handler", handler).unwrap();
        let mut loaded = sandbox.get_loaded_sandbox().unwrap();

        let res = loaded
            .handle_event("handler", "{}".to_string(), None)
            .unwrap();
        assert_eq!(
            res,
            r#"{"result":"<config mode=\"export\">value</config>"}"#
        );
    }

    #[test]
    fn handler_with_export_in_comment() {
        // "export" appears in a comment — auto-export should still fire
        let handler = Script::from_content(
            r#"
        function handler(event) {
            // TODO: export this data to CSV
            return { result: 42 };
        }
        "#,
        );

        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();
        sandbox.add_handler("handler", handler).unwrap();
        let mut loaded = sandbox.get_loaded_sandbox().unwrap();

        let res = loaded
            .handle_event("handler", "{}".to_string(), None)
            .unwrap();
        assert_eq!(res, r#"{"result":42}"#);
    }

    #[test]
    fn handler_with_export_in_identifier() {
        // "export" is part of an identifier — auto-export should still fire
        let handler = Script::from_content(
            r#"
        function handler(event) {
            const exportPath = "/tmp/out.csv";
            return { result: exportPath };
        }
        "#,
        );

        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();
        sandbox.add_handler("handler", handler).unwrap();
        let mut loaded = sandbox.get_loaded_sandbox().unwrap();

        let res = loaded
            .handle_event("handler", "{}".to_string(), None)
            .unwrap();
        assert_eq!(res, r#"{"result":"/tmp/out.csv"}"#);
    }

    #[test]
    fn handler_with_explicit_export_is_not_doubled() {
        // Script already has an export statement — auto-export should be skipped
        let handler = Script::from_content(
            r#"
        function handler(event) {
            return { result: "explicit" };
        }
        export { handler };
        "#,
        );

        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();
        sandbox.add_handler("handler", handler).unwrap();
        let mut loaded = sandbox.get_loaded_sandbox().unwrap();

        let res = loaded
            .handle_event("handler", "{}".to_string(), None)
            .unwrap();
        assert_eq!(res, r#"{"result":"explicit"}"#);
    }

    #[test]
    fn handler_with_export_default_function() {
        // `export function` — auto-export should be skipped
        let handler = Script::from_content(
            r#"
        export function handler(event) {
            return { result: "inline-export" };
        }
        "#,
        );

        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();
        sandbox.add_handler("handler", handler).unwrap();
        let mut loaded = sandbox.get_loaded_sandbox().unwrap();

        let res = loaded
            .handle_event("handler", "{}".to_string(), None)
            .unwrap();
        assert_eq!(res, r#"{"result":"inline-export"}"#);
    }

    // ── Module unit tests ────────────────────────────────────────────

    #[test]
    fn test_add_module() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module("utils", Script::from_content("export const x = 1;"))
            .unwrap();
        sandbox
            .add_module("helpers", Script::from_content("export const y = 2;"))
            .unwrap();

        assert_eq!(sandbox.get_number_of_modules(), 2);
    }

    #[test]
    fn test_add_module_with_custom_namespace() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module_ns(
                "math",
                Script::from_content("export const PI = 3.14;"),
                "mylib",
            )
            .unwrap();

        assert_eq!(sandbox.get_number_of_modules(), 1);
    }

    #[test]
    fn test_add_module_rejects_empty_name() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        let res = sandbox.add_module("", Script::from_content("export const x = 1;"));
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("must not be empty"));
    }

    #[test]
    fn test_add_module_rejects_empty_namespace() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        let res = sandbox.add_module_ns("utils", Script::from_content("export const x = 1;"), "");
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("must not be empty"));
    }

    #[test]
    fn test_add_module_rejects_colon_in_name() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        let res = sandbox.add_module("bad:name", Script::from_content("export const x = 1;"));
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("must not contain ':'"));
    }

    #[test]
    fn test_add_module_rejects_colon_in_namespace() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        let res = sandbox.add_module_ns(
            "utils",
            Script::from_content("export const x = 1;"),
            "bad:ns",
        );
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("must not contain ':'"));
    }

    #[test]
    fn test_add_module_rejects_reserved_namespace() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        let res =
            sandbox.add_module_ns("utils", Script::from_content("export const x = 1;"), "host");
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("reserved"));
    }

    #[test]
    fn test_add_module_rejects_duplicate() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module("utils", Script::from_content("export const x = 1;"))
            .unwrap();
        let res = sandbox.add_module("utils", Script::from_content("export const y = 2;"));
        assert!(res.is_err());
        assert!(format!("{}", res.unwrap_err()).contains("already exists"));
    }

    #[test]
    fn test_remove_module() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module("utils", Script::from_content("export const x = 1;"))
            .unwrap();
        sandbox.remove_module("utils").unwrap();

        assert_eq!(sandbox.get_number_of_modules(), 0);
    }

    #[test]
    fn test_remove_module_with_custom_namespace() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module_ns(
                "math",
                Script::from_content("export const PI = 3.14;"),
                "mylib",
            )
            .unwrap();
        sandbox.remove_module_ns("math", "mylib").unwrap();

        assert_eq!(sandbox.get_number_of_modules(), 0);
    }

    #[test]
    fn test_clear_modules() {
        let proto = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto.load_runtime().unwrap();

        sandbox
            .add_module("a", Script::from_content("export const x = 1;"))
            .unwrap();
        sandbox
            .add_module("b", Script::from_content("export const y = 2;"))
            .unwrap();

        sandbox.clear_modules();

        assert_eq!(sandbox.get_number_of_modules(), 0);
    }
}
