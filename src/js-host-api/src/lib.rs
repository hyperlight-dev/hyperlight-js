use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyperlight_js::{
    CpuTimeMonitor, HyperlightError, InterruptHandle, JSSandbox, LoadedJSSandbox, ProtoJSSandbox,
    SandboxBuilder, Script, Snapshot, WallClockMonitor,
};
use napi_derive::napi;
use serde_json::Value as JsonValue;

// ── napi-rs wrapper architecture ──────────────────────────────────────
//
// ## Why every wrapper uses `Arc<Mutex<Option<T>>>`
//
// napi-rs only exposes `&self` on `#[napi]` methods — there is no `&mut self`
// support. Hyperlight's sandbox types need mutable access for most operations,
// so we use interior mutability via `Mutex`.
//
// The `Option` enables **one-shot consumption**: when a type transitions to the
// next state (e.g. `JSSandbox` → `LoadedJSSandbox`), we `.take()` the inner
// value. Subsequent calls see `None` and get a clear "already consumed" error
// instead of silently operating on stale state.
//
// The `Arc` is needed because napi-rs async methods move `self` into a
// `spawn_blocking` closure on a background thread — we clone the `Arc` so the
// wrapper struct remains valid on the JS side while the Rust side works.
//
// ## Why `LoadedJSSandboxWrapper` stores fields outside the Mutex
//
// `call_handler()` holds the Mutex for the **entire duration** of
// guest code execution (potentially seconds). Two sync getters need
// to be callable during that time:
//
// - `interruptHandle` — the whole point is to `kill()` a *running* handler
// - `poisoned` — callers want to check state without blocking the event loop
//
// Both are cloned out of the `LoadedJSSandbox` at construction time and stored
// as separate `Arc` fields that never touch the Mutex. The `poisoned_flag`
// (`AtomicBool`) is updated inside every `spawn_blocking` closure where we
// already hold the lock, so it stays in sync without extra contention.

// ── Error codes ──────────────────────────────────────────────────────
//
// ## Why we embed error codes in the message string
//
// napi-rs supports custom error status types (`Error<S>`) for **sync**
// functions, but the `ToNapiValue` impl for `Result<T>` (used by async
// function return paths) is only implemented for `Result<T, Error<Status>>`.
// Our entire API is async (`spawn_blocking`), so we can't use a custom
// status type without hitting a compile error.
//
// **Workaround**: We use standard `napi::Result<T>` (= `Result<T, Error<Status>>`)
// and prefix each error message with `[ERR_CODE]`. A thin JavaScript wrapper
// (`lib.js`) parses the prefix and sets `error.code` on the JS side.
//
// **What would fix this properly**: napi-rs would need to implement
// `ToNapiValue` for `Result<T, S>` (generic over the error status type),
// not just `Result<T>`. This would allow:
// ```rust
// type HlResult<T> = Result<T, napi::Error<ErrorCode>>;
// #[napi]
// pub async fn call_handler(...) -> HlResult<JsonValue> { ... }
// ```
// See: https://github.com/napi-rs/napi-rs — `crates/napi/src/bindgen_runtime/js_values.rs`
//
// Until then, this workaround provides structured `error.code` values
// on the JS side without any consumer-visible hacks.

/// Domain-specific error codes for the Hyperlight JS host API.
///
/// Each variant maps to an `ERR_*` string that appears as `error.code`
/// on the JavaScript side, following the Node.js convention.
///
/// These codes are embedded as `[ERR_*]` prefixes in error messages by the
/// Rust side, then extracted and set as `error.code` by the JS wrapper.
#[derive(Debug)]
enum ErrorCode {
    /// Sandbox is in a poisoned (inconsistent) state — restore or unload.
    Poisoned,
    /// Execution was cancelled by the host (monitor timeout or manual `kill()`).
    Cancelled,
    /// Guest stack overflow — increase stack size or reduce recursion depth.
    StackOverflow,
    /// Guest abort (trap, panic, or fatal error in guest code).
    GuestAbort,
    /// Invalid arguments (bad types, empty names, zero sizes).
    InvalidArg,
    /// Object has already been consumed — each transition is one-shot.
    Consumed,
    /// Internal / unexpected failure (lock poison, task join error, etc.).
    Internal,
}

