use ktvmx::mock::MockVmcs;
use ktvmx::vmcs_ops::VmcsOps;
use ktvmx::vmx::*;
use ktvmx::vmx_events::*;
use ktvmx::vmx_exit::{GUEST_INTR_STATE_MOV_SS, GUEST_INTR_STATE_NMI, GUEST_INTR_STATE_STI};

fn entry_info(v: &MockVmcs) -> u32 {
    v.vmcs_read32(VM_ENTRY_INTR_INFO_FIELD)
}

#[test]
fn error_code_vectors() {
    for vec in 0..32u8 {
        let expect = matches!(vec, 8 | 10 | 11 | 12 | 13 | 14 | 17);
        assert_eq!(exception_has_error_code(vec), expect, "vector {vec}");
    }
    assert!(!exception_has_error_code(0x20), "external interrupts never carry one");
}

#[test]
fn inject_irq() {
    let mut v = MockVmcs::default();
    vmx_inject_irq(&mut v, 0x20);
    assert_eq!(entry_info(&v), 0x8000_0020);
    vmx_inject_irq(&mut v, 0xef);
    assert_eq!(entry_info(&v), 0x8000_00ef);
    // no error code or instruction length for an external interrupt
    assert!(!v.fields.contains_key(&VM_ENTRY_EXCEPTION_ERROR_CODE));
    assert!(!v.fields.contains_key(&VM_ENTRY_INSTRUCTION_LEN));
}

#[test]
fn inject_nmi() {
    let mut v = MockVmcs::default();
    vmx_inject_nmi(&mut v);
    assert_eq!(entry_info(&v), 0x8000_0202); // valid | type 2 | vector 2
}

#[test]
fn inject_soft_interrupt_needs_length() {
    let mut v = MockVmcs::default();
    vmx_inject_soft_intr(&mut v, 0x80, 2);
    assert_eq!(entry_info(&v), 0x8000_0480); // type 4 = software interrupt
    assert_eq!(v.vmcs_read32(VM_ENTRY_INSTRUCTION_LEN), 2);
}

#[test]
fn inject_exception_with_error_code() {
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, GP_VECTOR, Some(0), 0);
    assert_eq!(entry_info(&v), 0x8000_0b0d); // valid | deliver code | type 3 | #GP
    assert_eq!(v.vmcs_read32(VM_ENTRY_EXCEPTION_ERROR_CODE), 0);

    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, PF_VECTOR, Some(0x7), 0);
    assert_eq!(entry_info(&v), 0x8000_0b0e);
    assert_eq!(v.vmcs_read32(VM_ENTRY_EXCEPTION_ERROR_CODE), 7);
}

#[test]
fn inject_exception_without_error_code() {
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, UD_VECTOR, None, 0);
    assert_eq!(entry_info(&v), 0x8000_0306);
    assert!(!v.fields.contains_key(&VM_ENTRY_EXCEPTION_ERROR_CODE));
}

#[test]
fn error_code_is_ignored_for_vectors_that_take_none() {
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, UD_VECTOR, Some(0x1234), 0);
    assert_eq!(entry_info(&v), 0x8000_0306, "bit 11 must stay clear for #UD");
    assert!(!v.fields.contains_key(&VM_ENTRY_EXCEPTION_ERROR_CODE));
    // and a vector that does take one, but where the caller supplied none: no bit either
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, GP_VECTOR, None, 0);
    assert_eq!(entry_info(&v), 0x8000_030d);
}

#[test]
fn breakpoint_and_overflow_are_software_exceptions() {
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, BP_VECTOR, None, 1);
    assert_eq!(entry_info(&v), 0x8000_0603); // type 6
    assert_eq!(v.vmcs_read32(VM_ENTRY_INSTRUCTION_LEN), 1);
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, 4, None, 1);
    assert_eq!(entry_info(&v), 0x8000_0604);
    // hardware exceptions do not touch the length field
    let mut v = MockVmcs::default();
    vmx_inject_exception(&mut v, DE_VECTOR, None, 5);
    assert!(!v.fields.contains_key(&VM_ENTRY_INSTRUCTION_LEN));
}

#[test]
fn decode_exit_interruption_info() {
    // #PF with error code
    let i = decode_intr_info(0x8000_0b0e);
    assert_eq!(
        i,
        intr_info { valid: true, vector: 14, itype: INTR_TYPE_HARD_EXCEPTION, has_error_code: true, nmi_unblocking: false }
    );
    // NMI with the "unblocking due to IRET" bit
    let i = decode_intr_info(0x8000_1202);
    assert!(i.valid && i.nmi_unblocking && !i.has_error_code);
    assert_eq!(i.itype, INTR_TYPE_NMI_INTR);
    assert_eq!(i.vector, 2);
    // not valid
    assert!(!decode_intr_info(0x0000_0b0e).valid);
    // external interrupt
    let i = decode_intr_info(0x8000_0020);
    assert_eq!(i.itype, INTR_TYPE_EXT_INTR);
    assert_eq!(i.vector, 0x20);
}

