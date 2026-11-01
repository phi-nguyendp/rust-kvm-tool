use ktvmx::mock::MockVmcs;
use ktvmx::vmcs_ops::VmcsOps;
use ktvmx::vmx::*;
use ktvmx::vmx_bitmaps::*;

fn zero() -> [u8; 4096] {
    [0; 4096]
}
fn ones() -> [u8; 4096] {
    [0xff; 4096]
}

// ---- I/O bitmaps ------------------------------------------------------------------

#[test]
fn io_bitmap_low_ports_go_to_a() {
    let (mut a, mut b) = (zero(), zero());
    io_bitmap_set(&mut a, &mut b, 0x3f8, 1, true);
    // 0x3f8 = byte 0x7f, bit 0
    assert_eq!(a[0x7f], 0x01);
    assert!(b.iter().all(|&x| x == 0), "bitmap B untouched");
    io_bitmap_set(&mut a, &mut b, 0x3f8, 1, false);
    assert!(a.iter().all(|&x| x == 0), "clearing restores bitmap A");
}

#[test]
fn io_bitmap_high_ports_go_to_b() {
    let (mut a, mut b) = (zero(), zero());
    io_bitmap_set(&mut a, &mut b, 0x8000, 1, true);
    assert_eq!(b[0], 0x01);
    io_bitmap_set(&mut a, &mut b, 0xcf8, 1, true); // PCI config: still bitmap A
    assert_eq!(a[0xcf8 / 8], 1 << (0xcf8 % 8));
    io_bitmap_set(&mut a, &mut b, 0xfffe, 1, true);
    assert_eq!(b[0x7ffe / 8], 1 << (0x7ffe % 8));
    assert!(a.iter().filter(|&&x| x != 0).count() == 1);
}

#[test]
fn io_bitmap_multi_byte_access_covers_every_port() {
    let (mut a, mut b) = (zero(), zero());
    io_bitmap_set(&mut a, &mut b, 0x3f8, 4, true); // ports 3f8..3fb
    assert_eq!(a[0x7f], 0x0f);
    for p in 0x3f8..0x3fc {
        assert!(io_bitmap_intercepted(&a, &b, p, 1));
    }
    assert!(!io_bitmap_intercepted(&a, &b, 0x3fc, 1));
    // a 2-byte access at 0x3fb touches 0x3fb (intercepted) and 0x3fc (not) => exits
    assert!(io_bitmap_intercepted(&a, &b, 0x3fb, 2));
    // a 2-byte access at 0x3fc touches 3fc, 3fd => no exit
    assert!(!io_bitmap_intercepted(&a, &b, 0x3fc, 2));
}

#[test]
fn io_bitmap_access_straddling_a_and_b() {
    let (mut a, mut b) = (zero(), zero());
    io_bitmap_set(&mut a, &mut b, 0x7fff, 2, true); // 0x7fff in A, 0x8000 in B
    assert_eq!(a[0xfff], 0x80);
    assert_eq!(b[0], 0x01);
    assert!(io_bitmap_intercepted(&a, &b, 0x7ffe, 2));
    assert!(io_bitmap_intercepted(&a, &b, 0x8000, 1));
    assert!(!io_bitmap_intercepted(&a, &b, 0x7ffd, 2));
}

#[test]
fn io_access_beyond_ffff_always_exits() {
    let (a, b) = (zero(), zero());
    assert!(!io_bitmap_intercepted(&a, &b, 0xfffe, 2));
    assert!(io_bitmap_intercepted(&a, &b, 0xffff, 2), "touches port 0x10000");
    assert!(io_bitmap_intercepted(&a, &b, 0xfffd, 4));
    assert!(!io_bitmap_intercepted(&a, &b, 0xfffc, 4));
}

#[test]
fn io_bitmap_set_ignores_ports_past_ffff() {
    let (mut a, mut b) = (zero(), zero());
    io_bitmap_set(&mut a, &mut b, 0xffff, 4, true); // only port 0xffff is real
    assert_eq!(b[0xfff], 0x80);
    assert!(a.iter().all(|&x| x == 0));
    assert_eq!(b.iter().filter(|&&x| x != 0).count(), 1);
}

// ---- MSR bitmap -------------------------------------------------------------------

