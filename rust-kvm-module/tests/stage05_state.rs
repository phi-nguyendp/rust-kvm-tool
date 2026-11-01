use ktvmx::mock::*;
use ktvmx::vmcs_ops::*;
use ktvmx::vmx::*;
use ktvmx::vmx_controls::vmcs_config;
use ktvmx::vmx_state::*;

fn cfg() -> vmcs_config {
    vmcs_config {
        cr0_fixed0: 0x8000_0021,
        cr0_fixed1: 0xffff_ffff,
        cr4_fixed0: 0x2000,
        cr4_fixed1: 0x0037_67ff,
        ..Default::default()
    }
}

/// Controls as stage 4 would have left them: secondary controls active, EFER loaded on
/// entry. `unrestricted` turns on EPT + unrestricted guest.
fn controls(v: &mut MockVmcs, unrestricted: bool) {
    v.vmcs_write32(CPU_BASED_VM_EXEC_CONTROL, CPU_BASED_ACTIVATE_SECONDARY_CONTROLS);
    let sec = if unrestricted { SECONDARY_EXEC_ENABLE_EPT | SECONDARY_EXEC_UNRESTRICTED_GUEST } else { 0 };
    v.vmcs_write32(SECONDARY_VM_EXEC_CONTROL, sec);
    v.vmcs_write32(VM_ENTRY_CONTROLS, VM_ENTRY_LOAD_IA32_EFER);
}

fn long_vmcs(unrestricted: bool) -> MockVmcs {
    let mut v = MockVmcs::default();
    controls(&mut v, unrestricted);
    vmx_setup_guest_long_mode(&mut v, 0x1000, 0x8000, 0x5000);
    v
}
fn prot_vmcs() -> MockVmcs {
    let mut v = MockVmcs::default();
    controls(&mut v, true);
    vmx_setup_guest_protected_mode(&mut v, 0x1000, 0x8000);
    v
}
fn real_vmcs() -> MockVmcs {
    let mut v = MockVmcs::default();
    controls(&mut v, true);
    vmx_setup_guest_real_mode(&mut v, 0x1000, 0x8000);
    v
}
fn ar(v: &MockVmcs, seg: usize) -> u32 {
    v.vmcs_read32(vmx_segment_fields(seg).ar_bytes)
}
fn set_ar(v: &mut MockVmcs, seg: usize, val: u32) {
    v.vmcs_write32(vmx_segment_fields(seg).ar_bytes, val)
}
fn mod_ar(v: &mut MockVmcs, seg: usize, f: impl Fn(u32) -> u32) {
    let a = ar(v, seg);
    set_ar(v, seg, f(a))
}
fn check(v: &MockVmcs) -> Result<(), &'static str> {
    vmx_check_guest_state(v, &cfg())
}

// ---- segments ------------------------------------------------------------------------

#[test]
fn access_rights_of_classic_descriptors() {
    let code64 = kvm_segment { type_: 0xb, s: true, present: true, l: true, g: true, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&code64), 0xa09b);
    let data32 = kvm_segment { type_: 3, s: true, present: true, db: true, g: true, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&data32), 0xc093);
    let ring3 = kvm_segment { type_: 3, s: true, present: true, dpl: 3, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&ring3), 0xf3);
    let tss = kvm_segment { type_: 11, present: true, avl: true, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&tss), 0x108b);
}

#[test]
fn unusable_or_absent_is_just_bit_16() {
    let junk = kvm_segment { type_: 0xb, s: true, present: true, dpl: 3, g: true, unusable: true, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&junk), 1 << 16);
    let absent = kvm_segment { type_: 0xb, s: true, present: false, g: true, ..Default::default() };
    assert_eq!(vmx_segment_access_rights(&absent), 1 << 16);
}

#[test]
fn set_and_get_segment_round_trip() {
    let mut v = MockVmcs::default();
    let fs = kvm_segment { base: 0xffff_8880_1234_0000, limit: 0xffff_ffff, selector: 0x2b, type_: 3,
                           present: true, dpl: 3, db: true, s: true, g: true, avl: true, ..Default::default() };
    vmx_set_segment(&mut v, VCPU_SREG_FS, &fs);
    assert_eq!(v.vmcs_readl(GUEST_FS_BASE), fs.base);
    assert_eq!(v.vmcs_read32(GUEST_FS_LIMIT), fs.limit);
    assert_eq!(v.vmcs_read16(GUEST_FS_SELECTOR), 0x2b);
    assert_eq!(v.vmcs_read32(GUEST_FS_AR_BYTES), 0xd0f3);
    assert_eq!(vmx_get_segment(&v, VCPU_SREG_FS), fs);
    // other registers untouched
    assert!(!v.fields.contains_key(&GUEST_GS_BASE));
}