#[test]
fn read_exit_intr_info_from_vmcs() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(VM_EXIT_INTR_INFO, 0x8000_0306);
    let i = vmx_exit_intr_info(&v);
    assert!(i.valid);
    assert_eq!(i.vector, UD_VECTOR);
    assert_eq!(i.itype, INTR_TYPE_HARD_EXCEPTION);
}

#[test]
fn is_exception_n_checks_type_valid_and_vector() {
    assert!(is_exception_n(0x8000_0b0e, PF_VECTOR));
    assert!(!is_exception_n(0x8000_0b0e, GP_VECTOR));
    assert!(!is_exception_n(0x0000_0b0e, PF_VECTOR), "not valid");
    assert!(!is_exception_n(0x8000_000e, PF_VECTOR), "that is an external interrupt 14");
    assert!(!is_exception_n(0x8000_0603, BP_VECTOR), "software exception is a different type");
    assert!(is_exception_n(0x8000_0306, UD_VECTOR));
}

#[test]
fn complete_interrupts_does_nothing_without_a_vectoring_event() {
    let mut v = MockVmcs::default();
    assert!(!vmx_complete_interrupts(&mut v));
    assert!(v.writes.is_empty());
    v.vmcs_write32(IDT_VECTORING_INFO_FIELD, 0x0000_0020); // valid bit clear
    assert!(!vmx_complete_interrupts(&mut v));
    assert!(!v.fields.contains_key(&VM_ENTRY_INTR_INFO_FIELD));
}

#[test]
fn complete_interrupts_reinjects_an_interrupted_external_interrupt() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(IDT_VECTORING_INFO_FIELD, 0x8000_0020);
    assert!(vmx_complete_interrupts(&mut v));
    assert_eq!(entry_info(&v), 0x8000_0020);
    assert!(!v.fields.contains_key(&VM_ENTRY_EXCEPTION_ERROR_CODE));
    assert!(!v.fields.contains_key(&VM_ENTRY_INSTRUCTION_LEN));
}

#[test]
fn complete_interrupts_copies_the_error_code() {
    let mut v = MockVmcs::default();
    // a #PF whose delivery was interrupted by an EPT violation on the IDT/stack
    v.vmcs_write32(IDT_VECTORING_INFO_FIELD, 0x8000_0b0e);
    v.vmcs_write32(IDT_VECTORING_ERROR_CODE, 0x6);
    assert!(vmx_complete_interrupts(&mut v));
    assert_eq!(entry_info(&v), 0x8000_0b0e);
    assert_eq!(v.vmcs_read32(VM_ENTRY_EXCEPTION_ERROR_CODE), 6);
}

#[test]
fn complete_interrupts_copies_length_for_software_events() {
    for (idt, len) in [(0x8000_0480u32, 2u32), (0x8000_0603, 1), (0x8000_0501, 1)] {
        let mut v = MockVmcs::default();
        v.vmcs_write32(IDT_VECTORING_INFO_FIELD, idt);
        v.vmcs_write32(VM_EXIT_INSTRUCTION_LEN, len);
        assert!(vmx_complete_interrupts(&mut v));
        assert_eq!(entry_info(&v), idt);
        assert_eq!(v.vmcs_read32(VM_ENTRY_INSTRUCTION_LEN), len, "idt info {idt:#x}");
    }
    // a hardware exception must not drag along a stale exit instruction length
    let mut v = MockVmcs::default();
    v.vmcs_write32(IDT_VECTORING_INFO_FIELD, 0x8000_0306);
    v.vmcs_write32(VM_EXIT_INSTRUCTION_LEN, 3);
    vmx_complete_interrupts(&mut v);
    assert!(!v.fields.contains_key(&VM_ENTRY_INSTRUCTION_LEN));
}

#[test]
fn complete_interrupts_clears_the_undefined_bit_12() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(IDT_VECTORING_INFO_FIELD, 0x8000_1020);
    assert!(vmx_complete_interrupts(&mut v));
    assert_eq!(entry_info(&v), 0x8000_0020);
}

#[test]
fn interrupt_allowed_needs_if_and_no_shadow() {
    let mut v = MockVmcs::default();
    v.vmcs_writel(GUEST_RFLAGS, X86_EFLAGS_FIXED);
    assert!(!vmx_interrupt_allowed(&v), "IF clear");
    v.vmcs_writel(GUEST_RFLAGS, X86_EFLAGS_FIXED | X86_EFLAGS_IF);
    assert!(vmx_interrupt_allowed(&v));
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_STI);
    assert!(!vmx_interrupt_allowed(&v), "the instruction after STI is still protected");
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_MOV_SS);
    assert!(!vmx_interrupt_allowed(&v));
    // NMI blocking does not block *maskable* interrupts
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_NMI);
    assert!(vmx_interrupt_allowed(&v));
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, 0);
    // one event per entry
    vmx_inject_irq(&mut v, 0x30);
    assert!(!vmx_interrupt_allowed(&v));
}

