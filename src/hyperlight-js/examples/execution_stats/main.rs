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
//! Execution Stats Example: Demonstrates guest call statistics
//!
//! This example shows how to inspect timing and termination information
//! after each guest function call using the `guest-call-stats` feature.
//!
//! Features demonstrated:
//! 1. Wall-clock timing after a normal `handle_event` call
//! 2. CPU time measurement (when `monitor-cpu-time` is enabled)
//! 3. Stats with execution monitors — including which monitor fired
//! 4. Stats update on every call (not cumulative)
//!
//! Run with:
//!   cargo run --example execution_stats --features guest-call-stats,monitor-wall-clock,monitor-cpu-time
//!
//! Or via Just:
//!   just run-examples

#![allow(clippy::disallowed_macros)]

use std::time::Duration;

use anyhow::Result;
use hyperlight_js::{SandboxBuilder, Script};

fn main() -> Result<()> {
    println!("Execution Stats Example: Guest Call Statistics\n");

    // ── Setup ────────────────────────────────────────────────────────
    let proto = SandboxBuilder::new().build()?;
    let mut sandbox = proto.load_runtime()?;

    // A fast handler that returns immediately
    let fast_handler = Script::from_content(
        r#"
        function handler(event) {
            event.message = "Hello from the guest!";
            return event;
        }
        "#,
    );

    // A CPU-intensive handler that burns for a configurable duration
    let slow_handler = Script::from_content(
        r#"
        function handler(event) {
            const startTime = Date.now();
            const runtime = event.runtime || 200;
            let counter = 0;
            while (Date.now() - startTime < runtime) {
                counter++;
            }
            event.counter = counter;
            return event;
        }
        "#,
    );

    // ── Test 1: Basic wall-clock timing ──────────────────────────────
    println!("📊 Test 1: Basic wall-clock timing (fast handler)");

    sandbox.add_handler("fast", fast_handler)?;
    let mut loaded = sandbox.get_loaded_sandbox()?;

    // Before any call, stats are None
    assert!(loaded.last_call_stats().is_none());
    println!("   Before call: stats = None ✅");

    let result = loaded.handle_event("fast", r#"{"name": "World"}"#.to_string(), None)?;
    println!("   Result: {result}");

    let stats = loaded
        .last_call_stats()
        .expect("Stats should be populated after a call");
    println!("   ⏱️  Wall clock: {:?}", stats.wall_clock);
    print_cpu_time(stats);
    println!(
        "   🏁 Terminated by: {}",
        stats.terminated_by.unwrap_or("(none — completed normally)")
    );

    // ── Test 2: CPU-intensive handler ────────────────────────────────
    println!("\n📊 Test 2: CPU-intensive handler (200ms burn)");

    let mut sandbox = loaded.unload()?;
    sandbox.clear_handlers();
    sandbox.add_handler("slow", slow_handler)?;
    let mut loaded = sandbox.get_loaded_sandbox()?;

    let _ = loaded.handle_event("slow", r#"{"runtime": 200}"#.to_string(), None)?;

    let stats = loaded.last_call_stats().unwrap();
    println!("   ⏱️  Wall clock: {:?}", stats.wall_clock);
    print_cpu_time(stats);
    println!("   🏁 Terminated by: (none — completed normally)");

    // ── Test 3: Stats update on each call ────────────────────────────
    println!("\n📊 Test 3: Stats update on each call (50ms then 150ms)");

    let _ = loaded.handle_event("slow", r#"{"runtime": 50}"#.to_string(), None)?;
    let stats1 = loaded.last_call_stats().unwrap().clone();
    println!("   Call 1 wall clock: {:?}", stats1.wall_clock);

    let _ = loaded.handle_event("slow", r#"{"runtime": 150}"#.to_string(), None)?;
    let stats2 = loaded.last_call_stats().unwrap().clone();
    println!("   Call 2 wall clock: {:?}", stats2.wall_clock);
    println!(
        "   Stats replaced (not cumulative): call2 > call1 = {} ✅",
        stats2.wall_clock > stats1.wall_clock
    );

    // ── Test 4: With monitors — successful completion ────────────────
    #[cfg(feature = "monitor-wall-clock")]
    {
        use hyperlight_js::WallClockMonitor;

        println!("\n📊 Test 4: Monitored call — completes within limit");

        let monitor = WallClockMonitor::new(Duration::from_secs(5))?;
        let _ = loaded.handle_event_with_monitor(
            "slow",
            r#"{"runtime": 50}"#.to_string(),
            &monitor,
            None,
        )?;

        let stats = loaded.last_call_stats().unwrap();
        println!("   ⏱️  Wall clock: {:?}", stats.wall_clock);
        print_cpu_time(stats);
        println!(
            "   🏁 Terminated by: {} ✅",
            stats.terminated_by.unwrap_or("(none — completed normally)")
        );
    }

    // ── Test 5: With monitors — timeout fires ────────────────────────
    #[cfg(feature = "monitor-wall-clock")]
    {
        use hyperlight_js::WallClockMonitor;

        println!("\n📊 Test 5: Monitored call — wall-clock timeout fires");

        let snapshot = loaded.snapshot()?;
        let monitor = WallClockMonitor::new(Duration::from_millis(50))?;
        let result = loaded.handle_event_with_monitor(
            "slow",
            r#"{"runtime": 5000}"#.to_string(),
            &monitor,
            None,
        );

        match result {
            Ok(_) => println!("   ❌ Unexpected: handler completed"),
            Err(_) => {
                let stats = loaded.last_call_stats().unwrap();
                println!("   ⏱️  Wall clock: {:?}", stats.wall_clock);
                print_cpu_time(stats);
                println!(
                    "   💀 Terminated by: {} ✅",
                    stats.terminated_by.unwrap_or("(unknown)")
                );
                println!("   🔒 Poisoned: {}", loaded.poisoned());
            }
        }

        // Recover from poisoned state
        loaded.restore(snapshot.clone())?;
        println!(
            "   📸 Restored from snapshot — poisoned: {}",
            loaded.poisoned()
        );
    }

    // ── Test 6: Combined monitors — CPU monitor wins ─────────────────
    #[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
    {
        use hyperlight_js::{CpuTimeMonitor, WallClockMonitor};

        println!("\n📊 Test 6: Combined monitors — CPU monitor fires first");

        let snapshot = loaded.snapshot()?;

        // CPU limit is tight (30ms), wall-clock is generous (5s)
        let monitor = (
            WallClockMonitor::new(Duration::from_secs(5))?,
            CpuTimeMonitor::new(Duration::from_millis(30))?,
        );
        let result = loaded.handle_event_with_monitor(
            "slow",
            r#"{"runtime": 5000}"#.to_string(),
            &monitor,
            None,
        );

        match result {
            Ok(_) => println!("   ❌ Unexpected: handler completed"),
            Err(_) => {
                let stats = loaded.last_call_stats().unwrap();
                println!("   ⏱️  Wall clock: {:?}", stats.wall_clock);
                print_cpu_time(stats);
                println!(
                    "   💀 Terminated by: {} ✅",
                    stats.terminated_by.unwrap_or("(unknown)")
                );
            }
        }

        loaded.restore(snapshot.clone())?;
        println!("   📸 Restored from snapshot");
    }

    println!("\n🎉 Execution stats example complete!");
    println!("\n💡 Key Points:");
    println!("   - last_call_stats() returns None before any call");
    println!("   - Stats are replaced (not accumulated) on each call");
    println!("   - wall_clock is always available");
    println!("   - cpu_time requires the monitor-cpu-time feature");
    println!("   - terminated_by shows which monitor fired (or None for normal completion)");
    println!("   - Stats are captured even when the call returns Err");

    Ok(())
}

/// Helper to print CPU time, handling the feature-gated `Option`.
fn print_cpu_time(stats: &hyperlight_js::ExecutionStats) {
    match stats.cpu_time {
        Some(cpu) => println!("   🖥️  CPU time:   {:?}", cpu),
        None => println!("   🖥️  CPU time:   (not available — enable monitor-cpu-time feature)"),
    }
}
