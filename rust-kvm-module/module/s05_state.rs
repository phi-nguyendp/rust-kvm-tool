// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 05: read the *running host's* state into a `host_state`, and a
//! helper that gets a current VMCS ready, shared by stages 5-9.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c `vmx_set_constant_host_state()` and
//! `vmx_vcpu_load_vmcs()` (they read GDT/IDT/TR/CR0/CR3/CR4/MSRs of the CPU the vcpu is
//! about to run on), `alloc_vmcs_cpu()`.
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).
//!
//! ASSUMED names from other stages (fix the `use` if they differ):
//! `s02_vmxon::{vmx_enable_cpu, VmxCpu}`, `s03_vmcs::{vmclear, vmptrld, HwVmcs}`
//! (unsafe fns returning `vmx_enable::VmxResult`).

use core::arch::asm;

use kernel::prelude::*;

use crate::{
    hw,
    s02_vmxon::{vmx_enable_cpu, VmxCpu},
    s03_vmcs::{vmclear, vmptrld, HwVmcs},
    s04_controls::hw_vmcs_config,
    vmcs_ops::VmcsOps,
    vmx::*,
    vmx_controls::*,
    vmx_enable::VmxResult,
    vmx_state::*,
};

/// Operand of SGDT/SIDT/LGDT/LIDT: 2-byte limit then 8-byte base, no padding.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub(crate) struct DescPtr {
    pub(crate) limit: u16,
    pub(crate) base: u64,
}

