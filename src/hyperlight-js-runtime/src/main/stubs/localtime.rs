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
use core::ptr::null_mut;

use chrono::{DateTime, Datelike as _, NaiveDateTime, TimeDelta, Timelike as _};

use crate::libc;

#[unsafe(no_mangle)]
extern "C" fn localtime_r(time: *const libc::time_t, result: *mut libc::tm) -> *mut libc::tm {
    const MIN_OFFSET: libc::time_t = TimeDelta::MIN.num_seconds();
    const MAX_OFFSET: libc::time_t = TimeDelta::MAX.num_seconds();
    const UNIX_EPOCH: NaiveDateTime = DateTime::UNIX_EPOCH.naive_utc();

    let offset = unsafe { time.read() };
    let tm = unsafe { result.as_mut() }.unwrap();
    let mut overflow = false;

    // check that we don't overflow the offset when converting to a TimeDelta
    let offset = match offset {
        ..MIN_OFFSET => {
            overflow = true;
            TimeDelta::MIN
        }
        MAX_OFFSET.. => {
            overflow = true;
            TimeDelta::MAX
        }
        offset => TimeDelta::seconds(offset),
    };

    // check that we don't overflow the DateTime when adding the offset to the UNIX_EPOCH
    let time = match UNIX_EPOCH.checked_add_signed(offset) {
        Some(time) => time,
        None if offset.num_seconds() < 0 => {
            overflow = true;
            NaiveDateTime::MIN
        }
        None => {
            overflow = true;
            NaiveDateTime::MAX
        }
    };

    tm.tm_sec = time.second() as _;
    tm.tm_min = time.minute() as _;
    tm.tm_hour = time.hour() as _;
    tm.tm_mday = time.day() as _;
    tm.tm_mon = time.month0() as _;
    tm.tm_year = (time.year() - 1900) as _;
    tm.tm_wday = time.weekday().num_days_from_sunday() as _;
    tm.tm_yday = time.ordinal0() as _;
    tm.tm_isdst = 0;
    tm.__tm_gmtoff = 0;
    tm.__tm_zone = c"GMT".as_ptr() as _;

    if overflow {
        unsafe { libc::__errno_location().write(libc::EOVERFLOW as _) };
        return null_mut();
    };

    result
}
