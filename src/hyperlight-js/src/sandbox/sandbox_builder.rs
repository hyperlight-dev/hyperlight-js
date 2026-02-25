#[cfg(target_os = "linux")]
use std::time::Duration;

use hyperlight_host::sandbox::SandboxConfiguration;
use hyperlight_host::{is_hypervisor_present, GuestBinary, HyperlightError, Result};

use super::proto_js_sandbox::ProtoJSSandbox;
use crate::HostPrintFn;

/// A builder for a ProtoJSSandbox
pub struct SandboxBuilder {
    config: SandboxConfiguration,
    host_print_fn: Option<HostPrintFn>,
}

/// The minimum scratch size for the JS runtime sandbox.
///
/// The scratch region provides writable physical memory for:
///   - I/O buffers (input + output data)
///   - Page table copies (proportional to snapshot size — our ~13 MB guest
///     binary + heap produce ~72 KiB of page tables)
///   - Dynamically allocated pages (GDT/IDT, stack growth, Copy-on-Write
///     resolution during QuickJS initialisation)
///   - Exception stack and metadata (2 pages at the top)
///
/// Hyperlight's default scratch (288 KiB) is far too small for the JS
/// runtime guest: after fixed overheads there are only ~44 free pages,
/// which are exhausted during init.  1 MiB (0x10_0000) matches
/// hyperlight's own "large guest" test configuration and gives
/// comfortable headroom.
const MIN_SCRATCH_SIZE: usize = 0x10_0000; // 1 MiB

/// The minimum heap size is 4 MiB.  The QuickJS engine needs a
/// reasonable amount of heap during initialisation for builtins,
/// global objects, and the bytecode compiler.  This lives in the
/// identity-mapped snapshot region (NOT scratch).
const MIN_HEAP_SIZE: u64 = 4096 * 1024;

impl SandboxBuilder {
    /// Create a new SandboxBuilder
    pub fn new() -> Self {
        let mut config = SandboxConfiguration::default();
        config.set_heap_size(MIN_HEAP_SIZE);
        config.set_scratch_size(MIN_SCRATCH_SIZE);

        Self {
            config,
            host_print_fn: None,
        }
    }

    /// Set the host print function
    pub fn with_host_print_fn(mut self, host_print_fn: HostPrintFn) -> Self {
        self.host_print_fn = Some(host_print_fn);
        self
    }

    /// Set the guest output buffer size
    pub fn with_guest_output_buffer_size(mut self, guest_output_buffer_size: usize) -> Self {
        self.config.set_output_data_size(guest_output_buffer_size);
        self
    }

    /// Set the guest input buffer size
    /// This is the size of the buffer that the guest can write to
    /// to send data to the host
    /// The host can read from this buffer
    /// The guest can write to this buffer
    pub fn with_guest_input_buffer_size(mut self, guest_input_buffer_size: usize) -> Self {
        self.config.set_input_data_size(guest_input_buffer_size);
        self
    }

    /// Set the guest scratch size in bytes.
    /// The scratch region provides writable memory for the guest, including the
    /// dynamically-sized stack. Increase this if your guest code needs deep
    /// recursion or large local variables.
    /// Values smaller than the default (288KiB) are ignored.
    pub fn with_guest_scratch_size(mut self, guest_scratch_size: usize) -> Self {
        if guest_scratch_size > MIN_SCRATCH_SIZE {
            self.config.set_scratch_size(guest_scratch_size);
        }
        self
    }

    /// Set the guest heap size
    /// This is the size of the heap that code executing in the guest can use.
    /// If this value is too small then the guest will fail, usually with a malloc failed error
    /// The default (and minimum) value for this is set to the value of the MIN_HEAP_SIZE const.
    pub fn with_guest_heap_size(mut self, guest_heap_size: u64) -> Self {
        if guest_heap_size > MIN_HEAP_SIZE {
            self.config.set_heap_size(guest_heap_size);
        }
        self
    }

    /// Sets the offset from `SIGRTMIN` to determine the real-time signal used for
    /// interrupting the VCPU thread.
    ///
    /// The final signal number is computed as `SIGRTMIN + offset`, and it must fall within
    /// the valid range of real-time signals supported by the host system.
    ///
    /// Returns Ok(()) if the offset is valid, or an error if it exceeds the maximum real-time signal number.
    #[cfg(target_os = "linux")]
    pub fn set_interrupt_vcpu_sigrtmin_offset(&mut self, offset: u8) -> Result<()> {
        self.config.set_interrupt_vcpu_sigrtmin_offset(offset)?;
        Ok(())
    }

    /// Sets the interrupt retry delay
    /// This controls the delay between sending signals to the VCPU thread to interrupt it.
    #[cfg(target_os = "linux")]
    pub fn with_interrupt_retry_delay(mut self, delay: Duration) -> Self {
        self.config.set_interrupt_retry_delay(delay);
        self
    }

    /// Get the current configuration
    pub fn get_config(&self) -> &SandboxConfiguration {
        &self.config
    }

    /// Enable or disable crashdump generation for the sandbox
    /// When enabled, core dumps will be generated when the guest crashes
    /// This requires the `crashdump` feature to be enabled
    #[cfg(feature = "crashdump")]
    pub fn with_crashdump_enabled(mut self, enabled: bool) -> Self {
        self.config.set_guest_core_dump(enabled);
        self
    }

    /// Enable debugging for the guest runtime
    /// This will allow the guest runtime to be natively debugged using GDB or
    /// other debugging tools
    ///
    /// # Example:
    /// ```rust
    /// use hyperlight_js::SandboxBuilder;
    /// let sandbox = SandboxBuilder::new()
    ///    .with_debugging_enabled(8080) // Enable debugging on port 8080
    ///    .build()
    ///    .expect("Failed to build sandbox");
    /// ```
    /// # Note:
    /// This method is only available when the `gdb` feature is enabled
    /// and the code is compiled in debug mode.
    #[cfg(all(feature = "gdb", debug_assertions))]
    pub fn with_debugging_enabled(mut self, port: u16) -> Self {
        let debug_info = hyperlight_host::sandbox::config::DebugInfo { port };
        self.config.set_guest_debug_info(debug_info);
        self
    }

    /// Build the ProtoJSSandbox
    pub fn build(self) -> Result<ProtoJSSandbox> {
        if !is_hypervisor_present() {
            return Err(HyperlightError::NoHypervisorFound());
        }
        let guest_binary = GuestBinary::Buffer(super::JSRUNTIME);
        let proto_js_sandbox =
            ProtoJSSandbox::new(guest_binary, Some(self.config), self.host_print_fn)?;
        Ok(proto_js_sandbox)
    }
}

impl Default for SandboxBuilder {
    fn default() -> Self {
        Self::new()
    }
}
