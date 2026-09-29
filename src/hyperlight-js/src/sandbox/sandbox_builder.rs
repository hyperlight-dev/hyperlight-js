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
#[cfg(any(kvm, mshv3, hvf))]
use std::time::Duration;

use hyperlight_common::virtq::G2H_LOWER_SLOT_COUNT;
use hyperlight_common::vmem::PAGE_SIZE;
use hyperlight_host::sandbox::SandboxConfiguration;
use hyperlight_host::{is_hypervisor_present, GuestBinary, HyperlightError, Result};

use super::proto_js_sandbox::ProtoJSSandbox;
use crate::HostPrintFn;

/// A builder for a ProtoJSSandbox.
///
/// Transport setters record choices; [`Self::get_config`] and [`Self::build`]
/// resolve buffer sizes, pool budgets, and descriptor counts.
pub struct SandboxBuilder {
    base_config: SandboxConfiguration,
    host_print_fn: Option<HostPrintFn>,
    /// Requested legacy input capacity in bytes, also used for host replies.
    legacy_input_capacity: usize,
    /// Requested legacy output capacity in bytes, used for host requests.
    legacy_output_capacity: usize,
    /// Explicit input budget before minimum-size normalization, if set.
    requested_input_pool_pages: Option<usize>,
    /// Explicit output budget before minimum-size normalization, if set.
    requested_output_pool_pages: Option<usize>,
    /// Requested input buffer size in bytes, normalized during resolution.
    input_transport_buffer_size: usize,
    /// Requested output buffer size in bytes, normalized during resolution.
    output_transport_buffer_size: usize,
}

/// The minimum scratch size for the JS runtime sandbox.
///
/// The scratch region provides writable physical memory for:
///   - Transport queues and buffer pools
///   - Page table copies (proportional to snapshot size — our ~13 MB guest
///     binary + heap produce ~72 KiB of page tables)
///   - Dynamically allocated pages (GDT/IDT, stack growth, Copy-on-Write
///     resolution during QuickJS initialisation)
///   - Exception stack and metadata (2 pages at the top)
///
/// Hyperlight's default scratch is sized for smaller guests. The JS
/// runtime needs 1 MiB (0x10_0000) to leave room for QuickJS initialisation
/// after the transport and page-table overheads.
const MIN_SCRATCH_SIZE: usize = 0x10_0000; // 1 MiB

/// The minimum heap size is 4 MiB.  The QuickJS engine needs a
/// reasonable amount of heap during initialisation for builtins,
/// global objects, and the bytecode compiler.  This lives in the
/// identity-mapped snapshot region (NOT scratch).
const MIN_HEAP_SIZE: u64 = 4096 * 1024;

/// The previous 16 KiB message capacity for each direction.
const DEFAULT_LEGACY_BUFFER_SIZE: usize = 16 * 1024;

/// Default capacity of each full-size transport buffer.
const DEFAULT_TRANSPORT_BUFFER_SIZE: usize = 16 * 1024;

/// The previous 8 KiB minimum for byte-based buffer configuration.
const MIN_LEGACY_BUFFER_SIZE: usize = 8 * 1024;

impl SandboxBuilder {
    /// Create a new SandboxBuilder
    pub fn new() -> Self {
        let mut config = SandboxConfiguration::default();
        config.set_heap_size(MIN_HEAP_SIZE);
        config.set_scratch_size(MIN_SCRATCH_SIZE);

        Self {
            base_config: config,
            host_print_fn: None,
            legacy_input_capacity: DEFAULT_LEGACY_BUFFER_SIZE,
            legacy_output_capacity: DEFAULT_LEGACY_BUFFER_SIZE,
            input_transport_buffer_size: DEFAULT_TRANSPORT_BUFFER_SIZE,
            output_transport_buffer_size: DEFAULT_TRANSPORT_BUFFER_SIZE,
            requested_input_pool_pages: None,
            requested_output_pool_pages: None,
        }
    }

    /// Set the host print function
    pub fn with_host_print_fn(mut self, host_print_fn: HostPrintFn) -> Self {
        self.host_print_fn = Some(host_print_fn);
        self
    }

    /// Set the guest-to-host message capacity in bytes.
    #[deprecated(note = "Use with_output_transport_pool_pages for an explicit pool budget")]
    pub fn with_guest_output_buffer_size(mut self, guest_output_buffer_size: usize) -> Self {
        self.legacy_output_capacity = guest_output_buffer_size;
        self.requested_output_pool_pages = None;

        self
    }

