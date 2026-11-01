//! ktvmx: write a KVM-style VMX hypervisor, kernel-style (as a Rust Linux module).
//!
//! The stage files in this crate are `#![no_std]` pure logic so that they can be
//! unit-tested on the host (`cargo test`) *and* be `#[path]`-included verbatim by
//! the kernel module in `module/`. Everything that touches real hardware
//! (VMXON, VMLAUNCH, ...) lives in `module/` and is tested in QEMU, see COURSE.md.
#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::too_many_arguments)]

pub mod vmx;
pub mod vmcs_ops;
pub mod vmx_caps;
pub mod vmx_enable;
pub mod vmcs_fields;
pub mod vmx_controls;
pub mod vmx_state;
pub mod vmx_run;
pub mod vmx_exit;
pub mod vmx_ept;
pub mod vmx_events;
pub mod vmx_bitmaps;
pub mod vmx_uapi;
#[cfg(feature = "std")]
pub mod mock;
