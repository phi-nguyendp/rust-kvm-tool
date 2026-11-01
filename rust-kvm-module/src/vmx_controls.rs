//! Stage 4: VM-execution, VM-exit and VM-entry controls.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (adjust_vmx_controls, setup_vmcs_config,
//! init_vmcs), arch/x86/kvm/vmx/vmx.h (struct vmcs_config), arch/x86/include/asm/vmx.h.
//! Book: Intel SDM Vol. 3C appendix A.2-A.5 (capability MSRs), chapter 24.6-24.8
//! (control fields), 26.2.1 (checks on VMX controls).
#![allow(unused_variables)]

use crate::vmcs_ops::*;
use crate::vmx::*;

/// What this machine supports, chosen once at module load (kernel: `struct vmcs_config`,
/// the global `vmcs_config` filled by `setup_vmcs_config()`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct vmcs_config {
    /// VMCS region size in bytes (IA32_VMX_BASIC bits 44:32).
    pub size: u32,
    /// VMCS revision identifier (IA32_VMX_BASIC bits 30:0).
    pub revision_id: u32,
    pub pin_based_exec_ctrl: u32,
    pub cpu_based_exec_ctrl: u32,
    /// Only meaningful if `cpu_based_exec_ctrl` has `CPU_BASED_ACTIVATE_SECONDARY_CONTROLS`.
    pub cpu_based_2nd_exec_ctrl: u32,
    pub vmexit_ctrl: u32,
    pub vmentry_ctrl: u32,
    /// IA32_VMX_EPT_VPID_CAP (0 when neither EPT nor VPID is enabled), used by stages 8/10.
    pub ept_vpid_cap: u64,
    /// IA32_VMX_CR{0,4}_FIXED{0,1}: bits that must be 1 / may be 1 in CR0/CR4 while in VMX
    /// operation, and (stage 5) in the guest/host CR fields.
    pub cr0_fixed0: u64,
    pub cr0_fixed1: u64,
    pub cr4_fixed0: u64,
    pub cr4_fixed1: u64,
}

// What we insist on (`min`) and would like to have (`opt`), per control word. Same idea as
// the `min`/`opt` locals in the kernel's setup_vmcs_config(), but much smaller.
pub const MIN_PIN_BASED: u32 = PIN_BASED_EXT_INTR_MASK | PIN_BASED_NMI_EXITING;
pub const OPT_PIN_BASED: u32 = PIN_BASED_VMX_PREEMPTION_TIMER;
/// No `CPU_BASED_USE_IO_BITMAPS` / `CPU_BASED_USE_MSR_BITMAPS` yet (stage 10): every port
/// access exits unconditionally and so does every RDMSR/WRMSR.
pub const MIN_CPU_BASED: u32 = CPU_BASED_HLT_EXITING | CPU_BASED_UNCOND_IO_EXITING;
pub const OPT_CPU_BASED: u32 = CPU_BASED_ACTIVATE_SECONDARY_CONTROLS;
pub const MIN_SECONDARY: u32 = 0;
pub const OPT_SECONDARY: u32 =
    SECONDARY_EXEC_ENABLE_EPT | SECONDARY_EXEC_UNRESTRICTED_GUEST | SECONDARY_EXEC_ENABLE_VPID;
/// A 64-bit host, and we let the CPU swap EFER for us. (`VM_EXIT_ACK_INTR_ON_EXIT` stays
/// off: the host takes the interrupt itself after the exit, stage 6/9.)
pub const MIN_EXIT: u32 =
    VM_EXIT_HOST_ADDR_SPACE_SIZE | VM_EXIT_LOAD_IA32_EFER | VM_EXIT_SAVE_IA32_EFER;
pub const OPT_EXIT: u32 = VM_EXIT_SAVE_VMX_PREEMPTION_TIMER;
/// `VM_ENTRY_IA32E_MODE` is per guest (stage 5), never part of the machine-wide config.
pub const MIN_ENTRY: u32 = VM_ENTRY_LOAD_IA32_EFER;
pub const OPT_ENTRY: u32 = 0;

