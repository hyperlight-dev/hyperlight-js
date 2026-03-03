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
//! Integration tests for the `guest-call-stats` feature.
//!
//! These tests verify that `LoadedJSSandbox::last_call_stats()` returns
//! correct execution statistics after guest function calls, both with
//! and without execution monitors.

#![cfg(feature = "guest-call-stats")]
#![allow(clippy::disallowed_macros)]

use std::time::Duration;

#[cfg(feature = "monitor-cpu-time")]
use hyperlight_js::CpuTimeMonitor;
#[cfg(feature = "monitor-wall-clock")]
use hyperlight_js::WallClockMonitor;
use hyperlight_js::{SandboxBuilder, Script};

// ── Helpers ──────────────────────────────────────────────────────────

/// Create a sandbox with a simple handler that returns immediately.
fn create_fast_sandbox() -> hyperlight_js::LoadedJSSandbox {
    let handler = Script::from_content(
        r#"
        function handler(event) {
            event.handled = true;
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    sandbox.get_loaded_sandbox().unwrap()
}

/// Create a sandbox with a CPU-burning handler.
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
            return event;
        }
        "#,
    );

    let proto = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("handler", handler).unwrap();
    sandbox.get_loaded_sandbox().unwrap()
}

// ── Basic stats tests ────────────────────────────────────────────────

#[test]
fn last_call_stats_is_none_before_any_call() {
    let loaded = create_fast_sandbox();
    assert!(
        loaded.last_call_stats().is_none(),
        "Stats should be None before any call"
    );
}

#[test]
fn handle_event_populates_wall_clock() {
    let mut loaded = create_fast_sandbox();
    let event = r#"{"test": true}"#;

    let result = loaded.handle_event("handler", event.to_string(), None);
    assert!(result.is_ok());

    let stats = loaded
        .last_call_stats()
        .expect("Stats should be populated after a call");
    assert!(
        stats.wall_clock > Duration::ZERO,
        "Wall clock should be > 0, got {:?}",
        stats.wall_clock
    );
    assert_eq!(
        stats.terminated_by, None,
        "terminated_by should be None for a normal call"
    );
}