#[test]
fn msr_bitmap_positions() {
    assert_eq!(msr_bitmap_pos(0x0, false), Some((0x000, 0)));
    assert_eq!(msr_bitmap_pos(0x174, false), Some((0x000 + 0x174 / 8, 4)));
    assert_eq!(msr_bitmap_pos(0x174, true), Some((0x800 + 0x174 / 8, 4)));
    assert_eq!(msr_bitmap_pos(0x1fff, true), Some((0x800 + 0x3ff, 7)));
    assert_eq!(msr_bitmap_pos(MSR_EFER, false), Some((0x400 + 0x80 / 8, 0)));
    assert_eq!(msr_bitmap_pos(MSR_EFER, true), Some((0xc00 + 0x80 / 8, 0)));
    assert_eq!(msr_bitmap_pos(0xc000_1fff, true), Some((0xc00 + 0x3ff, 7)));
}

#[test]
fn msr_outside_the_ranges_has_no_bit() {
    assert_eq!(msr_bitmap_pos(0x2000, false), None);
    assert_eq!(msr_bitmap_pos(0x4000_0000, true), None); // Hyper-V/KVM paravirt MSRs
    assert_eq!(msr_bitmap_pos(0xbfff_ffff, false), None);
    assert_eq!(msr_bitmap_pos(0xc000_2000, false), None);
}

#[test]
fn fresh_msr_bitmap_intercepts_everything() {
    let mut bm = zero();
    msr_bitmap_init_all_intercept(&mut bm);
    assert!(bm.iter().all(|&x| x == 0xff));
    for msr in [0x10, MSR_IA32_SYSENTER_CS, MSR_EFER, 0x4000_0001, 0xdead_beef] {
        assert!(msr_bitmap_intercepted(&bm, msr, false), "{msr:#x} read");
        assert!(msr_bitmap_intercepted(&bm, msr, true), "{msr:#x} write");
    }
}

#[test]
fn disable_intercept_by_direction() {
    let mut bm = ones();
    vmx_disable_intercept_for_msr(&mut bm, MSR_IA32_SYSENTER_CS, MSR_TYPE_R);
    assert!(!msr_bitmap_intercepted(&bm, MSR_IA32_SYSENTER_CS, false));
    assert!(msr_bitmap_intercepted(&bm, MSR_IA32_SYSENTER_CS, true), "write still exits");
    vmx_disable_intercept_for_msr(&mut bm, MSR_FS_BASE, MSR_TYPE_RW);
    assert!(!msr_bitmap_intercepted(&bm, MSR_FS_BASE, false));
    assert!(!msr_bitmap_intercepted(&bm, MSR_FS_BASE, true));
    // neighbours unaffected
    assert!(msr_bitmap_intercepted(&bm, MSR_GS_BASE, false));
    assert!(msr_bitmap_intercepted(&bm, MSR_FS_BASE - 1, true));
    // exactly three bits cleared in total
    let cleared: u32 = bm.iter().map(|b| b.count_zeros()).sum();
    assert_eq!(cleared, 3);
}

#[test]
fn enable_intercept_undoes_disable() {
    let mut bm = ones();
    vmx_disable_intercept_for_msr(&mut bm, 0x1b, MSR_TYPE_RW);
    vmx_enable_intercept_for_msr(&mut bm, 0x1b, MSR_TYPE_W);
    assert!(!msr_bitmap_intercepted(&bm, 0x1b, false));
    assert!(msr_bitmap_intercepted(&bm, 0x1b, true));
    vmx_enable_intercept_for_msr(&mut bm, 0x1b, MSR_TYPE_R);
    assert!(bm.iter().all(|&x| x == 0xff));
}

#[test]
fn msr_disable_out_of_range_is_a_noop() {
    let mut bm = ones();
    vmx_disable_intercept_for_msr(&mut bm, 0x4000_0000, MSR_TYPE_RW);
    vmx_disable_intercept_for_msr(&mut bm, 0xc000_2000, MSR_TYPE_RW);
    assert!(bm.iter().all(|&x| x == 0xff));
    assert!(msr_bitmap_intercepted(&bm, 0x4000_0000, false));
    // and enabling on an all-zero bitmap does not write out of range either
    let mut z = zero();
    vmx_enable_intercept_for_msr(&mut z, 0x2000, MSR_TYPE_RW);
    assert!(z.iter().all(|&x| x == 0));
}

