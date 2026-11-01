//! Stage 2: entering VMX operation (VMXON / VMXOFF).
//!
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (`vmx_hardware_enable`/
//! `kvm_cpu_vmxon` in arch/x86/include/asm/virtext.h or vmx/vmx.c depending on the
//! version, `vmxon()` in vmx_ops.h, `alloc_vmxon_region`/`alloc_vmcs_cpu`),
//! arch/x86/kvm/vmx/vmx_ops.h (`vmx_asm1`: the fail-decoding asm).
//! Book: Intel SDM Vol. 3C "Enabling and Entering VMX Operation" (ch. 23: CR0/CR4
//! fixed bits, the VMXON region, IA32_FEATURE_CONTROL), Appendix A.7/A.8
//! (IA32_VMX_CR0/CR4_FIXED0/1), Vol. 3C ch. 30 "Conventions" (VMsucceed/VMfail) and
//! the VMXON/VMXOFF instruction pages (Vol. 2).
//!
//! The hardware has a state machine here; `vmx_cpu_state` models it so the exact
//! SDM rules for #UD / #GP / VMfailInvalid / VMfailValid can be tested without a CPU.
#![allow(non_camel_case_types)]

use crate::vmcs_ops::MsrOps;
use crate::vmx::*;
use crate::vmx_caps::vmx_basic;

/// "No current VMCS". Right after VMXON the current-VMCS pointer is this (SDM 24.1).
pub const VMCS_PTR_INVALID: u64 = !0;

/// The four IA32_VMX_CR{0,4}_FIXED{0,1} MSRs. A CR bit is *fixed to 1* if the bit is 1
/// in FIXED0, and *fixed to 0* if the bit is 0 in FIXED1. Everything else is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct vmx_cr_fixed {
    pub cr0_fixed0: u64,
    pub cr0_fixed1: u64,
    pub cr4_fixed0: u64,
    pub cr4_fixed1: u64,
}

/// Read the four MSRs (0x486..=0x489). `None` if any is missing.
pub fn vmx_read_cr_fixed<M: MsrOps>(msrs: &M) -> Option<vmx_cr_fixed> {
    let _ = (msrs,);
    todo!()
}

/// Force `cr` to satisfy the fixed bits: set every bit that is 1 in `fixed0`, clear
/// every bit that is 0 in `fixed1`. (kernel: the `cr4 | fixed0` logic in
/// `vmx_hardware_enable`; also `fixed_bits_valid` in nested.c for the checking side)
pub fn cr_fixed_adjust(cr: u64, fixed0: u64, fixed1: u64) -> u64 {
    let _ = (cr, fixed0, fixed1,);
    todo!()
}

/// Does `cr` already satisfy the fixed bits? (The hardware raises #GP for VMXON
/// otherwise; the SDM allows any value for which this is true.)
pub fn cr_fixed_ok(cr: u64, fixed0: u64, fixed1: u64) -> bool {
    let _ = (cr, fixed0, fixed1,);
    todo!()
}

/// Initialise a VMXON region in place: zero it, then store the VMCS revision identifier
/// (from IA32_VMX_BASIC) in bits 30:0 of the first dword, little endian, with bit 31
/// clear. `region` is the whole allocation (normally 4096 bytes, 4 KiB aligned by the
/// allocator). Fails if the region is smaller than `basic.vmcs_size`.
/// (kernel: alloc_vmxon_region / alloc_vmcs_cpu: `vmcs->hdr.revision_id = ...`)
pub fn vmxon_region_init(region: &mut [u8], basic: &vmx_basic) -> Result<(), &'static str> {
    let _ = (region, basic,);
    todo!()
}

/// The three outcomes of a VMX instruction that did not fault (SDM ch. 30, "Conventions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmxResult {
    /// VMsucceed: CF = ZF = 0.
    Succeed,
    /// VMfailInvalid: CF = 1 (there is no current VMCS to put an error number in).
    FailInvalid,
    /// VMfailValid: ZF = 1 (error number stored in the VM-instruction error field).
    FailValid,
}

