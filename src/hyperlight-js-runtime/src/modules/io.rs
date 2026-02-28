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

use crate::libc;

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod io {
    use super::*;

    #[rquickjs::function]
    pub fn print(txt: String) {
        for byte in txt.bytes() {
            let _ = unsafe { libc::putchar(byte as _) };
        }
        flush()
    }

    #[rquickjs::function]
    pub fn flush() {
        // Flush the output buffer of libc to make sure all output is printed out.
        let _ = unsafe { libc::fflush(core::ptr::null_mut()) };
    }
}
