//! Execution monitoring for JavaScript sandbox handlers.
//!
//! This module provides the [`ExecutionMonitor`] trait and built-in implementations
//! for monitoring and terminating handler execution based on resource limits.
//!
//! # Architecture — Why two traits?
//!
//! The monitoring system has a subtle design tension:
//!
//! 1. **Users** want a simple trait to implement: `get_monitor()` + `name()`.
//! 2. **The orchestrator** needs to race multiple monitors, identifying which
//!    one fired (by name) for metrics and logging.
//! 3. **Tuples** of monitors (e.g. `(WallClockMonitor, CpuTimeMonitor)`) are
//!    a **composition** of monitors, not a single monitor — they shouldn't
//!    pretend to be one by implementing `ExecutionMonitor`.
//!
//! The solution: **separate concerns into two traits**.
//!
//! - [`ExecutionMonitor`] — User-facing. Only two methods: `get_monitor()` and
//!   `name()`. Simple, clean, no composition logic.
//! - [`MonitorSet`] — Internal (sealed). One method: [`to_race()`](MonitorSet::to_race).
//!   Produces a single racing future that completes when the first monitor
//!   fires, emitting metrics and logging the winner. Automatically derived
//!   for every `ExecutionMonitor` via a blanket impl, and for tuples of up
//!   to 5 monitors via `tokio::select!` in a macro.
//!
//! The orchestrator's `handle_event_with_monitor` is bounded by
//! `M: MonitorSet`, not `M: ExecutionMonitor`. Users never need to know
//! `MonitorSet` exists — it's sealed so they can't implement it directly,
//! and it's derived automatically via the blanket impl.
//!
//! # Built-in Monitors
//!
//! - [`WallClockMonitor`] - Terminates execution after a wall-clock timeout
//!   (requires `monitor-wall-clock` feature)
//! - [`CpuTimeMonitor`] - Terminates execution after a CPU time limit
//!   (requires `monitor-cpu-time` feature)
//!
//! # Usage
//!
//! ```text
//! use hyperlight_js::{WallClockMonitor, CpuTimeMonitor, ExecutionMonitor};
//! use std::time::Duration;
//!
//! // Single monitor — ExecutionMonitor auto-satisfies MonitorSet via blanket impl
//! let monitor = WallClockMonitor::new(Duration::from_secs(5))?;
//! let result = loaded_sandbox.handle_event_with_monitor(
//!     "handler",
//!     "{}".to_string(),
//!     &monitor,
//!     None,
//! )?;
//!
//! // Multiple monitors — tuples implement MonitorSet with OR semantics.
//! // The first monitor to trigger terminates execution, and the winning
//! // monitor's name is logged so you know exactly which limit was breached.
//! let wall = WallClockMonitor::new(Duration::from_secs(5))?;
//! let cpu = CpuTimeMonitor::new(Duration::from_millis(500))?;
//! let result = loaded_sandbox.handle_event_with_monitor(
//!     "handler",
//!     "{}".to_string(),
//!     &(wall, cpu),
//!     None,
//! )?;
//! ```
//!
//! # Custom Monitors
//!
//! Implement [`ExecutionMonitor`] to create custom monitoring logic:
//!
//! ```text
//! use hyperlight_js::ExecutionMonitor;
//! use hyperlight_host::Result;
//! use std::future::Future;
//!
//! struct MyMonitor { limit: std::time::Duration }
//!
//! impl ExecutionMonitor for MyMonitor {
//!     fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static> {
//!         let limit = self.limit;
//!         Ok(async move {
//!             hyperlight_js::monitor::sleep(limit).await;
//!             tracing::warn!("Custom limit exceeded");
//!         })
//!     }
//!
//!     fn name(&self) -> &'static str { "my-monitor" }
//! }
//! ```
//!
//! # Fail-Closed Semantics
//!
//! If any monitor fails to initialize (`get_monitor()` returns `Err`), the handler
//! is **never executed**. Execution cannot proceed unmonitored due to a monitor
//! initialization failure. This is a deliberate security-first design choice.
//!
//! # Using Wall-Clock and CPU Monitors Together
//!
//! Wall-clock and CPU monitors are designed to be used together as a tuple
//! `(WallClockMonitor, CpuTimeMonitor)` to provide comprehensive protection:
//!
//! - **`CpuTimeMonitor`** catches compute-bound abuse (crypto mining, tight loops)
//! - **`WallClockMonitor`** catches resource exhaustion where the guest consumes
//!   **host resources without consuming CPU** — e.g. blocking on host calls. A guest doing
//!   "nothing" in terms of CPU can still starve the host of resources (sometimes
//!   called a **resource exhaustion attack** or **slowloris-style denial of service**)
//!   Right now this is not really possible to do in Hyperlight since there is no way for
//!   the guest to block without consuming CPU, but we want to be prepared for when this is possible.
//!
//! Neither alone is sufficient: CPU-only misses idle resource holding; wall-clock-only
//! is unfair to legitimately I/O-heavy workloads.
//!
//! # Runtime Configuration
//!
//! The shared async runtime thread count can be configured via environment variable:
//!
//! ```bash
//! export HYPERLIGHT_MONITOR_THREADS=4  # Default is 2
//! ```
//!
//! See the `runtime` module for details on the shared runtime.

