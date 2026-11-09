use ktvmx::mock::MockMsrs;
use ktvmx::vmx::*;
use ktvmx::vmx_caps::*;
use ktvmx::vmx_enable::*;

// IA32_VMX_CR{0,4}_FIXED{0,1} of a Skylake-class part.
const CR0_FIXED0: u64 = 0x8000_0021; // PG | NE | PE must be 1
const CR0_FIXED1: u64 = 0xffff_ffff;
const CR4_FIXED0: u64 = 0x0000_2000; // VMXE must be 1
const CR4_FIXED1: u64 = 0x0037_27ff;
const BASIC: u64 = 0x00da_0400_0000_0004;

// A typical 64-bit Linux kernel: PG|WP|NE|ET|MP|PE and PAE|PGE|OSFXSR|..|VMXE.
const CR0: u64 = 0x8005_0033;
const CR4_NO_VMXE: u64 = 0x0000_0020 | 0x0000_0080; // PAE | PGE
const CR4: u64 = CR4_NO_VMXE | X86_CR4_VMXE;

fn fixed() -> vmx_cr_fixed {
    vmx_cr_fixed {
        cr0_fixed0: CR0_FIXED0,
        cr0_fixed1: CR0_FIXED1,
        cr4_fixed0: CR4_FIXED0,
        cr4_fixed1: CR4_FIXED1,
    }
}

fn env() -> vmxon_env {
    vmxon_env {
        cr0: CR0,
        cr4: CR4,
        cpl: 0,
        feature_control: 0x5,
        fixed: fixed(),
        basic: vmx_basic_decode(BASIC),
        phys_addr_bits: 39,
    }
}

const REGION: u64 = 0x1234_5000;

#[test]
fn read_fixed_msrs() {
    let m = MockMsrs::default()
        .with(MSR_IA32_VMX_CR0_FIXED0, CR0_FIXED0)
        .with(MSR_IA32_VMX_CR0_FIXED1, CR0_FIXED1)
        .with(MSR_IA32_VMX_CR4_FIXED0, CR4_FIXED0)
        .with(MSR_IA32_VMX_CR4_FIXED1, CR4_FIXED1);
    assert_eq!(vmx_read_cr_fixed(&m), Some(fixed()));
    let mut m = m;
    m.0.remove(&MSR_IA32_VMX_CR4_FIXED1);
    assert_eq!(vmx_read_cr_fixed(&m), None);
}

#[test]
fn fixed_bits_adjust() {
    // already fine: unchanged
    assert_eq!(cr_fixed_adjust(CR0, CR0_FIXED0, CR0_FIXED1), CR0);
    // missing NE (bit 5): forced on
    let cr0_no_ne = CR0 & !X86_CR0_NE;
    assert_eq!(cr_fixed_adjust(cr0_no_ne, CR0_FIXED0, CR0_FIXED1), CR0);
    // CR4 without VMXE gets it (this is exactly what enabling VMX does)
    assert_eq!(cr_fixed_adjust(CR4_NO_VMXE, CR4_FIXED0, CR4_FIXED1), CR4);
    // bits fixed to 0 (0 in FIXED1) are cleared: bit 22 (PKE-ish) is above 0x3727ff
    assert_eq!(
        cr_fixed_adjust(CR4 | (1 << 23), CR4_FIXED0, CR4_FIXED1),
        CR4
    );
    // fixed0 and fixed1 interplay: bit 0 fixed1=0 wins over nothing else
    assert_eq!(cr_fixed_adjust(0xf, 0, 0b1010), 0b1010);
    assert_eq!(cr_fixed_adjust(0, 0b0101, u64::MAX), 0b0101);
}

#[test]
fn fixed_bits_check() {
    assert!(cr_fixed_ok(CR0, CR0_FIXED0, CR0_FIXED1));
    assert!(cr_fixed_ok(CR4, CR4_FIXED0, CR4_FIXED1));
    assert!(!cr_fixed_ok(CR0 & !X86_CR0_NE, CR0_FIXED0, CR0_FIXED1)); // fixed-1 bit clear
    assert!(!cr_fixed_ok(CR0 & !X86_CR0_PG, CR0_FIXED0, CR0_FIXED1));
    assert!(!cr_fixed_ok(CR4_NO_VMXE, CR4_FIXED0, CR4_FIXED1));
    assert!(!cr_fixed_ok(CR4 | (1 << 23), CR4_FIXED0, CR4_FIXED1)); // fixed-0 bit set
                                                                    // adjust always produces something ok
    for cr in [0u64, 1, 0xdead_beef, u64::MAX, CR0, CR4] {
        let a = cr_fixed_adjust(cr, CR4_FIXED0, CR4_FIXED1);
        assert!(cr_fixed_ok(a, CR4_FIXED0, CR4_FIXED1));
    }
}

