//! Execution Monitor Integration Tests

#![cfg(any(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
#![allow(clippy::disallowed_macros)]

use std::time::{Duration, Instant};

#[cfg(feature = "monitor-cpu-time")]
use hyperlight_js::CpuTimeMonitor;
#[cfg(feature = "monitor-wall-clock")]
use hyperlight_js::WallClockMonitor;
use hyperlight_js::{SandboxBuilder, Script};

/// Helper to create a sandbox with a CPU-burning handler.
/// The handler runs a tight loop for the specified number of milliseconds.
fn create_cpu_burning_sandbox() -> hyperlight_js::LoadedJSSandbox {
    let handler = Script::from_content(
        r#"
        function handler(event) {
            const startTime = Date.now();
            const runtime = event.runtime || 100;
            
            let counter = 0;
            while (Date.now() - startTime < runtime) {
                counter++;
            }
            
            event.counter = counter;
            event.actualRuntime = Date.now() - startTime;
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    sandbox.get_loaded_sandbox().unwrap()
}

#[test]
#[cfg(feature = "monitor-wall-clock")]
fn wall_clock_monitor_completes_fast_handler() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = WallClockMonitor::new(Duration::from_secs(5)).unwrap();

    // Handler runs for 100ms, timeout is 5 seconds - should complete normally
    let event = r#"{"runtime": 100}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_ok(), "Fast handler should complete: {:?}", result);
    let output = result.unwrap();
    assert!(output.contains("counter"), "Should have counter in output");
    assert!(!loaded.poisoned(), "Sandbox should not be poisoned");
}

#[test]
#[cfg(feature = "monitor-wall-clock")]
fn wall_clock_monitor_kills_slow_handler() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = WallClockMonitor::new(Duration::from_millis(500)).unwrap();
    let start = Instant::now();

    // Handler tries to run for 5 seconds, timeout is 500ms - should be killed
    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    let elapsed = start.elapsed();

    // Should have been killed around 500ms, not 5 seconds
    assert!(
        elapsed < Duration::from_secs(2),
        "Should terminate quickly, took {:?}",
        elapsed
    );
    assert!(
        elapsed >= Duration::from_millis(400),
        "Should run for at least 400ms, took {:?}",
        elapsed
    );

    // Result should be error since handler was killed
    assert!(result.is_err(), "Killed handler should return error");
    assert!(loaded.poisoned(), "Sandbox should be poisoned after kill");
}

#[test]
#[cfg(feature = "monitor-wall-clock")]
fn wall_clock_monitor_sandbox_recovers_with_restore() {
    let mut loaded = create_cpu_burning_sandbox();
    let snapshot = loaded.snapshot().unwrap();

    // Kill the handler
    let monitor = WallClockMonitor::new(Duration::from_millis(300)).unwrap();
    let event = r#"{"runtime": 5000}"#;
    let _ = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(loaded.poisoned(), "Should be poisoned after kill");

    // Restore from snapshot
    loaded.restore(&snapshot).unwrap();
    assert!(!loaded.poisoned(), "Should not be poisoned after restore");

    // Should be able to run again
    let monitor2 = WallClockMonitor::new(Duration::from_secs(5)).unwrap();
    let event2 = r#"{"runtime": 50}"#;
    let result = loaded.handle_event_with_monitor("handler", event2.to_string(), &monitor2, None);
    assert!(result.is_ok(), "Should work after restore: {:?}", result);
}

#[test]
#[cfg(feature = "monitor-cpu-time")]
fn cpu_time_monitor_completes_fast_handler() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = CpuTimeMonitor::new(Duration::from_secs(2)).unwrap();

    // Handler runs for 100ms CPU time, timeout is 2 seconds - should complete
    let event = r#"{"runtime": 100}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_ok(), "Fast handler should complete: {:?}", result);
    let output = result.unwrap();
    assert!(output.contains("counter"), "Should have counter in output");
    assert!(!loaded.poisoned(), "Sandbox should not be poisoned");
}

#[test]
#[cfg(feature = "monitor-cpu-time")]
fn cpu_time_monitor_kills_cpu_intensive_handler() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = CpuTimeMonitor::new(Duration::from_millis(500)).unwrap();
    let start = Instant::now();

    // Handler tries to burn CPU for 5 seconds, timeout is 500ms CPU time
    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    let elapsed = start.elapsed();

    // For a CPU-bound tight loop, CPU time ≈ wall time, so should terminate
    // within a few seconds at most
    assert!(
        elapsed < Duration::from_secs(5),
        "Should terminate within seconds, took {:?}",
        elapsed
    );

    // Result should be error since handler was killed
    assert!(result.is_err(), "Killed handler should return error");
    assert!(loaded.poisoned(), "Sandbox should be poisoned after kill");
}

#[test]
#[cfg(feature = "monitor-cpu-time")]
fn cpu_time_monitor_sandbox_recovers_with_restore() {
    let mut loaded = create_cpu_burning_sandbox();
    let snapshot = loaded.snapshot().unwrap();

    // Kill the handler with CPU monitor
    let monitor = CpuTimeMonitor::new(Duration::from_millis(300)).unwrap();
    let event = r#"{"runtime": 5000}"#;
    let _ = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(loaded.poisoned(), "Should be poisoned after kill");

    // Restore from snapshot
    loaded.restore(&snapshot).unwrap();
    assert!(!loaded.poisoned(), "Should not be poisoned after restore");

    // Should be able to run again
    let monitor2 = CpuTimeMonitor::new(Duration::from_secs(5)).unwrap();
    let event2 = r#"{"runtime": 50}"#;
    let result = loaded.handle_event_with_monitor("handler", event2.to_string(), &monitor2, None);
    assert!(result.is_ok(), "Should work after restore: {:?}", result);
}

