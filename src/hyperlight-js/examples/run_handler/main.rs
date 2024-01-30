#![allow(clippy::disallowed_macros)]
use std::collections::HashMap;
use std::path::PathBuf;
use std::{env, fs};

use anyhow::Result;
use env_logger::Env;
use hyperlight_js::{SandboxBuilder, Script};
use serde_json::Value;
use tracing_subscriber::prelude::*;

fn main() -> Result<()> {
    #[cfg(not(debug_assertions))]
    env_logger::Builder::from_env(Env::default().default_filter_or("error")).init();
    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(Env::default().default_filter_or("hyperlight_guest=trace"))
        .init();

    // Figure out which example to run
    // based on the command line arguments
    //
    // Each example is in a named directory
    // the argument is the name of the example to run (the name of the directory)
    //
    // if no argument is provided, prompt for the example to run
    //
    // if the argument does not specify the name of a directory, the echo example is run

    let mut tracy = false;
    let mut path = "";
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        path = &args[1];
    }

    // if the environment variable ENABLE_TRACY is set, enable the Tracy layer

    if let Ok(val) = env::var("ENABLE_TRACY") {
        // if the value is "1" or "true" enable Tracy
        if val == "1" || val.to_lowercase() == "true" {
            tracy = true;
        }
    }

    if tracy {
        println!("Adding Tracy layer to tracing subscriber");
        let registry = tracing_subscriber::registry().with(tracing_tracy::TracyLayer::default());
        tracing::subscriber::set_global_default(registry)?;
    }

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

    let mut events: HashMap<String, String> = HashMap::new();
    // set timeout to 5 seconds in debug mode as tracing the guest functions is slow
    #[cfg(debug_assertions)]
    let proto_js_sandbox = SandboxBuilder::new().build()?;
    #[cfg(not(debug_assertions))]
    let proto_js_sandbox = SandboxBuilder::new().build()?;

    // Any host functions required by the JavaScript handlers should be registered here

    // now load the runtime

    let mut js_sandbox = proto_js_sandbox.load_runtime()?;

    // check each directory in the data directory for sample events and handler functions
    // for each one found, add the handler to the sandbox and store the event data in hashmap
    // to run the sample later

    let mut num_handlers = 0;
    for entry in std::fs::read_dir(dir_path.clone())? {
        let entry = entry?;
        let dir_name = entry.file_name().into_string().unwrap();

        //Make sure that there is an data.json and a handler.js file in the directory
        let data_path = PathBuf::from(format!("{}/{}/data.json", dir_path, dir_name));
        let handler_path = PathBuf::from(format!("{}/{}/handler.js", dir_path, dir_name));

        // check that the files exist
        if data_path.is_file() && handler_path.is_file() {
            events.insert(
                dir_name.clone(),
                format!("{}/data.json", entry.path().as_os_str().to_string_lossy()),
            );

            let handler_path = format!("{}/handler.js", entry.path().as_os_str().to_string_lossy());
            let handler = Script::from_file(handler_path)?;
            js_sandbox.add_handler(dir_name, handler)?;
            num_handlers += 1;
        } else {
            println!("skipping directory: {}", dir_name);
            if !data_path.is_file() {
                println!("missing file: data.json");
            }
            if !handler_path.is_file() {
                println!("missing file: handler.js");
            }
        }
    }

    // create and load sandbox

    let start = std::time::Instant::now();
    let mut loaded_sbox = js_sandbox.get_loaded_sandbox()?;
    let elapsed = start.elapsed();
    println!(
        "Time to get loaded sandbox with  {} handlers: {:?}",
        num_handlers, elapsed
    );
    let mut input = String::new();
    match path {
        "" => {
            // read the path of the sample to run from the stdin:
            loop {
                println!("Enter the name of the example to run or 'exit' to quit:");
                input.clear();
                std::io::stdin().read_line(&mut input)?;
                input = input.trim().to_string();

                if input == "exit" {
                    break;
                }
                if !events.contains_key(input.as_str()) {
                    println!("The example {} does not exist", input);
                    continue;
                };
                let event_path = events.get(input.as_str()).unwrap().clone();
                invoke_function(&mut loaded_sbox, input.clone(), event_path)?;
            }
        }
        other => {
            // check if the example exists
            let function = match events.contains_key(other) {
                true => other,
                false => "echo",
            };
            println!("Running example: {}", function);
            let event_path = events.get(function).unwrap().clone();
            invoke_function(&mut loaded_sbox, function.to_string(), event_path)?;
        }
    }

    Ok(())
}

fn pretty_print_json(json_string: &str) -> Result<()> {
    let v: Value = serde_json::from_str(json_string)?;
    let pretty_json = serde_json::to_string_pretty(&v)?;
    println!("{}", pretty_json);
    Ok(())
}

fn invoke_function(
    loaded_sbox: &mut hyperlight_js::LoadedJSSandbox,
    function_name: String,
    event_path: String,
) -> Result<()> {
    let event = fs::read_to_string(event_path)?;
    println!("handler input:");
    pretty_print_json(&event)?;
    // handle request using registered handler
    let start = std::time::Instant::now();

    match loaded_sbox.handle_event(function_name.clone(), event, None) {
        Ok(res) => {
            let elapsed = start.elapsed();
            println!("handler output:");
            pretty_print_json(&res)?;
            println!("Time to execute: {:?}", elapsed);
        }
        Err(e) => {
            println!("Error calling function: {} : {:?}", function_name, e);
        }
    }
    Ok(())
}