#[test]
fn vmxon_region_layout() {
    let basic = vmx_basic_decode(BASIC);
    let mut page = [0xaau8; 4096];
    vmxon_region_init(&mut page, &basic).unwrap();
    assert_eq!(&page[..4], &4u32.to_le_bytes());
    assert!(page[4..].iter().all(|&b| b == 0), "region must be zeroed");
}

#[test]
fn vmxon_region_clears_bit31() {
    let mut basic = vmx_basic_decode(BASIC);
    basic.revision_id = 0x7fff_ffff;
    let mut page = [0u8; 4096];
    vmxon_region_init(&mut page, &basic).unwrap();
    assert_eq!(
        u32::from_le_bytes(page[..4].try_into().unwrap()),
        0x7fff_ffff
    );
}

#[test]
fn vmxon_region_too_small() {
    let basic = vmx_basic_decode(BASIC); // vmcs_size = 1024
    let mut small = [0u8; 512];
    assert!(vmxon_region_init(&mut small, &basic).is_err());
    let mut exact = [0u8; 1024];
    assert!(vmxon_region_init(&mut exact, &basic).is_ok());
}

#[test]
fn rflags_decoding() {
    assert_eq!(vmx_fail_kind(0x2), VmxResult::Succeed); // only the always-1 bit
    assert_eq!(vmx_fail_kind(0x246), VmxResult::FailValid); // ZF set
    assert_eq!(vmx_fail_kind(0x203), VmxResult::FailInvalid); // CF set
    assert_eq!(vmx_fail_kind(0x202), VmxResult::Succeed); // IF does not matter
    assert_eq!(
        vmx_fail_kind(X86_EFLAGS_CF | X86_EFLAGS_ZF),
        VmxResult::FailInvalid
    );
    assert_eq!(vmx_fail_kind(0), VmxResult::Succeed);
}

#[test]
fn vmxon_then_vmxoff() {
    let mut cpu = vmx_cpu_state::new();
    assert!(!cpu.in_vmx);
    assert_eq!(cpu.vmxon(&env(), REGION, 4), Ok(VmxResult::Succeed));
    assert!(cpu.in_vmx);
    assert_eq!(cpu.vmxon_ptr, REGION);
    assert_eq!(cpu.current_vmcs, VMCS_PTR_INVALID);
    assert_eq!(cpu.vmxoff(&env()), Ok(VmxResult::Succeed));
    assert!(!cpu.in_vmx);
    // and again
    assert_eq!(cpu.vmxon(&env(), REGION, 4), Ok(VmxResult::Succeed));
}

#[test]
fn vmxon_ud_without_cr4_vmxe() {
    let mut cpu = vmx_cpu_state::new();
    let e = vmxon_env {
        cr4: CR4_NO_VMXE,
        ..env()
    };
    assert_eq!(cpu.vmxon(&e, REGION, 4), Err(vmx_exception::UD));
    assert!(!cpu.in_vmx);
}

#[test]
fn vmxon_gp_cases() {
    let mut cpu = vmx_cpu_state::new();
    // CPL 3
    assert_eq!(
        cpu.vmxon(&vmxon_env { cpl: 3, ..env() }, REGION, 4),
        Err(vmx_exception::GP)
    );
    // CR0.NE clear (fixed-1 bit)
    assert_eq!(
        cpu.vmxon(
            &vmxon_env {
                cr0: CR0 & !X86_CR0_NE,
                ..env()
            },
            REGION,
            4
        ),
        Err(vmx_exception::GP)
    );
    // CR4 with a fixed-0 bit set
    assert_eq!(
        cpu.vmxon(
            &vmxon_env {
                cr4: CR4 | (1 << 23),
                ..env()
            },
            REGION,
            4
        ),
        Err(vmx_exception::GP)
    );
    // IA32_FEATURE_CONTROL: unlocked, or locked but VMXON-outside-SMX clear
    assert_eq!(
        cpu.vmxon(
            &vmxon_env {
                feature_control: 0x4,
                ..env()
            },
            REGION,
            4
        ),
        Err(vmx_exception::GP)
    );
    assert_eq!(
        cpu.vmxon(
            &vmxon_env {
                feature_control: 0x1,
                ..env()
            },
            REGION,
            4
        ),
        Err(vmx_exception::GP)
    );
    assert!(!cpu.in_vmx);
}

#[test]
fn vmxon_checks_come_in_sdm_order() {
    // CR4.VMXE clear beats everything: #UD even with CPL 3 and a bad pointer.
    let mut cpu = vmx_cpu_state::new();
    let e = vmxon_env {
        cr4: 0,
        cpl: 3,
        ..env()
    };
    assert_eq!(cpu.vmxon(&e, 0x123, 99), Err(vmx_exception::UD));
    // #GP beats a bad pointer.
    let e = vmxon_env { cpl: 3, ..env() };
    assert_eq!(cpu.vmxon(&e, 0x123, 99), Err(vmx_exception::GP));
}