#[test]
fn every_register_maps_to_its_own_fields() {
    let mut v = MockVmcs::default();
    for (i, seg) in [VCPU_SREG_ES, VCPU_SREG_CS, VCPU_SREG_SS, VCPU_SREG_DS, VCPU_SREG_FS,
                     VCPU_SREG_GS, VCPU_SREG_TR, VCPU_SREG_LDTR].into_iter().enumerate() {
        let s = kvm_segment { base: 0x1000 * (i as u64 + 1), limit: 0xff + i as u32, selector: 8 * (i as u16 + 1),
                              type_: 3, present: true, s: true, ..Default::default() };
        vmx_set_segment(&mut v, seg, &s);
    }
    for (i, seg) in [VCPU_SREG_ES, VCPU_SREG_CS, VCPU_SREG_SS, VCPU_SREG_DS, VCPU_SREG_FS,
                     VCPU_SREG_GS, VCPU_SREG_TR, VCPU_SREG_LDTR].into_iter().enumerate() {
        let g = vmx_get_segment(&v, seg);
        assert_eq!((g.base, g.limit, g.selector), (0x1000 * (i as u64 + 1), 0xff + i as u32, 8 * (i as u16 + 1)));
    }
}

#[test]
fn get_segment_decodes_unusable() {
    let mut v = MockVmcs::default();
    set_ar(&mut v, VCPU_SREG_LDTR, 1 << 16);
    let l = vmx_get_segment(&v, VCPU_SREG_LDTR);
    assert!(l.unusable);
    assert!(!l.present);
}

// ---- guest-mode builders ------------------------------------------------------------------

#[test]
fn common_state() {
    let v = real_vmcs();
    assert_eq!(v.vmcs_read64(VMCS_LINK_POINTER), !0u64);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    assert_eq!(v.vmcs_readl(GUEST_RSP), 0x8000);
    assert_eq!(v.vmcs_readl(GUEST_RFLAGS), 2);
    assert_eq!(v.vmcs_readl(GUEST_DR7), 0x400);
    assert_eq!(v.vmcs_read32(GUEST_GDTR_LIMIT), 0xffff);
    assert_eq!(v.vmcs_read32(GUEST_IDTR_LIMIT), 0xffff);
    assert_eq!(ar(&v, VCPU_SREG_LDTR), 1 << 16, "LDTR unusable");
    assert_eq!(ar(&v, VCPU_SREG_TR), 0x8b, "TR: busy TSS, present");
    assert_eq!(v.vmcs_read32(vmx_segment_fields(VCPU_SREG_TR).limit), 0xffff);
}

#[test]
fn real_mode_state() {
    let mut v = MockVmcs::default();
    controls(&mut v, true);
    v.vmcs_write32(VM_ENTRY_CONTROLS, VM_ENTRY_LOAD_IA32_EFER | VM_ENTRY_IA32E_MODE);
    vmx_setup_guest_real_mode(&mut v, 0x7c00, 0x7000);
    assert_eq!(v.vmcs_readl(GUEST_CR0), X86_CR0_NE | X86_CR0_ET);
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_ET);
    assert_eq!(v.vmcs_readl(CR0_GUEST_HOST_MASK), X86_CR0_NE);
    assert_eq!(v.vmcs_readl(GUEST_CR4), X86_CR4_VMXE);
    assert_eq!(v.vmcs_readl(CR4_READ_SHADOW), 0);
    assert_eq!(v.vmcs_readl(CR4_GUEST_HOST_MASK), X86_CR4_VMXE);
    assert_eq!(v.vmcs_read64(GUEST_IA32_EFER), 0);
    assert_eq!(v.vmcs_read32(VM_ENTRY_CONTROLS), VM_ENTRY_LOAD_IA32_EFER, "IA-32e bit cleared, rest kept");
    assert_eq!(ar(&v, VCPU_SREG_CS), 0x9b);
    assert_eq!(v.vmcs_read32(GUEST_CS_LIMIT), 0xffff);
    for seg in [VCPU_SREG_ES, VCPU_SREG_SS, VCPU_SREG_DS, VCPU_SREG_FS, VCPU_SREG_GS] {
        assert_eq!(ar(&v, seg), 0x93);
        assert_eq!(vmx_get_segment(&v, seg).base, 0);
    }
}

