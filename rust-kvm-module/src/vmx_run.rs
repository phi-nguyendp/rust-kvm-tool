//! Stage 6: VM entry and VM exit: the run loop around VMLAUNCH/VMRESUME.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (vmx_vcpu_run, vmx_vcpu_enter_exit,
//! vmx_handle_exit's failure paths, `union vmx_exit_reason`), arch/x86/kvm/vmx/vmenter.S
//! (__vmx_vcpu_run), arch/x86/include/uapi/asm/vmx.h (VMX_EXIT_REASONS).
//! Book: Intel SDM Vol. 3C chapter 26 (VM entries; 26.8 entry failures), chapter 27 (VM
//! exits; 27.2.1 basic exit information), 30.4 (VM-instruction error numbers), App. C.
//! The assembly itself is in `module/s06_run.rs`.
#![allow(unused_variables)]

use crate::vmcs_ops::*;
use crate::vmx::*;

// Guest general-purpose registers, in the kernel's order (arch/x86/kvm/kvm_cache_regs.h
// VCPU_REGS_*). This is *also* the layout of `vcpu_vmx::regs`, which the assembly indexes:
// reg N lives at byte offset 8*N from the start of the struct.
pub const VCPU_REGS_RAX: usize = 0;
pub const VCPU_REGS_RCX: usize = 1;
pub const VCPU_REGS_RDX: usize = 2;
pub const VCPU_REGS_RBX: usize = 3;
pub const VCPU_REGS_RSP: usize = 4;
pub const VCPU_REGS_RBP: usize = 5;
pub const VCPU_REGS_RSI: usize = 6;
pub const VCPU_REGS_RDI: usize = 7;
pub const VCPU_REGS_R8: usize = 8;
pub const VCPU_REGS_R9: usize = 9;
pub const VCPU_REGS_R10: usize = 10;
pub const VCPU_REGS_R11: usize = 11;
pub const VCPU_REGS_R12: usize = 12;
pub const VCPU_REGS_R13: usize = 13;
pub const VCPU_REGS_R14: usize = 14;
pub const VCPU_REGS_R15: usize = 15;
pub const NR_VCPU_REGS: usize = 16;

/// `vmx_vcpu_run_flags()` bit: use VMRESUME instead of VMLAUNCH (kernel: VMX_RUN_VMRESUME).
pub const VMX_RUN_VMRESUME: u32 = 1 << 0;

/// The `VM_EXIT_REASON` field (kernel: `union vmx_exit_reason`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct vmx_exit_reason(pub u32);

impl vmx_exit_reason {
    /// Bits 15:0: the basic exit reason (`EXIT_REASON_*`).
    pub fn basic(self) -> u32 {
        todo!()
    }
    /// Bit 31: the "exit" is really a failed VM entry (guest state or MSR-load check failed
    /// after the CPU started the entry). Kernel: `vmx_exit_reason.failed_vmentry`.
    pub fn failed_vmentry(self) -> bool {
        todo!()
    }
}

/// Per-vCPU software state (kernel: `struct vcpu_vmx`, trimmed). `repr(C)` with `regs`
/// first: the assembly in `module/s06_run.rs` loads/stores guest registers at
/// `vcpu_vmx + 8 * VCPU_REGS_*`.
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct vcpu_vmx {
    /// Guest GPRs while the guest is NOT running. `regs[VCPU_REGS_RSP]` is unused: the guest's
    /// RSP lives in the VMCS (`GUEST_RSP`), as in the kernel.
    pub regs: [u64; NR_VCPU_REGS],
    /// VMCS launch state: false until the first successful VMLAUNCH, true after
    /// (kernel: `loaded_vmcs->launched`). VMCLEAR makes it false again.
    pub launched: bool,
    /// The last VMLAUNCH/VMRESUME failed with VMfail (CF or ZF set) instead of entering the
    /// guest (kernel: `vmx->fail`, set by the assembly).
    pub fail: bool,
    /// `VM_EXIT_REASON` of the last real exit.
    pub exit_reason: vmx_exit_reason,
}

