use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Bencher, Criterion};
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
use hyperlight_js::{CpuTimeMonitor, WallClockMonitor};
use hyperlight_js::{SandboxBuilder, Script};

fn js_load_handler_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("js_get_loaded_sandbox");

    let bench_guest_function = |b: &mut Bencher<'_>, number_of_adds: i32| {
        let handler = Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }"#,
        );

        b.iter_custom(|iterations| {
            let mut js_sandbox = SandboxBuilder::new()
                .build()
                .unwrap()
                .load_runtime()
                .unwrap();

            let mut elapsed = Duration::ZERO;

            for _ in 0..iterations {
                // Load JS into the sandbox.
                let start = Instant::now();
                for i in 0..number_of_adds {
                    js_sandbox
                        .add_handler(format!("function{}", i), handler.clone())
                        .unwrap();
                }
                let loaded_js_sandbox = js_sandbox.get_loaded_sandbox().unwrap();
                elapsed += start.elapsed();
                js_sandbox = loaded_js_sandbox.unload().unwrap();
            }
            elapsed
        });
    };

    let bench_guest_function_with_snapshot = |b: &mut Bencher<'_>, number_of_adds: i32| {
        let handler = Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }"#,
        );

        b.iter_custom(|iterations| {
            let mut js_sandbox = SandboxBuilder::new()
                .build()
                .unwrap()
                .load_runtime()
                .unwrap();

            let mut elapsed = Duration::ZERO;

            for _ in 0..iterations {
                // Load JS into the sandbox.
                let start = Instant::now();
                for i in 0..number_of_adds {
                    js_sandbox
                        .add_handler(format!("function{}", i), handler.clone())
                        .unwrap();
                }
                let mut loaded_js_sandbox = js_sandbox.get_loaded_sandbox().unwrap();
                let _ = loaded_js_sandbox.snapshot().unwrap();
                elapsed += start.elapsed();
                js_sandbox = loaded_js_sandbox.unload().unwrap();
            }
            elapsed
        });
    };

    group.bench_function("jsload_1_handler", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 1);
    });
    group.bench_function("jsload_2_handlers", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 2);
    });
    group.bench_function("jsload_5_handlers", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 5);
    });
    group.bench_function("jsload_10_handlers", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 10);
    });
    group.bench_function("jsload_1_handler_with_snapshot", |b: &mut Bencher<'_>| {
        bench_guest_function_with_snapshot(b, 1);
    });
    group.bench_function("jsload_2_handlers_with_snapshot", |b: &mut Bencher<'_>| {
        bench_guest_function_with_snapshot(b, 2);
    });
    group.bench_function("jsload_5_handlers_with_snapshot", |b: &mut Bencher<'_>| {
        bench_guest_function_with_snapshot(b, 5);
    });
    group.bench_function("jsload_10_handlers_with_snapshot", |b: &mut Bencher<'_>| {
        bench_guest_function_with_snapshot(b, 10);
    });

    group.finish();
}

fn handle_events_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("handle_events");

    let bench_guest_function = |b: &mut Bencher<'_>, number_of_events: i32, gc: bool| {
        let handler = Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }"#,
        );

        b.iter_custom(|iterations| {
            let mut js_sandbox = SandboxBuilder::new()
                .build()
                .unwrap()
                .load_runtime()
                .unwrap();
            js_sandbox
                .add_handler("function1", handler.clone())
                .unwrap();
            let mut loaded_js_sandbox = js_sandbox.get_loaded_sandbox().unwrap();
            let event = r#"
            {
                "request": {
                    "uri": "/index.html"
                }
            }"#;
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                // Load JS into the sandbox.
                for _ in 0..number_of_events {
                    let start = Instant::now();
                    let _ =
                        loaded_js_sandbox.handle_event("function1", event.to_string(), Some(gc));
                    elapsed += start.elapsed();
                }
            }
            elapsed
        });
    };

    let bench_guest_function_with_restore = |b: &mut Bencher<'_>,
                                             number_of_events: i32,
                                             gc: bool| {
        let handler = Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }"#,
        );

        b.iter_custom(|iterations| {
            let mut js_sandbox = SandboxBuilder::new()
                .build()
                .unwrap()
                .load_runtime()
                .unwrap();
            js_sandbox
                .add_handler("function1", handler.clone())
                .unwrap();
            let mut loaded_js_sandbox = js_sandbox.get_loaded_sandbox().unwrap();
            let event = r#"
            {
                "request": {
                    "uri": "/index.html"
                }
            }"#;
            let snapshot = loaded_js_sandbox.snapshot().unwrap();
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                // Load JS into the sandbox.
                for _ in 0..number_of_events {
                    let start = Instant::now();
                    let _ =
                        loaded_js_sandbox.handle_event("function1", event.to_string(), Some(gc));
                    loaded_js_sandbox.restore(&snapshot).unwrap();
                    elapsed += start.elapsed();
                }
            }
            elapsed
        });
    };

    group.bench_function("handle_1_events_with_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 1, true);
    });
    group.bench_function("handle_2_events_with_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 2, true);
    });
    group.bench_function("handle_5_events_with_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 5, true);
    });
    group.bench_function("handle_10_events_with_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 10, true);
    });

    group.bench_function("handle_1_events_without_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 1, false);
    });
    group.bench_function("handle_2_events_without_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 2, false);
    });
    group.bench_function("handle_5_events_without_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 5, false);
    });
    group.bench_function("handle_10_events_without_gc", |b: &mut Bencher<'_>| {
        bench_guest_function(b, 10, false);
    });

    group.bench_function(
        "handle_1_events_with_restore_with_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 1, true);
        },
    );
    group.bench_function(
        "handle_2_events_with_restore_with_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 2, true);
        },
    );
    group.bench_function(
        "handle_5_events_with_restore_with_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 5, true);
        },
    );
    group.bench_function(
        "handle_10_events_with_restore_with_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 10, true);
        },
    );

    group.bench_function(
        "handle_1_events_with_restore_without_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 1, false);
        },
    );
    group.bench_function(
        "handle_2_events_with_restore_without_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 2, false);
        },
    );
    group.bench_function(
        "handle_5_events_with_restore_without_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 5, false);
        },
    );
    group.bench_function(
        "handle_10_events_with_restore_without_gc",
        |b: &mut Bencher<'_>| {
            bench_guest_function_with_restore(b, 10, false);
        },
    );

    group.finish();
}

