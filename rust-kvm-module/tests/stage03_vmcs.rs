use ktvmx::mock::MockVmcs;
use ktvmx::vmcs_fields::*;
use ktvmx::vmcs_ops::VmcsOps;
use ktvmx::vmx::*;
use ktvmx::vmx_caps::*;

const BASIC: u64 = 0x00da_0400_0000_0004;
// IA32_VMX_VMCS_ENUM: highest index 0x17 (VMX-preemption timer value, 0x482e) in bits 9:1.
const VMCS_ENUM: u64 = 0x2e;

#[test]
fn encoding_of_known_fields() {
    // GUEST_RIP = 0x681e: natural width, guest-state, index 0xf
    let f = GUEST_RIP;
    assert_eq!(vmcs_field_width(f), vmcs_width::Natural);
    assert_eq!(vmcs_field_type(f), vmcs_type::Guest);
    assert_eq!(vmcs_field_index(f), 0xf);
    assert!(!vmcs_field_access_high(f));
    assert!(!vmcs_field_readonly(f));
    assert_eq!(vmcs_field_bits(f), 64);

    // GUEST_CS_SELECTOR = 0x0802: 16-bit guest-state, index 1
    assert_eq!(vmcs_field_width(GUEST_CS_SELECTOR), vmcs_width::W16);
    assert_eq!(vmcs_field_type(GUEST_CS_SELECTOR), vmcs_type::Guest);
    assert_eq!(vmcs_field_index(GUEST_CS_SELECTOR), 1);
    assert_eq!(vmcs_field_bits(GUEST_CS_SELECTOR), 16);

    // VM_EXIT_REASON = 0x4402: 32-bit read-only
    assert_eq!(vmcs_field_width(VM_EXIT_REASON), vmcs_width::W32);
    assert_eq!(vmcs_field_type(VM_EXIT_REASON), vmcs_type::ReadOnly);
    assert!(vmcs_field_readonly(VM_EXIT_REASON));
    assert_eq!(vmcs_field_bits(VM_EXIT_REASON), 32);

    // EPT_POINTER = 0x201a: 64-bit control
    assert_eq!(vmcs_field_width(EPT_POINTER), vmcs_width::W64);
    assert_eq!(vmcs_field_type(EPT_POINTER), vmcs_type::Control);
    assert_eq!(vmcs_field_bits(EPT_POINTER), 64);

    // GUEST_PHYSICAL_ADDRESS = 0x2400: 64-bit but read-only
    assert_eq!(vmcs_field_width(GUEST_PHYSICAL_ADDRESS), vmcs_width::W64);
    assert!(vmcs_field_readonly(GUEST_PHYSICAL_ADDRESS));

    // HOST_RSP = 0x6c14: natural-width host-state, index 0xa
    assert_eq!(vmcs_field_type(HOST_RSP), vmcs_type::Host);
    assert_eq!(vmcs_field_width(HOST_RSP), vmcs_width::Natural);
    assert_eq!(vmcs_field_index(HOST_RSP), 0xa);

    // EXIT_QUALIFICATION = 0x6400: natural-width read-only
    assert_eq!(vmcs_field_width(EXIT_QUALIFICATION), vmcs_width::Natural);
    assert!(vmcs_field_readonly(EXIT_QUALIFICATION));

    // VIRTUAL_PROCESSOR_ID = 0: 16-bit control, index 0
    assert_eq!(vmcs_field_width(VIRTUAL_PROCESSOR_ID), vmcs_width::W16);
    assert_eq!(vmcs_field_type(VIRTUAL_PROCESSOR_ID), vmcs_type::Control);
    assert_eq!(vmcs_field_index(VIRTUAL_PROCESSOR_ID), 0);
}