impl ErrorCode {
    /// Returns the `ERR_*` code string (e.g. `"ERR_POISONED"`).
    fn as_code(&self) -> &'static str {
        match self {
            Self::Poisoned => "ERR_POISONED",
            Self::Cancelled => "ERR_CANCELLED",
            Self::StackOverflow => "ERR_STACK_OVERFLOW",
            Self::GuestAbort => "ERR_GUEST_ABORT",
            Self::InvalidArg => "ERR_INVALID_ARG",
            Self::Consumed => "ERR_CONSUMED",
            Self::Internal => "ERR_INTERNAL",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Minimum allowed timeout value in milliseconds.
const MIN_TIMEOUT_MS: u32 = 1;

/// Maximum allowed timeout value in milliseconds (1 hour).
/// Guards against accidental negative values from JS (which wrap to ~49.7 days
/// via ECMAScript's ToUint32 conversion) and other unreasonable durations.
const MAX_TIMEOUT_MS: u32 = 3_600_000;

/// Creates a napi error with a `[ERR_CODE]` prefix in the message.
///
/// The JS wrapper (`lib.js`) parses this prefix and promotes it to
/// `error.code`, giving consumers structured error handling:
///
/// ```js
/// try { await loaded.callHandler(...); }
/// catch (e) {
///     if (e.code === 'ERR_POISONED') { await loaded.restore(snapshot); }
/// }
/// ```
fn hl_error(code: ErrorCode, msg: impl std::fmt::Display) -> napi::Error {
    napi::Error::new(napi::Status::GenericFailure, format!("[{}] {}", code, msg))
}

// ── Error conversion ─────────────────────────────────────────────────

/// Maps [`HyperlightError`] variants to napi errors with structured codes.
fn to_napi_error(err: HyperlightError) -> napi::Error {
    let code = match &err {
        HyperlightError::PoisonedSandbox => ErrorCode::Poisoned,
        HyperlightError::ExecutionCanceledByHost() => ErrorCode::Cancelled,
        HyperlightError::JsonConversionFailure(_) => ErrorCode::InvalidArg,
        HyperlightError::StackOverflow() => ErrorCode::StackOverflow,
        HyperlightError::GuestAborted(_, _) => ErrorCode::GuestAbort,
        _ => ErrorCode::Internal,
    };
    hl_error(code, err)
}

/// Creates an error for "already consumed" conditions.
fn consumed_error(type_name: &str) -> napi::Error {
    hl_error(
        ErrorCode::Consumed,
        format!("{type_name} has already been consumed — each instance can only be used once"),
    )
}

/// Creates an error for invalid argument conditions.
fn invalid_arg_error(msg: &str) -> napi::Error {
    hl_error(ErrorCode::InvalidArg, msg)
}

/// Creates an error when a Mutex is poisoned (Rust-level, not sandbox-level).
fn lock_error() -> napi::Error {
    hl_error(
        ErrorCode::Internal,
        "Internal lock poisoned — this is a bug",
    )
}

/// Converts a tokio `JoinError` from `spawn_blocking` into an error.
fn join_error(err: tokio::task::JoinError) -> napi::Error {
    hl_error(
        ErrorCode::Internal,
        format!("Background task failed: {err}"),
    )
}

// ── Snapshot ─────────────────────────────────────────────────────────

/// A captured point-in-time state of a sandbox.
///
/// Take a snapshot before risky operations (e.g., running untrusted code
/// with monitors). If the sandbox becomes poisoned, restore from the
/// snapshot to recover.
///
/// ```js
/// const snapshot = await loaded.snapshot();
/// try {
///     await loaded.callHandler('handler', {}, { wallClockTimeoutMs: 1000 });
/// } catch (e) {
///     if (e.code === 'ERR_CANCELLED') await loaded.restore(snapshot);
/// }
/// ```
#[napi(js_name = "Snapshot")]
pub struct SnapshotWrapper {
    inner: Arc<Snapshot>,
}

// ── SandboxBuilder ───────────────────────────────────────────────────

/// Configures and creates a new sandbox.
///
/// Use the builder to set memory limits before constructing the sandbox.
/// All size setters support method chaining.
///
/// ```js
/// const proto = await new SandboxBuilder()
///     .setHeapSize(8 * 1024 * 1024)
///     .setStackSize(512 * 1024)
///     .build();
/// ```
#[napi(js_name = "SandboxBuilder")]
pub struct SandboxBuilderWrapper {
    inner: Arc<Mutex<Option<SandboxBuilder>>>,
}

impl Default for SandboxBuilderWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxBuilderWrapper {
    /// Apply a builder transformation while holding the lock, or error if
    /// consumed (after `build()` has been called).
    fn with_inner<F>(&self, f: F) -> napi::Result<&Self>
    where
        F: FnOnce(SandboxBuilder) -> SandboxBuilder,
    {
        let mut guard = self.inner.lock().map_err(|_| lock_error())?;
        let builder = guard
            .take()
            .ok_or_else(|| consumed_error("SandboxBuilder"))?;
        *guard = Some(f(builder));
        Ok(self)
    }

    /// Take ownership of the inner builder, or error if consumed.
    fn take_inner(&self) -> napi::Result<SandboxBuilder> {
        self.inner
            .lock()
            .map_err(|_| lock_error())?
            .take()
            .ok_or_else(|| consumed_error("SandboxBuilder"))
    }
}

#[napi]
impl SandboxBuilderWrapper {
    /// Create a new `SandboxBuilder` with default settings.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(SandboxBuilder::new()))),
        }
    }