#[test]
fn vmxon_bad_pointer_is_vmfailinvalid() {
    let mut cpu = vmx_cpu_state::new();
    assert_eq!(
        cpu.vmxon(&env(), REGION + 0x10, 4),
        Ok(VmxResult::FailInvalid)
    ); // unaligned
    assert_eq!(cpu.vmxon(&env(), 1 << 39, 4), Ok(VmxResult::FailInvalid)); // beyond phys width
    assert_eq!(
        cpu.vmxon(&env(), (1 << 39) - 0x1000, 4),
        Ok(VmxResult::Succeed)
    ); // last page
    let mut cpu = vmx_cpu_state::new();
    // IA32_VMX_BASIC bit 48: pointers must fit in 32 bits
    let mut e = env();
    e.basic.phys_addr_32bit = true;
    assert_eq!(cpu.vmxon(&e, 1 << 32, 4), Ok(VmxResult::FailInvalid));
    assert_eq!(cpu.vmxon(&e, 0xffff_f000, 4), Ok(VmxResult::Succeed));
    assert!(cpu.in_vmx);
}

#[test]
fn vmxon_wrong_revision_is_vmfailinvalid() {
    let mut cpu = vmx_cpu_state::new();
    assert_eq!(cpu.vmxon(&env(), REGION, 0), Ok(VmxResult::FailInvalid));
    assert_eq!(cpu.vmxon(&env(), REGION, 5), Ok(VmxResult::FailInvalid));
    assert_eq!(
        cpu.vmxon(&env(), REGION, 4 | 0x8000_0000),
        Ok(VmxResult::FailInvalid)
    );
    assert!(!cpu.in_vmx);
}

#[test]
fn double_vmxon_without_current_vmcs_is_vmfailinvalid() {
    let mut cpu = vmx_cpu_state::new();
    cpu.vmxon(&env(), REGION, 4).unwrap();
    assert_eq!(cpu.vmxon(&env(), REGION, 4), Ok(VmxResult::FailInvalid));
    assert_eq!(
        cpu.vm_instruction_error, 0,
        "no VMCS to hold an error number"
    );
    assert!(cpu.in_vmx);
}

#[test]
fn double_vmxon_with_current_vmcs_is_vmfailvalid() {
    let mut cpu = vmx_cpu_state::new();
    cpu.vmxon(&env(), REGION, 4).unwrap();
    cpu.current_vmcs = 0x8000; // as if VMPTRLD had run (stage 3)
    assert_eq!(cpu.vmxon(&env(), REGION, 4), Ok(VmxResult::FailValid));
    assert_eq!(cpu.vm_instruction_error, VMXERR_VMXON_IN_VMX_ROOT_OPERATION);
    assert_eq!(cpu.vm_instruction_error, 15);
}

#[test]
fn vmfail_helper() {
    let mut cpu = vmx_cpu_state::new();
    assert_eq!(cpu.vmfail(7), VmxResult::FailInvalid);
    assert_eq!(cpu.vm_instruction_error, 0);
    cpu.current_vmcs = 0x4000;
    assert_eq!(cpu.vmfail(7), VmxResult::FailValid);
    assert_eq!(cpu.vm_instruction_error, 7);
}

#[test]
fn vmxoff_without_vmxon_is_ud() {
    let mut cpu = vmx_cpu_state::new();
    assert_eq!(cpu.vmxoff(&env()), Err(vmx_exception::UD));
}

#[test]
fn vmxoff_ud_without_vmxe_gp_at_cpl3() {
    let mut cpu = vmx_cpu_state::new();
    cpu.vmxon(&env(), REGION, 4).unwrap();
    assert_eq!(
        cpu.vmxoff(&vmxon_env {
            cr4: CR4_NO_VMXE,
            ..env()
        }),
        Err(vmx_exception::UD)
    );
    assert_eq!(
        cpu.vmxoff(&vmxon_env { cpl: 3, ..env() }),
        Err(vmx_exception::GP)
    );
    assert!(cpu.in_vmx, "failed VMXOFF must not leave VMX operation");
}

#[test]
fn vmxoff_forgets_the_current_vmcs() {
    let mut cpu = vmx_cpu_state::new();
    cpu.vmxon(&env(), REGION, 4).unwrap();
    cpu.current_vmcs = 0x8000;
    cpu.vmxoff(&env()).unwrap();
    assert_eq!(cpu.current_vmcs, VMCS_PTR_INVALID);
    // second life: no stale current VMCS, so a failure is Invalid again
    cpu.vmxon(&env(), REGION, 4).unwrap();
    assert_eq!(cpu.vmxon(&env(), REGION, 4), Ok(VmxResult::FailInvalid));
}
