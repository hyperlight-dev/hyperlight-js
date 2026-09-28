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

/// Handle for reading CPU time of a specific thread.
pub(crate) struct ThreadCpuHandle {
    clock_id: libc::clockid_t,
}

// SAFETY: The process-scoped clock ID remains valid for the thread's lifetime,
// and POSIX permits reading a thread CPU clock from another thread.
unsafe impl Send for ThreadCpuHandle {}
unsafe impl Sync for ThreadCpuHandle {}

impl ThreadCpuHandle {
    pub(crate) fn for_current_thread() -> Option<Self> {
        use libc::{pthread_getcpuclockid, pthread_self};

        let thread_id = unsafe { pthread_self() };
        let mut clock_id: libc::clockid_t = 0;

        let result = unsafe { pthread_getcpuclockid(thread_id, &mut clock_id) };
        if result != 0 {
            return None;
        }

        Some(Self { clock_id })
    }

    pub(crate) fn elapsed(&self) -> Option<u64> {
        use libc::{clock_gettime, timespec};

        if self.clock_id == 0 {
            return None;
        }

        let mut ts = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };

        let result = unsafe { clock_gettime(self.clock_id, &mut ts) };
        if result != 0 {
            return None;
        }

        Some((ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64))
    }

    pub(crate) fn deadline_for(&self, timeout: Duration) -> Option<u64> {
        Some(timeout.as_nanos() as u64)
    }

    pub(crate) fn ticks_to_approx_nanos(&self, ticks: u64) -> u64 {
        ticks
    }
}
