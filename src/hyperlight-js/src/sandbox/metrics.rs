/*!
This module contains the definitions and implementations of the metrics used by the sandbox module
*/

use tracing::{instrument, Level};

use crate::{JSSandbox, LoadedJSSandbox, ProtoJSSandbox};

// Gauges, active sandboxes
static METRIC_ACTIVE_JS_SANDBOXES: &str = "active_js_sandboxes";
static METRIC_ACTIVE_LOADED_JS_SANDBOXES: &str = "active_loaded_js_sandboxes";
static METRIC_ACTIVE_PROTO_JS_SANDBOXES: &str = "active_proto_js_sandboxes";

// Counters, total sandboxes created during lifetime of the process
static METRIC_TOTAL_JS_SANDBOXES: &str = "js_sandboxes_total";
static METRIC_TOTAL_LOADED_JS_SANDBOXES: &str = "loaded_js_sandboxes_total";
static METRIC_TOTAL_PROTO_JS_SANDBOXES: &str = "proto_js_sandboxes_total";

// Counters, total number of times loaded sandboxes have been loaded/unloaded during the lifetime of the process
pub(crate) static METRIC_SANDBOX_LOADS: &str = "sandbox_loads_total";
pub(crate) static METRIC_SANDBOX_UNLOADS: &str = "sandbox_unloads_total";

// Counters, execution monitor terminations
pub(crate) static METRIC_MONITOR_TERMINATIONS: &str = "monitor_terminations_total";
pub(crate) static METRIC_MONITOR_TYPE_LABEL: &str = "monitor_type";

// Counters, total number of times event handlers have been called
#[cfg(feature = "function_call_metrics")]
static METRIC_EVENT_HANDLER_CALLS: &str = "event_handler_calls_total";
#[cfg(feature = "function_call_metrics")]
static METRIC_EVENT_HANDLER_CALLS_WITH_GC: &str = "event_handler_calls_with_gc_total";
#[cfg(feature = "function_call_metrics")]
static METRIC_EVENT_HANDLER_NAME: &str = "event_handler_name";

pub(crate) trait SandboxMetricsTrait {
    const GAUGE: &'static str;
    const COUNTER: &'static str;
}

pub(crate) struct SandboxMetricsGuard<T: SandboxMetricsTrait>(std::marker::PhantomData<T>);

#[cfg(feature = "function_call_metrics")]
pub(crate) struct EventHandlerMetricGuard<'a> {
    func_name: &'a str,
    gc: bool,
    start: std::time::Instant,
}

#[cfg(feature = "function_call_metrics")]
impl<'a> EventHandlerMetricGuard<'a> {
    #[instrument(skip_all, level=Level::DEBUG)]
    pub(crate) fn new(func_name: &'a str, gc: bool) -> Self {
        let start = std::time::Instant::now();
        Self {
            func_name,
            gc,
            start,
        }
    }
}

#[cfg(feature = "function_call_metrics")]
impl Drop for EventHandlerMetricGuard<'_> {
    #[instrument(skip_all, level=Level::DEBUG)]
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let func_name = self.func_name.to_string();
        if self.gc {
            metrics::histogram!(METRIC_EVENT_HANDLER_CALLS_WITH_GC, METRIC_EVENT_HANDLER_NAME => func_name).record(duration.as_micros() as f64);
        } else {
            metrics::histogram!(METRIC_EVENT_HANDLER_CALLS, METRIC_EVENT_HANDLER_NAME => func_name)
                .record(duration.as_micros() as f64);
        }
    }
}

impl<T: SandboxMetricsTrait> SandboxMetricsGuard<T> {
    #[instrument(skip_all, level=Level::DEBUG)]
    pub(crate) fn new() -> Self {
        metrics::gauge!(T::GAUGE).increment(1);
        metrics::counter!(T::COUNTER).increment(1);
        Self(std::marker::PhantomData)
    }
}

impl<T: SandboxMetricsTrait> Drop for SandboxMetricsGuard<T> {
    #[instrument(skip_all, level=Level::DEBUG)]
    fn drop(&mut self) {
        metrics::gauge!(T::GAUGE).decrement(1);
    }
}

impl SandboxMetricsTrait for JSSandbox {
    const GAUGE: &'static str = METRIC_ACTIVE_JS_SANDBOXES;
    const COUNTER: &'static str = METRIC_TOTAL_JS_SANDBOXES;
}

impl SandboxMetricsTrait for LoadedJSSandbox {
    const GAUGE: &'static str = METRIC_ACTIVE_LOADED_JS_SANDBOXES;
    const COUNTER: &'static str = METRIC_TOTAL_LOADED_JS_SANDBOXES;
}

impl SandboxMetricsTrait for ProtoJSSandbox {
    const GAUGE: &'static str = METRIC_ACTIVE_PROTO_JS_SANDBOXES;
    const COUNTER: &'static str = METRIC_TOTAL_PROTO_JS_SANDBOXES;
}

#[cfg(test)]
mod tests {
    use crate::{SandboxBuilder, Script};

    fn get_valid_handler() -> Script {
        Script::from_content(
            r#"
        function handler(event) {
            event.request.uri = "/redirected.html";
            return event
        }
        "#,
        )
    }

    fn get_valid_event() -> String {
        r#"
        {
            "request": {
                "uri": "/index.html"
            }
        }
        "#
        .to_string()
    }

    #[test]
    #[ignore = "Needs to run separately to not get influenced by other tests"]
    fn test_metrics() {
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder.install().unwrap();

        let snapshot = {
            let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
            let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

            sandbox
                .add_handler("handler".to_string(), get_valid_handler())
                .unwrap();

            let mut loaded_js_sandbox = sandbox.get_loaded_sandbox().unwrap();
            let gc = Some(true);
            let result =
                loaded_js_sandbox.handle_event("handler".to_string(), get_valid_event(), gc);

            assert!(result.is_ok());
            snapshotter.snapshot()
        };
        let snapshot = snapshot.into_vec();
        println!("Metrics snapshot: {:#?}", snapshot);
        if cfg!(feature = "function_call_metrics") {
            assert_eq!(snapshot.len(), 8);
        } else {
            assert_eq!(snapshot.len(), 7);
        }
    }
}