    /// Set the guest output buffer size in bytes.
    ///
    /// This buffer is used by the guest to send return values back to the
    /// host. If handlers return large payloads, increase this.
    ///
    /// @param size - Buffer size in bytes (must be > 0)
    /// @returns this (for chaining)
    /// @throws If size is 0
    #[napi]
    pub fn set_output_buffer_size(&self, size: u32) -> napi::Result<&Self> {
        if size == 0 {
            return Err(invalid_arg_error(
                "Output buffer size must be greater than 0",
            ));
        }
        self.with_inner(|b| b.with_guest_output_buffer_size(size as usize))
    }

    /// Set the guest input buffer size in bytes.
    ///
    /// This buffer is used to pass event data into the guest. If handlers
    /// receive large JSON payloads, increase this.
    ///
    /// @param size - Buffer size in bytes (must be > 0)
    /// @returns this (for chaining)
    /// @throws If size is 0
    #[napi]
    pub fn set_input_buffer_size(&self, size: u32) -> napi::Result<&Self> {
        if size == 0 {
            return Err(invalid_arg_error(
                "Input buffer size must be greater than 0",
            ));
        }
        self.with_inner(|b| b.with_guest_input_buffer_size(size as usize))
    }

    /// Set the guest stack size in bytes.
    ///
    /// Controls how much stack space is available for guest code execution.
    /// Deep recursion or large local variables need a bigger stack.
    ///
    /// @param size - Stack size in bytes (must be > 0)
    /// @returns this (for chaining)
    /// @throws If size is 0
    #[napi]
    pub fn set_stack_size(&self, size: u32) -> napi::Result<&Self> {
        if size == 0 {
            return Err(invalid_arg_error("Stack size must be greater than 0"));
        }
        self.with_inner(|b| b.with_guest_stack_size(size as u64))
    }

    /// Set the guest heap size in bytes.
    ///
    /// Controls how much heap memory the guest JavaScript engine can
    /// allocate. If handlers create many objects or large strings, increase
    /// this. Too small will cause `malloc failed` errors in the guest.
    ///
    /// @param size - Heap size in bytes (must be > 0)
    /// @returns this (for chaining)
    /// @throws If size is 0
    #[napi]
    pub fn set_heap_size(&self, size: u32) -> napi::Result<&Self> {
        if size == 0 {
            return Err(invalid_arg_error("Heap size must be greater than 0"));
        }
        self.with_inner(|b| b.with_guest_heap_size(size as u64))
    }

    /// Build a `ProtoJSSandbox` from this builder's configuration.
    ///
    /// This allocates the sandbox VM resources. The builder is consumed
    /// and cannot be reused after calling this method.
    ///
    /// This is an async operation — it returns a `Promise` and does not
    /// block the Node.js event loop.
    ///
    /// @returns A `Promise<ProtoJSSandbox>` ready to load the JavaScript runtime
    /// @throws On resource allocation failure, or if already consumed
    #[napi]
    pub async fn build(&self) -> napi::Result<ProtoJSSandboxWrapper> {
        let builder = self.take_inner()?;
        let proto_sandbox =
            tokio::task::spawn_blocking(move || builder.build().map_err(to_napi_error))
                .await
                .map_err(join_error)??;
        Ok(ProtoJSSandboxWrapper {
            inner: Arc::new(Mutex::new(Some(proto_sandbox))),
        })
    }
}

// ── ProtoJSSandbox ───────────────────────────────────────────────────