#[test]
fn every_field_in_vmx_rs_is_consistent() {
    // (field, width, type): the groups of asm/vmx.h's enum vmcs_field.
    use vmcs_type::*;
    use vmcs_width::*;
    let table: &[(u32, vmcs_width, vmcs_type)] = &[
        (GUEST_ES_SELECTOR, W16, Guest), (GUEST_TR_SELECTOR, W16, Guest),
        (HOST_ES_SELECTOR, W16, Host), (HOST_TR_SELECTOR, W16, Host),
        (IO_BITMAP_A, W64, Control), (MSR_BITMAP, W64, Control), (TSC_OFFSET, W64, Control),
        (VMCS_LINK_POINTER, W64, Guest), (GUEST_IA32_EFER, W64, Guest), (GUEST_IA32_PAT, W64, Guest),
        (HOST_IA32_EFER, W64, Host), (HOST_IA32_PAT, W64, Host),
        (PIN_BASED_VM_EXEC_CONTROL, W32, Control), (CPU_BASED_VM_EXEC_CONTROL, W32, Control),
        (EXCEPTION_BITMAP, W32, Control), (VM_EXIT_CONTROLS, W32, Control),
        (VM_ENTRY_CONTROLS, W32, Control), (VM_ENTRY_INTR_INFO_FIELD, W32, Control),
        (SECONDARY_VM_EXEC_CONTROL, W32, Control),
        (VM_INSTRUCTION_ERROR, W32, ReadOnly), (VM_EXIT_INTR_INFO, W32, ReadOnly),
        (IDT_VECTORING_INFO_FIELD, W32, ReadOnly), (VM_EXIT_INSTRUCTION_LEN, W32, ReadOnly),
        (VMX_INSTRUCTION_INFO, W32, ReadOnly),
        (GUEST_ES_LIMIT, W32, Guest), (GUEST_CS_AR_BYTES, W32, Guest),
        (GUEST_INTERRUPTIBILITY_INFO, W32, Guest), (GUEST_ACTIVITY_STATE, W32, Guest),
        (VMX_PREEMPTION_TIMER_VALUE, W32, Guest),
        (HOST_IA32_SYSENTER_CS, W32, Host),
        (CR0_GUEST_HOST_MASK, Natural, Control), (CR4_READ_SHADOW, Natural, Control),
        (GUEST_LINEAR_ADDRESS, Natural, ReadOnly),
        (GUEST_CR0, Natural, Guest), (GUEST_CR3, Natural, Guest), (GUEST_RSP, Natural, Guest),
        (GUEST_RFLAGS, Natural, Guest), (GUEST_GDTR_BASE, Natural, Guest),
        (HOST_CR3, Natural, Host), (HOST_RIP, Natural, Host), (HOST_TR_BASE, Natural, Host),
    ];
    for &(f, w, t) in table {
        assert_eq!(vmcs_field_width(f), w, "width of {f:#06x}");
        assert_eq!(vmcs_field_type(f), t, "type of {f:#06x}");
        assert_eq!(vmcs_field_readonly(f), t == ReadOnly, "readonly of {f:#06x}");
        assert!(vmcs_field_valid_encoding(f), "{f:#06x}");
        assert!(!vmcs_field_access_high(f), "{f:#06x}");
        assert!(
            vmcs_field_index(f) <= vmcs_enum_max_index(VMCS_ENUM),
            "{f:#06x} index must be within the Skylake enumeration"
        );
    }
}

#[test]
fn high_access_of_64bit_fields() {
    assert_eq!(vmcs_field_high(VMCS_LINK_POINTER), Some(0x2801));
    assert_eq!(vmcs_field_high(EPT_POINTER), Some(0x201b));
    let hi = 0x2801;
    assert!(vmcs_field_access_high(hi));
    assert!(vmcs_field_valid_encoding(hi));
    assert_eq!(vmcs_field_bits(hi), 32);
    assert_eq!(vmcs_field_width(hi), vmcs_width::W64);
    assert_eq!(vmcs_field_index(hi), vmcs_field_index(VMCS_LINK_POINTER));
    // only full-access 64-bit encodings have a high half
    assert_eq!(vmcs_field_high(hi), None);
    assert_eq!(vmcs_field_high(GUEST_RIP), None);
    assert_eq!(vmcs_field_high(GUEST_CS_SELECTOR), None);
    assert_eq!(vmcs_field_high(VM_EXIT_REASON), None);
}