    /// Set the host-to-guest message capacity in bytes.
    /// Also reserves output-pool space for host callback replies.
    #[deprecated(note = "Use with_input_transport_pool_pages for explicit pool budgets")]
    pub fn with_guest_input_buffer_size(mut self, guest_input_buffer_size: usize) -> Self {
        self.legacy_input_capacity = guest_input_buffer_size;
        self.requested_input_pool_pages = None;
        self.requested_output_pool_pages = None;

        self
    }

    /// Set the total guest-to-host buffer pool size in 4 KiB pages.
    ///
    /// One page is split into small buffers. The rest must fit at least two
    /// transport buffers, so a host callback's request and reply can coexist.
    /// Smaller budgets are raised to this minimum: nine pages with the default
    /// 16 KiB transport buffers.
    pub fn with_output_transport_pool_pages(mut self, pages: usize) -> Self {
        self.requested_output_pool_pages = Some(pages);

        self
    }

    /// Set the total host-to-guest buffer pool size in 4 KiB pages.
    ///
    /// Must fit at least two transport buffers: one for data and one spare for
    /// control traffic. Smaller budgets are raised to this minimum: eight pages
    /// with the default 16 KiB transport buffers.
    pub fn with_input_transport_pool_pages(mut self, pages: usize) -> Self {
        self.requested_input_pool_pages = Some(pages);

        self
    }

    /// Set each large guest-to-host transport buffer's capacity in bytes.
    ///
    /// Defaults to 16 KiB; Hyperlight clamps sizes to `256..=u32::MAX`.
    /// The fixed 256-byte small-buffer tier is unaffected. Messages larger
    /// than one buffer, including framing, are split across buffers.
    ///
    /// Pool and descriptor sizing is deferred to configuration resolution,
    /// preserving explicit page budgets or legacy message capacities.
    pub fn with_output_transport_buffer_size(mut self, size: usize) -> Self {
        self.output_transport_buffer_size = size;

        self
    }

    /// Set each host-to-guest transport buffer's capacity in bytes.
    ///
    /// Defaults to 16 KiB; Hyperlight clamps sizes to `256..=u32::MAX`.
    /// Messages larger than one buffer, including framing, are split across buffers.
    ///
    /// Pool and descriptor sizing is deferred to configuration resolution,
    /// preserving explicit page budgets or legacy message capacities.
    pub fn with_input_transport_buffer_size(mut self, size: usize) -> Self {
        self.input_transport_buffer_size = size;

        self
    }

    /// Set the guest scratch size in bytes.
    /// The scratch region provides writable memory for the guest, including the
    /// dynamically-sized stack. Increase this if your guest code needs deep
    /// recursion or large local variables.
    ///
    /// Values at or below the JS runtime default (1 MiB) are ignored.
    pub fn with_guest_scratch_size(mut self, guest_scratch_size: usize) -> Self {
        if guest_scratch_size > MIN_SCRATCH_SIZE {
            self.base_config.set_scratch_size(guest_scratch_size);
        }
        self
    }