/// A sandbox with VM resources allocated, ready to load the JS runtime.
///
/// This is a transitional state — call `loadRuntime()` to proceed to
/// `JSSandbox` where you can register handlers.
///
/// ```js
/// const proto = await new SandboxBuilder().build();
/// const sandbox = await proto.loadRuntime();
/// ```
#[napi(js_name = "ProtoJSSandbox")]
pub struct ProtoJSSandboxWrapper {
    inner: Arc<Mutex<Option<ProtoJSSandbox>>>,
}

impl ProtoJSSandboxWrapper {
    /// Take ownership of the inner value, returning a consumed-state error if
    /// this instance has already been used.
    fn take_inner(&self) -> napi::Result<ProtoJSSandbox> {
        self.inner
            .lock()
            .map_err(|_| lock_error())?
            .take()
            .ok_or_else(|| consumed_error("ProtoJSSandbox"))
    }
}

#[napi]
impl ProtoJSSandboxWrapper {
    /// Load the JavaScript runtime into the sandbox.
    ///
    /// This is an expensive operation — the QuickJS engine is initialized
    /// inside the sandbox. The `ProtoJSSandbox` is consumed and cannot be
    /// reused.
    ///
    /// Returns a `Promise` — does not block the Node.js event loop.
    ///
    /// @returns A `Promise<JSSandbox>` ready for handler registration
    /// @throws If the runtime fails to load, or if already consumed
    #[napi]
    pub async fn load_runtime(&self) -> napi::Result<JSSandboxWrapper> {
        let proto_sandbox = self.take_inner()?;
        let js_sandbox = tokio::task::spawn_blocking(move || {
            proto_sandbox.load_runtime().map_err(to_napi_error)
        })
        .await
        .map_err(join_error)??;
        Ok(JSSandboxWrapper {
            inner: Arc::new(Mutex::new(Some(js_sandbox))),
        })
    }
}

// ── JSSandbox ────────────────────────────────────────────────────────

/// A sandbox with the JavaScript runtime loaded, ready for handlers.
///
/// Register handler functions with `addHandler()`, then call
/// `getLoadedSandbox()` to transition to the execution-ready state.
///
/// Handler registration methods (`addHandler`, `removeHandler`,
/// `clearHandlers`) are synchronous since they're cheap operations.
/// `getLoadedSandbox()` is async since it compiles handlers into the guest.
///
/// ```js
/// const sandbox = await proto.loadRuntime();
/// sandbox.addHandler('greet', 'function handler(e) { return { msg: "hi " + e.name }; }');
/// const loaded = await sandbox.getLoadedSandbox();
/// ```
#[napi(js_name = "JSSandbox")]
pub struct JSSandboxWrapper {
    inner: Arc<Mutex<Option<JSSandbox>>>,
}

impl JSSandboxWrapper {
    /// Borrow the inner value mutably via Mutex, or error if consumed.
    fn with_inner_mut<F, R>(&self, f: F) -> napi::Result<R>
    where
        F: FnOnce(&mut JSSandbox) -> napi::Result<R>,
    {
        let mut guard = self.inner.lock().map_err(|_| lock_error())?;
        let sandbox = guard.as_mut().ok_or_else(|| consumed_error("JSSandbox"))?;
        f(sandbox)
    }

    /// Borrow the inner value immutably via Mutex, or error if consumed.
    fn with_inner_ref<F, R>(&self, f: F) -> napi::Result<R>
    where
        F: FnOnce(&JSSandbox) -> napi::Result<R>,
    {
        let guard = self.inner.lock().map_err(|_| lock_error())?;
        let sandbox = guard.as_ref().ok_or_else(|| consumed_error("JSSandbox"))?;
        f(sandbox)
    }

    /// Take ownership of the inner value via Mutex, or error if consumed.
    fn take_inner(&self) -> napi::Result<JSSandbox> {
        self.inner
            .lock()
            .map_err(|_| lock_error())?
            .take()
            .ok_or_else(|| consumed_error("JSSandbox"))
    }
}

#[napi]
impl JSSandboxWrapper {
    /// Register a named handler function in the sandbox.
    ///
    /// The `script` must define (or export) a function named `handler`.
    /// The `functionName` is a routing key used by `callHandler()` to dispatch calls.
    /// If the script contains no `export`, the runtime auto-appends `export { handler };`.
    /// Multiple handlers can be registered before calling `getLoadedSandbox()`.
    ///
    /// This is a synchronous operation (handler registration is cheap).
    ///
    /// @param functionName - Routing key for `callHandler()` dispatch (must be non-empty)
    /// @param script - JavaScript source defining a function named `handler`
    /// @throws If the handler name is empty, or if the sandbox is consumed
    #[napi]
    pub fn add_handler(&self, handler_name: String, script: String) -> napi::Result<()> {
        if handler_name.is_empty() {
            return Err(invalid_arg_error("Handler name must not be empty"));
        }
        self.with_inner_mut(|sandbox| {
            sandbox
                .add_handler(handler_name, Script::from_content(script))
                .map_err(to_napi_error)
        })
    }