#[test]
fn high_msr_quarters_do_not_alias_low_ones() {
    let mut bm = ones();
    vmx_disable_intercept_for_msr(&mut bm, 0xc000_0080, MSR_TYPE_RW); // EFER
    assert!(!msr_bitmap_intercepted(&bm, 0xc000_0080, false));
    assert!(!msr_bitmap_intercepted(&bm, 0xc000_0080, true));
    assert!(msr_bitmap_intercepted(&bm, 0x80, false), "MSR 0x80 is a different bit");
    assert!(msr_bitmap_intercepted(&bm, 0x80, true));
    assert_eq!(bm[0x400 + 0x10], 0xfe);
    assert_eq!(bm[0xc00 + 0x10], 0xfe);
}

// ---- CR access --------------------------------------------------------------------

fn qual(cr: u8, ty: u8, reg: u8, lmsw_mem: bool, lmsw: u16) -> u64 {
    cr as u64 | (ty as u64) << 4 | (lmsw_mem as u64) << 6 | (reg as u64) << 8 | (lmsw as u64) << 16
}

#[test]
fn decode_mov_to_cr4_from_rcx() {
    let c = vmx_decode_cr_access(qual(4, CR_ACCESS_MOV_TO_CR, 1, false, 0));
    assert_eq!(c.cr, 4);
    assert_eq!(c.access_type, CR_ACCESS_MOV_TO_CR);
    assert_eq!(c.gpr, 1);
}

#[test]
fn decode_mov_from_cr3_into_r15() {
    let c = vmx_decode_cr_access(qual(3, CR_ACCESS_MOV_FROM_CR, 15, false, 0));
    assert_eq!((c.cr, c.access_type, c.gpr), (3, CR_ACCESS_MOV_FROM_CR, 15));
}

#[test]
fn decode_clts_and_lmsw() {
    let c = vmx_decode_cr_access(qual(0, CR_ACCESS_CLTS, 0, false, 0));
    assert_eq!((c.cr, c.access_type), (0, CR_ACCESS_CLTS));
    let l = vmx_decode_cr_access(qual(0, CR_ACCESS_LMSW, 0, true, 0xbeef));
    assert_eq!(l.access_type, CR_ACCESS_LMSW);
    assert!(l.lmsw_mem);
    assert_eq!(l.lmsw_data, 0xbeef);
    let r = vmx_decode_cr_access(qual(0, CR_ACCESS_LMSW, 3, false, 0x0001));
    assert!(!r.lmsw_mem);
    assert_eq!(r.lmsw_data, 1);
}

const FIXED: vmx_cr_fixed = vmx_cr_fixed {
    // like a CPU with unrestricted guest: only NE is forced in CR0; VMXE forced in CR4
    cr0_fixed0: X86_CR0_NE,
    cr0_fixed1: 0xffff_ffff,
    cr4_fixed0: X86_CR4_VMXE,
    cr4_fixed1: 0x0027_77ff | X86_CR4_VMXE,
};

#[test]
fn vmx_set_cr0_splits_shadow_and_hardware() {
    let mut v = MockVmcs::default();
    vmx_set_cr0(&mut v, X86_CR0_PE | X86_CR0_ET, &FIXED);
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_PE | X86_CR0_ET);
    assert_eq!(v.vmcs_readl(GUEST_CR0), X86_CR0_PE | X86_CR0_ET | X86_CR0_NE);
    // the VMM owns the bit it forces: a guest clearing NE must exit
    assert_ne!(v.vmcs_readl(CR0_GUEST_HOST_MASK) & X86_CR0_NE, 0);
    // the guest owns PE (unrestricted guest): no exit when it changes
    assert_eq!(v.vmcs_readl(CR0_GUEST_HOST_MASK) & X86_CR0_PE, 0);
}

#[test]
fn vmx_set_cr4_hides_vmxe_from_the_guest() {
    let mut v = MockVmcs::default();
    vmx_set_cr4(&mut v, X86_CR4_PAE, &FIXED);
    assert_eq!(v.vmcs_readl(CR4_READ_SHADOW), X86_CR4_PAE);
    assert_eq!(v.vmcs_readl(GUEST_CR4), X86_CR4_PAE | X86_CR4_VMXE);
    assert_ne!(v.vmcs_readl(CR4_GUEST_HOST_MASK) & X86_CR4_VMXE, 0);
    // the guest's view of CR4 has no VMXE
    let view = vmx_guest_cr_view(
        v.vmcs_readl(GUEST_CR4),
        v.vmcs_readl(CR4_READ_SHADOW),
        v.vmcs_readl(CR4_GUEST_HOST_MASK),
    );
    assert_eq!(view, X86_CR4_PAE);
}

