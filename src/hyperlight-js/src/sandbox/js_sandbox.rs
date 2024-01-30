use std::collections::HashMap;
use std::fmt::Debug;

use hyperlight_host::sandbox::snapshot::Snapshot;
use hyperlight_host::{new_error, MultiUseSandbox, Result};
use tracing::{instrument, Level};

use super::loaded_js_sandbox::LoadedJSSandbox;
use crate::sandbox::metrics::SandboxMetricsGuard;
use crate::Script;

/// A Hyperlight Sandbox with a JavaScript run time loaded but no guest code.
pub struct JSSandbox {
    pub(super) inner: MultiUseSandbox,
    handlers: HashMap<String, Script>,
    // Snapshot of state before any handlers are added.
    // This is used to restore state back to a neutral JSSandbox.
    snapshot: Snapshot,
    // metric drop guard to manage sandbox metric
    _metric_guard: SandboxMetricsGuard<JSSandbox>,
}

impl JSSandbox {
    #[instrument(err(Debug), skip(inner), level=Level::INFO)]
    pub(super) fn new(mut inner: MultiUseSandbox) -> Result<Self> {
        let snapshot = inner.snapshot()?;
        Ok(Self {
            inner,
            handlers: HashMap::new(),
            snapshot,
            _metric_guard: SandboxMetricsGuard::new(),
        })
    }

    /// Creates a new `JSSandbox` from a `MultiUseSandbox` and a `Snapshot` of state before any handlers were added.
    pub(crate) fn from_loaded(mut loaded: MultiUseSandbox, snapshot: Snapshot) -> Result<Self> {
        loaded.restore(&snapshot)?;
        Ok(Self {
            inner: loaded,
            handlers: HashMap::new(),
            snapshot,
            _metric_guard: SandboxMetricsGuard::new(),
        })
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

    /// Returns whether the sandbox is currently poisoned.
    ///
    /// A poisoned sandbox is in an inconsistent state due to the guest not running to completion.
    /// This can happen when guest execution is interrupted (e.g., via `InterruptHandle::kill()`),
    /// when the guest panics, or when memory violations occur.
    ///
    pub fn poisoned(&self) -> bool {
        self.inner.poisoned()
    }

    #[cfg(test)]
    fn get_number_of_handlers(&self) -> usize {
        self.handlers.len()
    }

    /// Creates a new `LoadedJSSandbox` with the handlers that have been added to this `JSSandbox`.
    #[instrument(err(Debug), skip_all, level=Level::TRACE)]
    pub fn get_loaded_sandbox(mut self) -> Result<LoadedJSSandbox> {
        if self.handlers.is_empty() {
            return Err(new_error!("No handlers have been added to the sandbox"));
        }

        let handlers = self.handlers.clone();
        for (function_name, script) in handlers {
            let content = script.content().to_owned();

            let path = script
                .base_path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            self.inner
                .call::<()>("register_handler", (function_name, content, path))?;
        }

        LoadedJSSandbox::new(self.inner, self.snapshot)
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
    #[cfg(feature = "crashdump")]
    pub fn generate_crashdump(&self) -> Result<()> {
        self.inner.generate_crashdump()
    }
}

impl Debug for JSSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSSandbox")
            .field("handlers", &self.handlers)
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
}
