// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 02: VMXON/VMXOFF and per-CPU enabling.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (`vmx_hardware_enable`, `kvm_cpu_vmxon`),
//! arch/x86/kvm/vmx/vmx_ops.h (the fail-decoding asm), arch/x86/virt/vmx/hw.c.
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).
//!
//! WARNING: until the `todo!()`s below are replaced, loading the module with
//! `selftest>=2` panics the (throw-away QEMU) kernel. That is what `todo!()` does
//! in the kernel.

use core::arch::asm;

use crate::{
    hw,
    s01_caps::HwMsrs,
    vmx::*,
    vmx_caps::*,
    vmx_enable::*,
};

/// VMXON with the physical address of an initialised VMXON region.
/// Return how it ended, decoded with `vmx_fail_kind(rflags)`.
///
/// Hint: the operand is a *memory* operand holding the 64-bit physical address
/// (not the address itself), and the result is only in RFLAGS, so read RFLAGS right
/// after the instruction:
/// ```ignore
/// let flags: u64;
/// asm!("vmxon [{p}]", "pushfq", "pop {f}", p = in(reg) &region_phys, f = out(reg) flags);
/// vmx_fail_kind(flags)
/// ```
/// (asm! uses Intel syntax by default. `pushfq/pop` touches the stack, so do not pass
/// `options(nostack)`. Compare with `vmx_asm1` in vmx_ops.h, which uses `jc/jz` to a
/// fixup instead.)
///
/// # Safety
/// CR4.VMXE set, CR0/CR4 fixed bits satisfied, the region initialised (stage 2 host
/// logic), not already in VMX operation on this CPU.
pub(crate) unsafe fn vmxon(region_phys: u64) -> VmxResult {
    let _ = region_phys;
    todo!()
}

/// VMXOFF. Same result decoding. (kernel: `kvm_cpu_vmxoff`)
///
/// # Safety
/// Must be in VMX operation on this CPU; no VMCS may be left launched and still needed.
pub(crate) unsafe fn vmxoff() -> VmxResult {
    todo!()
}

/// The proof that this CPU is in VMX operation. Dropping it leaves VMX operation and
/// restores CR4. The fields are for `vmx_enable_cpu` to fill in.
pub(crate) struct VmxCpu {
    pub(crate) old_cr4: u64,
    pub(crate) _region: hw::PageBox,
}

impl Drop for VmxCpu {
    fn drop(&mut self) {
        // SAFETY: a VmxCpu only exists between a successful vmxon and here.
        unsafe {
            let _ = vmxoff();
            hw::write_cr4(self.old_cr4);
        }
    }
}

/// Enable VMX on the current CPU: this is `vmx_hardware_enable()`. In order:
/// 1. `vmx_probe(&HwMsrs, cpuid.1.ecx)`; map its error to a `&'static str`;
/// 2. if `feat_ctl_write` is `Some(v)`, `hw::wrmsr(MSR_IA32_FEATURE_CONTROL, v)`;
/// 3. read the fixed-bit MSRs with `vmx_read_cr_fixed(&HwMsrs)`;
/// 4. allocate a `hw::PageBox`, `vmxon_region_init` it with `caps.basic`;
/// 5. remember `old_cr4 = hw::read_cr4()`; compute the new CR4 with
///    `cr_fixed_adjust(old_cr4 | X86_CR4_VMXE, ...)` and write it (`hw::write_cr4`);
///    CR0 must already satisfy its fixed bits: check with `cr_fixed_ok`, and restore
///    CR4 and fail if not (Linux always runs with PG|PE|NE... but verify);
/// 6. `vmxon(region.phys())`; on failure restore CR4 and return an error;
/// 7. return the `VmxCpu`.
///
/// The module does not pin itself to a CPU: run QEMU with `-smp 1`, and think about why
/// (see the stage doc).
pub(crate) fn vmx_enable_cpu() -> core::result::Result<VmxCpu, &'static str> {
    todo!()
}

/// PROVIDED. Enable, check, disable, check CR4 is back, and do it twice.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let before = hw::read_cr4();
    {
        let _cpu = vmx_enable_cpu()?;
        if hw::read_cr4() & X86_CR4_VMXE == 0 {
            return Err("CR4.VMXE not set while in VMX operation");
        }
    }
    if hw::read_cr4() != before {
        return Err("CR4 was not restored by VMXOFF path");
    }
    drop(vmx_enable_cpu()?);
    if hw::read_cr4() != before {
        return Err("CR4 was not restored the second time");
    }
    Ok(())
}