use std::future::Future;
use std::pin::Pin;

use hyperlight_host::Result;

use crate::sandbox::metrics::{METRIC_MONITOR_TERMINATIONS, METRIC_MONITOR_TYPE_LABEL};

/// Record that a monitor triggered execution termination.
///
/// Emits the `monitor_terminations_total` counter metric with the winning
/// monitor's name as the `monitor_type` label, and logs a warning.
fn record_monitor_triggered(triggered_by: &'static str) {
    metrics::counter!(
        METRIC_MONITOR_TERMINATIONS,
        METRIC_MONITOR_TYPE_LABEL => triggered_by
    )
    .increment(1);

    tracing::warn!("Monitor '{triggered_by}' fired — requesting execution termination");
}

/// A monitor that enforces execution limits on handler invocations.
///
/// Implementations watch handler execution and signal termination when limits
/// are exceeded (time limits, CPU usage, resource quotas, etc.).
///
/// This is the **only trait users need to implement**. The sealed [`MonitorSet`]
/// trait is automatically derived via a blanket impl. See the
/// [module docs](self) for the full architecture rationale.
///
/// # Why `fn` returning `impl Future` instead of `async fn`
///
/// The method body executes synchronously on the **calling thread** and returns
/// an opaque `Future` that will be spawned on the shared monitor runtime.
/// This two-phase design lets monitors capture thread-local state (e.g.,
/// [`CpuTimeMonitor`]'s `pthread_getcpuclockid`) before the future migrates
/// to a tokio worker thread.
///
/// # Contract
///
/// - **Method body** (sync): Runs on the calling thread. Capture thread-local
///   state here. Return `Err` to fail closed (handler never runs).
/// - **Returned future** (async): Will be spawned on the monitor runtime. Stays pending
///   while within limits. **Completes when execution should be terminated.**
///   Will be aborted if the handler finishes first.
///
/// # Example
///
/// ```text
/// use hyperlight_js::ExecutionMonitor;
/// use hyperlight_host::Result;
/// use std::future::Future;
///
/// struct TimeoutMonitor { timeout: std::time::Duration }
///
/// impl ExecutionMonitor for TimeoutMonitor {
///     fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static> {
///         let timeout = self.timeout;
///         Ok(async move {
///             hyperlight_js::monitor::sleep(timeout).await;
///             tracing::warn!("Timeout exceeded");
///         })
///     }
///
///     fn name(&self) -> &'static str { "timeout" }
/// }
/// ```
pub trait ExecutionMonitor: Send + Sync {
    /// Prepare and return a monitoring future for a single handler invocation.
    ///
    /// The method body runs synchronously on the calling thread — use it to
    /// capture thread-local state (e.g., CPU clock handles). The returned
    /// future will be spawned on the shared monitor runtime.
    ///
    /// The future should stay pending while execution is within limits and
    /// complete (return `()`) when execution should be terminated. It will
    /// be aborted if the handler finishes normally before the monitor fires.
    ///
    /// # Errors
    ///
    /// Return `Err` if the monitor cannot initialize (e.g., OS API failure).
    /// This will prevent the handler from executing (fail-closed semantics).
    fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static>;

    /// Human-readable name for logging and metrics.
    fn name(&self) -> &'static str;
}

// =============================================================================
// MonitorSet — sealed composition trait
// =============================================================================
// See module-level docs ("Architecture — Why two traits?") for the full rationale.
// In short: keeps ExecutionMonitor clean (two methods, no composition) while
// giving the orchestrator a single racing future with metrics baked in.

