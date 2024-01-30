//! This crate provides a Hyperlight implementation for JavaScript guest code.
#![deny(dead_code, missing_docs, unused_mut)]
#![cfg_attr(not(any(test, debug_assertions)), warn(clippy::panic))]
#![cfg_attr(not(any(test, debug_assertions)), warn(clippy::expect_used))]
#![cfg_attr(not(any(test, debug_assertions)), warn(clippy::unwrap_used))]
#![cfg_attr(any(test, debug_assertions), allow(clippy::disallowed_macros))]

mod resolver;
mod script;

/// Sandbox module containing all sandbox-related types
pub mod sandbox;

use hyperlight_host::func::HostFunction;
/// A Hyperlight Sandbox with a JavaScript run time loaded but no guest code.
pub use sandbox::js_sandbox::JSSandbox;
/// A Hyperlight Sandbox with a JavaScript run time loaded and guest code loaded.
pub use sandbox::loaded_js_sandbox::LoadedJSSandbox;
/// A Hyperlight Sandbox with no JavaScript run time loaded and no guest code.
/// This is used to register new host functions prior to loading the JavaScript runtime.
pub use sandbox::proto_js_sandbox::ProtoJSSandbox;
/// A builder for creating a new `JSSandbox`
pub use sandbox::sandbox_builder::SandboxBuilder;
/// Types for working with JS script.
pub use script::Script;
/// The function to pass to a new `JSSandbox` to tell it how to handle
/// guest requests to print some output.
pub type HostPrintFn = HostFunction<i32, (String,)>;
/// The Result of a function call
pub type Result<T> = hyperlight_host::Result<T>;
/// Check if there is a hypervisor present
pub use hyperlight_host::is_hypervisor_present;
/// Create a generic HyperlightError
pub use hyperlight_host::new_error;
/// The error type for Hyperlight operations
pub type HyperlightError = hyperlight_host::HyperlightError;
/// A handle to interrupt guest code execution
pub use hyperlight_host::hypervisor::InterruptHandle;
/// The container to store the value of a single parameter to a guest
/// function.
pub type ParameterValue = hyperlight_host::func::ParameterValue;
/// The container to store the return value from a guest function call.
pub type ReturnValue = hyperlight_host::func::ReturnValue;
/// The type of the return value from a guest function call.
pub type ReturnType = hyperlight_host::func::ReturnType;
/// A snapshot of sandbox state that can be used to restore it later.
pub use hyperlight_host::sandbox::snapshot::Snapshot;
/// Configuration for sandbox resource limits and behavior.
pub use hyperlight_host::sandbox::SandboxConfiguration;
/// Module resolution and loading functionality.
pub use resolver::{FileMetadata, FileSystem, FileSystemEmbedded, ResolveError};
/// The monitor module — re-exports `sleep` so custom monitors don't couple to tokio directly.
pub use sandbox::monitor;
/// CPU time based execution monitor.
#[cfg(feature = "monitor-cpu-time")]
pub use sandbox::monitor::CpuTimeMonitor;
// Execution monitoring
/// Trait for implementing execution monitors that can terminate handler execution.
pub use sandbox::monitor::ExecutionMonitor;
/// Sealed trait for monitor composition — automatically derived for all
/// `ExecutionMonitor` impls and for tuples of up to 5 monitors.
pub use sandbox::monitor::MonitorSet;
/// Wall-clock based execution monitor.
#[cfg(feature = "monitor-wall-clock")]
pub use sandbox::monitor::WallClockMonitor;