#[test]
fn malformed_encodings() {
    assert!(!vmcs_field_valid_encoding(GUEST_RIP | 1)); // high access on a natural-width field
    assert!(!vmcs_field_valid_encoding(VM_EXIT_REASON | 1)); // ... on a 32-bit field
    assert!(!vmcs_field_valid_encoding(GUEST_CS_SELECTOR | 1)); // ... on a 16-bit field
    assert!(!vmcs_field_valid_encoding(GUEST_RIP | (1 << 12))); // reserved bit 12
    assert!(!vmcs_field_valid_encoding(GUEST_RIP | (1 << 15))); // reserved bits 31:15
    assert!(!vmcs_field_valid_encoding(0x8000_0000));
    assert!(vmcs_field_valid_encoding(0));
}

#[test]
fn enumeration_msr() {
    assert_eq!(vmcs_enum_max_index(VMCS_ENUM), 0x17);
    assert_eq!(vmcs_enum_max_index(0), 0);
    assert_eq!(vmcs_enum_max_index(0xffff_ffff_ffff_fc01), 0x0); // bit 0 and 63:10 are not the field
    assert_eq!(vmcs_enum_max_index(0x3fe), 0x1ff);
}

#[test]
fn masks() {
    assert_eq!(vmcs_field_mask(GUEST_CS_SELECTOR), 0xffff);
    assert_eq!(vmcs_field_mask(GUEST_CS_LIMIT), 0xffff_ffff);
    assert_eq!(vmcs_field_mask(0x2801), 0xffff_ffff); // high half of a 64-bit field
    assert_eq!(vmcs_field_mask(EPT_POINTER), u64::MAX);
    assert_eq!(vmcs_field_mask(GUEST_RIP), u64::MAX);
}

#[test]
fn checked_write_and_read() {
    let mut v = MockVmcs::default();
    assert_eq!(vmcs_checked_write(&mut v, VMCS_ENUM, GUEST_RIP, 0xffff_8000_dead_beef), Ok(()));
    assert_eq!(vmcs_checked_read(&v, VMCS_ENUM, GUEST_RIP), Ok(0xffff_8000_dead_beef));
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0xffff_8000_dead_beef);
    assert_eq!(v.writes, vec![(GUEST_RIP, 0xffff_8000_dead_beef)]);
}

#[test]
fn checked_write_truncates_like_hardware() {
    let mut v = MockVmcs::default();
    vmcs_checked_write(&mut v, VMCS_ENUM, GUEST_CS_SELECTOR, 0x1_2345_6789).unwrap();
    assert_eq!(v.vmcs_readl(GUEST_CS_SELECTOR), 0x6789);
    vmcs_checked_write(&mut v, VMCS_ENUM, GUEST_CS_LIMIT, 0x1_2345_6789).unwrap();
    assert_eq!(v.vmcs_readl(GUEST_CS_LIMIT), 0x2345_6789);
    vmcs_checked_write(&mut v, VMCS_ENUM, EPT_POINTER, 0x1_2345_6789).unwrap();
    assert_eq!(v.vmcs_readl(EPT_POINTER), 0x1_2345_6789);
    // high half of a 64-bit field is 32 bits wide
    vmcs_checked_write(&mut v, VMCS_ENUM, 0x2801, 0xaaaa_bbbb_cccc_dddd).unwrap();
    assert_eq!(v.vmcs_readl(0x2801), 0xcccc_dddd);
}