// (reason constant, name): the kernel's VMX_EXIT_REASONS, minus the EXIT_REASON_ prefix. PROVIDED.
pub const VMX_EXIT_REASONS: &[(u32, &str)] = &[
    (EXIT_REASON_EXCEPTION_NMI, "EXCEPTION_NMI"),
    (EXIT_REASON_EXTERNAL_INTERRUPT, "EXTERNAL_INTERRUPT"),
    (EXIT_REASON_TRIPLE_FAULT, "TRIPLE_FAULT"),
    (EXIT_REASON_INTERRUPT_WINDOW, "INTERRUPT_WINDOW"),
    (EXIT_REASON_NMI_WINDOW, "NMI_WINDOW"),
    (EXIT_REASON_TASK_SWITCH, "TASK_SWITCH"),
    (EXIT_REASON_CPUID, "CPUID"),
    (EXIT_REASON_HLT, "HLT"),
    (EXIT_REASON_INVD, "INVD"),
    (EXIT_REASON_INVLPG, "INVLPG"),
    (EXIT_REASON_RDPMC, "RDPMC"),
    (EXIT_REASON_RDTSC, "RDTSC"),
    (EXIT_REASON_VMCALL, "VMCALL"),
    (EXIT_REASON_VMCLEAR, "VMCLEAR"),
    (EXIT_REASON_VMLAUNCH, "VMLAUNCH"),
    (EXIT_REASON_VMPTRLD, "VMPTRLD"),
    (EXIT_REASON_VMREAD, "VMREAD"),
    (EXIT_REASON_VMRESUME, "VMRESUME"),
    (EXIT_REASON_VMWRITE, "VMWRITE"),
    (EXIT_REASON_VMOFF, "VMOFF"),
    (EXIT_REASON_VMON, "VMON"),
    (EXIT_REASON_CR_ACCESS, "CR_ACCESS"),
    (EXIT_REASON_DR_ACCESS, "DR_ACCESS"),
    (EXIT_REASON_IO_INSTRUCTION, "IO_INSTRUCTION"),
    (EXIT_REASON_MSR_READ, "MSR_READ"),
    (EXIT_REASON_MSR_WRITE, "MSR_WRITE"),
    (EXIT_REASON_INVALID_STATE, "INVALID_STATE"),
    (EXIT_REASON_MSR_LOAD_FAIL, "MSR_LOAD_FAIL"),
    (EXIT_REASON_MWAIT_INSTRUCTION, "MWAIT_INSTRUCTION"),
    (EXIT_REASON_MONITOR_TRAP_FLAG, "MONITOR_TRAP_FLAG"),
    (EXIT_REASON_PAUSE_INSTRUCTION, "PAUSE_INSTRUCTION"),
    (EXIT_REASON_MCE_DURING_VMENTRY, "MCE_DURING_VMENTRY"),
    (EXIT_REASON_TPR_BELOW_THRESHOLD, "TPR_BELOW_THRESHOLD"),
    (EXIT_REASON_APIC_ACCESS, "APIC_ACCESS"),
    (EXIT_REASON_EPT_VIOLATION, "EPT_VIOLATION"),
    (EXIT_REASON_EPT_MISCONFIG, "EPT_MISCONFIG"),
    (EXIT_REASON_INVEPT, "INVEPT"),
    (EXIT_REASON_RDTSCP, "RDTSCP"),
    (EXIT_REASON_PREEMPTION_TIMER, "PREEMPTION_TIMER"),
    (EXIT_REASON_INVVPID, "INVVPID"),
    (EXIT_REASON_WBINVD, "WBINVD"),
    (EXIT_REASON_XSETBV, "XSETBV"),
];

/// Name of a *basic* exit reason, or `"UNKNOWN"` (kernel: the `__print_symbolic` of
/// VMX_EXIT_REASONS in trace events). Look it up in `VMX_EXIT_REASONS`.
pub fn vmx_exit_reason_str(basic: u32) -> &'static str {
    todo!()
}

/// (VM-instruction error number, SDM Table 30-1 text). PROVIDED; the numbers are the kernel's
/// `VMXERR_*` from asm/vmx.h.
pub const VMX_INSTRUCTION_ERRORS: &[(u32, &str)] = &[
    (VMXERR_VMCALL_IN_VMX_ROOT_OPERATION, "VMCALL executed in VMX root operation"),
    (VMXERR_VMCLEAR_INVALID_ADDRESS, "VMCLEAR with invalid physical address"),
    (VMXERR_VMCLEAR_VMXON_POINTER, "VMCLEAR with VMXON pointer"),
    (VMXERR_VMLAUNCH_NONCLEAR_VMCS, "VMLAUNCH with non-clear VMCS"),
    (VMXERR_VMRESUME_NONLAUNCHED_VMCS, "VMRESUME with non-launched VMCS"),
    (VMXERR_VMRESUME_AFTER_VMXOFF, "VMRESUME after VMXOFF"),
    (VMXERR_ENTRY_INVALID_CONTROL_FIELD, "VM entry with invalid control field(s)"),
    (VMXERR_ENTRY_INVALID_HOST_STATE_FIELD, "VM entry with invalid host-state field(s)"),
    (VMXERR_VMPTRLD_INVALID_ADDRESS, "VMPTRLD with invalid physical address"),
    (VMXERR_VMPTRLD_VMXON_POINTER, "VMPTRLD with VMXON pointer"),
    (VMXERR_VMPTRLD_INCORRECT_VMCS_REVISION_ID, "VMPTRLD with incorrect VMCS revision identifier"),
    (VMXERR_UNSUPPORTED_VMCS_COMPONENT, "VMREAD/VMWRITE from/to unsupported VMCS component"),
    (VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT, "VMWRITE to read-only VMCS component"),
    (VMXERR_VMXON_IN_VMX_ROOT_OPERATION, "VMXON executed in VMX root operation"),
    (VMXERR_ENTRY_EVENTS_BLOCKED_BY_MOV_SS, "VM entry with events blocked by MOV SS"),
    (VMXERR_INVALID_OPERAND_TO_INVEPT_INVVPID, "Invalid operand to INVEPT/INVVPID"),
];