    /// Remove a previously registered handler by routing key.
    ///
    /// This is a synchronous operation.
    ///
    /// @param functionName - Routing key of the handler to remove (must be non-empty)
    /// @throws If the handler name is empty, or if the sandbox is consumed
    #[napi]
    pub fn remove_handler(&self, handler_name: String) -> napi::Result<()> {
        if handler_name.is_empty() {
            return Err(invalid_arg_error("Handler name must not be empty"));
        }
        self.with_inner_mut(|sandbox| sandbox.remove_handler(&handler_name).map_err(to_napi_error))
    }

    /// Remove all registered handlers.
    ///
    /// This is a synchronous operation.
    ///
    /// @throws If the sandbox is consumed
    #[napi]
    pub fn clear_handlers(&self) -> napi::Result<()> {
        self.with_inner_mut(|sandbox| {
            sandbox.clear_handlers();
            Ok(())
        })
    }

    /// Transition to an execution-ready `LoadedJSSandbox`.
    ///
    /// All registered handlers are compiled and loaded into the guest.
    /// The `JSSandbox` is consumed and cannot be reused. To change
    /// handlers later, call `loaded.unload()` to get back to this state.
    ///
    /// Returns a `Promise` — does not block the Node.js event loop.
    ///
    /// @returns A `Promise<LoadedJSSandbox>` ready to handle events
    /// @throws If loading fails, or if the sandbox is consumed
    #[napi]
    pub async fn get_loaded_sandbox(&self) -> napi::Result<LoadedJSSandboxWrapper> {
        let js_sandbox = self.take_inner()?;
        let loaded_sandbox = tokio::task::spawn_blocking(move || {
            js_sandbox.get_loaded_sandbox().map_err(to_napi_error)
        })
        .await
        .map_err(join_error)??;
        // Grab the interrupt handle and poisoned state before moving behind the Mutex.
        // These are stored separately so they never contend with the inner lock —
        // callers can read them even while guest code is executing on a background thread.
        let interrupt = loaded_sandbox.interrupt_handle();
        let poisoned_flag = Arc::new(AtomicBool::new(loaded_sandbox.poisoned()));
        Ok(LoadedJSSandboxWrapper {
            inner: Arc::new(Mutex::new(Some(loaded_sandbox))),
            interrupt,
            poisoned_flag,
        })
    }

    /// Whether the sandbox is in a poisoned (inconsistent) state.
    ///
    /// A poisoned sandbox has had its guest execution interrupted or
    /// aborted. Most operations will fail with an `ERR_POISONED` error code.
    #[napi(getter)]
    pub fn poisoned(&self) -> napi::Result<bool> {
        self.with_inner_ref(|sandbox| Ok(sandbox.poisoned()))
    }
}

// ── LoadedJSSandbox ──────────────────────────────────────────────────

/// An execution-ready sandbox with handlers loaded.
///
/// This is where the action happens — call `callHandler()` to invoke
/// handlers, optionally with timeout-protected execution.
///
/// All execution methods are async and return Promises, keeping the
/// Node.js event loop free while guest code runs on a background thread.
/// Concurrent calls to the same sandbox serialize naturally — the second
/// call waits for the first to finish.
///
/// ```js
/// const result = await loaded.callHandler('greet', { name: 'World' });
/// console.log(result); // { msg: "hi World" }
/// ```
#[napi(js_name = "LoadedJSSandbox")]
pub struct LoadedJSSandboxWrapper {
    inner: Arc<Mutex<Option<LoadedJSSandbox>>>,

    /// Stored **outside** the Mutex so callers can `kill()` a running handler.
    ///
    /// `call_handler()` holds the Mutex lock for the entire guest execution.
    /// If this lived behind the same lock, you could never interrupt anything —
    /// `kill()` would block until the handler finished (defeating the purpose).
    /// See the module-level architecture comment for the full rationale.
    interrupt: Arc<dyn InterruptHandle>,

