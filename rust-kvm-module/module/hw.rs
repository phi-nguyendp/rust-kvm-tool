// SPDX-License-Identifier: GPL-2.0
//! PROVIDED hardware helpers shared by all stages: rdmsr, CPUID, control registers,
//! and page-aligned zeroed pages with a physical address (VMXON region, VMCS, EPT
//! tables, guest RAM all need one). Written best-effort against the in-tree Rust API
//! of the kernel in ~/WS/linux_mainline; if it fails to compile, fix it here once.

use core::arch::asm;
use kernel::prelude::*;

pub(crate) fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: caller passes an MSR that exists (else #GP, which would oops).
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    ((hi as u64) << 32) | lo as u64
}

pub(crate) fn wrmsr(msr: u32, val: u64) {
    // SAFETY: as rdmsr.
    unsafe { asm!("wrmsr", in("ecx") msr, in("eax") val as u32, in("edx") (val >> 32) as u32, options(nomem, nostack, preserves_flags)) };
}

/// (eax, ebx, ecx, edx)
pub(crate) fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u32, u32, u32);
    // SAFETY: cpuid is always valid; rbx is reserved by LLVM so save/restore it.
    unsafe {
        asm!("mov {tmp:r}, rbx", "cpuid", "xchg {tmp:r}, rbx",
             tmp = out(reg) b, inout("eax") leaf => a, inout("ecx") subleaf => c, out("edx") d,
             options(nomem, nostack, preserves_flags));
    }
    (a, b, c, d)
}

pub(crate) fn read_cr0() -> u64 {
    let v: u64;
    // SAFETY: reading CR0 has no side effects.
    unsafe { asm!("mov {}, cr0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
pub(crate) fn read_cr3() -> u64 {
    let v: u64;
    // SAFETY: reading CR3 has no side effects.
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
pub(crate) fn read_cr4() -> u64 {
    let v: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
/// # Safety
/// Writing CR4 changes global CPU behaviour; keep the old value and restore it.
pub(crate) unsafe fn write_cr4(v: u64) {
    // SAFETY: forwarded to the caller.
    unsafe { asm!("mov cr4, {}", in(reg) v, options(nostack, preserves_flags)) };
}

extern "C" {
    // arch/x86/kernel/head64.c: EXPORT_SYMBOL(page_offset_base)
    static page_offset_base: u64;
}

/// Physical address of a direct-map (kmalloc'ed) virtual address, like `__pa()`.
pub(crate) fn virt_to_phys(p: *const u8) -> u64 {
    // SAFETY: page_offset_base is initialised at boot and never changes.
    (p as u64) - unsafe { page_offset_base }
}

#[repr(C, align(4096))]
struct Page4K([u8; 4096]);

/// One zeroed, 4 KiB-aligned page from kmalloc (kmalloc aligns power-of-two sizes
/// naturally, and the Rust `Kmalloc` allocator honours `align(4096)`).
pub(crate) struct PageBox(KBox<Page4K>);

impl PageBox {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self(KBox::new(Page4K([0; 4096]), GFP_KERNEL)?))
    }
    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.0 .0.as_ptr() as *mut u8
    }
    pub(crate) fn phys(&self) -> u64 {
        virt_to_phys(self.as_ptr())
    }
    pub(crate) fn as_slice(&self) -> &[u8; 4096] {
        &self.0 .0
    }
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8; 4096] {
        &mut self.0 .0
    }
}
