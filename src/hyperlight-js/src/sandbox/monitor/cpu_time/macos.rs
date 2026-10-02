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

use std::time::Duration;

// These symbols live in libSystem but are not all re-exported by libc.
unsafe extern "C" {
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn mach_thread_self() -> libc::mach_port_t;
    #[allow(non_upper_case_globals)]
    static mach_task_self_: libc::mach_port_t;
}

const MACH_PORT_NULL: libc::mach_port_t = 0;

/// Handle for reading a specific thread's CPU time using a Mach send right.
pub(crate) struct ThreadCpuHandle {
    thread_port: libc::mach_port_t,
}

impl ThreadCpuHandle {
    pub(crate) fn for_current_thread() -> Option<Self> {
        // SAFETY: Returns a new send right owned by this handle. Mach port names
        // are task-wide, so the monitor thread may use the right to inspect this thread.
        let thread_port = unsafe { mach_thread_self() };
        if thread_port == MACH_PORT_NULL {
            tracing::warn!("[CPU_TIME] mach_thread_self() returned a null port");
            return None;
        }

        let handle = Self { thread_port };
        handle.elapsed()?;

        Some(handle)
    }

    pub(crate) fn elapsed(&self) -> Option<u64> {
        // SAFETY: thread_basic_info contains only integer C fields, for which all-zero
        // bit patterns are valid; thread_info initializes the requested fields below.
        let mut info: libc::thread_basic_info = unsafe { std::mem::zeroed() };
        let mut count = libc::THREAD_BASIC_INFO_COUNT;

        // SAFETY: info is the structure required by THREAD_BASIC_INFO and count is its
        // size in natural_t units. The task-wide send right remains valid until Drop,
        // and Mach permits querying the represented thread from the monitor thread.
        let result = unsafe {
            libc::thread_info(
                self.thread_port,
                libc::THREAD_BASIC_INFO as libc::thread_flavor_t,
                &mut info as *mut libc::thread_basic_info as libc::thread_info_t,
                &mut count,
            )
        };

        if result != 0 {
            tracing::warn!(
                "[CPU_TIME] thread_info() failed with kern_return_t {}",
                result
            );
            return None;
        }

        let to_nanos = |t: libc::time_value_t| {
            (t.seconds.max(0) as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add((t.microseconds.max(0) as u64).saturating_mul(1_000))
        };

        Some(to_nanos(info.user_time).saturating_add(to_nanos(info.system_time)))
    }

    pub(crate) fn deadline_for(&self, timeout: Duration) -> Option<u64> {
        Some(timeout.as_nanos() as u64)
    }

    pub(crate) fn ticks_to_approx_nanos(&self, ticks: u64) -> u64 {
        ticks
    }
}

impl Drop for ThreadCpuHandle {
    fn drop(&mut self) {
        if self.thread_port != MACH_PORT_NULL {
            // SAFETY: Self exclusively owns the send right returned by mach_thread_self.
            // ThreadCpuHandle is not Clone, and Drop consumes that right exactly once.
            unsafe { mach_port_deallocate(mach_task_self_, self.thread_port) };
        }
    }
}
