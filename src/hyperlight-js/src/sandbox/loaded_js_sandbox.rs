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
use std::fmt::Debug;
use std::sync::Arc;

use hyperlight_host::hypervisor::InterruptHandle;
use hyperlight_host::sandbox::snapshot::Snapshot;
use hyperlight_host::HyperlightError::{self, JsonConversionFailure};
use hyperlight_host::{MultiUseSandbox, Result};
use tokio::task::JoinHandle;
use tracing::{instrument, Level};

#[cfg(feature = "guest-call-stats")]
use super::execution_stats::ExecutionStats;
use super::js_sandbox::JSSandbox;
use super::metrics::{METRIC_SANDBOX_LOADS, METRIC_SANDBOX_UNLOADS};
use super::monitor::runtime::get_monitor_runtime;
use super::monitor::MonitorSet;
#[cfg(feature = "function_call_metrics")]
use crate::sandbox::metrics::EventHandlerMetricGuard;
use crate::sandbox::metrics::SandboxMetricsGuard;

/// A Hyperlight Sandbox with a JavaScript run time loaded and guest JavaScript handlers loaded.
pub struct LoadedJSSandbox {
    inner: MultiUseSandbox,
    // Snapshot of state before the sandbox was loaded and before any handlers were added.
    // This is used to restore state back to a JSSandbox.
    snapshot: Arc<Snapshot>,
    // metric drop guard to manage sandbox metric
    _metric_guard: SandboxMetricsGuard<LoadedJSSandbox>,
    // Stats from the most recent handle_event / handle_event_with_monitor call.
    // None before any call has been made.
    #[cfg(feature = "guest-call-stats")]
    last_call_stats: Option<ExecutionStats>,
}

/// RAII guard that aborts a spawned monitor task on drop.
///
/// Wraps a tokio `JoinHandle` to ensure the monitor task is cancelled when
/// the guard goes out of scope — whether that's after normal completion or
/// on early return. Keeps the spawn-abort lifecycle in one place rather than
/// requiring manual `abort()` calls at each exit point.
struct MonitorTask(JoinHandle<()>);

impl Drop for MonitorTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl LoadedJSSandbox {
    #[instrument(err(Debug), skip_all, level=Level::INFO)]
    pub(super) fn new(inner: MultiUseSandbox, snapshot: Arc<Snapshot>) -> Result<LoadedJSSandbox> {
        metrics::counter!(METRIC_SANDBOX_LOADS).increment(1);
        Ok(LoadedJSSandbox {
            inner,
            snapshot,
            _metric_guard: SandboxMetricsGuard::new(),
            #[cfg(feature = "guest-call-stats")]
            last_call_stats: None,
        })
    }

    /// Handles an event by calling the specified function with the event data.
    #[instrument(err(Debug), skip(self, event, gc), level=Level::INFO)]
    pub fn handle_event<F>(
        &mut self,
        func_name: F,
        event: String,
        gc: Option<bool>,
    ) -> Result<String>
    where
        F: Into<String> + std::fmt::Debug,
    {
        // check that this string is a valid JSON

        let _json_val: serde_json::Value =
            serde_json::from_str(&event).map_err(JsonConversionFailure)?;

        let should_gc = gc.unwrap_or(true);
        let func_name = func_name.into();
        if func_name.is_empty() {
            return Err(HyperlightError::Error(
                "Handler name must not be empty".to_string(),
            ));
        }

        #[cfg(feature = "function_call_metrics")]
        let _metric_guard = EventHandlerMetricGuard::new(&func_name, should_gc);

        // --- guest-call-stats: capture timing before the call ---
        #[cfg(feature = "guest-call-stats")]
        let wall_start = std::time::Instant::now();

        #[cfg(all(feature = "guest-call-stats", feature = "monitor-cpu-time"))]
        let cpu_start = super::monitor::cpu_time::ThreadCpuHandle::for_current_thread()
            .and_then(|h| h.elapsed().map(|t| (h, t)));

        let result = self.inner.call(&func_name, (event, should_gc));

        // --- guest-call-stats: record timing after the call ---
        // CPU time is read first so the wall-clock measurement fully wraps it.
        #[cfg(feature = "guest-call-stats")]
        {
            #[cfg(feature = "monitor-cpu-time")]
            let cpu_time = cpu_start.and_then(|(handle, start_ticks)| {
                handle.elapsed().map(|end_ticks| {
                    let delta_nanos =
                        handle.ticks_to_approx_nanos(end_ticks.saturating_sub(start_ticks));
                    std::time::Duration::from_nanos(delta_nanos)
                })
            });
            #[cfg(not(feature = "monitor-cpu-time"))]
            let cpu_time: Option<std::time::Duration> = None;

            let wall_clock = wall_start.elapsed();

            self.last_call_stats = Some(ExecutionStats {
                wall_clock,
                cpu_time,
                terminated_by: None,
            });
        }

        result
    }

