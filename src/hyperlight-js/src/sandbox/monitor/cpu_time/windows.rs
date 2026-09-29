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

use std::sync::OnceLock;
use std::time::Duration;

use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;

pub(crate) struct ThreadCpuHandle {
    thread_handle: windows_sys::Win32::Foundation::HANDLE,
    start_cycles: u64,
}

static CPU_FREQUENCY_MHZ: OnceLock<Option<u32>> = OnceLock::new();

fn read_cpu_frequency_mhz() -> Option<u32> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    };

    let subkey: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "~MHz\0".encode_utf16().collect();

    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let result =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if result != 0 {
        tracing::warn!("[CPU_TIME] Failed to open registry key for CPU frequency");
        return None;
    }

    let mut mhz: u32 = 0;
    let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
    let mut data_type: u32 = 0;

    let result = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            &mut mhz as *mut u32 as *mut u8,
            &mut data_size,
        )
    };

    unsafe { RegCloseKey(hkey) };

    if result != 0 || data_type != REG_DWORD || mhz == 0 {
        tracing::warn!(
            result = result,
            data_type = data_type,
            "[CPU_TIME] Failed to read CPU frequency from registry"
        );
        return None;
    }

    tracing::debug!(
        cpu_frequency_mhz = mhz,
        "[CPU_TIME] Read CPU base frequency from registry"
    );

    Some(mhz)
}

fn get_cpu_frequency_mhz() -> Option<u32> {
    *CPU_FREQUENCY_MHZ.get_or_init(read_cpu_frequency_mhz)
}

// SAFETY: The duplicated thread handle is valid across threads and is closed in Drop.
unsafe impl Send for ThreadCpuHandle {}
unsafe impl Sync for ThreadCpuHandle {}

impl ThreadCpuHandle {
    pub(crate) fn for_current_thread() -> Option<Self> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread};

        if get_cpu_frequency_mhz().is_none() {
            tracing::warn!(
                "[CPU_TIME] Could not read CPU frequency from registry, \
                 CPU time monitoring unavailable"
            );
            return None;
        }

        let pseudo_handle = unsafe { GetCurrentThread() };
        let process = unsafe { GetCurrentProcess() };
        let mut real_handle = std::ptr::null_mut();

        let result = unsafe {
            DuplicateHandle(
                process,
                pseudo_handle,
                process,
                &mut real_handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };

        if result == 0 {
            return None;
        }

        let mut start_cycles = 0;
        if unsafe { QueryThreadCycleTime(real_handle, &mut start_cycles) } == 0 {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(real_handle) };
            return None;
        }

        Some(Self {
            thread_handle: real_handle,
            start_cycles,
        })
    }

    pub(crate) fn elapsed(&self) -> Option<u64> {
        if self.thread_handle.is_null() {
            return None;
        }

        let mut current_cycles = 0;
        if unsafe { QueryThreadCycleTime(self.thread_handle, &mut current_cycles) } == 0 {
            return None;
        }

        Some(current_cycles.saturating_sub(self.start_cycles))
    }

    pub(crate) fn deadline_for(&self, timeout: Duration) -> Option<u64> {
        let freq_mhz = get_cpu_frequency_mhz()? as u64;
        let nanos = timeout.as_nanos() as u64;
        Some(nanos.saturating_mul(freq_mhz) / 1_000)
    }

    pub(crate) fn ticks_to_approx_nanos(&self, ticks: u64) -> u64 {
        match get_cpu_frequency_mhz() {
            Some(freq_mhz) => ticks.saturating_mul(1_000) / freq_mhz as u64,
            None => 0,
        }
    }
}

impl Drop for ThreadCpuHandle {
    fn drop(&mut self) {
        if !self.thread_handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.thread_handle);
            }
        }
    }
}
