use ktvmx::mock::*;
use ktvmx::vmcs_ops::*;
use ktvmx::vmx::*;
use ktvmx::vmx_run::*;

// ---- layout the assembly relies on ---------------------------------------------------------

#[test]
fn register_order_is_the_kernels() {
    assert_eq!(
        [VCPU_REGS_RAX, VCPU_REGS_RCX, VCPU_REGS_RDX, VCPU_REGS_RBX, VCPU_REGS_RSP, VCPU_REGS_RBP,
         VCPU_REGS_RSI, VCPU_REGS_RDI, VCPU_REGS_R8, VCPU_REGS_R9, VCPU_REGS_R10, VCPU_REGS_R11,
         VCPU_REGS_R12, VCPU_REGS_R13, VCPU_REGS_R14, VCPU_REGS_R15],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );
    assert_eq!(NR_VCPU_REGS, 16);
}

#[test]
fn regs_are_at_offset_zero() {
    // module/s06_run.rs indexes `regs` as [vcpu + 8*N]
    assert_eq!(core::mem::offset_of!(vcpu_vmx, regs), 0);
    assert_eq!(core::mem::size_of::<[u64; NR_VCPU_REGS]>(), 128);
}

// ---- exit reason ------------------------------------------------------------------------------

#[test]
fn exit_reason_fields() {
    let r = vmx_exit_reason(EXIT_REASON_HLT);
    assert_eq!(r.basic(), 12);
    assert!(!r.failed_vmentry());
    let r = vmx_exit_reason(0x8000_0021); // failed entry, invalid guest state
    assert_eq!(r.basic(), EXIT_REASON_INVALID_STATE);
    assert!(r.failed_vmentry());
    // bits 30:16 carry other flags (e.g. "exit from enclave", "pending MTF") and are not the reason
    let r = vmx_exit_reason(0x0001_0000 | EXIT_REASON_CPUID);
    assert_eq!(r.basic(), EXIT_REASON_CPUID);
    assert!(!r.failed_vmentry());
}

#[test]
fn exit_reason_names() {
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_HLT), "HLT");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_CPUID), "CPUID");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_IO_INSTRUCTION), "IO_INSTRUCTION");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_EPT_VIOLATION), "EPT_VIOLATION");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_EXTERNAL_INTERRUPT), "EXTERNAL_INTERRUPT");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_EXCEPTION_NMI), "EXCEPTION_NMI");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_INVALID_STATE), "INVALID_STATE");
    assert_eq!(vmx_exit_reason_str(EXIT_REASON_PREEMPTION_TIMER), "PREEMPTION_TIMER");
    assert_eq!(vmx_exit_reason_str(9999), "UNKNOWN");
    // every entry of the table resolves to itself (no duplicates shadowing each other)
    for (r, n) in VMX_EXIT_REASONS {
        assert_eq!(vmx_exit_reason_str(*r), *n);
    }
}

#[test]
fn instruction_error_text() {
    assert_eq!(vmx_instruction_error_str(VMXERR_VMLAUNCH_NONCLEAR_VMCS), "VMLAUNCH with non-clear VMCS");
    assert_eq!(vmx_instruction_error_str(VMXERR_VMRESUME_NONLAUNCHED_VMCS), "VMRESUME with non-launched VMCS");
    assert_eq!(vmx_instruction_error_str(VMXERR_ENTRY_INVALID_CONTROL_FIELD), "VM entry with invalid control field(s)");
    assert_eq!(vmx_instruction_error_str(VMXERR_ENTRY_INVALID_HOST_STATE_FIELD), "VM entry with invalid host-state field(s)");
    assert_eq!(vmx_instruction_error_str(VMXERR_UNSUPPORTED_VMCS_COMPONENT), "VMREAD/VMWRITE from/to unsupported VMCS component");
    assert_eq!(vmx_instruction_error_str(0xdead), "unknown VM-instruction error");
    for (e, s) in VMX_INSTRUCTION_ERRORS {
        assert_eq!(vmx_instruction_error_str(*e), *s);
    }
}

// ---- launch / resume ----------------------------------------------------------------------------

