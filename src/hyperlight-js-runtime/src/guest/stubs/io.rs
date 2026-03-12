/*
Copyright 2026 The Hyperlight Authors.

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
use crate::libc;

#[unsafe(no_mangle)]
extern "C" fn putchar(c: libc::c_int) -> libc::c_int {
    unsafe { libc::_putchar(c as libc::c_char) };
    if c == '\n' as libc::c_int {
        // force a flush of the internal buffer in the hyperlight putchar implementation
        unsafe { libc::_putchar(0) };
    }
    (c as u8) as libc::c_int
}

#[unsafe(no_mangle)]
extern "C" fn fflush(f: *mut libc::c_void) -> libc::c_int {
    if !f.is_null() {
        // we only support flushing all streams, and stdout is our only stream
        unsafe { libc::__errno_location().write(libc::EINVAL as _) };
        return -1;
    }
    // flush stdout
    unsafe { libc::_putchar(0) };
    0
}