    /// Tracks poisoned state **outside** the Mutex for lock-free reads.
    ///
    /// The `poisoned` getter is a sync napi property (not async). If it tried
    /// to acquire the Mutex while `call_handler()` is running, it would block
    /// the Node.js event loop until guest execution finishes.
    ///
    /// Updated via `Ordering::Release` inside every `spawn_blocking` closure
    /// (where we already hold the lock), read via `Ordering::Acquire` in the
    /// getter. See the module-level architecture comment for the full rationale.
    poisoned_flag: Arc<AtomicBool>,
}

#[napi]
impl LoadedJSSandboxWrapper {
    /// Invoke a handler function with the given event data, optionally
    /// with execution monitors that enforce resource limits.
    ///
    /// Pass a JavaScript object directly — the API handles JSON
    /// serialization internally. Returns a parsed JavaScript object.
    ///
    /// Returns a `Promise` — the Node.js event loop stays free while the
    /// guest executes on a background thread. Concurrent calls to the
    /// same sandbox serialize via an internal lock.
    ///
    /// When `options` is omitted (or contains no timeouts), the handler
    /// runs without monitors. When timeouts are set, monitors race with
    /// **OR semantics** — whichever fires first terminates execution.
    ///
    /// ```js
    /// // Simple call — no monitors
    /// const result = await loaded.callHandler('greet', { name: 'World' });
    ///
    /// // With monitors — recommended for untrusted code
    /// const guarded = await loaded.callHandler('compute', data, {
    ///     wallClockTimeoutMs: 5000,
    ///     cpuTimeoutMs: 500,
    /// });
    /// ```
    ///
    /// @param handlerName - Name of a previously registered handler
    /// @param eventData - JavaScript object to pass as the event argument
    /// @param options - Optional timeout/GC configuration
    /// @returns A `Promise<object>` with the handler's return value
    /// @throws On missing handler, guest execution error, or `ERR_CANCELLED` if a monitor fires
    #[napi]
    pub async fn call_handler(
        &self,
        handler_name: String,
        event_data: JsonValue,
        options: Option<CallHandlerOptions>,
    ) -> napi::Result<JsonValue> {
        if handler_name.is_empty() {
            return Err(invalid_arg_error("Handler name must not be empty"));
        }

        let options = options.unwrap_or_default();

        // Validate timeout values eagerly before spawning a blocking task.
        // Zero or sub-millisecond timeouts would fire instantly, poisoning
        // the sandbox for no good reason. Values above MAX_TIMEOUT_MS guard
        // against accidental wrapping (e.g. JS `-1` → u32::MAX via ToUint32).
        if let Some(wall_ms) = options.wall_clock_timeout_ms
            && !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&wall_ms)
        {
            return Err(invalid_arg_error(&format!(
                    "wallClockTimeoutMs must be between {MIN_TIMEOUT_MS}ms and {MAX_TIMEOUT_MS}ms, got {wall_ms}"
                )));
        }
        if let Some(cpu_ms) = options.cpu_timeout_ms
            && !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&cpu_ms)
        {
            return Err(invalid_arg_error(&format!(
                    "cpuTimeoutMs must be between {MIN_TIMEOUT_MS}ms and {MAX_TIMEOUT_MS}ms, got {cpu_ms}"
                )));
        }

        let inner = self.inner.clone();
        let poisoned_flag = self.poisoned_flag.clone();
        let gc = options.gc;
        let wall_clock_timeout_ms = options.wall_clock_timeout_ms;
        let cpu_timeout_ms = options.cpu_timeout_ms;

        // Serialize the JS object to a JSON string for the hypervisor
        let event_json = serde_json::to_string(&event_data)
            .map_err(|e| invalid_arg_error(&format!("Failed to serialize event: {e}")))?;

        let result_json = tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|_| lock_error())?;
            let sandbox = guard
                .as_mut()
                .ok_or_else(|| consumed_error("LoadedJSSandbox"))?;

            // Dispatch to the appropriate Rust method based on whether
            // any monitor timeouts are specified.
            //
            // The three `handle_event_with_monitor` arms look duplicated, but
            // each constructs a different concrete monitor type (single or tuple).
            // The sealed `MonitorSet` trait is not object-safe, so we can't
            // erase the type behind a `dyn` — the match is structurally required.
            let result = match (wall_clock_timeout_ms, cpu_timeout_ms) {
                // No monitors — fast path
                (None, None) => sandbox
                    .handle_event(handler_name, event_json, gc)
                    .map_err(to_napi_error),
                // Both — tuple with OR semantics (recommended)
                (Some(wall_ms), Some(cpu_ms)) => {
                    let monitor = (
                        WallClockMonitor::new(Duration::from_millis(wall_ms as u64))
                            .map_err(to_napi_error)?,
                        CpuTimeMonitor::new(Duration::from_millis(cpu_ms as u64))
                            .map_err(to_napi_error)?,
                    );
                    sandbox
                        .handle_event_with_monitor(handler_name, event_json, &monitor, gc)
                        .map_err(to_napi_error)
                }
                // Wall-clock only
                (Some(wall_ms), None) => {
                    let monitor = WallClockMonitor::new(Duration::from_millis(wall_ms as u64))
                        .map_err(to_napi_error)?;
                    sandbox
                        .handle_event_with_monitor(handler_name, event_json, &monitor, gc)
                        .map_err(to_napi_error)
                }
                // CPU only
                (None, Some(cpu_ms)) => {
                    let monitor = CpuTimeMonitor::new(Duration::from_millis(cpu_ms as u64))
                        .map_err(to_napi_error)?;
                    sandbox
                        .handle_event_with_monitor(handler_name, event_json, &monitor, gc)
                        .map_err(to_napi_error)
                }
            };
            // Update poisoned flag while we hold the lock — keeps the getter
            // lock-free so it never blocks the Node.js event loop.
            poisoned_flag.store(sandbox.poisoned(), Ordering::Release);
            result
        })
        .await
        .map_err(join_error)??;
        // Parse the JSON string result back into a JS object
        serde_json::from_str(&result_json).map_err(|e| {
            hl_error(
                ErrorCode::Internal,
                format!("Failed to parse handler result as JSON: {e}"),
            )
        })
    }

    /// Unload all handlers and return to the `JSSandbox` state.
    ///
    /// Use this to register new handlers or to recover from a poisoned
    /// state. The `LoadedJSSandbox` is consumed.
    ///
    /// Returns a `Promise<JSSandbox>`.
    ///
    /// @returns A `Promise<JSSandbox>` ready for new handler registration
    /// @throws If already consumed
    #[napi]
    pub async fn unload(&self) -> napi::Result<JSSandboxWrapper> {
        let inner = self.inner.clone();
        let js_sandbox = tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|_| lock_error())?;
            let loaded = guard
                .take()
                .ok_or_else(|| consumed_error("LoadedJSSandbox"))?;
            loaded.unload().map_err(to_napi_error)
        })
        .await
        .map_err(join_error)??;
        Ok(JSSandboxWrapper {
            inner: Arc::new(Mutex::new(Some(js_sandbox))),
        })
    }

    /// Get a handle that can interrupt currently running guest code.
    ///
    /// Since `callHandler()` is async, you can call `kill()` from the
    /// same JavaScript thread while a handler is executing:
    ///
    /// ```js
    /// const handle = loaded.interruptHandle;
    /// const promise = loaded.callHandler('compute', {});
    /// setTimeout(() => handle.kill(), 5000);
    /// await promise; // throws after kill()
    /// ```
    ///
    /// The interrupt handle is stored separately and never contends with
    /// the sandbox lock — it is always available instantly.
    ///
    /// @returns An `InterruptHandle` with a `kill()` method
    #[napi(getter)]
    pub fn interrupt_handle(&self) -> InterruptHandleWrapper {
        InterruptHandleWrapper {
            inner: self.interrupt.clone(),
        }
    }

    /// Whether the sandbox is in a poisoned (inconsistent) state.
    ///
    /// A sandbox becomes poisoned when guest execution is interrupted
    /// (e.g., by a monitor timeout), when the guest panics, or on memory
    /// violations. Most operations will fail with an `ERR_POISONED` error code.
    ///
    /// This getter is lock-free — it reads from an atomic flag updated
    /// after every sandbox operation. It never blocks the event loop,
    /// even while a handler is executing on a background thread.
    ///
    /// Recovery options:
    /// - `restore(snapshot)` — revert to a captured state
    /// - `unload()` — discard handlers and start fresh
    #[napi(getter)]
    pub fn poisoned(&self) -> bool {
        self.poisoned_flag.load(Ordering::Acquire)
    }

    /// Capture the current sandbox state as a snapshot.
    ///
    /// Take a snapshot **before** risky operations so you can recover
    /// if the sandbox becomes poisoned.
    ///
    /// Returns a `Promise<Snapshot>`.
    ///
    /// @returns A `Promise<Snapshot>` that can be passed to `restore()`
    /// @throws If already consumed
    #[napi]
    pub async fn snapshot(&self) -> napi::Result<SnapshotWrapper> {
        let inner = self.inner.clone();
        let poisoned_flag = self.poisoned_flag.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|_| lock_error())?;
            let sandbox = guard
                .as_mut()
                .ok_or_else(|| consumed_error("LoadedJSSandbox"))?;
            let result = sandbox.snapshot().map_err(to_napi_error);
            poisoned_flag.store(sandbox.poisoned(), Ordering::Release);
            result
        })
        .await
        .map_err(join_error)??;
        Ok(SnapshotWrapper {
            inner: Arc::new(snapshot),
        })
    }

    /// Restore the sandbox to a previously captured snapshot state.
    ///
    /// This is the primary recovery mechanism for poisoned sandboxes.
    /// After restoring, the sandbox is unpoisoned and ready for use.
    ///
    /// Returns a `Promise<void>`.
    ///
    /// @param snapshot - A snapshot previously obtained from `snapshot()`
    /// @throws If the snapshot doesn't match this sandbox, or if consumed
    #[napi]
    pub async fn restore(&self, snapshot: &SnapshotWrapper) -> napi::Result<()> {
        let inner = self.inner.clone();
        let snap = snapshot.inner.clone();
        let poisoned_flag = self.poisoned_flag.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().map_err(|_| lock_error())?;
            let sandbox = guard
                .as_mut()
                .ok_or_else(|| consumed_error("LoadedJSSandbox"))?;
            let result = sandbox.restore(&snap).map_err(to_napi_error);
            poisoned_flag.store(sandbox.poisoned(), Ordering::Release);
            result
        })
        .await
        .map_err(join_error)?
    }
}

