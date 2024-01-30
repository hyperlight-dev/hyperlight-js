# Execution Monitors

This document describes the execution monitoring system in hyperlight-js, which provides resource-limited handler execution with automatic termination.

## Why Execution Monitors? 🤔

When running untrusted JavaScript code in a sandbox, you need protection against issues such as:

1. **Runaway execution** - Infinite loops or long computations that never return, starving other tasks
2. **Resource exhaustion attacks** - Malicious code holding host resources (file descriptors, sockets, memory, threads) without consuming CPU, starving the host (sometimes called a **slowloris-style denial of service**), right now this is probably not possible since the guest cant really pause without yielding to the host, but in the future we may add APIs that enable this.
3. **CPU quota enforcement** - Give each handler a fixed CPU budget (e.g., 100ms) to ensure fair sharing in multi-tenant scenarios

Execution monitors watch your handler and automatically kill it if it exceeds configured limits.

## Built-in Monitors

> **API note:** The code examples in this document use the **Rust crate** API
> (`handle_event_with_monitor`). The Node.js bindings expose the same
> functionality through the unified `callHandler()` method — see the
> [JS Host API README](../src/js-host-api/README.md) for JS usage.

Two monitors are provided, each behind a feature flag:

### WallClockMonitor (feature: `monitor-wall-clock`)

Terminates execution after a wall-clock (real time) duration.

```rust
use hyperlight_js::WallClockMonitor;
use std::time::Duration;

// Kill handler if it runs longer than 5 seconds
let monitor = WallClockMonitor::new(Duration::from_secs(5))?;
let result = loaded_sandbox.handle_event_with_monitor(
    "handler",
    "{}".to_string(),
    &monitor,
    None, // gc - defaults to true
)?;
```

### CpuTimeMonitor (feature: `monitor-cpu-time`)

Terminates execution after consuming a specified amount of **actual CPU time**.

```rust
use hyperlight_js::CpuTimeMonitor;
use std::time::Duration;

// Kill handler after 100ms of CPU time
let monitor = CpuTimeMonitor::new(Duration::from_millis(100))?;
let result = loaded_sandbox.handle_event_with_monitor(
    "handler",
    "{}".to_string(),
    &monitor,
    None,
)?;
```

### Using Both Together (Recommended) 🛡️

Wall-clock and CPU monitors are designed to be used **together** as a tuple to provide comprehensive protection:

- **`CpuTimeMonitor`** catches compute-bound abuse (e.g. tight loops)
- **`WallClockMonitor`** catches resource exhaustion where the guest consumes **host resources without consuming CPU** — e.g. blocking on host calls

Neither alone is sufficient: CPU-only misses idle resource holding; wall-clock-only is unfair to legitimately I/O-heavy workloads.

```rust
use hyperlight_js::{WallClockMonitor, CpuTimeMonitor};
use std::time::Duration;

// Tuple of monitors — first to trigger terminates execution (OR semantics)
let monitor = (
    WallClockMonitor::new(Duration::from_secs(5))?,
    CpuTimeMonitor::new(Duration::from_millis(500))?,
);
let result = loaded_sandbox.handle_event_with_monitor(
    "handler",
    "{}".to_string(),
    &monitor,
    None,
)?;
```

Tuples of up to 5 monitors are supported. Tuples implement the sealed `MonitorSet` trait (not `ExecutionMonitor` — they are a composition, not a single monitor).

**How it works:**

- Each sub-monitor's `get_monitor()` method is called on the calling thread (preserving thread-local state like CPU clock handles)
- All futures are raced via `tokio::select!` (generated at compile time by the tuple macro) on the shared monitor runtime
- The first future to complete wins and triggers `interrupt_handle.kill()`
- When a monitor fires, the `monitor_terminations_total` metric is emitted with the winning monitor's actual name as the `monitor_type` label (e.g. `monitor_type="cpu-time"`)
- The name is also logged as `triggered_by` at warn level

## Fail-Closed Semantics 🔒

If any monitor fails to initialize (`get_monitor()` returns `Err`), the handler is **never executed**. This ensures execution cannot proceed unmonitored due to a monitor initialization failure. This is a deliberate design choice.

For tuple monitors, if **any** sub-monitor fails to initialize, the entire tuple fails and the handler does not run.

## Feature Flags

Enable monitors in your `Cargo.toml`:

```toml
[dependencies]
hyperlight-js = { version = "0.17", features = ["monitor-wall-clock", "monitor-cpu-time"] }
```

| Feature | Dependencies | Description |
|---------|--------------|-------------|
| `monitor-wall-clock` | (none) | Wall-clock time monitor |
| `monitor-cpu-time` | `libc` (Linux), `windows-sys` (Windows) | CPU time monitor with OS-native APIs |

## Environment Variables

### `HYPERLIGHT_MONITOR_THREADS`

Controls the number of worker threads used by the monitor runtime.

```bash
export HYPERLIGHT_MONITOR_THREADS=4  # Default is 2
```

- Must be set **before** the first monitor is used
- Increase if you have many concurrent sandboxes

## Metrics

When a monitor terminates a handler, the following metric is emitted:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `monitor_terminations_total` | Counter | `monitor_type` | Number of times a monitor killed a handler |