/// Combine the controls we need and want with what the CPU allows in one capability MSR
/// (kernel: `adjust_vmx_controls()`).
///
/// A capability MSR (SDM A.3-A.5) holds, in its low 32 bits, the *allowed 0-settings*
/// (a bit that is 1 here **must** be 1) and, in its high 32 bits, the *allowed
/// 1-settings* (a bit that is 0 here **must** be 0).
///
/// Algorithm: `ctl = min | opt`; clear every bit the CPU does not allow to be 1; set every bit
/// the CPU forces to 1. If a bit of `min` did not survive, return `Err(())`.
pub fn adjust_vmx_controls(ctl_min: u32, ctl_opt: u32, msr_val: u64) -> Result<u32, ()> {
    todo!()
}

/// Which capability MSR describes this control word, given IA32_VMX_BASIC bit 55
/// ("true" controls supported). Kernel: `vmx_msr_*` selection at the top of
/// `setup_vmcs_config()` (`vmx_basic_vmcs_control...` / `MSR_IA32_VMX_TRUE_*`).
/// With bit 55 set use the TRUE_* variants, otherwise the plain ones: the plain MSRs report
/// "default1" bits as forced to 1 even when the CPU would let you clear them.
pub fn vmx_ctl_msr(true_msrs: bool, plain: u32, true_: u32) -> u32 {
    todo!()
}

/// Probe the CPU and pick the controls (kernel: `setup_vmcs_config()`).
///
/// 1. `IA32_VMX_BASIC`: `revision_id` = bits 30:0, `size` = bits 44:32. Reject (return
///    `Err`) if size > 4096, if bit 48 is set (VMCS addresses limited to 32 bits: useless on
///    a 64-bit kernel) or if the VMCS memory type (bits 53:50) is not 6 (write-back).
///    Bit 55 = true controls.
/// 2. pin-based, then primary processor-based controls with `adjust_vmx_controls`
///    (`Err` -> return `Err`).
/// 3. Only if the primary controls ended up with `CPU_BASED_ACTIVATE_SECONDARY_CONTROLS`:
///    secondary controls from `MSR_IA32_VMX_PROCBASED_CTLS2` (no TRUE variant). Otherwise
///    the secondary word is 0. **Unrestricted guest requires EPT** (SDM 26.2.1.1): if EPT
///    did not survive, drop `SECONDARY_EXEC_UNRESTRICTED_GUEST`.
/// 4. If EPT or VPID survived, read `MSR_IA32_VMX_EPT_VPID_CAP` into `ept_vpid_cap`.
/// 5. exit and entry controls.
/// 6. read the four `IA32_VMX_CRx_FIXEDy` MSRs.
///
/// A missing MSR (`MsrOps::rdmsr` -> `None`) is an `Err`.
pub fn setup_vmcs_config(msrs: &impl MsrOps) -> Result<vmcs_config, &'static str> {
    todo!()
}

/// Write the control fields of the current VMCS (kernel: the control half of `init_vmcs()`).
///
/// Always: `PIN_BASED_VM_EXEC_CONTROL`, `CPU_BASED_VM_EXEC_CONTROL`, `VM_EXIT_CONTROLS`,
/// `VM_ENTRY_CONTROLS` from `cfg`; `SECONDARY_VM_EXEC_CONTROL` **only** when the primary
/// controls activate secondary controls (writing it otherwise is a VMwrite to an
/// unsupported field: VM-instruction error 12).
/// And the "nothing special" defaults, so stale values never matter: `EXCEPTION_BITMAP` 0,
/// `PAGE_FAULT_ERROR_CODE_MASK`/`_MATCH` 0, `CR3_TARGET_COUNT` 0,
/// `VM_EXIT_MSR_STORE_COUNT`/`VM_EXIT_MSR_LOAD_COUNT`/`VM_ENTRY_MSR_LOAD_COUNT` 0 and
/// `VM_ENTRY_INTR_INFO_FIELD` 0 (bit 31 clear: no event injection).
pub fn vmx_vmcs_controls_write(vmcs: &mut impl VmcsOps, cfg: &vmcs_config) {
    todo!()
}
