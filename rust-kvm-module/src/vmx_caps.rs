//! Stage 1: can this CPU do VMX, and what does it support?
//!
//! Kernel counterparts: arch/x86/kernel/cpu/feat_ctl.c (init_ia32_feat_ctl: the
//! IA32_FEATURE_CONTROL dance), arch/x86/include/asm/vmx.h (vmx_basic_vmcs_*),
//! arch/x86/kvm/vmx/vmx.c (setup_vmcs_config, hardware_setup: the EPT/VPID
//! capability checks `cpu_has_vmx_ept_*`).
//! Book: Intel SDM Vol. 3C, "Discovering Support for VMX" and "Enabling and Entering
//! VMX Operation" (ch. 23), Appendix A.1 (IA32_VMX_BASIC), Appendix A.10
//! (IA32_VMX_EPT_VPID_CAP). Section numbers shift between SDM editions; the appendix
//! titles are stable.
//!
//! All hardware access goes through `MsrOps`, so this file runs on the host in
//! `cargo test` and in the kernel module unchanged. In the real module an `rdmsr` of
//! an MSR that does not exist is a #GP (an oops): only read what the SDM says exists.
#![allow(non_camel_case_types)]

use crate::vmcs_ops::MsrOps;
use crate::vmx::*;
use crate::vmx_caps::feat_ctl_state::{CanEnable, DisabledByBios, Enabled};
use crate::vmx_caps::vmx_probe_error::{DisabledByBios as DisabledByBiosErr, MsrMissing, NoVmx};

/// CPUID.1:ECX.VMX[bit 5]. (kernel: `cpu_has_vmx()` in arch/x86/kvm/vmx/vmx.h)
pub fn cpu_has_vmx(cpuid_1_ecx: u32) -> bool {
    cpuid_1_ecx & CPUID_1_ECX_VMX != 0
}

/// What the firmware left in IA32_FEATURE_CONTROL (MSR 0x3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum feat_ctl_state {
    /// Locked with "VMXON outside SMX" enabled, or nothing left to do.
    Enabled,
    /// Not locked yet: the OS may enable VMX itself by writing this value
    /// (current value | LOCKED | VMX_ENABLED_OUTSIDE_SMX). Writing locks the MSR
    /// until the next reset.
    CanEnable(u64),
    /// Locked with VMXON-outside-SMX clear: only a BIOS setting can fix it.
    DisabledByBios,
}

/// Classify an IA32_FEATURE_CONTROL value. We only care about VMXON outside SMX
/// (bit 2); the inside-SMX bit (bit 1) is for Intel TXT launches.
/// (kernel: init_ia32_feat_ctl() in arch/x86/kernel/cpu/feat_ctl.c)
pub fn vmx_feat_ctl_state(feature_control: u64) -> feat_ctl_state {
    if feature_control & FEAT_CTL_LOCKED != 0 {
        if feature_control & FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX == 0 {
            DisabledByBios
        } else {
            Enabled
        }
    } else if feature_control & FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX != 0 {
        // if it is not locked, we would need to lock it
        CanEnable(feature_control | FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX | FEAT_CTL_LOCKED)
    } else {
        CanEnable(feature_control | FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX | FEAT_CTL_LOCKED)
    }
}

/// True if the BIOS locked VMX off. (kernel: the "VMX disabled by BIOS" message)
pub fn vmx_disabled_by_bios(feature_control: u64) -> bool {
    if (feature_control & FEAT_CTL_LOCKED != 0)
        && (feature_control & FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX == 0)
    {
        true
    } else {
        false
    }
}

/// Decoded IA32_VMX_BASIC (MSR 0x480). SDM Appendix A.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct vmx_basic {
    /// bits 30:0. Must be written into the first 4 bytes of the VMXON region and of
    /// every VMCS (bit 31 of that dword stays 0 for a normal VMCS).
    pub revision_id: u32,
    /// bits 44:32. Size in bytes of the VMXON region / VMCS (at most 4096).
    pub vmcs_size: u32,
    /// bit 48. If set, physical addresses used by VMX (VMXON/VMCS pointers) must fit
    /// in 32 bits.
    pub phys_addr_32bit: bool,
    /// bit 49. Dual-monitor treatment of SMIs/SMM supported.
    pub dual_monitor: bool,
    /// bits 53:50. Memory type for VMCS accesses (6 = write-back, 0 = uncacheable).
    pub memtype: u8,
    /// bit 54. INS/OUTS instruction information is reported on I/O VM exits.
    pub ins_outs_info: bool,
    /// bit 55. The IA32_VMX_TRUE_*_CTLS MSRs exist (stage 4 uses them).
    pub true_controls: bool,
}

