use ktvmx::mock::*;
use ktvmx::vmcs_ops::*;
use ktvmx::vmx::*;
use ktvmx::vmx_controls::*;

/// Capability MSR: allowed-0 in the low half, allowed-1 in the high half.
fn cap(allowed0: u32, allowed1: u32) -> u64 {
    ((allowed1 as u64) << 32) | allowed0 as u64
}

const BASIC_REV: u64 = 0x12;
/// size 0x400, write-back (6) in bits 53:50, bit 55 (true controls) set.
const BASIC: u64 = (0x400u64 << 32) | (6u64 << 50) | (1u64 << 55) | BASIC_REV;

/// A Skylake-like machine: bit 55 set, TRUE_* MSRs present, EPT + VPID + unrestricted guest.
fn msrs() -> MockMsrs {
    // "default1" bits that are forced to 1 in each word (the reserved-1 bits of SDM A.3-A.5)
    let pin_def1 = 0x16;
    let cpu_def1 = 0x0401_e172;
    let exit_def1 = 0x0003_6dfb;
    let entry_def1 = 0x0000_11fb;
    MockMsrs::default()
        .with(MSR_IA32_VMX_BASIC, BASIC)
        .with(MSR_IA32_VMX_TRUE_PINBASED_CTLS, cap(pin_def1, 0x7f))
        .with(MSR_IA32_VMX_TRUE_PROCBASED_CTLS, cap(cpu_def1 & !CPU_BASED_USE_IO_BITMAPS, 0xfff9_fffe))
        .with(MSR_IA32_VMX_PROCBASED_CTLS2, cap(0, 0x007f_b1ff))
        .with(MSR_IA32_VMX_EPT_VPID_CAP, 0x0f01_0673_4141)
        .with(MSR_IA32_VMX_TRUE_EXIT_CTLS, cap(exit_def1, 0x00ff_ffff))
        .with(MSR_IA32_VMX_TRUE_ENTRY_CTLS, cap(entry_def1, 0x0000_ffff))
        // the plain (non-TRUE) MSRs: everything default1 forced, fewer optional features
        .with(MSR_IA32_VMX_PINBASED_CTLS, cap(pin_def1, 0x3f))
        .with(MSR_IA32_VMX_PROCBASED_CTLS, cap(cpu_def1 | CPU_BASED_USE_IO_BITMAPS, 0xfff9_fffe))
        .with(MSR_IA32_VMX_EXIT_CTLS, cap(exit_def1 | 0x4, 0x00ff_ffff))
        .with(MSR_IA32_VMX_ENTRY_CTLS, cap(entry_def1, 0x0000_ffff))
        .with(MSR_IA32_VMX_CR0_FIXED0, 0x8000_0021)
        .with(MSR_IA32_VMX_CR0_FIXED1, 0xffff_ffff)
        .with(MSR_IA32_VMX_CR4_FIXED0, 0x2000)
        .with(MSR_IA32_VMX_CR4_FIXED1, 0x0037_67ff)
}

// ---- adjust_vmx_controls -------------------------------------------------------

#[test]
fn adjust_keeps_min_and_supported_opt() {
    // CPU allows bits 0..=7 to be 1 and forces none to 1.
    let r = adjust_vmx_controls(0b0001, 0b0110, cap(0, 0xff)).unwrap();
    assert_eq!(r, 0b0111);
}

#[test]
fn adjust_drops_unsupported_opt() {
    // bit 2 is not allowed to be 1: an optional bit silently disappears
    let r = adjust_vmx_controls(0b0001, 0b0110, cap(0, 0b1011)).unwrap();
    assert_eq!(r, 0b0011);
}

#[test]
fn adjust_forces_default1_bits() {
    // bits 1 and 4 must be 1 even though we asked for neither
    let r = adjust_vmx_controls(0b0001, 0, cap(0b1_0010, 0xff)).unwrap();
    assert_eq!(r, 0b1_0011);
}

#[test]
fn adjust_fails_when_min_unsupported() {
    assert_eq!(adjust_vmx_controls(0b0100, 0, cap(0, 0b1011)), Err(()));
}

#[test]
fn adjust_min_bit_cleared_by_allowed1_even_if_forced_elsewhere() {
    // an allowed-1 of 0 for a bit we need beats us; forced bits elsewhere don't help
    assert_eq!(adjust_vmx_controls(1 << 7, 0, cap(0b11, 0b0111_1111)), Err(()));
}

#[test]
fn ctl_msr_selection() {
    assert_eq!(vmx_ctl_msr(true, MSR_IA32_VMX_PINBASED_CTLS, MSR_IA32_VMX_TRUE_PINBASED_CTLS),
               MSR_IA32_VMX_TRUE_PINBASED_CTLS);
    assert_eq!(vmx_ctl_msr(false, MSR_IA32_VMX_PINBASED_CTLS, MSR_IA32_VMX_TRUE_PINBASED_CTLS),
               MSR_IA32_VMX_PINBASED_CTLS);
}