#[test]
fn launch_then_resume() {
    let mut vcpu = vcpu_vmx::default();
    assert_eq!(vmx_vcpu_run_flags(&vcpu), 0, "fresh VMCS: VMLAUNCH");
    vcpu.launched = true;
    assert_eq!(vmx_vcpu_run_flags(&vcpu), VMX_RUN_VMRESUME);
}

#[test]
fn post_run_marks_launched_and_reads_the_reason() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(VM_EXIT_REASON, EXIT_REASON_HLT);
    let mut vcpu = vcpu_vmx::default();
    vmx_post_run(&mut vcpu, &v);
    assert!(vcpu.launched);
    assert_eq!(vcpu.exit_reason.basic(), EXIT_REASON_HLT);
}

#[test]
fn post_run_after_vmfail_changes_nothing() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(VM_EXIT_REASON, EXIT_REASON_HLT); // stale from an earlier exit
    let mut vcpu = vcpu_vmx::default();
    vcpu.fail = true;
    vcpu.exit_reason = vmx_exit_reason(EXIT_REASON_CPUID);
    vmx_post_run(&mut vcpu, &v);
    assert!(!vcpu.launched, "a VMfail does not launch the VMCS: the retry must still be VMLAUNCH");
    assert_eq!(vcpu.exit_reason, vmx_exit_reason::default(), "stale reason must not leak");
    // an already-launched VMCS stays launched
    let mut vcpu = vcpu_vmx { launched: true, fail: true, ..Default::default() };
    vmx_post_run(&mut vcpu, &v);
    assert!(vcpu.launched);
}

// ---- classification ------------------------------------------------------------------------------

fn after_exit(reason: u32, v: &MockVmcs) -> vcpu_vmx {
    let mut vcpu = vcpu_vmx::default();
    let mut v2 = v.clone();
    v2.vmcs_write32(VM_EXIT_REASON, reason);
    vmx_post_run(&mut vcpu, &v2);
    vcpu
}

#[test]
fn vmfail_reports_the_instruction_error() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(VM_INSTRUCTION_ERROR, VMXERR_ENTRY_INVALID_CONTROL_FIELD);
    let vcpu = vcpu_vmx { fail: true, ..Default::default() };
    assert_eq!(vm_entry_failure(&vcpu, &v),
               Some(vmx_entry_failure::InstructionError(VMXERR_ENTRY_INVALID_CONTROL_FIELD)));
    assert_eq!(vmx_vcpu_run_decide(&vcpu, &v),
               vmx_run_action::EntryFailed(vmx_entry_failure::InstructionError(7)));
}

#[test]
fn failed_vmentry_exit_reports_reason_and_qualification() {
    let mut v = MockVmcs::default();
    v.vmcs_writel(EXIT_QUALIFICATION, 0);
    let vcpu = after_exit(VMX_EXIT_REASONS_FAILED_VMENTRY | EXIT_REASON_INVALID_STATE, &v);
    assert_eq!(vm_entry_failure(&vcpu, &v),
               Some(vmx_entry_failure::ExitFailed { basic: EXIT_REASON_INVALID_STATE, qualification: 0 }));
    v.vmcs_writel(EXIT_QUALIFICATION, 3);
    let vcpu = after_exit(VMX_EXIT_REASONS_FAILED_VMENTRY | EXIT_REASON_MSR_LOAD_FAIL, &v);
    assert_eq!(vmx_vcpu_run_decide(&vcpu, &v),
               vmx_run_action::EntryFailed(vmx_entry_failure::ExitFailed { basic: 34, qualification: 3 }));
}

#[test]
fn normal_exit_is_not_a_failure() {
    let v = MockVmcs::default();
    let vcpu = after_exit(EXIT_REASON_HLT, &v);
    assert_eq!(vm_entry_failure(&vcpu, &v), None);
    assert_eq!(vmx_vcpu_run_decide(&vcpu, &v), vmx_run_action::Exit(EXIT_REASON_HLT));
}

#[test]
fn external_interrupt_is_for_the_host() {
    let v = MockVmcs::default();
    let vcpu = after_exit(EXIT_REASON_EXTERNAL_INTERRUPT, &v);
    assert_eq!(vmx_vcpu_run_decide(&vcpu, &v), vmx_run_action::HostInterrupt);
}