pub fn extract_bits(value: u64, hi: u32, lo: u32) -> u32 {
    // shifted and with the mask
    ((value >> lo) & ((1u64 << (hi - lo + 1)) - 1)) as u32
}

/// Decode IA32_VMX_BASIC. (kernel: vmx_basic_vmcs_revision_id(), vmx_basic_vmcs_size(),
/// vmx_basic_vmcs_mem_type() in arch/x86/include/asm/vmx.h)
pub fn vmx_basic_decode(raw: u64) -> vmx_basic {
    vmx_basic {
        revision_id: extract_bits(raw, 30, 0),
        vmcs_size: extract_bits(raw, 44, 32),
        phys_addr_32bit: extract_bits(raw, 48, 48) == 1,
        dual_monitor: extract_bits(raw, 49, 49) == 1,
        memtype: extract_bits(raw, 53, 50) as u8,
        ins_outs_info: extract_bits(raw, 54, 54) == 1,
        true_controls: extract_bits(raw, 55, 55) == 1,
    }
}

/// Decoded IA32_VMX_EPT_VPID_CAP (MSR 0x48c). SDM Appendix A.10.
/// Only present if the CPU supports "enable EPT" or "enable VPID".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ept_vpid_cap {
    /// bit 0: execute-only EPT translations (R=0, W=0, X=1).
    pub exec_only: bool,
    /// bit 6: 4-level EPT page walk (EPTP bits 5:3 = 3). Our stage 8 needs this.
    pub pwl4: bool,
    /// bit 8: EPT paging structures may be uncacheable.
    pub uc: bool,
    /// bit 14: EPT paging structures may be write-back. Stage 8 needs this too.
    pub wb: bool,
    /// bit 16: 2 MiB EPT pages.
    pub large_2m: bool,
    /// bit 17: 1 GiB EPT pages.
    pub large_1g: bool,
    /// bit 20: the INVEPT instruction.
    pub invept: bool,
    /// bit 21: accessed/dirty flags in EPT entries.
    pub ad: bool,
    /// bit 22: advanced VM-exit information for EPT violations.
    pub advanced_exit_info: bool,
    /// bit 25: INVEPT single-context.
    pub invept_single: bool,
    /// bit 26: INVEPT all-context.
    pub invept_all: bool,
    /// bit 32: the INVVPID instruction.
    pub invvpid: bool,
    /// bit 40: INVVPID individual-address.
    pub invvpid_addr: bool,
    /// bit 41: INVVPID single-context.
    pub invvpid_single: bool,
    /// bit 42: INVVPID all-context.
    pub invvpid_all: bool,
    /// bit 43: INVVPID single-context-retaining-globals.
    pub invvpid_single_nonglobal: bool,
}

/// Decode IA32_VMX_EPT_VPID_CAP.
/// (kernel: cpu_has_vmx_ept_4levels(), cpu_has_vmx_ept_mt_wb(), cpu_has_vmx_invept_*()
/// ... in arch/x86/kvm/vmx/capabilities.h)
pub fn vmx_ept_vpid_decode(raw: u64) -> ept_vpid_cap {
    let is_set = |bit: u32| -> bool { ((raw >> bit) & 1) != 0 };

    ept_vpid_cap {
        exec_only: is_set(0),
        pwl4: is_set(6),
        uc: is_set(8),
        wb: is_set(14),
        large_2m: is_set(16),
        large_1g: is_set(17),
        invept: is_set(20),
        ad: is_set(21),
        advanced_exit_info: is_set(22),
        invept_single: is_set(25),
        invept_all: is_set(26),
        invvpid: is_set(32),
        invvpid_addr: is_set(40),
        invvpid_single: is_set(41),
        invvpid_all: is_set(42),
        invvpid_single_nonglobal: is_set(43),
    }
}