#[test]
fn protected_mode_state() {
    let v = prot_vmcs();
    assert_eq!(v.vmcs_readl(GUEST_CR0), X86_CR0_PE | X86_CR0_NE | X86_CR0_ET);
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_PE | X86_CR0_ET);
    assert_eq!(v.vmcs_read64(GUEST_IA32_EFER), 0);
    assert_eq!(ar(&v, VCPU_SREG_CS), 0xc09b);
    assert_eq!(v.vmcs_read16(GUEST_CS_SELECTOR), 0x08);
    assert_eq!(v.vmcs_read32(GUEST_CS_LIMIT), 0xffff_ffff);
    assert_eq!(ar(&v, VCPU_SREG_SS), 0xc093);
    assert_eq!(v.vmcs_read16(GUEST_SS_SELECTOR), 0x10);
    assert_eq!(v.vmcs_read32(GUEST_DS_LIMIT), 0xffff_ffff);
}

#[test]
fn long_mode_state() {
    let v = long_vmcs(false);
    assert_eq!(v.vmcs_readl(GUEST_CR0), 0x8000_0031);
    assert_eq!(v.vmcs_readl(CR0_READ_SHADOW), X86_CR0_PE | X86_CR0_PG | X86_CR0_ET);
    assert_eq!(v.vmcs_readl(CR0_GUEST_HOST_MASK), X86_CR0_NE);
    assert_eq!(v.vmcs_readl(GUEST_CR4), X86_CR4_PAE | X86_CR4_VMXE);
    assert_eq!(v.vmcs_readl(CR4_READ_SHADOW), X86_CR4_PAE);
    assert_eq!(v.vmcs_readl(CR4_GUEST_HOST_MASK), X86_CR4_VMXE);
    assert_eq!(v.vmcs_readl(GUEST_CR3), 0x5000);
    assert_eq!(v.vmcs_read64(GUEST_IA32_EFER), EFER_LME | EFER_LMA);
    assert_eq!(v.vmcs_read32(VM_ENTRY_CONTROLS), VM_ENTRY_LOAD_IA32_EFER | VM_ENTRY_IA32E_MODE);
    assert_eq!(ar(&v, VCPU_SREG_CS), 0xa09b, "L=1, D=0");
    assert_eq!(ar(&v, VCPU_SREG_SS), 0xc093);
    assert_eq!(v.vmcs_readl(GUEST_FS_BASE), 0);
}

