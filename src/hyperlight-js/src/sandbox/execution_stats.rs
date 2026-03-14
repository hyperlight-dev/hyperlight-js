/*
Copyright 2026 The Hyperlight Authors.

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
//! Execution statistics captured during guest function calls.
//!
//! When the `guest-call-stats` feature is enabled, every call to
//! [`handle_event`](super::loaded_js_sandbox::LoadedJSSandbox::handle_event) or
//! [`handle_event_with_monitor`](super::loaded_js_sandbox::LoadedJSSandbox::handle_event_with_monitor)
//! stores an [`ExecutionStats`] on the sandbox, retrievable via
//! [`last_call_stats()`](super::loaded_js_sandbox::LoadedJSSandbox::last_call_stats).
//!
//! # What's measured
//!
//! | Field | When populated |
//! |---|---|
//! | `wall_clock` | Always (when feature is on) |
//! | `cpu_time` | Only when `monitor-cpu-time` feature is also enabled |
//! | `terminated_by` | Only when a monitor killed the call |
//!
//! # Example
//!
//! ```text
//! let result = loaded.handle_event("handler", event, None);
//! if let Some(stats) = loaded.last_call_stats() {
//!     println!("Wall clock: {:?}", stats.wall_clock);
//!     if let Some(cpu) = stats.cpu_time {
//!         println!("CPU time: {:?}", cpu);
//!     }
//!     if let Some(monitor) = stats.terminated_by {
//!         println!("Terminated by: {}", monitor);
//!     }
//! }
//! ```

use std::time::Duration;

/// Statistics from the most recent guest function call.
///
/// Retrieved via
/// [`LoadedJSSandbox::last_call_stats()`](super::loaded_js_sandbox::LoadedJSSandbox::last_call_stats).
///
/// Stats are captured even when the call returns an error (e.g. monitor
/// termination, guest abort). They are overwritten on each subsequent call
/// — they are **not** cumulative.
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Wall-clock (elapsed) time for the guest call.
    ///
    /// Measured with `std::time::Instant` — always available when the
    /// `guest-call-stats` feature is enabled.
    pub wall_clock: Duration,

    /// CPU time consumed by the guest call.
    ///
    /// Only populated when the `monitor-cpu-time` feature is also enabled.
    ///
    /// `None` if `monitor-cpu-time` is not enabled, or if the CPU time
    /// handle could not be obtained for the calling thread.
    pub cpu_time: Option<Duration>,

    /// Name of the monitor that terminated execution, if any.
    ///
    /// `Some("wall-clock")` or `Some("cpu-time")` when a built-in monitor
    /// killed the call. `Some(<name>)` for custom monitors. `None` when
    /// the call completed (or failed) without monitor intervention.
    pub terminated_by: Option<&'static str>,
}
