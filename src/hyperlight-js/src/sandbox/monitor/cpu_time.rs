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

// ============================================================================
// Platform-specific ThreadCpuHandle implementations
// ============================================================================

/// Handle for reading CPU time of a specific thread.
///
/// Each platform works in its own native tick unit — the monitor loop is
/// unit-agnostic and just compares `u64` ticks against a deadline.
///
/// - **Linux**: Ticks are nanoseconds (from `clock_gettime`)
/// - **Windows**: Ticks are TSC reference cycles (from `QueryThreadCycleTime`)
#[cfg(target_os = "linux")]
pub(crate) struct ThreadCpuHandle {
    clock_id: libc::clockid_t,
}

// SAFETY: ThreadCpuHandle contains a clock_id obtained from pthread_getcpuclockid.
// The clock_id is process-scoped and remains valid for the lifetime of the thread.
// clock_gettime() with a thread CPU clock is explicitly safe to call from any thread
// per POSIX specification - it reads the CPU time counter for the specified thread.
#[cfg(target_os = "linux")]
unsafe impl Send for ThreadCpuHandle {}
#[cfg(target_os = "linux")]
unsafe impl Sync for ThreadCpuHandle {}

#[cfg(target_os = "linux")]
impl ThreadCpuHandle {
    /// Create a handle for the current thread's CPU time.
    pub fn for_current_thread() -> Option<Self> {
        use libc::{pthread_getcpuclockid, pthread_self};

        let thread_id = unsafe { pthread_self() };
        let mut clock_id: libc::clockid_t = 0;

        let result = unsafe { pthread_getcpuclockid(thread_id, &mut clock_id) };
        if result != 0 {
            return None;
        }

        Some(Self { clock_id })
    }

    /// Get the elapsed CPU ticks for this thread.
    ///
    /// On Linux, ticks are nanoseconds (the native unit of `clock_gettime`).
    pub fn elapsed(&self) -> Option<u64> {
        use libc::{clock_gettime, timespec};

        if self.clock_id == 0 {
            return None;
        }

        let mut ts = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };

        let result = unsafe { clock_gettime(self.clock_id, &mut ts) };
        if result != 0 {
            return None;
        }

        Some((ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64))
    }

    /// Convert a `Duration` timeout into a tick budget in the platform's native unit.
    ///
    /// On Linux, ticks are nanoseconds so this is an identity conversion.
    pub fn deadline_for(&self, timeout: Duration) -> Option<u64> {
        Some(timeout.as_nanos() as u64)
    }

    /// Convert ticks to approximate nanoseconds (for logging and sleep calculations).
    ///
    /// On Linux, ticks are nanoseconds so this is an identity conversion.
    pub fn ticks_to_approx_nanos(&self, ticks: u64) -> u64 {
        ticks
    }
}

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;

#[cfg(target_os = "windows")]
pub(crate) struct ThreadCpuHandle {
    thread_handle: windows_sys::Win32::Foundation::HANDLE,
    /// Cached start cycles for relative measurement
    start_cycles: u64,
}

/// Cached CPU frequency in MHz, read from the Windows registry once per process.
#[cfg(target_os = "windows")]
static CPU_FREQUENCY_MHZ: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();

/// Read the CPU's nominal frequency in MHz from the Windows registry.
///
/// Reads `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\~MHz`.
/// This is the processor's base/rated frequency and matches the tick rate
/// of `QueryThreadCycleTime` (which uses the invariant TSC on modern CPUs).
///
#[cfg(target_os = "windows")]
fn read_cpu_frequency_mhz() -> Option<u32> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    };

    // Null-terminated UTF-16 strings for registry path and value name.
    // Path: HARDWARE\DESCRIPTION\System\CentralProcessor\0  (the trailing \0 is
    // the subkey named "0", i.e. the first logical processor; the final \0 is
    // the null terminator required by the Win32 API).
    let subkey: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "~MHz\0".encode_utf16().collect();

    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let result =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if result != 0 {
        tracing::warn!("[CPU_TIME] Failed to open registry key for CPU frequency");
        return None;
    }

    let mut mhz: u32 = 0;
    let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
    let mut data_type: u32 = 0;

    let result = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            &mut mhz as *mut u32 as *mut u8,
            &mut data_size,
        )
    };

    unsafe { RegCloseKey(hkey) };

    if result != 0 || data_type != REG_DWORD || mhz == 0 {
        tracing::warn!(
            result = result,
            data_type = data_type,
            "[CPU_TIME] Failed to read CPU frequency from registry"
        );
        return None;
    }

    tracing::debug!(
        cpu_frequency_mhz = mhz,
        "[CPU_TIME] Read CPU base frequency from registry"
    );

    Some(mhz)
}