// =============================================================================
// Monitor overhead benchmark
// =============================================================================
// Measures the cost of handle_event_with_monitor vs handle_event to quantify
// the overhead of spinning up the monitoring machinery (runtime spawn,
// tokio::select!, CPU clock capture, etc.) on a fast handler that completes
// well within limits.
#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
fn monitor_overhead_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("monitor_overhead");

    let handler = Script::from_content(
        r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }"#,
    );

    let event = r#"
    {
        "request": {
            "uri": "/index.html"
        }
    }"#;

    // Baseline: handle_event with no monitoring at all.
    group.bench_function("handle_event_no_monitor", |b: &mut Bencher<'_>| {
        let mut js_sandbox = SandboxBuilder::new()
            .build()
            .unwrap()
            .load_runtime()
            .unwrap();
        js_sandbox.add_handler("handler", handler.clone()).unwrap();
        let mut loaded = js_sandbox.get_loaded_sandbox().unwrap();

        b.iter(|| {
            loaded
                .handle_event("handler", event.to_string(), Some(true))
                .unwrap();
        });
    });

    // With wall-clock monitor only.
    group.bench_function("handle_event_wall_clock", |b: &mut Bencher<'_>| {
        let mut js_sandbox = SandboxBuilder::new()
            .build()
            .unwrap()
            .load_runtime()
            .unwrap();
        js_sandbox.add_handler("handler", handler.clone()).unwrap();
        let mut loaded = js_sandbox.get_loaded_sandbox().unwrap();
        let monitor = WallClockMonitor::new(Duration::from_secs(5)).unwrap();

        b.iter(|| {
            loaded
                .handle_event_with_monitor("handler", event.to_string(), &monitor, Some(true))
                .unwrap();
        });
    });

    // With CPU time monitor only.
    group.bench_function("handle_event_cpu_time", |b: &mut Bencher<'_>| {
        let mut js_sandbox = SandboxBuilder::new()
            .build()
            .unwrap()
            .load_runtime()
            .unwrap();
        js_sandbox.add_handler("handler", handler.clone()).unwrap();
        let mut loaded = js_sandbox.get_loaded_sandbox().unwrap();
        let monitor = CpuTimeMonitor::new(Duration::from_secs(5)).unwrap();

        b.iter(|| {
            loaded
                .handle_event_with_monitor("handler", event.to_string(), &monitor, Some(true))
                .unwrap();
        });
    });

    // With both wall-clock + CPU time monitors (tuple).
    group.bench_function("handle_event_wall_and_cpu", |b: &mut Bencher<'_>| {
        let mut js_sandbox = SandboxBuilder::new()
            .build()
            .unwrap()
            .load_runtime()
            .unwrap();
        js_sandbox.add_handler("handler", handler.clone()).unwrap();
        let mut loaded = js_sandbox.get_loaded_sandbox().unwrap();
        let wall = WallClockMonitor::new(Duration::from_secs(5)).unwrap();
        let cpu = CpuTimeMonitor::new(Duration::from_secs(5)).unwrap();
        let monitors = (wall, cpu);

        b.iter(|| {
            loaded
                .handle_event_with_monitor("handler", event.to_string(), &monitors, Some(true))
                .unwrap();
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(20));
    targets = js_load_handler_benchmark, handle_events_benchmark
}

#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
criterion_group! {
    name = monitor_benches;
    config = Criterion::default().measurement_time(Duration::from_secs(20));
    targets = monitor_overhead_benchmark
}

#[cfg(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time"))]
criterion_main!(benches, monitor_benches);

#[cfg(not(all(feature = "monitor-wall-clock", feature = "monitor-cpu-time")))]
criterion_main!(benches);