#[test]
fn guest_view_mixes_shadow_and_real_bits() {
    // host owns bit 5 (value 0x20): shadow says 0, hardware says 1; guest owns bit 0
    assert_eq!(vmx_guest_cr_view(0b100001, 0b000000, 0b100000), 0b000001);
    assert_eq!(vmx_guest_cr_view(0b000000, 0b100000, 0b100000), 0b100000);
    assert_eq!(vmx_guest_cr_view(0xdead, 0xbeef, 0), 0xdead);
    assert_eq!(vmx_guest_cr_view(0xdead, 0xbeef, u64::MAX), 0xbeef);
}

#[test]
fn cr_write_exit_rule() {
    let mask = X86_CR0_NE | X86_CR0_PG;
    let shadow = X86_CR0_PE | X86_CR0_NE;
    // changes only a guest-owned bit (PE): no exit
    assert!(!cr_write_causes_exit(mask, shadow, X86_CR0_NE));
    // changes a host-owned bit relative to the shadow: exit
    assert!(cr_write_causes_exit(mask, shadow, X86_CR0_PE | X86_CR0_NE | X86_CR0_PG));
    assert!(cr_write_causes_exit(mask, shadow, X86_CR0_PE));
    // writing the shadow value back never exits
    assert!(!cr_write_causes_exit(mask, shadow, shadow));
    assert!(!cr_write_causes_exit(0, shadow, 0));
}

#[test]
fn lmsw_can_set_but_not_clear_pe() {
    assert_eq!(lmsw_result(0, 1), 1);
    assert_eq!(lmsw_result(X86_CR0_PE, 0), X86_CR0_PE, "PE sticks");
    // only low 4 bits are loaded, upper bits kept
    assert_eq!(lmsw_result(X86_CR0_PG | X86_CR0_PE | 0xe, 0xfff0 | 0x2), X86_CR0_PG | X86_CR0_PE | 0x2);
    assert_eq!(lmsw_result(X86_CR0_PG, 0x0008), X86_CR0_PG | 0x8);
}

fn vcpu_with(cr0: u64, cr4: u64) -> MockVmcs {
    let mut v = MockVmcs::default();
    vmx_set_cr0(&mut v, cr0, &FIXED);
    vmx_set_cr4(&mut v, cr4, &FIXED);
    v
}

#[test]
fn handle_mov_to_cr0() {
    let mut v = vcpu_with(0, 0);
    let mut g = [0u64; 16];
    g[2] = X86_CR0_PE | X86_CR0_ET | X86_CR0_PG; // RDX
    handle_cr_access(&mut v, &mut g, qual(0, CR_ACCESS_MOV_TO_CR, 2, false, 0), &FIXED).unwrap();
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), g[2]);
    assert_eq!(v.vmcs_readl(GUEST_CR0), g[2] | X86_CR0_NE);
}

#[test]
fn handle_mov_to_cr3_and_back() {
    let mut v = vcpu_with(X86_CR0_PE, 0);
    let mut g = [0u64; 16];
    g[3] = 0x1234_5000; // RBX
    handle_cr_access(&mut v, &mut g, qual(3, CR_ACCESS_MOV_TO_CR, 3, false, 0), &FIXED).unwrap();
    assert_eq!(v.vmcs_readl(GUEST_CR3), 0x1234_5000);
    handle_cr_access(&mut v, &mut g, qual(3, CR_ACCESS_MOV_FROM_CR, 8, false, 0), &FIXED).unwrap();
    assert_eq!(g[8], 0x1234_5000);
}

#[test]
fn handle_mov_from_cr4_hides_vmxe() {
    let mut v = vcpu_with(X86_CR0_PE, X86_CR4_PAE);
    let mut g = [0xffu64; 16];
    handle_cr_access(&mut v, &mut g, qual(4, CR_ACCESS_MOV_FROM_CR, 0, false, 0), &FIXED).unwrap();
    assert_eq!(g[0], X86_CR4_PAE);
}

#[test]
fn handle_mov_to_cr_from_rsp_uses_guest_rsp() {
    let mut v = vcpu_with(0, 0);
    v.vmcs_writel(GUEST_RSP, X86_CR0_PE);
    let mut g = [0u64; 16];
    g[VCPU_REGS_RSP] = 0xbad; // stale: must not be used
    handle_cr_access(&mut v, &mut g, qual(0, CR_ACCESS_MOV_TO_CR, 4, false, 0), &FIXED).unwrap();
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_PE);
}