#[test]
fn builders_overwrite_the_previous_mode() {
    let mut v = long_vmcs(true);
    vmx_setup_guest_real_mode(&mut v, 0x100, 0x200);
    assert_eq!(v.vmcs_read32(VM_ENTRY_CONTROLS) & VM_ENTRY_IA32E_MODE, 0);
    assert_eq!(v.vmcs_read64(GUEST_IA32_EFER), 0);
    assert_eq!(v.vmcs_readl(GUEST_CR0), X86_CR0_NE | X86_CR0_ET);
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn builders_produce_states_the_cpu_accepts() {
    assert_eq!(check(&real_vmcs()), Ok(()));
    assert_eq!(check(&prot_vmcs()), Ok(()));
    assert_eq!(check(&long_vmcs(true)), Ok(()));
    assert_eq!(check(&long_vmcs(false)), Ok(()));
}

#[test]
fn real_mode_needs_unrestricted_guest() {
    // Without it CR0.PE and CR0.PG must be 1 (IA32_VMX_CR0_FIXED0): no real mode in a VM.
    let mut v = MockVmcs::default();
    controls(&mut v, false);
    vmx_setup_guest_real_mode(&mut v, 0, 0);
    assert_eq!(check(&v), Err(ERR_CR0_FIXED));
    let mut v = MockVmcs::default();
    controls(&mut v, false);
    vmx_setup_guest_protected_mode(&mut v, 0, 0);
    assert_eq!(check(&v), Err(ERR_CR0_FIXED));
}

// ---- host state ---------------------------------------------------------------------------

fn host() -> host_state {
    host_state {
        cr0: 0x8005_0033, cr3: 0x1_2345_6000, cr4: 0x0037_26f0 | X86_CR4_VMXE,
        cs_sel: 0x10, ss_sel: 0x18, ds_sel: 0x2b, es_sel: 0x2b, fs_sel: 0x1b, gs_sel: 0x0, tr_sel: 0x40,
        fs_base: 0x7f00_0000_1000, gs_base: 0xffff_8880_7fc0_0000, tr_base: 0xfffffe00_0000_3000,
        gdtr_base: 0xfffffe00_0000_1000, idtr_base: 0xffff_ffff_ff57_c000,
        sysenter_cs: 0x10, sysenter_esp: 0xfffffe00_0000_2000, sysenter_eip: 0xffff_ffff_8100_0000,
        efer: EFER_SCE | EFER_LME | EFER_LMA | EFER_NX, rsp: 0xffff_c900_0000_1ff8, rip: 0xffff_ffff_a000_0123,
    }
}

#[test]
fn host_state_is_written_field_by_field() {
    let h = host();
    let mut v = MockVmcs::default();
    vmx_set_host_state(&mut v, &h);
    assert_eq!(v.vmcs_readl(HOST_CR0), h.cr0);
    assert_eq!(v.vmcs_readl(HOST_CR3), h.cr3);
    assert_eq!(v.vmcs_readl(HOST_CR4), h.cr4);
    assert_eq!(v.vmcs_readl(HOST_FS_BASE), h.fs_base);
    assert_eq!(v.vmcs_readl(HOST_GS_BASE), h.gs_base);
    assert_eq!(v.vmcs_readl(HOST_TR_BASE), h.tr_base);
    assert_eq!(v.vmcs_readl(HOST_GDTR_BASE), h.gdtr_base);
    assert_eq!(v.vmcs_readl(HOST_IDTR_BASE), h.idtr_base);
    assert_eq!(v.vmcs_read32(HOST_IA32_SYSENTER_CS), 0x10);
    assert_eq!(v.vmcs_readl(HOST_IA32_SYSENTER_ESP), h.sysenter_esp);
    assert_eq!(v.vmcs_readl(HOST_IA32_SYSENTER_EIP), h.sysenter_eip);
    assert_eq!(v.vmcs_read64(HOST_IA32_EFER), h.efer);
    assert_eq!(v.vmcs_readl(HOST_RSP), h.rsp);
    assert_eq!(v.vmcs_readl(HOST_RIP), h.rip);
}

#[test]
fn host_selectors_have_rpl_and_ti_cleared() {
    let mut h = host();
    h.fs_sel = 0x1b; // RPL 3
    h.tr_sel = 0x47; // RPL 3, TI 1
    let mut v = MockVmcs::default();
    vmx_set_host_state(&mut v, &h);
    assert_eq!(v.vmcs_read16(HOST_CS_SELECTOR), 0x10);
    assert_eq!(v.vmcs_read16(HOST_SS_SELECTOR), 0x18);
    assert_eq!(v.vmcs_read16(HOST_DS_SELECTOR), 0x28);
    assert_eq!(v.vmcs_read16(HOST_ES_SELECTOR), 0x28);
    assert_eq!(v.vmcs_read16(HOST_FS_SELECTOR), 0x18);
    assert_eq!(v.vmcs_read16(HOST_GS_SELECTOR), 0x00);
    assert_eq!(v.vmcs_read16(HOST_TR_SELECTOR), 0x40);
}

// ---- vmx_check_guest_state: break one rule at a time -------------------------------------------

fn expect(v: &MockVmcs, err: &'static str) {
    assert_eq!(check(v), Err(err));
}

#[test]
fn link_pointer() {
    let mut v = long_vmcs(false);
    v.vmcs_write64(VMCS_LINK_POINTER, 0);
    expect(&v, ERR_LINK_POINTER);
}

#[test]
fn control_registers() {
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CR0, 0x8000_0011); // NE clear
    expect(&v, ERR_CR0_FIXED);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CR0, 0x8000_0031 | (1 << 40));
    expect(&v, ERR_CR0_FIXED);
    // PG without PE is only reachable when PE/PG are exempt from the fixed bits
    let mut v = long_vmcs(true);
    v.vmcs_writel(GUEST_CR0, X86_CR0_PG | X86_CR0_NE | X86_CR0_ET);
    expect(&v, ERR_CR0_PG_NOPE);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CR4, X86_CR4_PAE); // VMXE clear
    expect(&v, ERR_CR4_FIXED);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CR3, 1 << 60);
    expect(&v, ERR_CR3_RESERVED);
}