// ---- setup_vmcs_config -----------------------------------------------------------

#[test]
fn basic_fields_decoded() {
    let c = setup_vmcs_config(&msrs()).unwrap();
    assert_eq!(c.revision_id, 0x12);
    assert_eq!(c.size, 0x400);
    assert_eq!(c.ept_vpid_cap, 0x0f01_0673_4141);
    assert_eq!((c.cr0_fixed0, c.cr0_fixed1), (0x8000_0021, 0xffff_ffff));
    assert_eq!((c.cr4_fixed0, c.cr4_fixed1), (0x2000, 0x0037_67ff));
}

#[test]
fn pin_based_controls() {
    let c = setup_vmcs_config(&msrs()).unwrap();
    assert_eq!(c.pin_based_exec_ctrl & MIN_PIN_BASED, MIN_PIN_BASED);
    assert_ne!(c.pin_based_exec_ctrl & PIN_BASED_VMX_PREEMPTION_TIMER, 0, "optional but allowed");
    assert_eq!(c.pin_based_exec_ctrl & 0x16, 0x16, "default1 bits are forced on");
}

#[test]
fn cpu_based_controls() {
    let c = setup_vmcs_config(&msrs()).unwrap();
    let cpu = c.cpu_based_exec_ctrl;
    assert_eq!(cpu & MIN_CPU_BASED, MIN_CPU_BASED);
    assert_ne!(cpu & CPU_BASED_ACTIVATE_SECONDARY_CONTROLS, 0);
    // stage 10 introduces bitmaps; until then everything must exit
    assert_eq!(cpu & CPU_BASED_USE_MSR_BITMAPS, 0);
    assert_eq!(cpu & CPU_BASED_USE_IO_BITMAPS, 0);
}

#[test]
fn secondary_controls() {
    let c = setup_vmcs_config(&msrs()).unwrap();
    let s = c.cpu_based_2nd_exec_ctrl;
    assert_ne!(s & SECONDARY_EXEC_ENABLE_EPT, 0);
    assert_ne!(s & SECONDARY_EXEC_UNRESTRICTED_GUEST, 0);
    assert_ne!(s & SECONDARY_EXEC_ENABLE_VPID, 0);
}

#[test]
fn exit_and_entry_controls() {
    let c = setup_vmcs_config(&msrs()).unwrap();
    assert_eq!(c.vmexit_ctrl & MIN_EXIT, MIN_EXIT);
    assert_eq!(c.vmexit_ctrl & VM_EXIT_ACK_INTR_ON_EXIT, 0, "host takes the interrupt itself");
    assert_eq!(c.vmentry_ctrl & VM_ENTRY_LOAD_IA32_EFER, VM_ENTRY_LOAD_IA32_EFER);
    assert_eq!(c.vmentry_ctrl & VM_ENTRY_IA32E_MODE, 0, "IA-32e is per guest, stage 5");
    assert_eq!(c.vmentry_ctrl & 0x11fb, 0x11fb, "default1 bits are forced on");
}

#[test]
fn true_msrs_are_used_only_when_basic_bit_55_is_set() {
    // With TRUE_* the preemption timer (bit 6) is allowed, the plain pin MSR says no.
    let with_true = setup_vmcs_config(&msrs()).unwrap();
    assert_ne!(with_true.pin_based_exec_ctrl & PIN_BASED_VMX_PREEMPTION_TIMER, 0);
    let mut m = msrs();
    m.0.insert(MSR_IA32_VMX_BASIC, BASIC & !(1 << 55));
    let plain = setup_vmcs_config(&m).unwrap();
    assert_eq!(plain.pin_based_exec_ctrl & PIN_BASED_VMX_PREEMPTION_TIMER, 0);
    // and the plain exit MSR forces bit 2 on, the TRUE one does not
    assert_eq!(with_true.vmexit_ctrl & 0x4, 0);
    assert_ne!(plain.vmexit_ctrl & 0x4, 0);
}

#[test]
fn unrestricted_guest_needs_ept() {
    let mut m = msrs();
    // secondary MSR: unrestricted guest + VPID allowed, EPT not
    m.0.insert(MSR_IA32_VMX_PROCBASED_CTLS2,
               cap(0, SECONDARY_EXEC_UNRESTRICTED_GUEST | SECONDARY_EXEC_ENABLE_VPID));
    let c = setup_vmcs_config(&m).unwrap();
    assert_eq!(c.cpu_based_2nd_exec_ctrl & SECONDARY_EXEC_ENABLE_EPT, 0);
    assert_eq!(c.cpu_based_2nd_exec_ctrl & SECONDARY_EXEC_UNRESTRICTED_GUEST, 0);
    assert_ne!(c.cpu_based_2nd_exec_ctrl & SECONDARY_EXEC_ENABLE_VPID, 0);
}

