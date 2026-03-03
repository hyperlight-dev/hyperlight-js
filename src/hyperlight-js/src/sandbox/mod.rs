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
//! The `sandbox` module contains the sandbox types for the Hyperlight JavaScript runtime.
use std::env;
/// Execution statistics captured after each guest function call.
#[cfg(feature = "guest-call-stats")]
pub mod execution_stats;
/// Definition of a host function that can be called from guest JavaScript code.
pub(crate) mod host_fn;
/// A Hyperlight Sandbox with a JavaScript run time loaded but no guest code.
pub(crate) mod js_sandbox;
/// A Hyperlight Sandbox with a JavaScript run time loaded and guest code loaded.
pub(crate) mod loaded_js_sandbox;
/// Metric definitions for Sandbox module.
pub(crate) mod metrics;
/// Execution monitoring and enforcement (timeouts, resource limits, etc.).
pub mod monitor;
/// A Hyperlight Sandbox with no JavaScript run time loaded and no guest code.
/// This is used to register new host functions prior to loading the JavaScript runtime.
pub(crate) mod proto_js_sandbox;
/// A builder for creating a new `JSSandbox`
pub(crate) mod sandbox_builder;
// This include! macro is replaced by the build.rs script.
// The build.rs script reads the hyperlight-js-runtime binary into a static byte array named JSRUNTIME.
include!(concat!(env!("OUT_DIR"), "/host_resource.rs"));