// ── CallHandlerOptions ───────────────────────────────────────────────

/// Options for `callHandler()`.
///
/// All fields are optional. When no timeouts are set, the handler runs
/// without monitors. When one or both timeouts are set, monitors race
/// with **OR semantics** — whichever fires first terminates execution.
///
/// ```js
/// // No options — simple call
/// await loaded.callHandler('handler', data);
///
/// // With GC control only
/// await loaded.callHandler('handler', data, { gc: false });
///
/// // Recommended: both monitors for untrusted code
/// await loaded.callHandler('handler', data, {
///     wallClockTimeoutMs: 5000,
///     cpuTimeoutMs: 500,
/// });
/// ```
#[napi(object)]
#[derive(Default)]
pub struct CallHandlerOptions {
    /// Wall-clock timeout in milliseconds (minimum: 1ms).
    ///
    /// Terminates execution after this amount of real (elapsed) time.
    /// Catches resource exhaustion where the guest holds host resources
    /// without burning CPU (e.g., sleeping, blocking on I/O).
    pub wall_clock_timeout_ms: Option<u32>,

    /// CPU time timeout in milliseconds (minimum: 1ms).
    ///
    /// Terminates execution after this amount of actual CPU time. Catches
    /// compute-bound abuse (tight loops, crypto mining). Does not count
    /// time spent sleeping or blocked. Supported on Linux and Windows.
    pub cpu_timeout_ms: Option<u32>,

    /// Whether to run garbage collection after the handler call.
    /// Defaults to `true` if not specified.
    pub gc: Option<bool>,
}

// ── InterruptHandle ──────────────────────────────────────────────────

/// A handle for manually killing currently running guest code.
///
/// Since `callHandler()` is async, you can grab an interrupt handle
/// and call `kill()` from the same JavaScript thread while the handler
/// is executing — no worker threads required:
///
/// ```js
/// const handle = loaded.interruptHandle;
/// const promise = loaded.callHandler('compute', {});
/// setTimeout(() => handle.kill(), 5000);
/// try { await promise; } catch (e) { /* killed */ }
/// ```
///
/// For automatic timeout-based interruption, pass timeout options to
/// `callHandler()` which handles everything for you.
#[napi(js_name = "InterruptHandle")]
pub struct InterruptHandleWrapper {
    inner: Arc<dyn InterruptHandle>,
}

#[napi]
impl InterruptHandleWrapper {
    /// Immediately terminate the currently executing guest code.
    ///
    /// The sandbox will be poisoned after this call. Use `restore()` with
    /// a snapshot to recover, or `unload()` to discard handlers.
    #[napi]
    pub fn kill(&self) {
        self.inner.kill();
    }
}