// =============================================================================
// Tuple monitor tests — recommended usage pattern (CPU + wall-clock together).
// Tuples race sub-monitors via tokio::select!; the winner's name is logged.
// =============================================================================

#[test]
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn tuple_monitor_kills_cpu_intensive_handler() {
    let mut loaded = create_cpu_burning_sandbox();

    // Recommended pattern: CPU limit with wall-clock backstop
    let monitor = (
        CpuTimeMonitor::new(Duration::from_millis(500)).unwrap(),
        WallClockMonitor::new(Duration::from_secs(5)).unwrap(),
    );

    let start = Instant::now();
    let event = r#"{"runtime": 10000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    let elapsed = start.elapsed();

    // CPU monitor should fire first (tight loop ≈ 100% CPU utilisation)
    assert!(result.is_err(), "Should be killed by CPU monitor");
    assert!(loaded.poisoned(), "Sandbox should be poisoned");
    assert!(
        elapsed < Duration::from_secs(3),
        "CPU monitor should fire well before wall-clock, took {:?}",
        elapsed
    );
}

#[test]
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn tuple_monitor_completes_fast_handler() {
    let mut loaded = create_cpu_burning_sandbox();

    let monitor = (
        CpuTimeMonitor::new(Duration::from_secs(2)).unwrap(),
        WallClockMonitor::new(Duration::from_secs(5)).unwrap(),
    );

    // Handler runs for 100ms — well within both limits
    let event = r#"{"runtime": 100}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_ok(), "Fast handler should complete: {:?}", result);
    assert!(!loaded.poisoned(), "Sandbox should not be poisoned");
}

#[test]
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn tuple_monitor_sandbox_recovers_with_restore() {
    let mut loaded = create_cpu_burning_sandbox();
    let snapshot = loaded.snapshot().unwrap();

    // Kill with tuple monitor
    let monitor = (
        CpuTimeMonitor::new(Duration::from_millis(300)).unwrap(),
        WallClockMonitor::new(Duration::from_secs(3)).unwrap(),
    );
    let event = r#"{"runtime": 10000}"#;
    let _ = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    assert!(loaded.poisoned(), "Should be poisoned after kill");

    // Restore and verify recovery
    loaded.restore(&snapshot).unwrap();
    assert!(!loaded.poisoned(), "Should not be poisoned after restore");

    let monitor2 = (
        CpuTimeMonitor::new(Duration::from_secs(5)).unwrap(),
        WallClockMonitor::new(Duration::from_secs(10)).unwrap(),
    );
    let event2 = r#"{"runtime": 50}"#;
    let result = loaded.handle_event_with_monitor("handler", event2.to_string(), &monitor2, None);
    assert!(result.is_ok(), "Should work after restore: {:?}", result);
}

// =============================================================================
// Additional monitor edge-case tests
// =============================================================================

/// A monitor that always fails to initialize — used to test fail-closed
/// semantics in tuple composition.
#[cfg(feature = "monitor-wall-clock")]
struct FailingMonitor;

#[cfg(feature = "monitor-wall-clock")]
impl hyperlight_js::ExecutionMonitor for FailingMonitor {
    fn get_monitor(
        &self,
    ) -> hyperlight_js::Result<impl std::future::Future<Output = ()> + Send + 'static> {
        Err::<std::future::Ready<()>, _>(hyperlight_js::HyperlightError::Error(
            "Simulated initialization failure".to_string(),
        ))
    }

    fn name(&self) -> &'static str {
        "failing-monitor"
    }
}

/// Fail-closed: if one sub-monitor in a tuple fails to init, the whole
/// tuple must fail and the handler must NOT run.
#[test]
#[cfg(feature = "monitor-wall-clock")]
fn tuple_with_failing_monitor_is_fail_closed() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = (
        WallClockMonitor::new(Duration::from_secs(5)).unwrap(),
        FailingMonitor,
    );

    let event = r#"{"runtime": 50}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_err(), "Should fail when sub-monitor fails");
    assert!(
        result.unwrap_err().to_string().contains("failed to start"),
        "Error should mention monitor failure"
    );
    // Sandbox should NOT be poisoned — we never ran the handler
    assert!(
        !loaded.poisoned(),
        "Sandbox should not be poisoned when monitor fails to start"
    );
}

/// The same monitor instance can be reused across multiple calls.
#[test]
#[cfg(feature = "monitor-wall-clock")]
fn monitor_reuse_across_calls() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = WallClockMonitor::new(Duration::from_secs(5)).unwrap();

    let event = r#"{"runtime": 50}"#;

    // First call
    let result1 = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    assert!(result1.is_ok(), "First call should succeed: {:?}", result1);
    assert!(!loaded.poisoned());

    // Second call with same monitor instance
    let result2 = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    assert!(result2.is_ok(), "Second call should succeed: {:?}", result2);
    assert!(!loaded.poisoned());
}

/// Single-element tuple monitors should work identically to a bare monitor.
#[test]
#[cfg(feature = "monitor-wall-clock")]
fn single_element_tuple_monitor() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = (WallClockMonitor::new(Duration::from_millis(500)).unwrap(),);
    let start = Instant::now();

    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Should be killed by 1-tuple monitor");
    assert!(loaded.poisoned(), "Sandbox should be poisoned");
    assert!(
        elapsed < Duration::from_secs(2),
        "Should terminate quickly, took {:?}",
        elapsed
    );
}
