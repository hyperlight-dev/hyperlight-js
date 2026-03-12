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
#![cfg_attr(hyperlight, no_std)]

//! A custom native module providing basic math operations.
//! Used as a test fixture for the native module extension system.

#[rquickjs::module(rename_vars = "camelCase")]
pub mod math {
    /// Add two numbers.
    #[rquickjs::function]
    pub fn add(a: f64, b: f64) -> f64 {
        a + b
    }

    /// Multiply two numbers.
    #[rquickjs::function]
    pub fn multiply(a: f64, b: f64) -> f64 {
        a * b
    }
}
