/*
Copyright 2026  The Hyperlight Authors.

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

use std::path::PathBuf;

use hyperlight_js::{
    HostFunctionFilter, HostFunctionModule, LoadedJSSandbox, SandboxBuilder, Script, Snapshot,
};

fn host_function_modules() -> Vec<HostFunctionModule> {
    let mut counter = HostFunctionModule::new("host:counter");
    counter.register("step", |value: i32| value + 1);
    vec![counter]
}

fn main() -> hyperlight_js::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "write".to_owned());
    let path = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/persistent-js-snapshot"));

    match command.as_str() {
        "write" => write_snapshot(path),
        "read" => read_snapshot(path),
        other => Err(hyperlight_js::new_error!(
            "Unknown command '{other}'; expected 'write' or 'read'"
        )),
    }
}

fn write_snapshot(path: PathBuf) -> hyperlight_js::Result<()> {
    let mut sandbox = SandboxBuilder::new()
        .with_host_function_modules(host_function_modules)
        .build()?
        .load_runtime()?;
    sandbox.add_handler(
        "counter",
        Script::from_content(
            r#"
            import * as counter from "host:counter";
            let count = 0;
            function handler(event) {
                count = counter.step(count);
                event.count = count;
                return event;
            }
            "#,
        ),
    )?;
    let mut loaded = sandbox.get_loaded_sandbox()?;
    let result = loaded.handle_event("counter", "{}".to_owned(), None)?;
    println!("Before snapshot: {result}");

    let snapshot = loaded.snapshot()?;
    let digest = snapshot.save(path, "application-ready")?;
    println!("Saved application-ready as {digest}");
    Ok(())
}

fn read_snapshot(path: PathBuf) -> hyperlight_js::Result<()> {
    let snapshot = Snapshot::load(path, "application-ready")?;
    let restorer = SandboxBuilder::new()
        .with_host_function_modules(host_function_modules)
        .with_host_function_filter(HostFunctionFilter::snapshot_requirements())
        .build_from_snapshot(snapshot)?;
    let mut loaded = restorer.restore::<LoadedJSSandbox>()?;
    let result = loaded.handle_event("counter", "{}".to_owned(), None)?;
    println!("After restore: {result}");
    Ok(())
}