#[test]
fn ia32e_mode_needs_paging() {
    let mut v = long_vmcs(true);
    v.vmcs_writel(GUEST_CR0, X86_CR0_PE | X86_CR0_NE | X86_CR0_ET); // PG off
    expect(&v, ERR_IA32E_PG);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CR4, X86_CR4_VMXE); // PAE off
    expect(&v, ERR_IA32E_PAE);
}

#[test]
fn efer_consistency() {
    let mut v = long_vmcs(false);
    v.vmcs_write64(GUEST_IA32_EFER, EFER_LME | EFER_LMA | (1 << 3));
    expect(&v, ERR_EFER_RESERVED);
    let mut v = long_vmcs(false);
    v.vmcs_write64(GUEST_IA32_EFER, EFER_LME); // LMA clear but entry says IA-32e
    expect(&v, ERR_EFER_LMA_ENTRY);
    let mut v = long_vmcs(false);
    v.vmcs_write64(GUEST_IA32_EFER, EFER_LMA); // LMA without LME while paging
    expect(&v, ERR_EFER_LMA_LME);
    let mut v = prot_vmcs();
    v.vmcs_write64(GUEST_IA32_EFER, EFER_LMA); // legacy guest with LMA set
    expect(&v, ERR_EFER_LMA_ENTRY);
    // not loaded on entry -> not checked
    let mut v = long_vmcs(false);
    v.vmcs_write64(GUEST_IA32_EFER, 0xffff);
    v.vmcs_clear_bits(VM_ENTRY_CONTROLS, VM_ENTRY_LOAD_IA32_EFER as u64);
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn rflags() {
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RFLAGS, 0);
    expect(&v, ERR_RFLAGS);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RFLAGS, 2 | (1 << 3));
    expect(&v, ERR_RFLAGS);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RFLAGS, 2 | (1 << 63));
    expect(&v, ERR_RFLAGS);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RFLAGS, 2 | X86_EFLAGS_VM);
    expect(&v, ERR_RFLAGS_VM);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RFLAGS, 2 | X86_EFLAGS_IF | X86_EFLAGS_ZF);
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn rip() {
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RIP, 0x0000_8000_0000_0000); // non-canonical
    expect(&v, ERR_RIP);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_RIP, 0xffff_8000_0000_0000); // canonical
    assert_eq!(check(&v), Ok(()));
    let mut v = prot_vmcs();
    v.vmcs_writel(GUEST_RIP, 0x1_0000_0000); // bits 63:32 in a 32-bit guest
    expect(&v, ERR_RIP);
}

#[test]
fn cs_l_and_d() {
    let mut v = long_vmcs(false);
    let mut cs = vmx_get_segment(&v, VCPU_SREG_CS);
    cs.db = true;
    vmx_set_segment(&mut v, VCPU_SREG_CS, &cs);
    expect(&v, ERR_CS_L_D);
}

#[test]
fn segment_access_rights_reserved_bits() {
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_DS, |a| a | 0x100);
    expect(&v, ERR_SEG_AR_RESERVED);
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_ES, |a| a | (1 << 17));
    expect(&v, ERR_SEG_AR_RESERVED);
}

#[test]
fn segment_s_and_p() {
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_DS, |a| a & !0x80);
    expect(&v, ERR_SEG_P);
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_GS, |a| a & !0x10);
    expect(&v, ERR_SEG_S);
}

#[test]
fn segment_granularity() {
    let mut v = long_vmcs(false);
    v.vmcs_write32(GUEST_CS_LIMIT, 0xffff_f000); // G=1 but limit[11:0] != 0xfff
    expect(&v, ERR_SEG_GRANULARITY);
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_ES, |a| a & !(1 << 15)); // G=0 with limit[31:20] != 0
    expect(&v, ERR_SEG_GRANULARITY);
}

