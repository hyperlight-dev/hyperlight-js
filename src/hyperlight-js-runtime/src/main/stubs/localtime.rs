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
use chrono::{DateTime, TimeDelta, Datelike as _, Timelike as _};

use crate::libc;

#[unsafe(no_mangle)]
extern "C" fn localtime_r(time: *const libc::time_t, result: *mut libc::tm) -> *mut libc::tm {
    let offset = unsafe { time.read() };

    let Some(offset) = TimeDelta::try_seconds(offset) else {
        unsafe { libc::__errno_location().write(libc::EOVERFLOW as _) };
        return result;
    };
    let Some(time) = DateTime::UNIX_EPOCH.checked_add_signed(offset) else {
        unsafe { libc::__errno_location().write(libc::EOVERFLOW as _) };
        return result;
    };

    unsafe {
        result.write(libc::tm {
            tm_sec: time.second() as _,
            tm_min: time.minute() as _,
            tm_hour: time.hour() as _,
            tm_mday: time.day() as _,
            tm_mon: time.month0() as _,
            tm_year: (time.year() - 1900) as _,
            tm_wday: time.weekday().num_days_from_sunday() as _,
            tm_yday: time.ordinal0() as _,
            tm_isdst: 0,
            __tm_gmtoff: 0,
            __tm_zone: c"GMT".as_ptr() as _,
        })
    };

    result
}