/// Text of a VM-instruction error number, `"unknown VM-instruction error"` if not in the table.
pub fn vmx_instruction_error_str(err: u32) -> &'static str {
    todo!()
}

/// Flags for the next entry. VMLAUNCH only works on a *clear* VMCS and VMRESUME only on a
/// *launched* one (SDM 30.3, errors 4 and 5), so: `VMX_RUN_VMRESUME` iff `vcpu.launched`
/// (kernel: `__vmx_vcpu_run_flags()`).
pub fn vmx_vcpu_run_flags(vcpu: &vcpu_vmx) -> u32 {
    todo!()
}

/// Bookkeeping right after VMLAUNCH/VMRESUME returned to the host (kernel: the tail of
/// `vmx_vcpu_run()` / `vmx_vcpu_enter_exit()`):
/// - if the instruction VMfailed (`vcpu.fail`): nothing was entered, so the exit reason is
///   meaningless (reset it to default) and `launched` must NOT change;
/// - otherwise `launched = true` and `exit_reason` = `VM_EXIT_REASON` of the current VMCS.
pub fn vmx_post_run(vcpu: &mut vcpu_vmx, vmcs: &impl VmcsOps) {
    todo!()
}

/// Why VM entry did not enter the guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum vmx_entry_failure {
    /// VMLAUNCH/VMRESUME itself failed (VMfailValid): the number is `VM_INSTRUCTION_ERROR`.
    /// Controls or host state are wrong (error 7, 8), launch state mixed up (4, 5), ...
    InstructionError(u32),
    /// The CPU began the entry, then found the *guest* state / MSR-load list invalid: a VM exit
    /// with bit 31 of `VM_EXIT_REASON` set. `basic` is the basic reason (33 =
    /// `EXIT_REASON_INVALID_STATE`, 34 = `EXIT_REASON_MSR_LOAD_FAIL`, 41 = machine check).
    ExitFailed { basic: u32, qualification: u64 },
}

/// Classify the outcome of an entry attempt (kernel: `vmx_handle_exit()`'s
/// `exit_reason.failed_vmentry` / `vmx->fail` branches). `None` = a normal VM exit.
/// `vcpu.fail` -> `InstructionError(VM_INSTRUCTION_ERROR)`; else if
/// `vcpu.exit_reason.failed_vmentry()` -> `ExitFailed` with `EXIT_QUALIFICATION`.
pub fn vm_entry_failure(vcpu: &vcpu_vmx, vmcs: &impl VmcsOps) -> Option<vmx_entry_failure> {
    todo!()
}

/// What the run loop does after one entry attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum vmx_run_action {
    /// External interrupt arrived while the guest ran: let the host take it (interrupts were
    /// off since before the entry), then enter again. Kernel: `handle_external_interrupt_irqoff()`
    /// + `EXIT_FASTPATH`/resume.
    HostInterrupt,
    /// A real exit the caller must handle (stage 7); the number is the basic reason.
    Exit(u32),
    /// The guest never ran: report and stop.
    EntryFailed(vmx_entry_failure),
}

/// Decide what to do after `vmx_post_run` (kernel: the top of `__vmx_handle_exit()`):
/// entry failure first (`vm_entry_failure`), then `EXIT_REASON_EXTERNAL_INTERRUPT` ->
/// `HostInterrupt`, everything else `Exit(basic)`.
pub fn vmx_vcpu_run_decide(vcpu: &vcpu_vmx, vmcs: &impl VmcsOps) -> vmx_run_action {
    todo!()
}

/// The loop around the hardware (kernel: `vcpu_enter_guest()` calling `vmx_vcpu_run()`, then
/// `vmx_handle_exit()`). Repeat: take `vmx_vcpu_run_flags`; call `enter(vcpu, vmcs, flags)`
/// (hardware: runs the guest until the next exit and must set `vcpu.fail` accordingly);
/// `vmx_post_run`; `vmx_vcpu_run_decide`; on `HostInterrupt` call `host_irq()` and go round
/// again; any other action ends the loop and is returned.
pub fn vmx_run_vcpu<V: VmcsOps>(
    vcpu: &mut vcpu_vmx,
    vmcs: &mut V,
    enter: &mut dyn FnMut(&mut vcpu_vmx, &mut V, u32),
    host_irq: &mut dyn FnMut(),
) -> vmx_run_action {
    todo!()
}
