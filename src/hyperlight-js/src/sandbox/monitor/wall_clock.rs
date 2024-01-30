//! Wall-clock time based execution monitor.

use std::future::Future;
use std::time::Duration;

use hyperlight_host::{HyperlightError, Result};

use super::ExecutionMonitor;

/// Monitors handler execution using wall-clock time.
///
/// Terminates execution if the handler runs longer than the configured timeout.
/// This measures real elapsed time, including time spent blocked or waiting.
///
/// # Resource Exhaustion Protection
///
/// Wall-clock monitoring is essential for catching resource exhaustion attacks where
/// the guest consumes **host resources without consuming CPU** — e.g. blocking on host calls. A guest doing "nothing"
/// in terms of CPU can still starve the host. This is sometimes called a **slowloris-style
/// denial of service**. This cannot happen at present in Hyperlight since there is no way of blocking
/// without consuming CPU, however in the future this may change. For comprehensive protection, combine with [`CpuTimeMonitor`]
/// via a tuple:
///
/// ```text
/// let monitor = (
///     WallClockMonitor::new(Duration::from_secs(5))?,
///     CpuTimeMonitor::new(Duration::from_millis(500))?,
/// );
/// ```
///
/// # Example
///
/// ```text
/// use hyperlight_js::WallClockMonitor;
/// use std::time::Duration;
///
/// let monitor = WallClockMonitor::new(Duration::from_secs(5))?;
/// let result = sandbox.handle_event_with_monitor("handler", "{}".to_string(), &monitor, None)?;
/// ```
#[derive(Debug, Clone)]
pub struct WallClockMonitor {
    timeout: Duration,
}

impl WallClockMonitor {
    /// Create a new wall-clock monitor with the specified timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if `timeout` is zero.
    pub fn new(timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(HyperlightError::Error(
                "timeout must be non-zero".to_string(),
            ));
        }
        Ok(Self { timeout })
    }
}

impl ExecutionMonitor for WallClockMonitor {
    fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static> {
        let timeout = self.timeout;
        Ok(async move {
            super::sleep(timeout).await;
            tracing::warn!(
                timeout_ms = timeout.as_millis() as u64,
                "Wall-clock timeout exceeded, terminating execution"
            );
        })
    }

    fn name(&self) -> &'static str {
        "wall-clock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_duration_rejected() {
        let result = WallClockMonitor::new(Duration::ZERO);
        assert!(result.is_err(), "Zero duration should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("non-zero"),
            "Error should mention non-zero: {err}"
        );
    }

    #[test]
    fn test_valid_duration_accepted() {
        let result = WallClockMonitor::new(Duration::from_millis(100));
        assert!(result.is_ok(), "Valid duration should be accepted");
    }

    #[test]
    fn test_get_monitor_returns_future() {
        let monitor = WallClockMonitor::new(Duration::from_secs(1)).unwrap();
        let future = monitor.get_monitor();
        assert!(future.is_ok(), "get_monitor() should return Ok");
    }

    #[test]
    fn test_get_monitor_reuse() {
        // The same monitor instance should produce separate futures
        let monitor = WallClockMonitor::new(Duration::from_secs(1)).unwrap();
        let future1 = monitor.get_monitor();
        let future2 = monitor.get_monitor();
        assert!(future1.is_ok(), "First call should succeed");
        assert!(future2.is_ok(), "Second call should succeed");
    }
}
