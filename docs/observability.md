# Observability

hyperlight-js provides the following observability features:

* [Metrics](#metrics) metrics are provided using Prometheus.

## Metrics

hyperlight-js provides metrics using Prometheus. The metrics are registered using either the [default_registry](https://docs.rs/prometheus/latest/prometheus/fn.default_registry.html) or a registry instance provided by the host application.

To provide a registry to hyperlight-js, use the `set_metrics_registry` function and pass a reference to a registry with `static` lifetime:

```rust
use hyperlight_host::metrics::set_metrics_registry;
use prometheus::Registry;
use lazy_static::lazy_static;

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
}

set_metrics_registry(&REGISTRY);
```

The following metrics are provided and are enabled by default:

* `hyperlight_guest_error_count` - a vector of counters that tracks the number of guest errors by code and message.
* `hyperlight_number_of_cancelled_guest_execution` - a counter that tracks the number of guest executions that have been cancelled because the execution time exceeded the time allowed.
* `active_js_sandboxes` - a gauge that tracks the current number of JS sandboxes in this process.
* `active_loaded_js_sandboxes` - a gauge that tracks the current number of loaded JS sandboxes in this process.
* `sandbox_unloads_total` - a counter that tracks the number of times that `unload` has been called on a LoadedJSSandbox.
* `sandbox_loads_total` - a counter that tracks the number of times that `load` has been called on a JSSandbox.
* `active_proto_js_sandboxes` - a gauge that tracks the current number of proto JS sandboxes in this process.
* `js_sandboxes_total` - a counter that tracks the total number of JS sandboxes that have been created by this process.
* `loaded_js_sandboxes_total` - a counter that tracks the total number of loaded JS sandboxes that have been created by this process.
* `proto_js_sandboxes_total` - a counter that tracks the total number of proto JS sandboxes that have been created by this process.
* `monitor_terminations_total` - a counter that tracks the number of times an execution monitor terminated a handler, labelled by `monitor_type` with the actual monitor name that fired (e.g., `wall-clock`, `cpu-time`). For tuple monitors, the label is the specific sub-monitor that triggered termination — not a generic `composite` label.

The following metrics are provided and are enabled by default using the feature `function_call_metrics` but can be disabled:

* `event_handler_calls_total` - a histogram that tracks the total number of event handler calls, labelled by `event_handler_name`.
* `event_handler_calls_with_gc_total` - a histogram that tracks the total number of event handler calls that include garbage collection, labelled by `event_handler_name`.
* `hyperlight_guest_function_call_duration_microseconds` - a vector of histograms that tracks the execution time of guest functions in microseconds by function name. The histogram also tracks the number of calls to each function.
* `hyperlight_host_function_calls_duration_microseconds` - a vector of histograms that tracks the execution time of host functions in microseconds by function name. The histogram also tracks the number of calls to each function.

There is an example of how to gather metrics in the [examples/metrics](../src/hyperlight-js/examples/metrics) directory.

## JS Runtime Tracing

To trace the guest JS runtime, use the `trace_guest` feature for the `hyperlight-js` crate. This enables tracing of the guest JS runtime using the [tracing](https://docs.rs/tracing/latest/tracing/) crate.

There are already some tracing spans and events in the hyperlight-js codebase that get emitted when this feature is enabled.
To collect and view the traces, you need to use a subscriber that implements the `opentelemetry` protocol, such as the [tracing-opentelemetry](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/) crate.

There is an example of how to set up tracing with `tracing-opentelemetry` in the [examples/tracing-otlp](../src/hyperlight-js/examples/tracing-otlp) directory.
You need to have an `OpenTelemetry` collector running to receive and export the traces to your desired back-end (e.g., Jaeger, Zipkin, etc.).
To run the tracing example with Docker, you can use the following command to start an OpenTelemetry collector that exports traces to Jaeger:

```bash
docker run -p 16686:16686 -p 4317:4317 -p 4318:4318 -e COLLECTOR_OTLP_ENABLED=true jaegertracing/all-in-one:latest
```

Then, you can run the tracing example with the `trace_guest` feature enabled:

```bash
RUST_LOG="hyperlight_guest=Trace,hyperlight_guest_bin=Trace" cargo run --example tracing-otlp --features trace_guest
```

You can then view the traces in the Jaeger UI at `http://localhost:16686`.