/// Everything stage 1 learns about the CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct vmx_caps {
    pub basic: vmx_basic,
    /// `None` when the CPU has neither EPT nor VPID.
    pub ept_vpid: Option<ept_vpid_cap>,
    /// `Some(v)`: IA32_FEATURE_CONTROL was unlocked; write `v` to it before VMXON.
    pub feat_ctl_write: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum vmx_probe_error {
    /// CPUID.1:ECX.VMX is clear (under QEMU: forgot `-cpu host,vmx=on`?).
    NoVmx,
    /// Locked off by the firmware.
    DisabledByBios,
    /// An MSR the SDM says must exist is missing (`MsrOps::rdmsr` returned `None`).
    MsrMissing(u32),
}

/// Probe VMX support, in this order (tests check the order of the errors):
/// 1. `cpu_has_vmx(cpuid_1_ecx)` else `NoVmx`;
/// 2. read IA32_FEATURE_CONTROL, `DisabledByBios` / remember `CanEnable`'s value;
/// 3. read and decode IA32_VMX_BASIC;
/// 4. EPT/VPID: read IA32_VMX_PROCBASED_CTLS (0x482); its *high* dword says which
///    control bits may be 1. If "activate secondary controls" (bit 31) may be 1, read
///    IA32_VMX_PROCBASED_CTLS2 (0x48b); only if *its* high dword allows "enable EPT"
///    (bit 1) or "enable VPID" (bit 5), read IA32_VMX_EPT_VPID_CAP and decode it.
///    Otherwise `ept_vpid` is `None`. (SDM Appendix A.3, A.3.3, A.10)
/// A missing MSR is `MsrMissing(msr)`; that includes 0x48b/0x48c when the rules above
/// say they must exist.
/// (kernel: init_ia32_feat_ctl, then setup_vmcs_config/hardware_setup)
pub fn vmx_probe<M: MsrOps>(msrs: &M, cpuid_1_ecx: u32) -> Result<vmx_caps, vmx_probe_error> {
    if !cpu_has_vmx(cpuid_1_ecx) {
        Err(NoVmx)
    } else {
        let feate_ctl = match msrs.rdmsr(MSR_IA32_FEATURE_CONTROL) {
            Some(msr_ia32_fs) => vmx_feat_ctl_state(msr_ia32_fs),
            None => return Err(MsrMissing(MSR_IA32_FEATURE_CONTROL)),
        };

        if feate_ctl == DisabledByBios {
            return Err(DisabledByBiosErr);
        }

        let vmx_basic_value = match msrs.rdmsr(MSR_IA32_VMX_BASIC) {
            Some(raw) => vmx_basic_decode(raw),
            None => {
                return Err(MsrMissing(MSR_IA32_VMX_BASIC));
            }
        };

        let vmx_procbased_ctls = match msrs.rdmsr(MSR_IA32_VMX_PROCBASED_CTLS) {
            Some(raw) => raw,
            None => return Err(MsrMissing(MSR_IA32_VMX_PROCBASED_CTLS)),
        };

        let vmx_procbased_ctls = vmx_procbased_ctls >> 32; // get hight part

        let ept_vpid = if extract_bits(vmx_procbased_ctls, 31, 31) == 1 {
            let vmx_procbased_ctls2 = match msrs.rdmsr(MSR_IA32_VMX_PROCBASED_CTLS2) {
                Some(raw) => raw,
                None => return Err(MsrMissing(MSR_IA32_VMX_PROCBASED_CTLS2)),
            };
            let vmx_procbased_ctls2 = vmx_procbased_ctls2 >> 32;

            if extract_bits(vmx_procbased_ctls2, 1, 1) == 1
                || extract_bits(vmx_procbased_ctls2, 5, 5) == 1
            {
                match msrs.rdmsr(MSR_IA32_VMX_EPT_VPID_CAP) {
                    Some(raw) => Some(vmx_ept_vpid_decode(raw)),
                    None => return Err(MsrMissing(MSR_IA32_VMX_EPT_VPID_CAP)),
                }
            } else {
                None
            }
        } else {
            None
        };

        let feat_ctl_write = match feate_ctl {
            CanEnable(value) => Some(value),
            _ => None,
        };

        Ok(vmx_caps {
            basic: vmx_basic_value,
            ept_vpid: ept_vpid,
            feat_ctl_write: feat_ctl_write,
        })
    }
}