    /// Returns the execution statistics from the most recent guest function call.
    ///
    /// Returns `None` before any call has been made. After each `handle_event` or
    /// `handle_event_with_monitor` call, this returns the timing and termination
    /// information from that call — stats are **not** cumulative.
    ///
    /// Stats are captured even when the call returns an error (e.g. the sandbox
    /// was poisoned by a monitor timeout).
    ///
    /// # Example
    ///
    /// ```text
    /// let _ = loaded.handle_event("handler", event, None);
    /// if let Some(stats) = loaded.last_call_stats() {
    ///     println!("Wall clock: {:?}", stats.wall_clock);
    ///     println!("CPU time: {:?}", stats.cpu_time);
    ///     println!("Terminated by: {:?}", stats.terminated_by);
    /// }
    /// ```
    #[cfg(feature = "guest-call-stats")]
    pub fn last_call_stats(&self) -> Option<&ExecutionStats> {
        self.last_call_stats.as_ref()
    }

    /// Unloads the Handlers from the sandbox and returns a `JSSandbox` with the JavaScript runtime loaded.
    #[instrument(err(Debug), skip_all, level=Level::DEBUG)]
    pub fn unload(self) -> Result<JSSandbox> {
        JSSandbox::from_loaded(self.inner, self.snapshot).inspect(|_| {
            metrics::counter!(METRIC_SANDBOX_UNLOADS).increment(1);
        })
    }

    /// Take a snapshot of the the current state of the sandbox.
    /// This can be used to restore the state of the sandbox later.
    #[instrument(err(Debug), skip_all, level=Level::DEBUG)]
    pub fn snapshot(&mut self) -> Result<Arc<Snapshot>> {
        self.inner.snapshot()
    }

    /// Restore the state of the sandbox to a previous snapshot.
    #[instrument(err(Debug), skip_all, level=Level::DEBUG)]
    pub fn restore(&mut self, snapshot: Arc<Snapshot>) -> Result<()> {
        self.inner.restore(snapshot)?;
        Ok(())
    }

    /// Get a handle to the interrupt handler for this sandbox,
    /// capable of interrupting guest execution.
    pub fn interrupt_handle(&self) -> Arc<dyn InterruptHandle> {
        self.inner.interrupt_handle()
    }

    /// Returns whether the sandbox is currently poisoned.
    ///
    /// A poisoned sandbox is in an inconsistent state due to the guest not running to completion.
    /// This can happen when guest execution is interrupted (e.g., via `InterruptHandle::kill()`),
    /// when the guest panics, or when memory violations occur.
    ///
    /// When poisoned, most operations will fail with `PoisonedSandbox` error.
    /// Use `restore()` with a snapshot or `unload()` to recover from a poisoned state.
    pub fn poisoned(&self) -> bool {
        self.inner.poisoned()
    }

