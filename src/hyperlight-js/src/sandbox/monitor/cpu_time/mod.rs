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
//! CPU time based execution monitor.
//!
//! This module provides monitoring based on actual CPU execution time rather than
//! wall-clock time, making it more accurate for billing and resistant to time-wasting
//! attacks that use sleep/blocking calls.
//!
//! For comprehensive protection, combine with [`WallClockMonitor`] via a tuple to
//! catch both compute-bound abuse and resource exhaustion:
//!
//! ```text
//! let monitor = (
//!     WallClockMonitor::new(Duration::from_secs(5))?,
//!     CpuTimeMonitor::new(Duration::from_millis(500))?,
//! );
//! ```

use std::future::Future;
use std::time::Duration;

use hyperlight_host::{HyperlightError, Result};

use super::ExecutionMonitor;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::ThreadCpuHandle;
#[cfg(target_os = "macos")]
pub(crate) use macos::ThreadCpuHandle;
#[cfg(target_os = "windows")]
pub(crate) use windows::ThreadCpuHandle;

/// Monitors handler execution using CPU time.
///
/// Terminates execution if the handler consumes more CPU time than the configured limit.
/// This measures actual computation time, not time spent blocked or waiting.
///
/// # Combining with Wall-Clock Monitoring
///
/// `CpuTimeMonitor` only catches compute-bound abuse. To also catch resource exhaustion
/// (where a guest holds host resources without burning CPU), combine with
/// [`WallClockMonitor`] as a tuple:
///
/// ```text
/// let monitor = (
///     WallClockMonitor::new(Duration::from_secs(5))?,
///     CpuTimeMonitor::new(Duration::from_millis(500))?,
/// );
/// ```
///
/// The tuple races both monitors — whichever fires first terminates execution,
/// and the winning monitor's name is logged.
///
/// # Platform Support
///
/// - **Linux**: Uses `pthread_getcpuclockid` and `clock_gettime` (nanosecond precision)
/// - **macOS**: Uses a mach thread port and `thread_info(THREAD_BASIC_INFO)`, summing
///   user + system time (microsecond precision).
/// - **Windows**: Uses `QueryThreadCycleTime` (reference cycles at CPU base frequency).
///   The timeout is converted to a cycle budget once at setup using the CPU's nominal
///   frequency from the Windows registry (`HKLM\...\CentralProcessor\0\~MHz`).
///   Monitoring compares raw cycle counts directly.
///   Accuracy depends on invariant TSC support but should be good on modern CPUs.
///
/// # Example
///
/// ```text
/// use hyperlight_js::CpuTimeMonitor;
/// use std::time::Duration;
///
/// let monitor = CpuTimeMonitor::new(Duration::from_millis(100))?;
/// let result = sandbox.handle_event_with_monitor("handler", "{}".to_string(), &monitor, None)?;
/// ```
#[derive(Debug, Clone)]
pub struct CpuTimeMonitor {
    cpu_timeout: Duration,
}

impl CpuTimeMonitor {
    /// Create a new CPU time monitor.
    ///
    /// # Arguments
    ///
    /// * `cpu_timeout` - Maximum CPU time allowed for execution.
    ///
    /// # Errors
    ///
    /// Returns an error if `cpu_timeout` is zero.
    pub fn new(cpu_timeout: Duration) -> Result<Self> {
        if cpu_timeout.is_zero() {
            return Err(HyperlightError::Error(
                "cpu_timeout must be non-zero".to_string(),
            ));
        }
        Ok(Self { cpu_timeout })
    }
}