#[test]
fn no_secondary_controls_at_all() {
    let mut m = msrs();
    // the CPU does not allow "activate secondary controls": the 2nd MSR must not even be read
    m.0.insert(MSR_IA32_VMX_TRUE_PROCBASED_CTLS, cap(0x0401_e172 & !CPU_BASED_USE_IO_BITMAPS, 0x7ff9_fffe));
    m.0.remove(&MSR_IA32_VMX_PROCBASED_CTLS2);
    m.0.remove(&MSR_IA32_VMX_EPT_VPID_CAP);
    let c = setup_vmcs_config(&m).unwrap();
    assert_eq!(c.cpu_based_exec_ctrl & CPU_BASED_ACTIVATE_SECONDARY_CONTROLS, 0);
    assert_eq!(c.cpu_based_2nd_exec_ctrl, 0);
    assert_eq!(c.ept_vpid_cap, 0);
}

#[test]
fn rejects_unusable_hardware() {
    // HLT exiting not allowed (bit 7 absent from allowed-1)
    let mut m = msrs();
    m.0.insert(MSR_IA32_VMX_TRUE_PROCBASED_CTLS, cap(0x0401_e172 & !CPU_BASED_USE_IO_BITMAPS, 0xfff9_ff7e));
    assert!(setup_vmcs_config(&m).is_err());
    // VMCS bigger than a page
    let mut m = msrs();
    m.0.insert(MSR_IA32_VMX_BASIC, (BASIC & !(0x1fffu64 << 32)) | (0x1001u64 << 32));
    assert!(setup_vmcs_config(&m).is_err());
    // 32-bit physical addresses only (bit 48)
    let mut m = msrs();
    m.0.insert(MSR_IA32_VMX_BASIC, BASIC | (1 << 48));
    assert!(setup_vmcs_config(&m).is_err());
    // VMCS memory type uncacheable (0) instead of write-back
    let mut m = msrs();
    m.0.insert(MSR_IA32_VMX_BASIC, BASIC & !(0xfu64 << 50));
    assert!(setup_vmcs_config(&m).is_err());
    // a VMX MSR that does not exist
    let mut m = msrs();
    m.0.remove(&MSR_IA32_VMX_CR4_FIXED1);
    assert!(setup_vmcs_config(&m).is_err());
}

// ---- vmx_vmcs_controls_write -----------------------------------------------------

#[test]
fn controls_are_written_to_the_vmcs() {
    let cfg = setup_vmcs_config(&msrs()).unwrap();
    let mut v = MockVmcs::default();
    vmx_vmcs_controls_write(&mut v, &cfg);
    assert_eq!(v.vmcs_read32(PIN_BASED_VM_EXEC_CONTROL), cfg.pin_based_exec_ctrl);
    assert_eq!(v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL), cfg.cpu_based_exec_ctrl);
    assert_eq!(v.vmcs_read32(SECONDARY_VM_EXEC_CONTROL), cfg.cpu_based_2nd_exec_ctrl);
    assert_eq!(v.vmcs_read32(VM_EXIT_CONTROLS), cfg.vmexit_ctrl);
    assert_eq!(v.vmcs_read32(VM_ENTRY_CONTROLS), cfg.vmentry_ctrl);
}

#[test]
fn defaults_are_written_even_over_stale_values() {
    let cfg = setup_vmcs_config(&msrs()).unwrap();
    let mut v = MockVmcs::default();
    for f in [EXCEPTION_BITMAP, CR3_TARGET_COUNT, VM_ENTRY_INTR_INFO_FIELD, VM_ENTRY_MSR_LOAD_COUNT,
              VM_EXIT_MSR_LOAD_COUNT, VM_EXIT_MSR_STORE_COUNT, PAGE_FAULT_ERROR_CODE_MASK,
              PAGE_FAULT_ERROR_CODE_MATCH] {
        v.fields.insert(f, 0xdead);
    }
    vmx_vmcs_controls_write(&mut v, &cfg);
    for f in [EXCEPTION_BITMAP, CR3_TARGET_COUNT, VM_ENTRY_INTR_INFO_FIELD, VM_ENTRY_MSR_LOAD_COUNT,
              VM_EXIT_MSR_LOAD_COUNT, VM_EXIT_MSR_STORE_COUNT, PAGE_FAULT_ERROR_CODE_MASK,
              PAGE_FAULT_ERROR_CODE_MATCH] {
        assert_eq!(v.vmcs_readl(f), 0, "field {:#x} must be reset", f);
    }
}

#[test]
fn secondary_field_untouched_without_secondary_controls() {
    let mut cfg = setup_vmcs_config(&msrs()).unwrap();
    cfg.cpu_based_exec_ctrl &= !CPU_BASED_ACTIVATE_SECONDARY_CONTROLS;
    let mut v = MockVmcs::default();
    vmx_vmcs_controls_write(&mut v, &cfg);
    assert!(!v.writes.iter().any(|(f, _)| *f == SECONDARY_VM_EXEC_CONTROL),
            "VMWRITE to an unsupported field fails with VM-instruction error 12");
    assert_eq!(v.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL), cfg.cpu_based_exec_ctrl);
}
