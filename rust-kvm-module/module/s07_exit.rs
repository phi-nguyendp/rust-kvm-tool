// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 07: the VM-exit loop.
//! Kernel counterparts: arch/x86/kvm/x86.c `vcpu_run()` / `vcpu_enter_guest()` calling
//! `kvm_x86_call(handle_exit)` = `vmx_handle_exit()` in arch/x86/kvm/vmx/vmx.c.
//! NOT COMPILED by the course author (see CLAUDE.md): expect a small fix or two, and the
//! stage-6 interface below is an ASSUMPTION, adapt `run_guest`/`selftest` to what
//! your `s06_run.rs` really offers.
//!
//! ASSUMED from stage 6 (`crate::s06_run`):
//! - `struct vcpu_vmx { pub regs: [u64; 16], .. }`: guest GPRs, kernel order (VCPU_REGS_*),
//!   filled in on every exit and loaded again on every entry;
//! - `vcpu_vmx::new_flat32(entry_phys: u64, rsp: u64) -> Result<vcpu_vmx, &'static str>`:
//!   VMXON on this CPU, a VMCS made current, a flat 32-bit protected-mode guest (paging off:
//!   guest-physical == host-physical, no EPT yet), RIP = `entry_phys`, with HLT / unconditional
//!   I/O / MSR exiting on (CPUID always exits); dropping it does VMCLEAR + VMXOFF;
//! - `vcpu_vmx::run(&mut self) -> u32`: VMLAUNCH or VMRESUME, returns the raw `VM_EXIT_REASON`,
//!   with host interrupts enabled again on return.

use kernel::prelude::*;

use crate::{
    hw,
    s03_vmcs::HwVmcs,
    s06_run::vcpu_vmx,
    vmx::*,
    vmx_exit::*,
};

/// `hw::cpuid` in the shape `exit_ctx::host_cpuid` wants.
pub(crate) fn host_cpuid(leaf: u32, subleaf: u32) -> [u32; 4] {
    let (a, b, c, d) = hw::cpuid(leaf, subleaf);
    [a, b, c, d]
}

/// Run `vmx` until the guest halts. `uart` collects what the guest prints.
///
/// Loop (at most `MAX_EXITS` iterations: a guest that never halts must not hang the
/// kernel): `vmx.run()`, then `vmx_handle_exit(&mut vmx.regs, &mut HwVmcs, &mut ctx)` and
/// act on the returned `exit_action`:
/// `Resume` -> next iteration; `Halt` -> `Ok(())`; anything else -> `Err` (say which exit
/// reason in the pr_err! so you can debug it).
/// (kernel: `vcpu_run` loops on `vcpu_enter_guest` while it returns 1.)
pub(crate) fn run_guest(vmx: &mut vcpu_vmx, uart: &mut uart_com1) -> core::result::Result<(), &'static str> {
    const MAX_EXITS: usize = 1000;
    let _ = (&vmx, &uart, MAX_EXITS);
    todo!("loop: vmx.run(); vmx_handle_exit(); Resume => continue, Halt => Ok")
}

/// guest/07_exit32.S, assembled by `make -C guest 07_exit32.bin` (33 bytes, position independent).
const GUEST_CODE: [u8; 33] = [
    0xb8, 0x01, 0x00, 0x00, 0x00, 0x0f, 0xa2, 0x89, 0xce, 0xb8, 0x00, 0x00, 0x00, 0x40, 0x0f, 0xa2,
    0x89, 0xdf, 0xba, 0xf8, 0x03, 0x00, 0x00, 0xb0, 0x48, 0xee, 0xb0, 0x69, 0xee, 0xb0, 0x0a, 0xee,
    0xf4,
];

/// PROVIDED. Boots `GUEST_CODE` in a page of its own and checks everything the guest did.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let mut page = hw::PageBox::new().map_err(|_| "alloc guest page")?;
    page.as_mut_slice()[..GUEST_CODE.len()].copy_from_slice(&GUEST_CODE);

    // The guest is flat-32, paging off: it executes at the page's *physical* address, and
    // its stack (unused here) at the top of the page.
    let phys = page.phys();
    let mut vmx = vcpu_vmx::new_flat32(phys, phys + 4096)?;
    let mut uart = uart_com1::new();
    run_guest(&mut vmx, &mut uart)?;

    if uart.output() != b"Hi\n" {
        pr_err!("ktvmx: console = {:?}\n", uart.output());
        return Err("guest console output is not \"Hi\\n\"");
    }
    let ecx1 = vmx.regs[VCPU_REGS_RSI] as u32;
    if ecx1 & CPUID_1_ECX_VMX != 0 {
        return Err("CPUID.1 still advertises VMX to the guest");
    }
    if ecx1 & CPUID_1_ECX_HYPERVISOR == 0 {
        return Err("CPUID.1 does not set the hypervisor bit");
    }
    if vmx.regs[VCPU_REGS_RDI] as u32 != u32::from_le_bytes(*b"KtVm") {
        return Err("CPUID.0x40000000 signature wrong");
    }
    Ok(())
}