impl ExecutionMonitor for CpuTimeMonitor {
    fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static> {
        // Capture CPU time handle on the calling thread
        let cpu_handle = ThreadCpuHandle::for_current_thread().ok_or_else(|| {
            HyperlightError::Error("Failed to get CPU time handle for current thread".to_string())
        })?;

        let cpu_timeout = self.cpu_timeout;

        // Compute deadline in platform-native ticks (nanos on Linux, TSC cycles on Windows).
        // This conversion is done once here, not on every poll iteration.
        let start_ticks = cpu_handle
            .elapsed()
            .ok_or_else(|| HyperlightError::Error("Failed to read initial CPU time".to_string()))?;
        let tick_budget = cpu_handle.deadline_for(cpu_timeout).ok_or_else(|| {
            HyperlightError::Error("Failed to compute CPU tick deadline".to_string())
        })?;
        let deadline = start_ticks.saturating_add(tick_budget);

        Ok(async move {
            loop {
                // Read current ticks in the platform's native unit
                let current = match cpu_handle.elapsed() {
                    Some(t) => t,
                    None => {
                        // CPU time reading failed mid-execution. Log the error
                        // and return immediately to trigger termination (fail-closed).
                        tracing::error!(
                            "Failed to read CPU time — terminating execution (fail-closed)"
                        );
                        return;
                    }
                };

                if current >= deadline {
                    let elapsed_ticks = current.saturating_sub(start_ticks);
                    let elapsed_ms = cpu_handle.ticks_to_approx_nanos(elapsed_ticks) / 1_000_000;
                    tracing::warn!(
                        cpu_elapsed_ms = elapsed_ms,
                        cpu_timeout_ms = cpu_timeout.as_millis() as u64,
                        "CPU time limit exceeded, terminating execution"
                    );
                    return;
                }

                // Adaptive sleep: half of remaining time, clamped to reasonable bounds.
                // Tokio's timer wheel resolution is ~1ms, so that's our effective floor.
                // The maximum keeps polls frequent enough for reasonable deadline accuracy.
                // Convert remaining ticks to approximate nanos for the sleep Duration.
                const MIN_POLL_INTERVAL: Duration = Duration::from_millis(1);
                const MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);
                const ADAPTIVE_DIVISOR: u64 = 2;

                let remaining = deadline.saturating_sub(current);
                let remaining_nanos = cpu_handle.ticks_to_approx_nanos(remaining);
                let sleep_duration = Duration::from_nanos(remaining_nanos / ADAPTIVE_DIVISOR)
                    .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);

                super::sleep(sleep_duration).await;
            }
        })
    }

    fn name(&self) -> &'static str {
        "cpu-time"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_cpu_handle_for_current_thread() {
        let handle = ThreadCpuHandle::for_current_thread();
        assert!(handle.is_some(), "CPU time handle should be available");
    }

    #[test]
    fn test_thread_cpu_handle_elapsed() {
        let handle = ThreadCpuHandle::for_current_thread().unwrap();

        // Do a small amount of CPU work
        let mut sum: u64 = 0;
        for i in 0..1_000_000u64 {
            sum = sum.wrapping_add(i);
        }
        std::hint::black_box(sum);

        let ticks = handle.elapsed();
        assert!(ticks.is_some(), "Should be able to read CPU time");
        // Even small amounts of work should register measurable ticks
        assert!(
            ticks.unwrap() > 0,
            "Elapsed ticks should be non-zero after doing work"
        );
    }

    #[test]
    fn test_zero_duration_rejected() {
        let result = CpuTimeMonitor::new(Duration::ZERO);
        assert!(result.is_err(), "Zero duration should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("non-zero"),
            "Error should mention non-zero: {err}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_cpu_time_precision() {
        // Test that we can measure sub-millisecond CPU time on Windows.
        // elapsed() returns raw TSC cycles; convert via ticks_to_approx_nanos for assertions.
        let handle = ThreadCpuHandle::for_current_thread().unwrap();

        // Do ~1ms of CPU work (rough estimate)
        let mut sum: u64 = 0;
        for i in 0..500_000u64 {
            sum = sum.wrapping_add(i);
        }
        std::hint::black_box(sum);

        let ticks = handle.elapsed().unwrap();
        let time_ns = handle.ticks_to_approx_nanos(ticks);
        let time_ms = time_ns as f64 / 1_000_000.0;

        // Should register something measurable (even if not exactly 1ms)
        println!(
            "Measured CPU time: {:.3}ms ({} ns, {} ticks)",
            time_ms, time_ns, ticks
        );

        // Should be non-zero and less than 100ms (sanity check)
        assert!(time_ns > 0, "CPU time should be non-zero");
        assert!(time_ns < 100_000_000, "CPU time should be less than 100ms");
    }
}
