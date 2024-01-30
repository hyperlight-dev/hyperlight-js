use alloc::string::String;

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod io {
    use super::*;

    #[rquickjs::function]
    pub fn print(txt: String) {
        unsafe extern "C" {
            safe fn putchar(c: core::ffi::c_int) -> core::ffi::c_int;
        }
        for byte in txt.bytes() {
            let _ = putchar(byte as _);
        }
        flush()
    }

    #[rquickjs::function]
    pub fn flush() {
        // Flush the output buffer of libc to make sure all output is printed out.
        unsafe extern "C" {
            fn fflush(f: *mut core::ffi::c_void) -> core::ffi::c_int;
        }
        let _ = unsafe { fflush(core::ptr::null_mut()) };
    }
}
