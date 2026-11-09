use ktvmx::mock::MockMsrs;
use ktvmx::vmx::*;
use ktvmx::vmx_caps::*;

// Values read from a Skylake-class client part (Core i7-6xxx). They are what a
// `-cpu host,vmx=on` guest sees too, because KVM passes most of them through.
const BASIC: u64 = 0x00da_0400_0000_0004; // rev 4, 1024 bytes, WB, true ctls
const PROCBASED: u64 = 0xfff9_fffe_0401_e172; // allowed-1 high dword has bit 31
const PROCBASED2: u64 = 0x0055_5cfe_0000_0000; // high dword: EPT (1), VPID (5), ...
const EPT_VPID: u64 = 0x0000_0f01_0673_4141;
const FC_LOCKED_ON: u64 = 0x5; // locked | VMXON outside SMX
const ECX_VMX: u32 = 1 << 5;

fn good_msrs() -> MockMsrs {
    MockMsrs::default()
        .with(MSR_IA32_FEATURE_CONTROL, FC_LOCKED_ON)
        .with(MSR_IA32_VMX_BASIC, BASIC)
        .with(MSR_IA32_VMX_PROCBASED_CTLS, PROCBASED)
        .with(MSR_IA32_VMX_PROCBASED_CTLS2, PROCBASED2)
        .with(MSR_IA32_VMX_EPT_VPID_CAP, EPT_VPID)
}

#[test]
fn cpuid_vmx_bit() {
    assert!(cpu_has_vmx(ECX_VMX));
    assert!(cpu_has_vmx(0xffff_ffff));
    assert!(!cpu_has_vmx(0));
    assert!(!cpu_has_vmx(!ECX_VMX));
}

#[test]
fn feature_control_states() {
    // 0: unlocked, nothing enabled: the OS may enable and lock it.
    assert_eq!(vmx_feat_ctl_state(0), feat_ctl_state::CanEnable(0x5));
    // enabled bits set but not locked: still ours to lock; keep what was there.
    assert_eq!(vmx_feat_ctl_state(0x6), feat_ctl_state::CanEnable(0x7));
    assert_eq!(vmx_feat_ctl_state(0x4), feat_ctl_state::CanEnable(0x5));
    // locked + outside SMX: ready (0x5), also with inside-SMX (0x7).
    assert_eq!(vmx_feat_ctl_state(0x5), feat_ctl_state::Enabled);
    assert_eq!(vmx_feat_ctl_state(0x7), feat_ctl_state::Enabled);
    // locked but VMXON-outside-SMX clear: the BIOS turned VMX off. 0x3 = locked + inside SMX only.
    assert_eq!(vmx_feat_ctl_state(0x1), feat_ctl_state::DisabledByBios);
    assert_eq!(vmx_feat_ctl_state(0x3), feat_ctl_state::DisabledByBios);
}

#[test]
fn disabled_by_bios_only_when_locked_off() {
    assert!(vmx_disabled_by_bios(0x1));
    assert!(vmx_disabled_by_bios(0x3));
    assert!(!vmx_disabled_by_bios(0x0)); // unlocked: not the BIOS's last word
    assert!(!vmx_disabled_by_bios(0x5));
    assert!(!vmx_disabled_by_bios(0x7));
}

#[test]
fn vmx_basic_skylake() {
    let b = vmx_basic_decode(BASIC);
    assert_eq!(b.revision_id, 4);
    assert_eq!(b.vmcs_size, 1024);
    assert!(!b.phys_addr_32bit);
    assert!(b.dual_monitor);
    assert_eq!(b.memtype, 6);
    assert!(b.ins_outs_info);
    assert!(b.true_controls);
}

#[test]
fn vmx_basic_fields_do_not_overlap() {
    // bit 31 is reserved: not part of the revision id.
    assert_eq!(vmx_basic_decode(0x8000_0000).revision_id, 0);
    assert_eq!(vmx_basic_decode(0x7fff_ffff).revision_id, 0x7fff_ffff);
    // a 13-bit size field: all ones is 8191, and nothing leaks into bits 48+.
    let b = vmx_basic_decode(0x1fff_0000_0000);
    assert_eq!(
        (b.vmcs_size, b.phys_addr_32bit, b.dual_monitor, b.memtype),
        (8191, false, false, 0)
    );
    // an old-style part: 32-bit addresses, uncacheable VMCS, no true controls.
    let b = vmx_basic_decode((1 << 48) | (0x400 << 32) | 1);
    assert!(b.phys_addr_32bit);
    assert_eq!((b.revision_id, b.vmcs_size, b.memtype), (1, 0x400, 0));
    assert!(!b.true_controls && !b.ins_outs_info && !b.dual_monitor);
}

#[test]
fn ept_vpid_skylake() {
    let c = vmx_ept_vpid_decode(EPT_VPID);
    assert!(c.exec_only && c.pwl4 && c.uc && c.wb);
    assert!(c.large_2m && c.large_1g && c.invept && c.ad && c.advanced_exit_info);
    assert!(c.invept_single && c.invept_all);
    assert!(c.invvpid && c.invvpid_addr && c.invvpid_single && c.invvpid_all);
    assert!(c.invvpid_single_nonglobal);
}

