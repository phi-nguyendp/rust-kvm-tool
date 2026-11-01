// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 03: VMCLEAR / VMPTRLD / VMREAD / VMWRITE.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx_ops.h (`vmcs_clear`, `vmcs_load`,
//! `__vmcs_readl`, `__vmcs_writel`, `vmx_asm1/vmx_asm2`).
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).
//! Needs stage 2's `vmx_enable_cpu()`; loading with `selftest>=3` panics until the
//! `todo!()`s are replaced.

use core::arch::asm;

use kernel::prelude::*;

use crate::{
    hw,
    s02_vmxon::vmx_enable_cpu,
    vmcs_fields::*,
    vmcs_ops::VmcsOps,
    vmx::*,
    vmx_caps::*,
    vmx_enable::*,
};

/// VMCLEAR: flush the VMCS at `vmcs_phys` to memory and mark it *clear* (launch state
/// "clear", so the next entry must be VMLAUNCH). Memory operand, like VMXON:
/// `asm!("vmclear [{p}]", "pushfq", "pop {f}", ...)`. Decode with `vmx_fail_kind`.
///
/// # Safety
/// In VMX operation (a `VmxCpu` is alive); `vmcs_phys` is a valid VMCS region address.
pub(crate) unsafe fn vmclear(vmcs_phys: u64) -> VmxResult {
    let _ = vmcs_phys;
    todo!()
}

/// VMPTRLD: make the VMCS at `vmcs_phys` the *current* VMCS of this CPU.
///
/// # Safety
/// As `vmclear`; also its first dword must hold the revision id (`vmcs_region_init`).
pub(crate) unsafe fn vmptrld(vmcs_phys: u64) -> VmxResult {
    let _ = vmcs_phys;
    todo!()
}

/// VMREAD of the current VMCS. `Err(result)` is VMfailInvalid/VMfailValid (no current
/// VMCS, unsupported field). Intel syntax: `vmread {val}, {field}` (destination first;
/// the field encoding is a register operand). RFLAGS tells success.
///
/// # Safety
/// In VMX operation.
pub(crate) unsafe fn vmread(field: u32) -> core::result::Result<u64, VmxResult> {
    let _ = field;
    todo!()
}

/// VMWRITE to the current VMCS. Intel syntax: `vmwrite {field}, {value}` (field first).
///
/// # Safety
/// In VMX operation.
pub(crate) unsafe fn vmwrite(field: u32, value: u64) -> VmxResult {
    let _ = (field, value);
    todo!()
}

/// PROVIDED. `VmcsOps` on the real current VMCS, so every stage that is generic over
/// `VmcsOps` (stages 4-10) runs on hardware through this type.
/// (kernel: `vmcs_readl()` / `vmcs_writel()`; those call `vmread_error()` on failure,
/// which warns once; we log.)
pub(crate) struct HwVmcs;

impl VmcsOps for HwVmcs {
    fn vmcs_readl(&self, field: u32) -> u64 {
        // SAFETY: only used while a VmxCpu is alive and a VMCS is current.
        match unsafe { vmread(field) } {
            Ok(v) => v,
            Err(r) => {
                pr_err!("ktvmx: vmread({:#x}) failed: {:?}\n", field, r);
                0
            }
        }
    }
    fn vmcs_writel(&mut self, field: u32, value: u64) {
        // SAFETY: as above.
        let r = unsafe { vmwrite(field, value) };
        if r != VmxResult::Succeed {
            pr_err!("ktvmx: vmwrite({:#x}, {:#x}) failed: {:?}\n", field, value, r);
        }
    }
}

/// PROVIDED. vmxon, vmclear, vmptrld, write GUEST_RIP, read it back, poke a read-only
/// field, vmclear, vmxoff.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let _cpu = vmx_enable_cpu()?;
    let basic = vmx_basic_decode(hw::rdmsr(MSR_IA32_VMX_BASIC));
    let mut page = hw::PageBox::new().map_err(|_| "alloc vmcs")?;
    vmcs_region_init(page.as_mut_slice(), &basic, false)?;
    let phys = page.phys();

    // SAFETY: in VMX operation (guard above); phys is an initialised VMCS region.
    unsafe {
        if vmclear(phys) != VmxResult::Succeed {
            return Err("vmclear failed");
        }
        if vmptrld(phys) != VmxResult::Succeed {
            return Err("vmptrld failed");
        }
    }

    let mut vmcs = HwVmcs;
    const MAGIC: u64 = 0xffff_8000_dead_beef;
    vmcs.vmcs_writel(GUEST_RIP, MAGIC);
    if vmcs.vmcs_readl(GUEST_RIP) != MAGIC {
        return Err("GUEST_RIP did not read back");
    }
    vmcs.vmcs_writel(GUEST_CS_SELECTOR, 0x10);
    if vmcs.vmcs_readl(GUEST_CS_SELECTOR) != 0x10 {
        return Err("GUEST_CS_SELECTOR did not read back");
    }

    // Host logic agrees with hardware about read-only fields (stage 3 host half):
    let venum = hw::rdmsr(MSR_IA32_VMX_VMCS_ENUM);
    if vmcs_checked_write(&mut vmcs, venum, VM_EXIT_REASON, 0)
        != Err(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT)
    {
        return Err("vmcs_checked_write let a read-only field through");
    }
    // ... and the CPU does the same (unless IA32_VMX_MISC[29] allows VMWRITE to any field).
    // SAFETY: in VMX operation with a current VMCS.
    unsafe {
        match vmwrite(VM_EXIT_REASON, 0) {
            VmxResult::FailValid => {
                if vmread(VM_INSTRUCTION_ERROR) != Ok(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT as u64) {
                    return Err("VMfailValid, but VM-instruction error is not 13");
                }
            }
            VmxResult::Succeed => pr_info!("ktvmx: CPU allows VMWRITE to read-only fields\n"),
            VmxResult::FailInvalid => return Err("vmwrite: unexpected VMfailInvalid"),
        }
        if vmclear(phys) != VmxResult::Succeed {
            return Err("final vmclear failed");
        }
    }
    Ok(())
}
