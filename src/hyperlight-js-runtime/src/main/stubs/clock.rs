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
use hyperlight_guest_bin::error::Result;
use hyperlight_guest_bin::host_function;
use hyperlight_guest_bin::libc;

fn micros_since_epoch() -> u64 {
    #[host_function("CurrentTimeMicros")]
    fn current_time_micros() -> Result<u64>;

    current_time_micros().unwrap_or(1609459200u64 * 1_000_000u64)
}

#[unsafe(no_mangle)]
extern "C" fn clock_gettime_2(clk_id: libc::clockid_t, ts: *mut libc::timespec) -> libc::c_int {
    const CLOCK_REALTIME: libc::clockid_t = libc::CLOCK_REALTIME as libc::clockid_t;
    const CLOCK_MONOTONIC: libc::clockid_t = libc::CLOCK_MONOTONIC as libc::clockid_t;

    if clk_id != CLOCK_REALTIME && clk_id != CLOCK_MONOTONIC {
        unsafe { libc::errno = libc::EINVAL as _ };
        return -1;
    }
    let micros = micros_since_epoch();
    unsafe {
        ts.write(libc::timespec {
            tv_sec: (micros / 1_000_000) as _,
            tv_nsec: ((micros % 1_000_000) * 1000) as _,
        })
    };
    0
}