    /// Handles an event with execution monitoring.
    ///
    /// The monitor enforces execution limits (time, CPU usage, etc.) and will
    /// terminate execution if limits are exceeded. If terminated, the sandbox
    /// will be poisoned and an error is returned.
    ///
    /// # Fail-Closed Semantics
    ///
    /// If the monitor fails to initialize, the handler is **never executed**.
    /// Execution cannot proceed unmonitored.
    ///
    /// # Tuple Monitors (OR semantics)
    ///
    /// Pass a tuple of monitors to enforce multiple limits. The first monitor
    /// to fire terminates execution, and the winning monitor's name is logged:
    ///
    /// ```text
    /// let monitor = (
    ///     WallClockMonitor::new(Duration::from_secs(5))?,
    ///     CpuTimeMonitor::new(Duration::from_millis(500))?,
    /// );
    /// loaded.handle_event_with_monitor("handler", "{}".into(), &monitor, None)?;
    /// ```
    ///
    /// # Arguments
    ///
    /// * `func_name` - The name of the handler function to call.
    /// * `event` - JSON string payload to pass to the handler.
    /// * `monitor` - The execution monitor (or tuple of monitors) to enforce limits.
    ///   Tuples race all sub-monitors; the first to fire wins and its name is logged.
    /// * `gc` - Whether to run garbage collection after the call (defaults to `true` if `None`).
    ///
    /// # Returns
    ///
    /// The handler result string on success, or an error if execution failed
    /// or was terminated by the monitor. If terminated, the sandbox will be
    /// poisoned and subsequent calls will fail until restored or unloaded.
    ///
    /// # Example
    ///
    /// ```text
    /// use hyperlight_js::WallClockMonitor;
    /// use std::time::Duration;
    ///
    /// let monitor = WallClockMonitor::new(Duration::from_secs(5))?;
    /// let result = loaded.handle_event_with_monitor(
    ///     "handler",
    ///     "{}".to_string(),
    ///     &monitor,
    ///     None,
    /// )?;
    /// println!("Handler returned: {}", result);
    /// ```
    #[instrument(err(Debug), skip(self, event, monitor, gc), level=Level::INFO)]
    pub fn handle_event_with_monitor<F, M>(
        &mut self,
        func_name: F,
        event: String,
        monitor: &M,
        gc: Option<bool>,
    ) -> Result<String>
    where
        F: Into<String> + std::fmt::Debug,
        M: MonitorSet,
    {
        let func_name = func_name.into();
        if func_name.is_empty() {
            return Err(HyperlightError::Error(
                "Handler name must not be empty".to_string(),
            ));
        }
        let interrupt_handle = self.interrupt_handle();

        // Phase 1: Build the racing future on the calling thread.
        // to_race() calls each sub-monitor's get_monitor() here, where
        // monitors can capture thread-local state (e.g., CPU clock handles).
        // If any monitor fails to initialize, we fail closed — handler never runs.
        let racing_future = monitor.to_race().map_err(|e| {
            tracing::error!("Failed to initialize execution monitor: {}", e);
            HyperlightError::Error(format!("Execution monitor failed to start: {}", e))
        })?;

        // Phase 2: Spawn the racing future on the shared runtime.
        // When the first monitor fires, to_race() returns the winner's name.
        // We record the metric/log, store the winner name for stats, then
        // call kill() to terminate the guest.
        // kill() is safe to call even if the guest already finished — hyperlight's
        // InterruptHandle checks RUNNING_BIT and clear_cancel() at the start of
        // the next guest call clears any stale CANCEL_BIT.
        let runtime = get_monitor_runtime().ok_or_else(|| {
            tracing::error!("Monitor runtime is unavailable");
            HyperlightError::Error("Monitor runtime is unavailable".to_string())
        })?;

        // Shared slot for the winning monitor's name. The monitor task writes
        // the winner *before* calling kill(), and handle_event only returns
        // *after* kill takes effect, so the read after handle_event is safe.
        let terminated_by = Arc::new(std::sync::Mutex::new(None::<&'static str>));
        let terminated_by_writer = terminated_by.clone();

        let _monitor_task = MonitorTask(runtime.spawn(async move {
            let winner = racing_future.await;
            super::monitor::record_monitor_triggered(winner);
            // Store the winner name before kill — ordering guarantee:
            // handle_event returns only after kill() poisons the sandbox,
            // so the caller sees the write.
            if let Ok(mut guard) = terminated_by_writer.lock() {
                *guard = Some(winner);
            }
            interrupt_handle.kill();
        }));

        // Phase 3: Execute the handler (blocking). When this returns (success
        // or error), _monitor_task drops and aborts the spawned monitor task.
        let result = self.handle_event(&func_name, event, gc);

        // Phase 4: Patch terminated_by into the stats captured by handle_event.
        // If the monitor fired, the winner name was written before kill(), so
        // we can read it safely now.
        #[cfg(feature = "guest-call-stats")]
        if let Ok(guard) = terminated_by.lock()
            && let Some(winner) = *guard
            && let Some(stats) = &mut self.last_call_stats
        {
            stats.terminated_by = Some(winner);
        }

        result
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

impl Debug for LoadedJSSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedJSSandbox").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SandboxBuilder, Script};

    fn get_valid_handler() -> Script {
        Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }
        "#,
        )
    }

    fn get_valid_event() -> String {
        r#"
        {
            "request": {
                "uri": "/index.html"
            }
        }
        "#
        .to_string()
    }

    fn get_static_counter_handler() -> Script {
        Script::from_content(
            r#"
        let count = 0;
        function handler(event) {
            event.count = ++count;
            return event
        }
        "#,
        )
    }

    fn get_static_counter_event() -> String {
        r#"
        {
            "count": 0
        }
        "#
        .to_string()
    }

    fn get_loaded_sandbox() -> Result<LoadedJSSandbox> {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

        sandbox.add_handler("handler", get_valid_handler()).unwrap();

        sandbox.get_loaded_sandbox()
    }

    #[test]
    fn test_handle_event() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

        sandbox.add_handler("handler", get_valid_handler()).unwrap();

        let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
        let gc = Some(true);
        let result = loaded_js_sandbox.handle_event("handler".to_string(), get_valid_event(), gc);

        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_event_accumulates_state() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox
            .add_handler("handler", get_static_counter_handler())
            .unwrap();

        let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
        let gc = Some(true);
        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);

        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 1);

        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);
        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 2);
    }

    #[test]
    fn test_snapshot_and_restore() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

        sandbox
            .add_handler("handler", get_static_counter_handler())
            .unwrap();

        let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
        let gc = Some(true);

        let result = loaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap();

        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 1);

        // Take a snapshot after handling 1 event
        let snapshot = loaded_js_sandbox.snapshot().unwrap();

        // Handle 2 more events
        let result = loaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 2);

        let result = loaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 3);

        // Restore the snapshot
        loaded_js_sandbox.restore(snapshot.clone()).unwrap();

        // Handle the event again, should reset to initial state
        let result = loaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 2);

        // unload and reload, and restore
        let mut js_sandbox = loaded_js_sandbox.unload().unwrap();

        js_sandbox
            .add_handler("handler2", get_static_counter_handler())
            .unwrap();

        let mut reloaded_js_sandbox = js_sandbox.get_loaded_sandbox().unwrap();

        // handler2 should be available, not handler
        let result = reloaded_js_sandbox
            .handle_event("handler2", get_static_counter_event(), gc)
            .unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 1);

        reloaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap_err();

        // restore to snapshot before unload/reload
        reloaded_js_sandbox.restore(snapshot.clone()).unwrap();
        // handler should be available again
        let result = reloaded_js_sandbox
            .handle_event("handler", get_static_counter_event(), gc)
            .unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response_json["count"], 2);

        // but handler2 should not be available
        reloaded_js_sandbox
            .handle_event("handler2", get_static_counter_event(), gc)
            .unwrap_err();
    }

    #[test]
    fn test_add_handler_unload_and_reuse_resets_state() {
        let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
        let mut sandbox = proto_js_sandbox.load_runtime().unwrap();
        sandbox
            .add_handler("handler", get_static_counter_handler())
            .unwrap();
        let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
        let gc = Some(true);

        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);
        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 1);

        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);
        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 2);

        // Unload the sandbox
        let mut sandbox = loaded_js_sandbox.unload().unwrap();
        sandbox
            .add_handler("handler", get_static_counter_handler())
            .unwrap();
        // Add the handler again
        let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
        let gc = Some(true);

        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);
        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 1);

        let result = loaded_js_sandbox.handle_event("handler", get_static_counter_event(), gc);
        assert!(result.is_ok());
        let response = result.unwrap();
        let response_json: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response_json["count"], 2);
    }

    #[test]
    fn test_unload() {
        let sandbox = get_loaded_sandbox().unwrap();

        let result = sandbox.unload();

        assert!(result.is_ok());
    }

    use crate::sandbox::monitor::ExecutionMonitor;

    /// A mock monitor that always fails to initialize (returns Err).
    /// Used to test fail-closed behavior.
    struct FailingMonitor;

    impl ExecutionMonitor for FailingMonitor {
        fn get_monitor(
            &self,
        ) -> hyperlight_host::Result<impl std::future::Future<Output = ()> + Send + 'static>
        {
            Err::<std::future::Ready<()>, _>(hyperlight_host::HyperlightError::Error(
                "Simulated initialization failure".to_string(),
            ))
        }

        fn name(&self) -> &'static str {
            "failing-monitor"
        }
    }

    #[test]
    fn test_handle_event_with_monitor_fails_if_monitor_cannot_start() {
        let mut loaded = get_loaded_sandbox().unwrap();
        let monitor = FailingMonitor;

        // Should fail because monitor returns Err (fail closed, not open)
        let result = loaded.handle_event_with_monitor("handler", get_valid_event(), &monitor, None);

        assert!(result.is_err(), "Should fail when monitor can't start");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("failed to start"),
            "Error should mention monitor failure: {}",
            err
        );

        // Sandbox should NOT be poisoned - we never ran the handler
        assert!(
            !loaded.poisoned(),
            "Sandbox should not be poisoned when monitor fails to start"
        );
    }
}