#[test]
fn handle_clts_and_lmsw() {
    let mut v = vcpu_with(X86_CR0_PE | 0x8, 0); // TS set
    let mut g = [0u64; 16];
    handle_cr_access(&mut v, &mut g, qual(0, CR_ACCESS_CLTS, 0, false, 0), &FIXED).unwrap();
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW) & 0x8, 0, "TS cleared");
    assert_eq!(v.vmcs_readl(GUEST_CR0) & 0x8, 0);
    assert_ne!(v.vmcs_readl(GUEST_CR0) & X86_CR0_PE, 0);
    handle_cr_access(&mut v, &mut g, qual(0, CR_ACCESS_LMSW, 0, false, 0x0008), &FIXED).unwrap();
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_PE | 0x8);
}

#[test]
fn handle_cr8_is_unsupported() {
    let mut v = vcpu_with(0, 0);
    let mut g = [0u64; 16];
    assert!(handle_cr_access(&mut v, &mut g, qual(8, CR_ACCESS_MOV_TO_CR, 0, false, 0), &FIXED).is_err());
}

// ---- VPID -------------------------------------------------------------------------

#[test]
fn vpid_zero_is_reserved() {
    let mut a = vpid_allocator::new();
    assert!(a.vpid_in_use(0));
    assert_eq!(a.vpid_alloc(), Some(1));
    assert_eq!(a.vpid_alloc(), Some(2));
    a.vpid_free(0);
    assert!(a.vpid_in_use(0), "freeing 0 is a no-op");
    assert_eq!(a.vpid_alloc(), Some(3));
}

#[test]
fn vpid_free_reuses_lowest() {
    let mut a = vpid_allocator::new();
    let v: Vec<u16> = (0..5).map(|_| a.vpid_alloc().unwrap()).collect();
    assert_eq!(v, [1, 2, 3, 4, 5]);
    a.vpid_free(2);
    a.vpid_free(4);
    assert_eq!(a.vpid_alloc(), Some(2));
    assert_eq!(a.vpid_alloc(), Some(4));
    assert_eq!(a.vpid_alloc(), Some(6));
    // double free is harmless
    a.vpid_free(3);
    a.vpid_free(3);
    assert_eq!(a.vpid_alloc(), Some(3));
    assert_eq!(a.vpid_alloc(), Some(7));
}

#[test]
fn vpid_space_is_65535_ids() {
    let mut a = vpid_allocator::new();
    for want in 1..=65535u32 {
        assert_eq!(a.vpid_alloc(), Some(want as u16));
    }
    assert_eq!(a.vpid_alloc(), None);
    a.vpid_free(65535);
    assert_eq!(a.vpid_alloc(), Some(65535));
    assert_eq!(a.vpid_alloc(), None);
}

#[test]
fn invvpid_descriptor_layout() {
    assert_eq!(invvpid_descriptor(0x1234, 0xffff_8000_dead_b000), [0x1234, 0xffff_8000_dead_b000]);
    assert_eq!(invvpid_descriptor(0, 0), [0, 0]);
    assert_eq!(invvpid_descriptor(0xffff, 0)[0], 0xffff, "upper 48 bits stay zero");
}

#[test]
fn invvpid_capability_bits() {
    let none = 0u64;
    for t in 0..4 {
        assert!(!invvpid_type_supported(none, t));
    }
    // INVVPID bit alone, with no extent bits: nothing is supported
    assert!(!invvpid_type_supported(1 << 32, VMX_VPID_EXTENT_SINGLE_CONTEXT));
    let cap = (1u64 << 32) | (1 << 41) | (1 << 42);
    assert!(!invvpid_type_supported(cap, VMX_VPID_EXTENT_INDIVIDUAL_ADDR));
    assert!(invvpid_type_supported(cap, VMX_VPID_EXTENT_SINGLE_CONTEXT));
    assert!(invvpid_type_supported(cap, VMX_VPID_EXTENT_ALL_CONTEXT));
    assert!(!invvpid_type_supported(cap, VMX_VPID_EXTENT_SINGLE_NON_GLOBAL));
    // extent bits without bit 32 do not count either
    assert!(!invvpid_type_supported((1 << 41) | (1 << 42), VMX_VPID_EXTENT_ALL_CONTEXT));
    // unknown types are never supported
    assert!(!invvpid_type_supported(u64::MAX, 4));
}