#[test]
fn ept_vpid_single_bits() {
    let none = ept_vpid_cap::default();
    assert_eq!(vmx_ept_vpid_decode(0), none);
    assert_eq!(
        vmx_ept_vpid_decode(1 << 6),
        ept_vpid_cap { pwl4: true, ..none }
    );
    assert_eq!(
        vmx_ept_vpid_decode(1 << 14),
        ept_vpid_cap { wb: true, ..none }
    );
    assert_eq!(
        vmx_ept_vpid_decode(1 << 21),
        ept_vpid_cap { ad: true, ..none }
    );
    assert_eq!(
        vmx_ept_vpid_decode(1 << 26),
        ept_vpid_cap {
            invept_all: true,
            ..none
        }
    );
    assert_eq!(
        vmx_ept_vpid_decode(1 << 32),
        ept_vpid_cap {
            invvpid: true,
            ..none
        }
    );
    assert_eq!(
        vmx_ept_vpid_decode(1 << 43),
        ept_vpid_cap {
            invvpid_single_nonglobal: true,
            ..none
        }
    );
}

#[test]
fn ept_vpid_older_part_without_1g_or_ad() {
    // e.g. a Nehalem/Westmere-like value: 4-level, WB, 2M, INVEPT, no A/D, no 1G.
    let raw = (1u64 << 6) | (1 << 14) | (1 << 16) | (1 << 20) | (1 << 25) | (1 << 26);
    let c = vmx_ept_vpid_decode(raw);
    assert!(c.pwl4 && c.wb && c.large_2m);
    assert!(!c.large_1g && !c.ad);
}

#[test]
fn probe_happy_path() {
    let caps = vmx_probe(&good_msrs(), ECX_VMX).expect("probe");
    assert_eq!(caps.basic, vmx_basic_decode(BASIC));
    assert_eq!(caps.ept_vpid, Some(vmx_ept_vpid_decode(EPT_VPID)));
    assert_eq!(caps.feat_ctl_write, None);
}

#[test]
fn probe_no_vmx_is_checked_first() {
    // even with no MSRs at all, NoVmx wins: we must not touch MSRs of a CPU without VMX.
    assert_eq!(
        vmx_probe(&MockMsrs::default(), 0),
        Err(vmx_probe_error::NoVmx)
    );
}

#[test]
fn probe_disabled_by_bios() {
    let m = good_msrs().with(MSR_IA32_FEATURE_CONTROL, 0x1);
    assert_eq!(vmx_probe(&m, ECX_VMX), Err(vmx_probe_error::DisabledByBios));
    // checked before IA32_VMX_BASIC is read:
    let m = MockMsrs::default().with(MSR_IA32_FEATURE_CONTROL, 0x1);
    assert_eq!(vmx_probe(&m, ECX_VMX), Err(vmx_probe_error::DisabledByBios));
}

#[test]
fn probe_unlocked_feature_control_asks_for_a_write() {
    let m = good_msrs().with(MSR_IA32_FEATURE_CONTROL, 0);
    let caps = vmx_probe(&m, ECX_VMX).expect("probe");
    assert_eq!(
        caps.feat_ctl_write,
        Some(FEAT_CTL_LOCKED | FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX)
    );
}

#[test]
fn probe_missing_msrs() {
    let mut m = good_msrs();
    m.0.remove(&MSR_IA32_FEATURE_CONTROL);
    assert_eq!(
        vmx_probe(&m, ECX_VMX),
        Err(vmx_probe_error::MsrMissing(MSR_IA32_FEATURE_CONTROL))
    );

    let mut m = good_msrs();
    m.0.remove(&MSR_IA32_VMX_BASIC);
    assert_eq!(
        vmx_probe(&m, ECX_VMX),
        Err(vmx_probe_error::MsrMissing(MSR_IA32_VMX_BASIC))
    );

    // EPT is advertised in the secondary controls, so its capability MSR must exist.
    let mut m = good_msrs();
    m.0.remove(&MSR_IA32_VMX_EPT_VPID_CAP);
    assert_eq!(
        vmx_probe(&m, ECX_VMX),
        Err(vmx_probe_error::MsrMissing(MSR_IA32_VMX_EPT_VPID_CAP))
    );
}

#[test]
fn probe_without_secondary_controls_never_reads_them() {
    // allowed-1 bit 31 clear -> no secondary controls -> 0x48b/0x48c do not exist
    // (and a real rdmsr of them would #GP). Do not even look.
    let mut m = good_msrs().with(MSR_IA32_VMX_PROCBASED_CTLS, PROCBASED & !(1 << 63));
    m.0.remove(&MSR_IA32_VMX_PROCBASED_CTLS2);
    m.0.remove(&MSR_IA32_VMX_EPT_VPID_CAP);
    let caps = vmx_probe(&m, ECX_VMX).expect("probe");
    assert_eq!(caps.ept_vpid, None);
}

#[test]
fn probe_secondary_controls_without_ept_or_vpid() {
    // secondary controls exist but allow neither EPT (bit 1) nor VPID (bit 5).
    let sec = 0x0000_0001u64 << 32; // only "virtualize APIC accesses"
    let mut m = good_msrs().with(MSR_IA32_VMX_PROCBASED_CTLS2, sec);
    m.0.remove(&MSR_IA32_VMX_EPT_VPID_CAP);
    assert_eq!(vmx_probe(&m, ECX_VMX).expect("probe").ept_vpid, None);
}

#[test]
fn probe_vpid_only_still_reads_the_cap_msr() {
    let sec = (SECONDARY_EXEC_ENABLE_VPID as u64) << 32;
    let m = good_msrs().with(MSR_IA32_VMX_PROCBASED_CTLS2, sec);
    assert!(vmx_probe(&m, ECX_VMX).expect("probe").ept_vpid.is_some());
}
