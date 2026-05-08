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

unsafe extern "C" {
    fn fflush(stream: *mut core::ffi::c_void) -> core::ffi::c_int;
    fn putchar(c: core::ffi::c_int) -> core::ffi::c_int;
}

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod io {
    use super::*;

    #[rquickjs::function]
    pub fn print(txt: String) {
        for c in txt.bytes() {
            let _ = unsafe { putchar(c as core::ffi::c_int) };
        }
        flush()
    }

    #[rquickjs::function]
    pub fn flush() {
        // Flush the output buffer of libc to make sure all output is printed out.

        #[cfg(hyperlight)]
        {
            unsafe extern "C" {
                static stdout: *mut core::ffi::c_void;
                static stderr: *mut core::ffi::c_void;
            }

            // In Hyperlight, fflush(NULL) is not supported, so we need to flush both stdout and stderr separately.
            let _ = unsafe { fflush(stdout) };
            let _ = unsafe { fflush(stderr) };
        }

        #[cfg(not(hyperlight))]
        {
            // In the native runtime, we can just fflush(NULL) to flush all output streams.
            let _ = unsafe { fflush(core::ptr::null_mut()) };
        }
    }
}