/// Get the cached CPU frequency in MHz, reading from registry on first call.
#[cfg(target_os = "windows")]
fn get_cpu_frequency_mhz() -> Option<u32> {
    *CPU_FREQUENCY_MHZ.get_or_init(read_cpu_frequency_mhz)
}

// SAFETY: ThreadCpuHandle contains a real thread handle obtained via DuplicateHandle.
// Unlike the pseudo-handle from GetCurrentThread(), a duplicated handle is valid
// for use from any thread. QueryThreadCycleTime() is explicitly thread-safe when
// called with a valid thread handle. The handle is properly closed in Drop.
#[cfg(target_os = "windows")]
unsafe impl Send for ThreadCpuHandle {}
#[cfg(target_os = "windows")]
unsafe impl Sync for ThreadCpuHandle {}

#[cfg(target_os = "windows")]
impl ThreadCpuHandle {
    /// Create a handle for the current thread's CPU time.
    ///
    /// Uses `QueryThreadCycleTime` with the CPU's nominal frequency from the
    /// Windows registry for cycle-based measurement.
    pub fn for_current_thread() -> Option<Self> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread};

        // Ensure CPU frequency is available (read from registry, once per process)
        if get_cpu_frequency_mhz().is_none() {
            tracing::warn!(
                "[CPU_TIME] Could not read CPU frequency from registry, \
                 CPU time monitoring unavailable"
            );
            return None;
        }

        // GetCurrentThread returns a pseudo-handle that can't be used from other threads.
        // We need to duplicate it to get a real handle.
        let pseudo_handle = unsafe { GetCurrentThread() };
        let process = unsafe { GetCurrentProcess() };
        let mut real_handle: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();

        let result = unsafe {
            DuplicateHandle(
                process,
                pseudo_handle,
                process,
                &mut real_handle,
                0,
                0, // FALSE
                DUPLICATE_SAME_ACCESS,
            )
        };

        if result == 0 {
            return None;
        }

        // Capture starting cycle count
        let mut start_cycles: u64 = 0;
        if unsafe { QueryThreadCycleTime(real_handle, &mut start_cycles) } == 0 {
            // Clean up handle if we can't get cycles
            unsafe { windows_sys::Win32::Foundation::CloseHandle(real_handle) };
            return None;
        }

        Some(Self {
            thread_handle: real_handle,
            start_cycles,
        })
    }

    /// Get the elapsed CPU ticks for this thread.
    ///
    /// On Windows, ticks are raw TSC reference cycles from `QueryThreadCycleTime`.
    /// No conversion is performed — the monitor works directly in the platform's
    /// native unit, converting only for logging via `ticks_to_approx_nanos`.
    pub fn elapsed(&self) -> Option<u64> {
        if self.thread_handle.is_null() {
            return None;
        }

        let mut current_cycles: u64 = 0;
        let result = unsafe { QueryThreadCycleTime(self.thread_handle, &mut current_cycles) };

        if result == 0 {
            return None;
        }

        Some(current_cycles.saturating_sub(self.start_cycles))
    }

    /// Convert a `Duration` timeout into a tick budget in the platform's native unit.
    ///
    /// On Windows, converts nanoseconds to TSC reference cycles using the CPU's
    /// nominal frequency from the registry. This is done once at monitor setup,
    /// not on every poll.
    pub fn deadline_for(&self, timeout: Duration) -> Option<u64> {
        let freq_mhz = get_cpu_frequency_mhz()? as u64;
        let nanos = timeout.as_nanos() as u64;
        // cycles = nanos * freq_mhz / 1000
        // (inverse of: nanos = cycles * 1000 / freq_mhz)
        Some(nanos.saturating_mul(freq_mhz) / 1_000)
    }

    /// Convert ticks to approximate nanoseconds (for logging and sleep calculations).
    ///
    /// On Windows, converts TSC reference cycles to nanoseconds using the CPU's
    /// nominal frequency. Precision is not critical — this is used for
    /// human-readable log output and adaptive sleep duration clamping.
    pub fn ticks_to_approx_nanos(&self, ticks: u64) -> u64 {
        match get_cpu_frequency_mhz() {
            Some(freq_mhz) => ticks.saturating_mul(1_000) / freq_mhz as u64,
            None => 0,
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for ThreadCpuHandle {
    fn drop(&mut self) {
        if !self.thread_handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.thread_handle);
            }
        }
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
