use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context as _, Result};
use clap::Parser;
use tracing::instrument;

struct Host;

impl hyperlight_js_runtime::host::Host for Host {
    fn resolve_module(&self, base: String, name: String) -> Result<String> {
        let base = PathBuf::from(base);
        let path = base.join(&name);

        let path = path
            .canonicalize()
            .with_context(|| format!("Resolving module {name:?} from {base:?}"))?;
        Ok(path.display().to_string())
    }

    fn load_module(&self, name: String) -> Result<String> {
        fs::read_to_string(&name).with_context(|| format!("Loading module {name:?}"))
    }
}

const EXAMPLES: &str = "\u{001b}[1;4mExamples:\u{001b}[0m
  Run a handler script located at ./handler.js with an event '{\"name\":\"hyperlight-js-runtime\"}':
    $ cat ./handler.js
    function handler(event) {
        return `hello ${event.name}`
    }

    $ hyperlight-js-runtime ./handler.js '{\"name\":\"hyperlight-js-runtime\"}'
    Handler result: \"hello hyperlight-js-runtime\"

  Example handler script (index.js) and module (math.js):
    $ cat ./index.js
    import * as math from './math.js';
    function handler(event) {
        console.log(JSON.stringify(event));
        return math.add(event.a, 41);
    }

    $ cat ./math.js
    const add = (a, b) => a + b;
    export { add };

    $ hyperlight-js-runtime ./index.js '{\"a\":1,\"b\":[1,2,3]}'
    {\"a\":1,\"b\":[1,2,3]}
    Handler result: 42
";

/// Run a JavaScript handler script with a given event as they would have been run inside `hyperlight-js`.
///
/// The handler script is expected to export a function named `handler` that takes a single argument
/// (the event) and returns a value.
#[derive(clap::Parser)]
#[command(version, about)]
#[clap(after_help = EXAMPLES)]
struct Cli {
    /// The path to the JavaScript handler script file.
    file: PathBuf,

    /// The event to pass to the handler function as a JSON string.
    event: String,
}

#[instrument(skip_all, level = "info")]
fn main() -> Result<()> {
    let Cli { file, event } = Cli::parse();

    let handler_script = fs::read_to_string(&file)
        .with_context(|| format!("Reading handler script from {:?}", file))?;

    let handler_pwd = file.parent().unwrap_or_else(|| Path::new("."));

    env::set_current_dir(handler_pwd).with_context(|| {
        format!("Setting current directory to handler script directory {handler_pwd:?}")
    })?;

    let mut runtime = hyperlight_js_runtime::JsRuntime::new(Host)?;

    runtime.register_host_function("fs", "readFile", move |path: String| -> Result<String> {
        Ok(fs::read_to_string(&path)?)
    })?;

    runtime.register_handler("handler".to_string(), handler_script, String::from("."))?;

    let result = runtime.run_handler("handler".to_string(), event, false)?;
    println!("Handler result: {result}");

    Ok(())
}