#[test]
fn read_masks_too() {
    let mut v = MockVmcs::default();
    v.fields.insert(GUEST_CS_SELECTOR, 0xffff_ffff_ffff);
    assert_eq!(vmcs_checked_read(&v, VMCS_ENUM, GUEST_CS_SELECTOR), Ok(0xffff));
}

#[test]
fn readonly_fields_refuse_writes_but_can_be_read() {
    let mut v = MockVmcs::default();
    v.fields.insert(VM_EXIT_REASON, 12); // HLT, put there "by hardware"
    assert_eq!(
        vmcs_checked_write(&mut v, VMCS_ENUM, VM_EXIT_REASON, 0),
        Err(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT)
    );
    assert_eq!(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT, 13);
    assert!(v.writes.is_empty(), "nothing must be written");
    assert_eq!(vmcs_checked_read(&v, VMCS_ENUM, VM_EXIT_REASON), Ok(12));
    assert_eq!(
        vmcs_checked_write(&mut v, VMCS_ENUM, GUEST_PHYSICAL_ADDRESS, 0),
        Err(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT)
    );
}

#[test]
fn unsupported_fields() {
    let mut v = MockVmcs::default();
    // malformed
    assert_eq!(vmcs_checked_write(&mut v, VMCS_ENUM, GUEST_RIP | 1, 0), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
    assert_eq!(vmcs_checked_write(&mut v, VMCS_ENUM, 0x1_0000, 0), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
    // well formed, but index 0x18 > 0x17 enumerated by this CPU
    assert_eq!(vmcs_checked_write(&mut v, VMCS_ENUM, 0x4830, 0), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
    assert_eq!(VMXERR_UNSUPPORTED_VMCS_COMPONENT, 12);
    assert_eq!(vmcs_checked_read(&v, VMCS_ENUM, 0x4830), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
    assert_eq!(vmcs_checked_read(&v, VMCS_ENUM, GUEST_RIP | 1), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
    assert!(v.writes.is_empty());
    // The same index is fine on a CPU that enumerates more.
    assert_eq!(vmcs_checked_write(&mut v, 0x30, 0x4830, 5), Ok(()));
    // Unsupported beats read-only: an unsupported *read-only* field reports 12, not 13.
    assert_eq!(vmcs_checked_write(&mut v, 0x2, 0x4414, 0), Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT));
}

#[test]
fn vmcs_region_layout() {
    let basic = vmx_basic_decode(BASIC);
    let mut page = [0x55u8; 4096];
    vmcs_region_init(&mut page, &basic, false).unwrap();
    assert_eq!(u32::from_le_bytes(page[..4].try_into().unwrap()), 4);
    assert_eq!(&page[4..8], &[0; 4], "VMX-abort indicator");
    assert!(page[8..1024].iter().all(|&b| b == 0));
    assert!(page[1024..].iter().all(|&b| b == 0x55), "only vmcs_size bytes are ours to clear");
}

#[test]
fn shadow_vmcs_indicator_is_bit_31() {
    let basic = vmx_basic_decode(BASIC);
    let mut page = [0u8; 4096];
    vmcs_region_init(&mut page, &basic, true).unwrap();
    assert_eq!(u32::from_le_bytes(page[..4].try_into().unwrap()), 0x8000_0004);
}

#[test]
fn vmcs_region_bad_sizes() {
    let mut basic = vmx_basic_decode(BASIC);
    let mut small = [0u8; 512];
    assert!(vmcs_region_init(&mut small, &basic, false).is_err()); // smaller than 1024
    basic.vmcs_size = 0;
    let mut page = [0u8; 4096];
    assert!(vmcs_region_init(&mut page, &basic, false).is_err());
    basic.vmcs_size = 8191; // the field is 13 bits wide, but the SDM caps it at 4096
    assert!(vmcs_region_init(&mut page, &basic, false).is_err());
    basic.vmcs_size = 4096;
    assert!(vmcs_region_init(&mut page, &basic, false).is_ok());
}
