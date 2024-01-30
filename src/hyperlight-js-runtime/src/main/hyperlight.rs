extern crate alloc;

use core::ffi::*;
use core::time::Duration;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use anyhow::{anyhow, Context as _};
use hashbrown::HashMap;
use hyperlight_common::flatbuffer_wrappers::function_call::FunctionCall;
use hyperlight_common::flatbuffer_wrappers::guest_error::ErrorCode;
use hyperlight_common::flatbuffer_wrappers::util::get_flatbuffer_result;
use hyperlight_common::func::ParameterTuple;
use hyperlight_guest::error::{HyperlightGuestError, Result};
use hyperlight_guest_bin::{guest_function, host_function};
use spin::Mutex;
use tracing::instrument;

struct Host;

pub trait CatchGuestErrorExt {
    type Ok;
    fn catch(self) -> anyhow::Result<Self::Ok>;
}

impl<T> CatchGuestErrorExt for hyperlight_guest::error::Result<T> {
    type Ok = T;
    fn catch(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow!("{}: {}", String::from(e.kind), e.message))
    }
}

impl hyperlight_js_runtime::host::Host for Host {
    fn resolve_module(&self, base: String, name: String) -> anyhow::Result<String> {
        #[host_function("ResolveModule")]
        fn resolve_module(base: String, name: String) -> Result<String>;

        resolve_module(base.clone(), name.clone())
            .catch()
            .with_context(|| format!("Resolving module {name:?} from {base:?}"))
    }

    fn load_module(&self, name: String) -> anyhow::Result<String> {
        #[host_function("LoadModule")]
        fn load_module(name: String) -> Result<String>;

        load_module(name.clone())
            .catch()
            .with_context(|| format!("Loading module {name:?}"))
    }
}

static RUNTIME: spin::Lazy<Mutex<hyperlight_js_runtime::JsRuntime>> = spin::Lazy::new(|| {
    Mutex::new(hyperlight_js_runtime::JsRuntime::new(Host).unwrap_or_else(|e| {
        panic!("Failed to initialize JS runtime: {e:#?}");
    }))
});

#[unsafe(no_mangle)]
#[instrument(skip_all, level = "info")]
pub extern "C" fn hyperlight_main() {
    // dereference RUNTIME to force its initialization
    // of the Lazy static
    let _ = &*RUNTIME;
}

#[guest_function("register_handler")]
#[instrument(skip_all, level = "info")]
fn register_handler(
    function_name: String,
    handler_script: String,
    handler_pwd: String,
) -> Result<()> {
    RUNTIME
        .lock()
        .register_handler(function_name, handler_script, handler_pwd)?;
    Ok(())
}

#[host_function("CallHostJsFunction")]
fn call_host_js_function(module_name: String, func_name: String, args: String) -> Result<String>;

#[guest_function("RegisterHostModules")]
fn register_host_modules(host_modules_json: String) -> Result<()> {
    // The serialization in here has to match the serialization of
    // HostModule in src/hyperlight_js/src/sandbox/host_fn.rs
    let host_modules: HashMap<String, Vec<String>> = serde_json::from_str(&host_modules_json)
        .map_err(|e| {
            HyperlightGuestError::new(
                ErrorCode::GuestError,
                format!("Failed to parse host modules JSON: {e:#?}"),
            )
        })?;

    let mut runtime = RUNTIME.lock();

    for (module_name, functions) in host_modules {
        for function_name in functions {
            let module_name = module_name.clone();
            runtime.register_json_host_function(
                module_name.clone(),
                function_name.clone(),
                move |args: String| -> anyhow::Result<String> {
                    call_host_js_function(module_name.clone(), function_name.clone(), args)
                        .map_err(|e| anyhow!("Calling host function {module_name:?} {function_name:?} failed: {e:#?}"))
                },
            )?;
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn srand(_seed: u32) {
    // No-op
}

#[unsafe(no_mangle)]
pub fn guest_dispatch_function(function_call: FunctionCall) -> Result<Vec<u8>> {
    let params = function_call.parameters.unwrap_or_default();
    let function_name = function_call.function_name;
    let (event, run_gc) = ParameterTuple::from_value(params)?;
    let result = RUNTIME.lock().run_handler(function_name, event, run_gc)?;
    Ok(get_flatbuffer_result(result.as_str()))
}

/// # Safety
/// This function is used by the C code to get the current time in seconds and nanoseconds.
/// `ts` must be a valid pointer to an array of two `u64` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _current_time(ts: *mut u64) -> c_int {
    #[host_function("CurrentTimeMicros")]
    fn current_time_micros() -> Result<u64>;

    let dur = current_time_micros().unwrap_or(1609459200u64 * 1_000_000u64);
    let dur = Duration::from_micros(dur);

    let ts = unsafe { core::slice::from_raw_parts_mut(ts, 2) };
    ts[0] = dur.as_secs();
    ts[1] = dur.subsec_nanos() as u64;

    0
}

#[unsafe(no_mangle)]
pub extern "C" fn putchar(c: c_int) -> c_int {
    unsafe { hyperlight_guest_bin::host_comm::_putchar(c as c_char) };
    if c == '\n' as c_int {
        // force a flush of the internal buffer in the hyperlight putchar implementation
        unsafe { hyperlight_guest_bin::host_comm::_putchar(0) };
    }
    (c as c_char) as c_int
}

#[unsafe(no_mangle)]
pub extern "C" fn fflush(f: *mut c_void) -> c_int {
    if !f.is_null() {
        // we only support flushing all streams, and stdout is our only stream
        return -1;
    }
    // flush stdout
    putchar('\0' as _);
    0
}