// ---- the loop ----------------------------------------------------------------------------------------

/// A fake CPU: each call to `enter` consumes the next scripted outcome.
enum Outcome { Exit(u32), VmFail(u32) }

fn run(script: Vec<Outcome>) -> (vmx_run_action, vcpu_vmx, Vec<u32>, u32) {
    let mut vcpu = vcpu_vmx::default();
    let mut v = MockVmcs::default();
    let mut script = script.into_iter();
    let mut flags_seen = vec![];
    let mut irqs = 0;
    let action = {
        let mut enter = |c: &mut vcpu_vmx, v: &mut MockVmcs, flags: u32| {
            flags_seen.push(flags);
            match script.next().expect("loop entered the guest more often than scripted") {
                Outcome::Exit(r) => {
                    c.fail = false;
                    v.vmcs_write32(VM_EXIT_REASON, r);
                }
                Outcome::VmFail(e) => {
                    c.fail = true;
                    v.vmcs_write32(VM_INSTRUCTION_ERROR, e);
                }
            }
        };
        let mut host_irq = || irqs += 1;
        vmx_run_vcpu(&mut vcpu, &mut v, &mut enter, &mut host_irq)
    };
    (action, vcpu, flags_seen, irqs)
}

#[test]
fn hlt_first_try() {
    let (a, vcpu, flags, irqs) = run(vec![Outcome::Exit(EXIT_REASON_HLT)]);
    assert_eq!(a, vmx_run_action::Exit(EXIT_REASON_HLT));
    assert_eq!(flags, vec![0]);
    assert_eq!(irqs, 0);
    assert!(vcpu.launched);
}

#[test]
fn host_interrupts_are_absorbed_and_the_guest_resumed() {
    let (a, _, flags, irqs) = run(vec![
        Outcome::Exit(EXIT_REASON_EXTERNAL_INTERRUPT),
        Outcome::Exit(EXIT_REASON_EXTERNAL_INTERRUPT),
        Outcome::Exit(EXIT_REASON_CPUID),
    ]);
    assert_eq!(a, vmx_run_action::Exit(EXIT_REASON_CPUID));
    assert_eq!(irqs, 2);
    assert_eq!(flags, vec![0, VMX_RUN_VMRESUME, VMX_RUN_VMRESUME]);
}

#[test]
fn vmfail_on_launch_is_reported_and_leaves_the_vmcs_unlaunched() {
    let (a, vcpu, flags, irqs) = run(vec![Outcome::VmFail(VMXERR_ENTRY_INVALID_HOST_STATE_FIELD)]);
    assert_eq!(a, vmx_run_action::EntryFailed(vmx_entry_failure::InstructionError(8)));
    assert_eq!(flags, vec![0]);
    assert!(!vcpu.launched);
    assert_eq!(irqs, 0);
}

#[test]
fn guest_state_failure_ends_the_loop() {
    let (a, _, flags, _) = run(vec![
        Outcome::Exit(EXIT_REASON_EXTERNAL_INTERRUPT),
        Outcome::Exit(VMX_EXIT_REASONS_FAILED_VMENTRY | EXIT_REASON_INVALID_STATE),
    ]);
    assert_eq!(a, vmx_run_action::EntryFailed(vmx_entry_failure::ExitFailed { basic: 33, qualification: 0 }));
    assert_eq!(flags.len(), 2);
}

#[test]
fn fail_flag_is_cleared_between_entries() {
    // first entry VMfails, the caller fixes things and runs again: a stale `fail` must not stick
    let mut vcpu = vcpu_vmx::default();
    let mut v = MockVmcs::default();
    vcpu.fail = true; // left over
    let mut enter = |c: &mut vcpu_vmx, v: &mut MockVmcs, _f: u32| {
        v.vmcs_write32(VM_EXIT_REASON, EXIT_REASON_HLT);
        // the "assembly" only ever sets fail on failure; it does not clear it on success
        let _ = c;
    };
    let mut irq = || {};
    let a = vmx_run_vcpu(&mut vcpu, &mut v, &mut enter, &mut irq);
    assert_eq!(a, vmx_run_action::Exit(EXIT_REASON_HLT));
}