#[test]
fn stats_update_on_each_call() {
    let mut loaded = create_cpu_burning_sandbox();

    // First call — 50ms burn
    let _ = loaded.handle_event("handler", r#"{"runtime": 50}"#.to_string(), None);
    let stats1 = loaded.last_call_stats().unwrap().clone();

    // Second call — 100ms burn (should be slower)
    let _ = loaded.handle_event("handler", r#"{"runtime": 100}"#.to_string(), None);
    let stats2 = loaded.last_call_stats().unwrap().clone();

    // Stats should have been replaced, not accumulated
    // The second call should have a longer wall clock
    assert!(
        stats2.wall_clock >= Duration::from_millis(80),
        "Second call wall clock ({:?}) should be >= 80ms",
        stats2.wall_clock
    );
    assert!(
        stats2.wall_clock > stats1.wall_clock,
        "Second call ({:?}) should take longer than first ({:?})",
        stats2.wall_clock,
        stats1.wall_clock
    );
}

#[test]
fn stats_available_even_when_handler_errors() {
    let mut loaded = create_fast_sandbox();

    // Call a non-existent handler
    let result = loaded.handle_event("nonexistent", r#"{}"#.to_string(), None);
    assert!(result.is_err(), "Should fail for non-existent handler");

    // Stats should still be available from the last successful setup
    // Note: if the error happens before inner.call(), stats won't be set.
    // But if inner.call() fails (runtime error), stats ARE set.
    // The JSON validation passes, the call to the VM happens but fails.
    // Stats are populated because the timing wraps inner.call().
    let stats = loaded.last_call_stats();
    // For a non-existent handler, inner.call() does fail, and we DO capture timing
    assert!(
        stats.is_some(),
        "Stats should be available even after a failed call"
    );
}

// ── CPU time tests ───────────────────────────────────────────────────

#[test]
#[cfg(feature = "monitor-cpu-time")]
fn handle_event_populates_cpu_time_when_feature_enabled() {
    let mut loaded = create_cpu_burning_sandbox();
    let event = r#"{"runtime": 50}"#;

    let result = loaded.handle_event("handler", event.to_string(), None);
    assert!(result.is_ok());

    let stats = loaded.last_call_stats().unwrap();
    assert!(
        stats.cpu_time.is_some(),
        "cpu_time should be Some when monitor-cpu-time feature is enabled"
    );
    let cpu_time = stats.cpu_time.unwrap();
    assert!(
        cpu_time > Duration::ZERO,
        "cpu_time should be > 0 for a CPU-burning handler, got {:?}",
        cpu_time
    );
}

#[test]
#[cfg(not(feature = "monitor-cpu-time"))]
fn handle_event_cpu_time_is_none_without_feature() {
    let mut loaded = create_fast_sandbox();
    let event = r#"{"test": true}"#;

    let _ = loaded.handle_event("handler", event.to_string(), None);

    let stats = loaded.last_call_stats().unwrap();
    assert!(
        stats.cpu_time.is_none(),
        "cpu_time should be None without monitor-cpu-time feature"
    );
}

// ── Monitor termination tests ────────────────────────────────────────

#[test]
#[cfg(feature = "monitor-wall-clock")]
fn wall_clock_termination_sets_terminated_by() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = WallClockMonitor::new(Duration::from_millis(50)).unwrap();

    // Handler burns for 5 seconds — well past the 50ms timeout
    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_err(), "Should have been terminated by monitor");

    let stats = loaded
        .last_call_stats()
        .expect("Stats should be available after monitor termination");
    assert_eq!(
        stats.terminated_by,
        Some("wall-clock"),
        "terminated_by should be 'wall-clock'"
    );
    assert!(
        stats.wall_clock > Duration::ZERO,
        "Wall clock should still be measured"
    );
}

#[test]
#[cfg(feature = "monitor-cpu-time")]
fn cpu_time_termination_sets_terminated_by() {
    let mut loaded = create_cpu_burning_sandbox();
    let monitor = CpuTimeMonitor::new(Duration::from_millis(50)).unwrap();

    // Handler burns CPU for 5 seconds — well past the 50ms CPU timeout
    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(
        result.is_err(),
        "Should have been terminated by CPU monitor"
    );

    let stats = loaded
        .last_call_stats()
        .expect("Stats should be available after CPU monitor termination");
    assert_eq!(
        stats.terminated_by,
        Some("cpu-time"),
        "terminated_by should be 'cpu-time'"
    );
}

#[test]
#[cfg(feature = "monitor-wall-clock")]
fn monitor_completion_normal_has_no_terminated_by() {
    let mut loaded = create_fast_sandbox();
    let monitor = WallClockMonitor::new(Duration::from_secs(5)).unwrap();

    // Handler is fast, timeout is generous — should complete normally
    let event = r#"{"test": true}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_ok(), "Should complete normally");

    let stats = loaded.last_call_stats().unwrap();
    assert_eq!(
        stats.terminated_by, None,
        "terminated_by should be None when monitor didn't fire"
    );
}

#[test]
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn tuple_monitor_reports_correct_winner() {
    let mut loaded = create_cpu_burning_sandbox();

    // CPU limit is very tight (30ms), wall clock is generous (5s).
    // CPU monitor should win the race.
    let wall = WallClockMonitor::new(Duration::from_secs(5)).unwrap();
    let cpu = CpuTimeMonitor::new(Duration::from_millis(30)).unwrap();
    let monitor = (wall, cpu);

    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_err(), "Should have been terminated");

    let stats = loaded.last_call_stats().unwrap();
    assert_eq!(
        stats.terminated_by,
        Some("cpu-time"),
        "CPU monitor should win with tight 30ms limit vs 5s wall"
    );
}

#[test]
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn tuple_monitor_wall_clock_wins_when_tighter() {
    let mut loaded = create_cpu_burning_sandbox();

    // Wall clock is tight (50ms), CPU limit is generous (5s).
    // Wall clock monitor should win the race.
    let wall = WallClockMonitor::new(Duration::from_millis(50)).unwrap();
    let cpu = CpuTimeMonitor::new(Duration::from_secs(5)).unwrap();
    let monitor = (wall, cpu);

    let event = r#"{"runtime": 5000}"#;
    let result = loaded.handle_event_with_monitor("handler", event.to_string(), &monitor, None);

    assert!(result.is_err(), "Should have been terminated");

    let stats = loaded.last_call_stats().unwrap();
    assert_eq!(
        stats.terminated_by,
        Some("wall-clock"),
        "Wall-clock monitor should win with tight 50ms limit vs 5s CPU"
    );
}