    /// Set the guest heap size
    /// This is the size of the heap that code executing in the guest can use.
    /// If this value is too small then the guest will fail, usually with a malloc failed error
    /// The default (and minimum) value for this is set to the value of the MIN_HEAP_SIZE const.
    pub fn with_guest_heap_size(mut self, guest_heap_size: u64) -> Self {
        if guest_heap_size > MIN_HEAP_SIZE {
            self.base_config.set_heap_size(guest_heap_size);
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
        self.base_config
            .set_interrupt_vcpu_sigrtmin_offset(offset)?;
        Ok(())
    }

    /// Sets the interrupt retry delay
    /// This controls the delay between sending signals to the VCPU thread to interrupt it.
    ///
    /// Only available for the hypervisor backends that use retrying interrupts
    /// (KVM and MSHV on Linux, Hypervisor.framework on macOS).
    #[cfg(any(kvm, mshv3, hvf))]
    pub fn with_interrupt_retry_delay(mut self, delay: Duration) -> Self {
        self.base_config.set_interrupt_retry_delay(delay);
        self
    }

    /// Get an owned snapshot of the effective configuration used by [`Self::build`].
    ///
    /// Resolves transport sizes without changing the builder. Modifying the
    /// returned value does not affect subsequent configuration or builds.
    pub fn get_config(&self) -> SandboxConfiguration {
        let mut config = self.base_config;
        self.configure_transport(&mut config);

        config
    }

    /// Enable or disable crashdump generation for the sandbox
    /// When enabled, core dumps will be generated when the guest crashes
    /// This requires the `crashdump` feature to be enabled
    #[cfg(crashdump)]
    pub fn with_crashdump_enabled(mut self, enabled: bool) -> Self {
        self.base_config.set_guest_core_dump(enabled);
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
    /// This method is only available when the `gdb` feature is enabled, the
    /// code is compiled in debug mode, and the target architecture is x86_64.
    /// hyperlight-host only implements the gdb debug stub on x86_64.
    #[cfg(gdb)]
    pub fn with_debugging_enabled(mut self, port: u16) -> Self {
        let debug_info = hyperlight_host::sandbox::config::DebugInfo { port };
        self.base_config.set_guest_debug_info(debug_info);
        self
    }

    /// Build the ProtoJSSandbox
    pub fn build(self) -> Result<ProtoJSSandbox> {
        if !is_hypervisor_present() {
            return Err(HyperlightError::NoHypervisorFound());
        }

        let config = self.get_config();
        let guest_binary = GuestBinary::Buffer(super::JSRUNTIME.to_vec());
        let proto_js_sandbox = ProtoJSSandbox::new(guest_binary, Some(config), self.host_print_fn)?;
        Ok(proto_js_sandbox)
    }

    /// Normalizes buffer sizes, applies explicit pool budgets or legacy
    /// capacities, and sizes descriptor queues from the complete buffer counts.
    /// Reserves a control spare and room for simultaneous callback requests/replies.
    ///
    /// Saturating arithmetic leaves unrepresentable pool sizes for Hyperlight's
    /// checked layout validation to reject at build time.
    fn configure_transport(&self, config: &mut SandboxConfiguration) {
        config.set_h2g_buffer_size(self.input_transport_buffer_size);
        config.set_g2h_buffer_size(self.output_transport_buffer_size);

        let inbufsz = config.get_h2g_buffer_size();
        let outbufsz = config.get_g2h_buffer_size();

        let input_cap = Self::legacy_capacity(self.legacy_input_capacity);
        let output_cap = Self::legacy_capacity(self.legacy_output_capacity);

        let input_pages = match self.requested_input_pool_pages {
            Some(pages) => pages.max(Self::pages_for_buffers(2, inbufsz)),
            None => {
                let data_buffers = input_cap.div_ceil(inbufsz);

                // Keep one buffer free for control calls that release retained external payloads.
                Self::pages_for_buffers(data_buffers + 1, inbufsz)
            }
        };

        let output_pages = match self.requested_output_pool_pages {
            Some(pages) => pages.max(1 + Self::pages_for_buffers(2, outbufsz)),
            None => {
                let req_bufs = output_cap.div_ceil(outbufsz);
                let reply_bufs = input_cap.div_ceil(outbufsz);
                // The output pool needs one extra page for its fixed 256-byte small-buffer tier.
                1 + Self::pages_for_buffers(req_bufs + reply_bufs, outbufsz)
            }
        };

        config.set_h2g_pool_pages(input_pages);
        config.set_g2h_pool_pages(output_pages);

        let input_buffers = input_pages.saturating_mul(PAGE_SIZE) / inbufsz;

        // Exclude the small-buffer page when counting large buffers, then add
        // its G2H_LOWER_SLOT_COUNT small buffers. Each buffer needs a descriptor.
        let output_buffers =
            (output_pages - 1).saturating_mul(PAGE_SIZE) / outbufsz + G2H_LOWER_SLOT_COUNT;

        config.set_h2g_queue_size(input_buffers.max(SandboxConfiguration::DEFAULT_H2G_QUEUE_SIZE));
        config.set_g2h_queue_size(output_buffers.max(SandboxConfiguration::DEFAULT_G2H_QUEUE_SIZE));
    }

    /// Applies the legacy 8 KiB minimum and rounds byte capacity up to whole pages.
    /// Saturates if the rounded capacity cannot be represented.
    fn legacy_capacity(bytes: usize) -> usize {
        bytes
            .max(MIN_LEGACY_BUFFER_SIZE)
            .div_ceil(PAGE_SIZE)
            .saturating_mul(PAGE_SIZE)
    }

    /// Rounds the combined buffer storage up to pages, saturating on overflow.
    fn pages_for_buffers(count: usize, buffer_size: usize) -> usize {
        count.saturating_mul(buffer_size).div_ceil(PAGE_SIZE)
    }
}

impl Default for SandboxBuilder {
    fn default() -> Self {
        Self::new()
    }
}