/// Prevents external crates from implementing [`MonitorSet`] directly.
///
/// Uses the [sealed trait pattern](https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed).
mod private {
    pub trait Sealed {}
}

/// A composable set of monitors that produces a single racing future.
///
/// This trait is **sealed** — you cannot implement it directly. It is
/// automatically derived for:
///
/// - Any type that implements [`ExecutionMonitor`] (wraps the single future)
/// - Tuples of up to 5 `ExecutionMonitor` implementors (races via `tokio::select!`)
///
/// The orchestration layer (`handle_event_with_monitor`) bounds on
/// `M: MonitorSet` and calls [`to_race()`](MonitorSet::to_race) to get
/// a single future that completes when the first monitor fires.
pub trait MonitorSet: private::Sealed + Send + Sync {
    /// Produce a single future that races all monitors in this set.
    ///
    /// Each sub-monitor's `get_monitor()` is called on the **calling thread**
    /// so monitors can capture thread-local state (e.g., CPU clock handles).
    /// The returned future completes when the first monitor fires, emitting
    /// the `monitor_terminations_total` metric and a warning log with the
    /// winning monitor's name.
    fn to_race(&self) -> Result<Pin<Box<dyn Future<Output = ()> + Send>>>;
}

// Every ExecutionMonitor is automatically a MonitorSet of one.
impl<M: ExecutionMonitor> private::Sealed for M {}

impl<M: ExecutionMonitor> MonitorSet for M {
    fn to_race(&self) -> Result<Pin<Box<dyn Future<Output = ()> + Send>>> {
        let future = self.get_monitor()?;
        let name = self.name();
        Ok(Box::pin(async move {
            future.await;
            record_monitor_triggered(name);
        }))
    }
}

// =============================================================================
// Tuple composition — OR semantics via tokio::select!
// =============================================================================

/// Generates a [`MonitorSet`] impl for a tuple of N `ExecutionMonitor`s.
///
/// Each sub-monitor's `get_monitor()` runs on the calling thread (preserving
/// thread-local state). The generated `to_race()` uses `tokio::select!` to
/// race all futures. The tuple is NOT an `ExecutionMonitor` — it's a composition that
/// satisfies `MonitorSet` directly.
macro_rules! impl_monitor_set_tuple {
    (($($p:ident: $P:ident),+)) => {
        impl<$($P: ExecutionMonitor),+> private::Sealed for ($($P,)+) {}

        impl<$($P: ExecutionMonitor),+> MonitorSet for ($($P,)+) {
            fn to_race(&self) -> Result<Pin<Box<dyn Future<Output = ()> + Send>>> {
                let ($($p,)+) = &self;
                // Each get_monitor() runs here on the calling thread,
                // preserving thread-local state (e.g. CPU clock handles).
                $(let $p = ($p.get_monitor()?, $p.name());)+

                Ok(Box::pin(async move {
                    // Race all monitors — first to complete wins.
                    let winner = tokio::select! {
                        $(_ = $p.0 => $p.1,)+
                    };
                    record_monitor_triggered(winner);
                }))
            }
        }
    };
}

// 1-tuple: not strictly necessary (bare `M: ExecutionMonitor` satisfies
// `MonitorSet` via the blanket impl), but a user might write `(monitor,)`
// and expect it to compile. No conflict with the blanket — `(T,)` and `T`
// are distinct types in Rust.
impl_monitor_set_tuple!((m0: M0));
impl_monitor_set_tuple!((m0: M0, m1: M1));
impl_monitor_set_tuple!((m0: M0, m1: M1, m2: M2));
impl_monitor_set_tuple!((m0: M0, m1: M1, m2: M2, m3: M3));
impl_monitor_set_tuple!((m0: M0, m1: M1, m2: M2, m3: M3, m4: M4));

// Feature-gated monitor implementations
#[cfg(feature = "monitor-wall-clock")]
mod wall_clock;
#[cfg(feature = "monitor-wall-clock")]
pub use wall_clock::WallClockMonitor;

#[cfg(feature = "monitor-cpu-time")]
mod cpu_time;
#[cfg(feature = "monitor-cpu-time")]
pub use cpu_time::CpuTimeMonitor;

// Shared runtime for monitor orchestration
pub(crate) mod runtime;

/// Async sleep function used by monitors.
///
/// Re-exported here so that custom monitor implementations don't couple
/// directly to `tokio`.  If the underlying async runtime changes in a
/// future release, only this re-export needs updating — downstream
/// monitors remain source-compatible.
pub use tokio::time::sleep;