pub(crate) fn sgdt() -> DescPtr {
    let mut d = DescPtr::default();
    // SAFETY: stores 10 bytes into `d`.
    unsafe { asm!("sgdt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    d
}
pub(crate) fn sidt() -> DescPtr {
    let mut d = DescPtr::default();
    // SAFETY: stores 10 bytes into `d`.
    unsafe { asm!("sidt [{}]", in(reg) &mut d, options(nostack, preserves_flags)) };
    d
}
/// # Safety
/// `d` must describe the real, mapped GDT.
pub(crate) unsafe fn lgdt(d: &DescPtr) {
    // SAFETY: forwarded.
    unsafe { asm!("lgdt [{}]", in(reg) d, options(nostack, preserves_flags)) };
}
/// # Safety
/// `d` must describe the real, mapped IDT.
pub(crate) unsafe fn lidt(d: &DescPtr) {
    // SAFETY: forwarded.
    unsafe { asm!("lidt [{}]", in(reg) d, options(nostack, preserves_flags)) };
}

macro_rules! read_seg {
    ($reg:literal) => {{
        let v: u16;
        // SAFETY: reading a segment selector has no side effects.
        unsafe { asm!(concat!("mov {0:x}, ", $reg), out(reg) v, options(nomem, nostack, preserves_flags)) };
        v
    }};
}

/// Base address from a 16-byte 64-bit system-segment (TSS) descriptor in the GDT.
fn tss_base(gdt: &DescPtr, tr_sel: u16) -> u64 {
    let gdt_base = gdt.base;
    let d = (gdt_base + (tr_sel & !7) as u64) as *const u8;
    // SAFETY: the TSS descriptor lies inside the mapped GDT (selector came from `str`).
    let (lo, hi) = unsafe {
        (core::ptr::read_unaligned(d as *const u64), core::ptr::read_unaligned(d.add(8) as *const u32))
    };
    ((lo >> 16) & 0xff_ffff) | (((lo >> 56) & 0xff) << 24) | ((hi as u64) << 32)
}

/// Snapshot of the CPU we are running on: what `vmx_set_host_state()` writes into the VMCS.
/// `rsp`/`rip` are left 0: the entry assembly (stage 6) writes them.
pub(crate) fn read_host_state() -> host_state {
    let gdt = sgdt();
    let idt = sidt();
    let tr: u16;
    // SAFETY: `str` has no side effects.
    unsafe { asm!("str {0:x}", out(reg) tr, options(nomem, nostack, preserves_flags)) };
    host_state {
        cr0: hw::read_cr0(),
        cr3: hw::read_cr3(),
        cr4: hw::read_cr4(),
        cs_sel: read_seg!("cs"),
        ss_sel: read_seg!("ss"),
        ds_sel: read_seg!("ds"),
        es_sel: read_seg!("es"),
        fs_sel: read_seg!("fs"),
        gs_sel: read_seg!("gs"),
        tr_sel: tr,
        fs_base: hw::rdmsr(MSR_FS_BASE),
        gs_base: hw::rdmsr(MSR_GS_BASE),
        tr_base: tss_base(&gdt, tr),
        gdtr_base: gdt.base,
        idtr_base: idt.base,
        sysenter_cs: hw::rdmsr(MSR_IA32_SYSENTER_CS) as u32,
        sysenter_esp: hw::rdmsr(MSR_IA32_SYSENTER_ESP),
        sysenter_eip: hw::rdmsr(MSR_IA32_SYSENTER_EIP),
        efer: hw::rdmsr(MSR_EFER),
        rsp: 0,
        rip: 0,
    }
}

/// VMX operation on + one VMCS cleared and made current, torn down on drop
/// (kernel: `loaded_vmcs` + `vmx_hardware_enable`). The helper of stage 5-9 selftests.
pub(crate) struct LoadedVmcs {
    pub(crate) cfg: vmcs_config,
    pub(crate) region: hw::PageBox,
    // dropped last: VMXOFF after the VMCLEAR in `drop`
    _cpu: VmxCpu,
}

impl LoadedVmcs {
    pub(crate) fn new() -> core::result::Result<Self, &'static str> {
        let cfg = hw_vmcs_config()?;
        let cpu = vmx_enable_cpu()?;
        let mut region = hw::PageBox::new().map_err(|_| "alloc vmcs")?;
        // the VMCS revision identifier goes in the first dword (bit 31 = 0: not a shadow VMCS)
        region.as_mut_slice()[0..4].copy_from_slice(&(cfg.revision_id & 0x7fff_ffff).to_le_bytes());
        let phys = region.phys();
        // SAFETY: in VMX operation (`cpu` is alive); `phys` is an initialised VMCS region.
        unsafe {
            if vmclear(phys) != VmxResult::Succeed {
                return Err("vmclear failed");
            }
            if vmptrld(phys) != VmxResult::Succeed {
                return Err("vmptrld failed");
            }
        }
        Ok(Self { cfg, region, _cpu: cpu })
    }
}

impl Drop for LoadedVmcs {
    fn drop(&mut self) {
        // SAFETY: still in VMX operation (`_cpu` is dropped after this).
        unsafe {
            let _ = vmclear(self.region.phys());
        }
    }
}

/// PROVIDED. Writes a complete VMCS with the stage 4/5 code, reads some of it back through
/// real VMREADs, and runs the guest-state checker on hardware-written values.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let lv = LoadedVmcs::new()?;
    let cfg = lv.cfg;
    let mut v = HwVmcs;

    vmx_vmcs_controls_write(&mut v, &cfg);
    if v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL) != cfg.cpu_based_exec_ctrl {
        return Err("controls did not read back");
    }

    let hs = read_host_state();
    pr_info!(
        "ktvmx: host: cs {:#x} ss {:#x} tr {:#x} tr_base {:#x} gdt {:#x} idt {:#x} cr3 {:#x}\n",
        hs.cs_sel,
        hs.ss_sel,
        hs.tr_sel,
        hs.tr_base,
        hs.gdtr_base,
        hs.idtr_base,
        hs.cr3
    );
    if hs.cr4 & X86_CR4_VMXE == 0 {
        return Err("host CR4.VMXE is clear: HOST_CR4 would be rejected");
    }
    vmx_set_host_state(&mut v, &hs);
    if v.vmcs_readl(HOST_CR3) != hs.cr3 || v.vmcs_readl(HOST_TR_BASE) != hs.tr_base {
        return Err("host state did not read back");
    }

    // Guest: 64-bit mode, nothing mapped yet (we only validate, we do not enter).
    vmx_setup_guest_long_mode(&mut v, 0x1000, 0x2000, 0x3000);
    if v.vmcs_read32(GUEST_CS_AR_BYTES) != 0xa09b {
        return Err("long-mode CS access rights are not 0xa09b");
    }
    let cs = vmx_get_segment(&v, VCPU_SREG_CS);
    if !(cs.l && !cs.db && cs.present && cs.s && cs.type_ == 0xb) {
        return Err("vmx_get_segment(CS) does not decode what vmx_set_segment wrote");
    }
    vmx_check_guest_state(&v, &cfg).map_err(|e| {
        pr_err!("ktvmx: guest state check: {}\n", e);
        "guest state rejected by vmx_check_guest_state"
    })?;

    v.vmcs_writel(GUEST_CR3, 1 << 60);
    if vmx_check_guest_state(&v, &cfg) != Err(ERR_CR3_RESERVED) {
        return Err("checker missed a bad CR3");
    }
    Ok(())
}
