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
use alloc::string::String;

use anyhow::Result;

/// A trait representing the host environment for the JS runtime.
/// In hyperlight this would represent the host function calls that
/// the runtime needs.
pub trait Host: Send + Sync {
    /// Resolve a module name to a module specifier (usually a path).
    /// The base is the specifier of the module that is importing the module.
    fn resolve_module(&self, base: String, name: String) -> Result<String>;

    /// Obtain the module source code for a given module specifier.
    fn load_module(&self, name: String) -> Result<String>;
}
