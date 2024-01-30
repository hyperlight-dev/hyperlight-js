//! Shared Tokio runtime for execution monitor orchestration.
//!
//! This module provides a lazily-initialized, shared Tokio runtime used by
//! the monitor orchestration layer in [`handle_event_with_monitor`](crate::LoadedJSSandbox::handle_event_with_monitor)
//! to spawn monitor futures. Using a shared runtime avoids spawning new OS
//! threads for each monitored handler invocation.
//!
//! # Configuration
//!
//! The runtime thread count can be configured via the `HYPERLIGHT_MONITOR_THREADS`
//! environment variable. This must be set before the first monitor is used.
//!
//! ```bash
//! # Set to 4 worker threads (default is 2)
//! export HYPERLIGHT_MONITOR_THREADS=4
//! ```
//!
//! # Internal Details
//!
//! Custom [`ExecutionMonitor`](super::ExecutionMonitor) implementations do **not**
//! need to interact with this runtime directly. The orchestration layer in
//! `handle_event_with_monitor` handles spawning the monitor future and aborting
//! it when the handler completes. Custom monitors simply return a `Future` from
//! their `get_monitor()` method.

use std::sync::LazyLock;

use tokio::runtime::Runtime;

/// Environment variable to configure the number of monitor runtime worker threads.
pub(crate) const ENV_MONITOR_THREADS: &str = "HYPERLIGHT_MONITOR_THREADS";

/// Default number of worker threads for the monitor runtime.
/// Two threads allows for concurrent wall-clock and CPU time monitoring.
const DEFAULT_MONITOR_RUNTIME_WORKERS: usize = 2;

/// Shared Tokio runtime for all execution monitors.
///
/// Lazily initialized on first access. If runtime creation fails (e.g. under
/// resource exhaustion), the `None` is cached permanently — no retry mechanism,
/// by design, to avoid retry storms.
static MONITOR_RUNTIME: LazyLock<Option<Runtime>> = LazyLock::new(|| {
    let workers = std::env::var(ENV_MONITOR_THREADS)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MONITOR_RUNTIME_WORKERS);

    match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("hl-exec-monitor")
        .enable_time()
        .build()
    {
        Ok(rt) => {
            tracing::debug!(workers, "Initialized monitor runtime");
            Some(rt)
        }
        Err(e) => {
            tracing::error!(
                "Failed to create execution monitor runtime: {}. Monitors will be unavailable.",
                e
            );
            None
        }
    }
});

/// Get the shared monitor runtime.
///
/// The runtime is lazily initialized on first access. Thread count is determined by:
/// 1. The `HYPERLIGHT_MONITOR_THREADS` environment variable (if set and valid)
/// 2. Default of 2 threads otherwise
///
/// Returns `None` if runtime creation fails.
///
/// # Example
///
/// ```text
/// if let Some(runtime) = get_monitor_runtime() {
///     runtime.spawn(async {
///         // Async monitoring task
///     });
/// }
/// ```
pub(crate) fn get_monitor_runtime() -> Option<&'static Runtime> {
    MONITOR_RUNTIME.as_ref()
}