/// Decode RFLAGS after a VMX instruction. If both CF and ZF are set, it is
/// VMfailInvalid (the hardware never produces that, but be deterministic).
/// (kernel: the `vmx_asm1`/`vmx_asm2` macros in vmx_ops.h use `jc`/`jz`)
pub fn vmx_fail_kind(rflags: u64) -> VmxResult {
    let _ = (rflags,);
    todo!()
}

/// A fault raised instead of a VMX instruction result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum vmx_exception {
    UD,
    GP,
}

/// What the CPU looks like when VMXON/VMXOFF executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct vmxon_env {
    pub cr0: u64,
    pub cr4: u64,
    pub cpl: u8,
    /// Current IA32_FEATURE_CONTROL value.
    pub feature_control: u64,
    pub fixed: vmx_cr_fixed,
    pub basic: vmx_basic,
    /// Physical-address width of the CPU (CPUID.80000008H:EAX[7:0]), e.g. 39 or 46.
    pub phys_addr_bits: u8,
}

/// Per-logical-processor VMX state, as the SDM describes it. Each logical processor
/// has its own, which is why `kvm` calls `vmxon` once per CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct vmx_cpu_state {
    /// In VMX operation (root mode) after a successful VMXON.
    pub in_vmx: bool,
    pub vmxon_ptr: u64,
    /// Current-VMCS pointer (stage 3 sets it with VMPTRLD; tests set it directly).
    pub current_vmcs: u64,
    /// The VM-instruction error field of the current VMCS, last VMfailValid.
    pub vm_instruction_error: u32,
}

impl vmx_cpu_state {
    pub fn new() -> Self {
        Self {
            in_vmx: false,
            vmxon_ptr: VMCS_PTR_INVALID,
            current_vmcs: VMCS_PTR_INVALID,
            vm_instruction_error: 0,
        }
    }

    /// The SDM's `VMfail(ErrorNumber)`: if the current-VMCS pointer is valid, it is
    /// VMfailValid and `err` is stored in `vm_instruction_error`; otherwise
    /// VMfailInvalid and nothing is stored.
    pub fn vmfail(&mut self, err: u32) -> VmxResult {
    let _ = (err,);
    todo!()
    }

    /// Execute VMXON with the operand `ptr` (physical address of the VMXON region),
    /// whose first dword currently holds `region_revision`. Follow the SDM pseudo-code
    /// of VMXON, in this order:
    /// 1. CR4.VMXE = 0                              -> `Err(UD)`
    /// 2. already in VMX operation                  -> `VMfail(VMXERR_VMXON_IN_VMX_ROOT_OPERATION)`
    ///    (note: `vmfail` decides Valid vs Invalid)
    /// 3. CPL > 0, CR0 or CR4 violates the fixed bits, or IA32_FEATURE_CONTROL is not
    ///    locked or lacks "VMXON outside SMX"      -> `Err(GP)`
    /// 4. `ptr` not 4 KiB aligned, has bits at or above `phys_addr_bits`, or (when
    ///    `basic.phys_addr_32bit`) above bit 31     -> `Ok(FailInvalid)`
    /// 5. `region_revision != basic.revision_id`    -> `Ok(FailInvalid)`
    ///    (a region whose bit 31 is set cannot match either)
    /// 6. otherwise enter VMX operation: `in_vmx`, `vmxon_ptr = ptr`, current VMCS
    ///    pointer invalid; `Ok(Succeed)`.
    pub fn vmxon(
        &mut self,
        env: &vmxon_env,
        ptr: u64,
        region_revision: u32,
    ) -> Result<VmxResult, vmx_exception> {
    let _ = (env, ptr, region_revision,);
    todo!()
    }

    /// Execute VMXOFF. `Err(UD)` if not in VMX operation or CR4.VMXE = 0;
    /// `Err(GP)` if CPL > 0; otherwise leave VMX operation: `in_vmx = false`, the
    /// current-VMCS pointer becomes invalid again, `Ok(Succeed)`.
    pub fn vmxoff(&mut self, env: &vmxon_env) -> Result<VmxResult, vmx_exception> {
    let _ = (env,);
    todo!()
    }
}

impl Default for vmx_cpu_state {
    fn default() -> Self {
        Self::new()
    }
}
