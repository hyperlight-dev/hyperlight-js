#![allow(clippy::disallowed_macros)]
use std::thread::{spawn, JoinHandle};

use hyperlight_js::{LoadedJSSandbox, Result, SandboxBuilder, Script};

fn fn_writer(_msg: String) -> Result<i32> {
    Ok(0)
}

fn main() -> Result<()> {
    // Install prometheus metrics exporter.
    // We only install the metrics recorder here, but you can also use the
    // `metrics_exporter_prometheus::PrometheusBuilder::new().install()` method
    // to install a HTTP listener that serves the metrics.
    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus exporter");

    // generate some metrics
    do_stuff();

    // Render out the metrics in prometheus exposition format.
    // At this point, we should have created 20 of each sandbox, but 0 would be active
    // since they were dropped
    let payload = prometheus_handle.render();
    println!("Prometheus metrics:\n{}", payload);

    Ok(())
}

fn do_stuff() {
    let mut join_handles: Vec<JoinHandle<Result<()>>> = vec![];

    for _ in 0..20 {
        let handle = spawn(move || -> Result<()> {
            // Create a new JS sandbox.
            let mut js_sandbox = SandboxBuilder::new()
                .with_host_print_fn(fn_writer.into())
                .build()?
                .load_runtime()?;

            // Load a Some JS into the sandbox.

            let handler = Script::from_content(
                r#"
                function handler(event) {
                    event.request.uri = "/redirected.html";
                    return event
                }"#,
            );

            js_sandbox.add_handler("function1", handler.clone())?;

            js_sandbox.add_handler("function2", handler.clone())?;

            js_sandbox.add_handler("function3", handler.clone())?;

            let mut loaded_js_sandbox = js_sandbox.get_loaded_sandbox()?;

            // Call guest functions 50 times to generate some metrics.

            loaded_js_sandbox = call_funcs(loaded_js_sandbox, 50);

            js_sandbox = loaded_js_sandbox.unload()?;

            js_sandbox.add_handler("function1", handler.clone())?;

            js_sandbox.add_handler("function2", handler.clone())?;

            js_sandbox.add_handler("function3", handler)?;

            loaded_js_sandbox = js_sandbox.get_loaded_sandbox()?;

            // Call guest functions 50 times to generate some more metrics.

            call_funcs(loaded_js_sandbox, 50);

            Ok(())
        });

        join_handles.push(handle);
    }

    for join_handle in join_handles {
        let result = join_handle.join();
        assert!(result.is_ok());
    }
}

fn call_funcs(mut loaded_js_sandbox: LoadedJSSandbox, iterations: i32) -> LoadedJSSandbox {
    let mut count = 0;
    while count < iterations {
        let event = r#"
            {
                "request": {
                    "uri": "/index.html"
                }
            }"#;

        let _ = loaded_js_sandbox.handle_event("function1", event.to_string(), None);
        let _ = loaded_js_sandbox.handle_event("function2", event.to_string(), None);
        let _ = loaded_js_sandbox.handle_event("function3", event.to_string(), None);
        count += 1;
    }

    loaded_js_sandbox
}