The `monitor_type` label contains the actual monitor name that fired (e.g., `wall-clock`, `cpu-time`). For tuple monitors, the label is the specific sub-monitor that triggered termination, not a generic `composite` label.

See [Observability](./observability.md) for details on collecting metrics.

## The Shared Runtime

Execution monitors use a shared Tokio runtime to minimize overhead. The runtime is:

- **Lazily initialized** on first monitor use
- **Shared** across all monitors (wall-clock, CPU, and custom)
- **Cached via `OnceLock`** - thread-safe, zero runtime cost after initialization

The orchestration layer in `handle_event_with_monitor` spawns the monitor future on this runtime and aborts it when the handler completes. Individual monitor implementations do not interact with the runtime directly.

## Implementing a Custom Monitor

For custom monitoring logic (e.g., custom metrics, iteration limits), implement the `ExecutionMonitor` trait:

```rust
use hyperlight_js::ExecutionMonitor;
use hyperlight_host::Result;
use std::future::Future;
use std::time::Duration;

pub struct MyCustomMonitor {
    check_interval: Duration,
}

impl ExecutionMonitor for MyCustomMonitor {
    /// The method body runs on the calling thread — use it to capture
    /// thread-local state. The returned future will be spawned on the
    /// shared monitor runtime.
    ///
    /// The future should stay pending while everything is OK, and
    /// complete (return `()`) when execution should be terminated.
    /// It will be aborted if the handler finishes first.
    fn get_monitor(&self) -> Result<impl Future<Output = ()> + Send + 'static> {
        let interval = self.check_interval;

        Ok(async move {
            loop {
                hyperlight_js::monitor::sleep(interval).await;

                // Your custom check goes here.
                // Replace this with your actual condition:
                let limit_exceeded = true;
                if limit_exceeded {
                    tracing::warn!("Custom limit exceeded, terminating");
                    return;  // Future completes → orchestration calls kill()
                }
            }
        })
    }

    fn name(&self) -> &'static str {
        "my-custom-monitor"
    }
}
```

### Why `fn get_monitor()` instead of `async fn monitor()`

The `get_monitor()` method is deliberately **not** `async fn`. The method body executes synchronously on the **calling thread**, and returns a `Future` that will be spawned onto the shared monitor runtime (a separate tokio thread pool).

This two-phase design is required because some monitors need to capture thread-local state from the calling thread before monitoring begins. For example, `CpuTimeMonitor` uses `pthread_getcpuclockid(pthread_self())` to obtain a CPU clock handle for the thread that will execute the guest — this must happen on that thread, not on a tokio worker thread.

### Key Points for Custom Monitors

1. **Just implement `ExecutionMonitor`** — The sealed `MonitorSet` trait is automatically derived via a blanket impl. You never need to touch it.
2. **Return `Err` if initialization fails** - The handler call will fail (fail closed, never unmonitored)
3. **Future completes = terminate** - The orchestration layer calls `interrupt_handle.kill()` when your future completes
4. **Future stays pending = all good** - If the handler finishes normally, your future is aborted
5. **Don't call `kill()` yourself** - The orchestration handles it. Just return from the future
6. **Don't block the runtime** - Use async operations, not blocking calls
7. **Compose with tuples** - Your custom monitor can be combined with built-in monitors via tuples

### Composing Custom Monitors

Custom monitors compose naturally with built-in monitors via tuples:

```rust
let monitor = (
    WallClockMonitor::new(Duration::from_secs(5))?,
    CpuTimeMonitor::new(Duration::from_millis(500))?,
    MyCustomMonitor { check_interval: Duration::from_millis(100) },
);
loaded.handle_event_with_monitor("handler", "{}".into(), &monitor, None)?;
```

## Error Handling

When a handler is terminated by a monitor:

1. `handle_event_with_monitor()` returns an error
2. The sandbox becomes "poisoned" (`sandbox.poisoned() == true`)
3. To reuse the sandbox, call `sandbox.restore(&snapshot)`

```rust
let snapshot = loaded_sandbox.snapshot()?;

let result = loaded_sandbox.handle_event_with_monitor(
    "handler",
    "{}".to_string(),
    &monitor,
    None,
);

if result.is_err() && loaded_sandbox.poisoned() {
    // Handler was killed - restore to continue using sandbox
    loaded_sandbox.restore(&snapshot)?;
}
```

## Performance Considerations

- **Monitor overhead is minimal** - Shared runtime, no thread spawning per call
- **CPU monitoring uses adaptive polling** - Sleeps for half the remaining budget, clamped between 1ms and 10ms, tightening precision as the deadline approaches
- **Wall-clock monitoring uses `monitor::sleep`** - No busy waiting, async runtime abstracted
- **Feature flags** - Only pay for the monitor implementations you use (wall-clock, CPU time)
- **Tuple monitors** use compile-time `tokio::select!` single `Box::pin` for the composed future

## See Also

- [Examples README](../src/js-host-api/examples/README.md) - interrupt.js and cpu-timeout.js examples
- [JS Host API README](../src/js-host-api/README.md) - Node.js bindings with `callHandler()`
- [Observability](./observability.md) - Metrics including `monitor_terminations_total`
