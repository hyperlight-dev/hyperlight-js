/*
Copyright 2025  The Hyperlight Authors.

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
extern crate hyperlight_js;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::spawn;
use std::{env, fs};

use anyhow::Result;
use hyperlight_js::{Result as HyperlightResult, SandboxBuilder, Script};
use opentelemetry::trace::TracerProvider;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::attribute::SERVICE_VERSION;
use rand::RngExt;
use tracing::{span, Level};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use uuid::Uuid;

const ENDPOINT_ADDR: &str = "http://localhost:4318/v1/traces";

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let provider = init_tracing_subscriber(ENDPOINT_ADDR)?;

    run_example()?;

    provider.shutdown()?;

    Ok(())
}

fn init_tracing_subscriber(
    addr: &str,
) -> Result<SdkTracerProvider, Box<dyn Error + Send + Sync + 'static>> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(addr)
        .build()?;

    let version = KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION"));
    let resource = Resource::builder()
        .with_service_name("hyperlight_js_otel_example")
        .with_attribute(version)
        .build();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    global::set_tracer_provider(provider.clone());
    let tracer = provider.tracer("trace-demo");

    let otel_layer = OpenTelemetryLayer::new(tracer);

    // Try using the environment otherwise set default filters
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::from_default_env()
            .add_directive("hyperlight_host=info".parse().unwrap())
            .add_directive("tracing=info".parse().unwrap())
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(otel_layer)
        .try_init()?;

    Ok(provider)
}

fn run_example() -> HyperlightResult<()> {
    // create a top-level span for this example
    let span = span!(Level::INFO, "example");
    let _entered = span.enter();

    let dir_path = match env::var("CARGO_MANIFEST_DIR") {
        Ok(val) => format!("{}/examples/data", val),
        Err(_e) => {
            let mut exe = env::current_exe().unwrap();
            exe.pop();
            exe.pop();
            exe.pop();
            exe.pop();
            exe.push("src/hyperlight_js/examples/data");
            exe.as_os_str().to_string_lossy().to_string()
        }
    };

    // load all of the example handlers and input data into HashMaps for
    // later use in worker threads
    let mut event_names = Vec::new();
    let mut event_data: HashMap<String, String> = HashMap::new();
    let mut event_handlers: HashMap<String, Script> = HashMap::new();

    for entry in std::fs::read_dir(dir_path.clone())? {
        let entry = entry?;
        let dir_name = entry.file_name().into_string().unwrap();

        // Make sure that there is a data.json and a handler.js file in the directory
        let data_path = PathBuf::from(format!("{}/{}/data.json", dir_path, dir_name));
        let handler_path = PathBuf::from(format!("{}/{}/handler.js", dir_path, dir_name));

        // check that the files exist
        if data_path.is_file() && handler_path.is_file() {
            event_names.push(dir_name.clone());
            event_data.insert(
                dir_name.clone(),
                format!("{}/data.json", entry.path().as_os_str().to_string_lossy()),
            );

            let handler_path = format!("{}/handler.js", entry.path().as_os_str().to_string_lossy());
            let handler = Script::from_file(handler_path)?;

            event_handlers.insert(dir_name.clone(), handler);
        } else {
            println!("skipping directory: {}", dir_name);
            if !data_path.is_file() {
                println!("  missing file: data.json");
            }
            if !handler_path.is_file() {
                println!("  missing file: handler.js");
            }
        }
    }

    let mut handles = Vec::new();

    // Create 5 worker threads that each create a new sandbox and execute random functions
    // from the samples folder
    for _ in 0..5 {
        let span = span.clone();
        let event_names_shared = Arc::new(event_names.clone());
        let event_data_shared = Arc::new(event_data.clone());
        let event_handlers_shared = Arc::new(event_handlers.clone());
        let handle = spawn(move || -> Result<()> {
            // We need to re-enter the parent span in a new thread for tracing to work correctly!
            let _entered = span.enter();

            // create a new span for each worker thread
            let id = Uuid::new_v4();
            let span = span!(Level::INFO, "worker thread", %id);
            let _entered = span.enter();

            // Build a new ProtoJSSandbox
            let proto_js_sandbox = SandboxBuilder::new().build()?;

            // Register host functions required by the JavaScript handlers here

            // load the runtime
            let mut js_sandbox = proto_js_sandbox.load_runtime()?;

            // load all of the event handlers into the sandbox
            let mut event_handlers = HashMap::new();
            event_handlers.clone_from(&event_handlers_shared);
            for (name, handler) in event_handlers.into_iter() {
                js_sandbox.add_handler(name.clone(), handler)?;
            }

            // create loaded sandbox
            let mut loaded_sbox = js_sandbox.get_loaded_sandbox()?;

            // call a random function a random (from 0-20) times
            for _ in 0..rand::rng().random_range(0..20) {
                let function_id = rand::rng().random_range(0..event_names_shared.len());
                let function = event_names_shared.get(function_id).unwrap();
                let event_path = event_data_shared.get(function.as_str()).unwrap().clone();
                invoke_function(&mut loaded_sbox, function.to_string(), event_path)?;
            }
            Ok(())
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap()?;
    }

    Ok(())
}

fn invoke_function(
    loaded_sbox: &mut hyperlight_js::LoadedJSSandbox,
    function_name: String,
    event_path: String,
) -> Result<()> {
    let event = fs::read_to_string(event_path)?;

    match loaded_sbox.handle_event(function_name.clone(), event, None) {
        Ok(_res) => {}
        Err(e) => {
            println!("Error calling function: {} : {:?}", function_name, e);
        }
    }
    Ok(())
}