#[test]
fn nmi_allowed_rules() {
    let mut v = MockVmcs::default();
    assert!(vmx_nmi_allowed(&v), "IF does not matter for NMIs");
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_NMI);
    assert!(!vmx_nmi_allowed(&v));
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_STI);
    assert!(!vmx_nmi_allowed(&v));
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, GUEST_INTR_STATE_MOV_SS);
    assert!(!vmx_nmi_allowed(&v));
    v.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, 0);
    vmx_inject_irq(&mut v, 0x30);
    assert!(!vmx_nmi_allowed(&v));
}

#[test]
fn irq_window_toggle_preserves_other_controls() {
    let mut v = MockVmcs::default();
    let base = CPU_BASED_HLT_EXITING | CPU_BASED_ACTIVATE_SECONDARY_CONTROLS;
    v.vmcs_write32(CPU_BASED_VM_EXEC_CONTROL, base);
    vmx_enable_irq_window(&mut v);
    assert_eq!(v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL), base | CPU_BASED_INTR_WINDOW_EXITING);
    vmx_enable_irq_window(&mut v); // idempotent
    assert_eq!(v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL), base | CPU_BASED_INTR_WINDOW_EXITING);
    vmx_disable_irq_window(&mut v);
    assert_eq!(v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL), base);
}

#[test]
fn exception_bitmap_helpers() {
    let mut v = MockVmcs::default();
    v.vmcs_write32(EXCEPTION_BITMAP, 1 << UD_VECTOR);
    vmx_update_exception_bitmap(&mut v, PF_VECTOR, true);
    assert_eq!(v.vmcs_read32(EXCEPTION_BITMAP), 1 << 6 | 1 << 14);
    assert!(vmx_exception_intercepted(&v, PF_VECTOR));
    assert!(vmx_exception_intercepted(&v, UD_VECTOR));
    assert!(!vmx_exception_intercepted(&v, GP_VECTOR));
    vmx_update_exception_bitmap(&mut v, UD_VECTOR, false);
    assert_eq!(v.vmcs_read32(EXCEPTION_BITMAP), 1 << 14);
    vmx_update_exception_bitmap(&mut v, 31, true);
    assert_eq!(v.vmcs_read32(EXCEPTION_BITMAP), 1 << 14 | 1 << 31);
}

#[test]
fn preemption_timer_conversion() {
    // 1 ms at 2.8 GHz = 2_800_000 cycles; rate 5 -> /32
    assert_eq!(preemption_timer_value(1_000_000, 2_800_000, 5), 87_500);
    // rate 0: raw TSC ticks
    assert_eq!(preemption_timer_value(1_000_000, 2_800_000, 0), 2_800_000);
    // only bits 4:0 of IA32_VMX_MISC are the rate; the rest are other capabilities
    assert_eq!(preemption_timer_value(1_000_000, 2_800_000, 0x7000_0000_0000_0405), 87_500);
    assert_eq!(preemption_timer_value(0, 2_800_000, 5), 0);
    // 100 s at 2.8 GHz does not fit 32 bits at rate 0: saturate
    assert_eq!(preemption_timer_value(100_000_000_000, 2_800_000, 0), u32::MAX);
    // u64 intermediate would overflow: 2^40 ns * 3e6 kHz
    assert_eq!(preemption_timer_value(1 << 40, 3_000_000, 10), ((1u128 << 40) * 3_000_000 / 1_000_000 >> 10).min(u32::MAX as u128) as u32);
}

#[test]
fn arm_and_disarm_preemption_timer() {
    let mut v = MockVmcs::default();
    let pin = PIN_BASED_EXT_INTR_MASK | PIN_BASED_NMI_EXITING;
    v.vmcs_write32(PIN_BASED_VM_EXEC_CONTROL, pin);
    vmx_arm_preemption_timer(&mut v, 1234);
    assert_eq!(v.vmcs_read32(VMX_PREEMPTION_TIMER_VALUE), 1234);
    assert_eq!(v.vmcs_read32(PIN_BASED_VM_EXEC_CONTROL), pin | PIN_BASED_VMX_PREEMPTION_TIMER);
    vmx_arm_preemption_timer(&mut v, 99); // re-arm
    assert_eq!(v.vmcs_read32(VMX_PREEMPTION_TIMER_VALUE), 99);
    vmx_disarm_preemption_timer(&mut v);
    assert_eq!(v.vmcs_read32(PIN_BASED_VM_EXEC_CONTROL), pin);
}