#[test]
fn segment_base() {
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_CS_BASE, 1 << 32);
    expect(&v, ERR_SEG_BASE);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_FS_BASE, 0x0000_8000_0000_0000);
    expect(&v, ERR_SEG_BASE);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_GS_BASE, 0xffff_8880_0000_0000); // FS/GS bases may be 64-bit
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn segment_types() {
    let setty = |seg: usize, ty: u32, unrestricted: bool| {
        let mut v = long_vmcs(unrestricted);
        mod_ar(&mut v, seg, |a| (a & !0xf) | ty);
        v
    };
    expect(&setty(VCPU_SREG_CS, 5, false), ERR_SEG_TYPE);
    expect(&setty(VCPU_SREG_CS, 3, false), ERR_SEG_TYPE); // type 3 CS needs unrestricted guest
    assert_eq!(check(&setty(VCPU_SREG_CS, 3, true)), Ok(()));
    assert_eq!(check(&setty(VCPU_SREG_CS, 0xf, false)), Ok(())); // conforming, DPL 0 <= SS.DPL
    expect(&setty(VCPU_SREG_SS, 2, false), ERR_SEG_TYPE);
    assert_eq!(check(&setty(VCPU_SREG_SS, 7, false)), Ok(()));
    expect(&setty(VCPU_SREG_DS, 2, false), ERR_SEG_TYPE); // not accessed
    expect(&setty(VCPU_SREG_ES, 0xa, false), ERR_SEG_TYPE); // code, readable, not accessed
    expect(&setty(VCPU_SREG_FS, 9, false), ERR_SEG_TYPE); // code, accessed, not readable
    assert_eq!(check(&setty(VCPU_SREG_GS, 0xb, false)), Ok(())); // readable code is fine
}

#[test]
fn segment_dpl_and_rpl() {
    let mut v = long_vmcs(false);
    mod_ar(&mut v, VCPU_SREG_CS, |a| a | (1 << 5)); // CS.DPL=1, SS.DPL=0
    expect(&v, ERR_SEG_DPL);
    let mut v = long_vmcs(false);
    v.vmcs_write16(GUEST_SS_SELECTOR, 0x11); // SS.RPL=1 vs CS.RPL=0
    expect(&v, ERR_SS_RPL);
    let mut v = long_vmcs(false);
    v.vmcs_write16(GUEST_DS_SELECTOR, 0x13); // DS.RPL=3 > DS.DPL=0
    expect(&v, ERR_SEG_DPL);
    // the same RPL mismatch is waived for a real-mode / PE=0 unrestricted guest
    let mut v = real_vmcs();
    v.vmcs_write16(GUEST_SS_SELECTOR, 0x3);
    v.vmcs_write16(GUEST_DS_SELECTOR, 0x3);
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn unusable_segments_are_not_checked() {
    let mut v = long_vmcs(false);
    set_ar(&mut v, VCPU_SREG_DS, (1 << 16) | 0x100); // junk, but unusable
    v.vmcs_write32(GUEST_DS_LIMIT, 0x1234);
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn task_register() {
    let mut v = long_vmcs(false);
    v.vmcs_write16(GUEST_TR_SELECTOR, 0x44);
    expect(&v, ERR_TR_TI);
    let mut v = long_vmcs(false);
    set_ar(&mut v, VCPU_SREG_TR, 1 << 16);
    expect(&v, ERR_TR_UNUSABLE);
    let mut v = long_vmcs(false);
    set_ar(&mut v, VCPU_SREG_TR, 0x83); // 16-bit busy TSS: illegal in IA-32e mode
    expect(&v, ERR_TR_TYPE);
    let mut v = long_vmcs(false);
    set_ar(&mut v, VCPU_SREG_TR, 0x9b); // S=1
    expect(&v, ERR_TR_TYPE);
    let mut v = prot_vmcs();
    set_ar(&mut v, VCPU_SREG_TR, 0x83); // fine outside IA-32e
    assert_eq!(check(&v), Ok(()));
    let mut v = long_vmcs(false);
    v.vmcs_write32(GUEST_TR_LIMIT, 0x67 | (1 << 20)); // G=0 with limit[31:20] != 0
    expect(&v, ERR_SEG_GRANULARITY);
}

#[test]
fn local_descriptor_table_register() {
    let mut v = long_vmcs(false);
    set_ar(&mut v, VCPU_SREG_LDTR, 0x82); // usable LDT, type 2, present
    assert_eq!(check(&v), Ok(()));
    set_ar(&mut v, VCPU_SREG_LDTR, 0x83);
    expect(&v, ERR_LDTR_TYPE);
    set_ar(&mut v, VCPU_SREG_LDTR, 0x82);
    v.vmcs_write16(GUEST_LDTR_SELECTOR, 4);
    expect(&v, ERR_LDTR_TYPE);
}

#[test]
fn descriptor_tables() {
    let mut v = long_vmcs(false);
    v.vmcs_write32(GUEST_GDTR_LIMIT, 0x1_0000);
    expect(&v, ERR_DTR_LIMIT);
    let mut v = long_vmcs(false);
    v.vmcs_writel(GUEST_IDTR_BASE, 0x0000_8000_0000_0000);
    expect(&v, ERR_DTR_BASE);
}
