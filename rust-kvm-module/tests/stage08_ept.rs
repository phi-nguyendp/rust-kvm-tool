use ktvmx::mock::MockGuestMem;
use ktvmx::vmcs_ops::GuestMem;
use ktvmx::vmx::*;
use ktvmx::vmx_ept::*;

const MEM: usize = 8 << 20;
const TABLES_START: u64 = 0x10_0000;
const TABLES_END: u64 = 0x20_0000;
const DATA: u64 = 0x30_0000; // 2 MiB-aligned HPA region: 0x40_0000 below

struct Ept {
    mem: MockGuestMem,
    alloc: BumpAlloc,
    root: u64,
}

impl Ept {
    fn new() -> Self {
        let mut mem = MockGuestMem::new(MEM);
        let mut alloc = BumpAlloc::new(TABLES_START, TABLES_END);
        let root = ept_new_table(&mut mem, &mut alloc).unwrap();
        Ept { mem, alloc, root }
    }
    fn map(&mut self, gpa: u64, hpa: u64, perms: u64) -> Result<(), EptError> {
        ept_map_page(&mut self.mem, &mut self.alloc, self.root, gpa, hpa, perms)
    }
    fn map_2m(&mut self, gpa: u64, hpa: u64, perms: u64) -> Result<(), EptError> {
        ept_map_2m(&mut self.mem, &mut self.alloc, self.root, gpa, hpa, perms)
    }
    fn tr(&self, gpa: u64, access: u64) -> Result<u64, EptFault> {
        ept_translate(&self.mem, self.root, gpa, access)
    }
    fn tables_used(&self) -> u64 {
        (self.alloc.next - TABLES_START) / 4096
    }
    fn entry(&self, table: u64, idx: usize) -> u64 {
        let mut b = [0u8; 8];
        assert!(self.mem.read(table + 8 * idx as u64, &mut b));
        u64::from_le_bytes(b)
    }
    fn set_entry(&mut self, table: u64, idx: usize, v: u64) {
        assert!(self.mem.write(table + 8 * idx as u64, &v.to_le_bytes()));
    }
}

#[test]
fn leaf_entry_layout() {
    // R|W|X, write-back, 4 KiB: pa | 6<<3 | 7
    assert_eq!(ept_entry(0x1234_5000, EPT_PERM_RWX, VMX_EPT_MT_WB, false), 0x1234_5037);
    // large leaf sets bit 7
    assert_eq!(ept_entry(0x20_0000, EPT_PERM_R | EPT_PERM_W, VMX_EPT_MT_WB, true), 0x20_0000 | 0x80 | 0x30 | 3);
    // uncached device memory, read-only
    assert_eq!(ept_entry(0xfee0_0000, EPT_PERM_R, VMX_EPT_MT_UC, false), 0xfee0_0001);
    // junk outside the defined fields is masked away
    assert_eq!(ept_entry(0xfff0_0000_0000_1fff, 0xff, 0xff, false) & 0xfff0_0000_0000_0fff, 0x3f); // memtype masked to 3 bits
    assert_eq!(ept_entry(0xfff0_0000_0000_1fff, 0xff, 0xff, false) & EPT_ADDR_MASK, 0x1000);
}

#[test]
fn table_entry_is_rwx_with_reserved_bits_clear() {
    let e = ept_table_entry(0x5000);
    assert_eq!(e, 0x5007);
    assert_eq!(e & 0xf8, 0, "bits 7:3 are reserved in a non-leaf entry");
    assert!(!ept_misconfig_check(e, 4));
    assert!(!ept_misconfig_check(e, 3));
}

#[test]
fn eptp_encoding() {
    assert_eq!(construct_eptp(0x1000, false), 0x1000 | 6 | 3 << 3);
    assert_eq!(construct_eptp(0x1000, false), 0x101e);
    assert_eq!(construct_eptp(0x1000, true), 0x105e);
    assert_eq!(construct_eptp(0x7_fffe_f000, true) & EPT_ADDR_MASK, 0x7_fffe_f000);
}

#[test]
fn index_per_level() {
    let gpa = (1u64 << 39) | (2 << 30) | (3 << 21) | (4 << 12) | 0x567;
    assert_eq!(ept_index(gpa, 4), 1);
    assert_eq!(ept_index(gpa, 3), 2);
    assert_eq!(ept_index(gpa, 2), 3);
    assert_eq!(ept_index(gpa, 1), 4);
    assert_eq!(ept_index(0xffff_ffff_ffff_f000, 4), 0x1ff);
    assert_eq!(ept_index(0xffff_ffff_ffff_f000, 1), 0x1ff);
    assert_eq!(ept_index(0x1000, 4), 0);
}

#[test]
fn present_means_any_of_rwx() {
    assert!(!ept_present(0));
    assert!(!ept_present(0xffff_ffff_ffff_fff8)); // lots of bits, no R/W/X
    assert!(ept_present(EPT_PERM_X));
    assert!(ept_present(EPT_PERM_R));
}

#[test]
fn misconfig_rules() {
    let wb = VMX_EPT_MT_WB << 3;
    // not present is never a misconfiguration, whatever the other bits say
    assert!(!ept_misconfig_check(0, 1));
    assert!(!ept_misconfig_check(0xffff_ffff_ffff_fff8, 4));
    // write without read
    assert!(ept_misconfig_check(EPT_PERM_W | wb, 1));
    assert!(ept_misconfig_check(EPT_PERM_W | EPT_PERM_X | wb, 1));
    // execute-only is legal
    assert!(!ept_misconfig_check(EPT_PERM_X | wb, 1));
    // reserved memory types 2, 3, 7 on leaves; 0 1 4 5 6 are fine
    for mt in 0..8u64 {
        let e = EPT_PERM_RWX | mt << 3;
        let bad = matches!(mt, 2 | 3 | 7);
        assert_eq!(ept_misconfig_check(e, 1), bad, "PTE memtype {mt}");
        assert_eq!(ept_misconfig_check(e | VMX_EPT_PAGE_SIZE_BIT, 2), bad, "2M memtype {mt}");
        assert_eq!(ept_misconfig_check(e | VMX_EPT_PAGE_SIZE_BIT, 3), bad, "1G memtype {mt}");
    }
    // non-leaf: bits 6:3 reserved
    assert!(ept_misconfig_check(EPT_PERM_RWX | 1 << 3, 4));
    assert!(ept_misconfig_check(EPT_PERM_RWX | 1 << 6, 3));
    assert!(ept_misconfig_check(EPT_PERM_RWX | 1 << 5, 2));
    // level 4 cannot be a leaf: bit 7 reserved
    assert!(ept_misconfig_check(EPT_PERM_RWX | VMX_EPT_PAGE_SIZE_BIT, 4));
    // ...but bit 7 at level 1 is ignored
    assert!(!ept_misconfig_check(EPT_PERM_RWX | wb | VMX_EPT_PAGE_SIZE_BIT, 1));
    // large leaves must be aligned
    assert!(ept_misconfig_check(0x20_1000 | EPT_PERM_RWX | wb | 0x80, 2));
    assert!(!ept_misconfig_check(0x20_0000 | EPT_PERM_RWX | wb | 0x80, 2));
    assert!(ept_misconfig_check(0x20_0000 | EPT_PERM_RWX | wb | 0x80, 3), "1G leaf at 2M alignment");
    assert!(!ept_misconfig_check(0x4000_0000 | EPT_PERM_RWX | wb | 0x80, 3));
    // accessed / dirty / ignored high bits are fine
    assert!(!ept_misconfig_check(DATA | EPT_PERM_RWX | wb | VMX_EPT_ACCESS_BIT | VMX_EPT_DIRTY_BIT, 1));
}

#[test]
fn map_4k_and_translate() {
    let mut e = Ept::new();
    assert_eq!(e.tables_used(), 1);
    e.map(0x1000, DATA, EPT_PERM_RWX).unwrap();
    assert_eq!(e.tables_used(), 4, "root + PDPT + PD + PT");
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Ok(DATA));
    assert_eq!(e.tr(0x1abc, EPT_PERM_R | EPT_PERM_W), Ok(DATA + 0xabc));
    // a neighbour reuses the same intermediate tables
    e.map(0x2000, DATA + 0x5000, EPT_PERM_RWX).unwrap();
    assert_eq!(e.tables_used(), 4);
    assert_eq!(e.tr(0x2fff, EPT_PERM_X), Ok(DATA + 0x5fff));
    // the leaf is exactly what ept_entry builds
    let pt = e.entry(e.entry(e.entry(e.root, 0) & EPT_ADDR_MASK, 0) & EPT_ADDR_MASK, 0) & EPT_ADDR_MASK;
    assert_eq!(e.entry(pt, 1), ept_entry(DATA, EPT_PERM_RWX, VMX_EPT_MT_WB, false));
}

#[test]
fn walk_uses_all_four_levels_for_far_addresses() {
    let mut e = Ept::new();
    let gpa = (3u64 << 39) | (5 << 30) | (7 << 21) | (9 << 12);
    e.map(gpa, DATA, EPT_PERM_RWX).unwrap();
    assert_eq!(e.tr(gpa + 0x10, EPT_PERM_R), Ok(DATA + 0x10));
    assert_ne!(e.entry(e.root, 3), 0);
    assert_eq!(e.entry(e.root, 0), 0);
}

#[test]
fn unmapped_addresses_violate() {
    let mut e = Ept::new();
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Err(EptFault::Violation { level: 4, perms: 0 }));
    e.map(0x1000, DATA, EPT_PERM_RWX).unwrap();
    // same PT, other slot
    assert_eq!(e.tr(0x5000, EPT_PERM_R), Err(EptFault::Violation { level: 1, perms: 0 }));
    // same PD, other PT
    assert_eq!(e.tr(0x40_0000, EPT_PERM_R), Err(EptFault::Violation { level: 2, perms: 0 }));
    // other PML4 slot
    assert_eq!(e.tr(1 << 39, EPT_PERM_R), Err(EptFault::Violation { level: 4, perms: 0 }));
}

#[test]
fn permissions_are_checked() {
    let mut e = Ept::new();
    e.map(0x1000, DATA, EPT_PERM_R).unwrap();
    e.map(0x2000, DATA + 0x1000, EPT_PERM_R | EPT_PERM_X).unwrap();
    e.map(0x3000, DATA + 0x2000, EPT_PERM_X).unwrap();
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Ok(DATA));
    assert_eq!(e.tr(0x1000, EPT_PERM_W), Err(EptFault::Violation { level: 1, perms: 1 }));
    assert_eq!(e.tr(0x1000, EPT_PERM_X), Err(EptFault::Violation { level: 1, perms: 1 }));
    assert_eq!(e.tr(0x2000, EPT_PERM_X), Ok(DATA + 0x1000));
    assert_eq!(e.tr(0x2000, EPT_PERM_W), Err(EptFault::Violation { level: 1, perms: 5 }));
    // execute-only page: fetch ok, read not
    assert_eq!(e.tr(0x3000, EPT_PERM_X), Ok(DATA + 0x2000));
    assert_eq!(e.tr(0x3000, EPT_PERM_R), Err(EptFault::Violation { level: 1, perms: 4 }));
    // a combined access needs all of it
    assert_eq!(e.tr(0x2000, EPT_PERM_R | EPT_PERM_X), Ok(DATA + 0x1000));
    assert_eq!(e.tr(0x2000, EPT_PERM_R | EPT_PERM_W), Err(EptFault::Violation { level: 1, perms: 5 }));
}

#[test]
fn effective_permissions_are_the_and_of_every_level() {
    let mut e = Ept::new();
    e.map(0x1000, DATA, EPT_PERM_RWX).unwrap();
    assert_eq!(e.tr(0x1000, EPT_PERM_W), Ok(DATA));
    // take W and X away from the PML4 entry: the RWX leaf is now effectively read-only
    let pdpt = e.entry(e.root, 0) & EPT_ADDR_MASK;
    e.set_entry(e.root, 0, ept_table_entry(pdpt) & !(EPT_PERM_W | EPT_PERM_X));
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Ok(DATA));
    assert_eq!(e.tr(0x1000, EPT_PERM_W), Err(EptFault::Violation { level: 1, perms: 1 }));
}

#[test]
fn map_2m_pages() {
    let mut e = Ept::new();
    e.map_2m(0x20_0000, 0x40_0000, EPT_PERM_RWX).unwrap();
    assert_eq!(e.tables_used(), 3, "root + PDPT + PD, no PT");
    assert_eq!(e.tr(0x20_0000, EPT_PERM_R), Ok(0x40_0000));
    assert_eq!(e.tr(0x2f_1234, EPT_PERM_W), Ok(0x4f_1234));
    assert_eq!(e.tr(0x3f_ffff, EPT_PERM_W), Ok(0x5f_ffff));
    assert_eq!(e.tr(0x40_0000, EPT_PERM_R), Err(EptFault::Violation { level: 2, perms: 0 }));
    let pd = e.entry(e.entry(e.root, 0) & EPT_ADDR_MASK, 0) & EPT_ADDR_MASK;
    assert_eq!(e.entry(pd, 1), ept_entry(0x40_0000, EPT_PERM_RWX, VMX_EPT_MT_WB, true));
    assert!(!ept_misconfig_check(e.entry(pd, 1), 2));
}

#[test]
fn mixing_page_sizes_is_refused() {
    let mut e = Ept::new();
    e.map_2m(0x20_0000, 0x40_0000, EPT_PERM_RWX).unwrap();
    assert_eq!(e.map(0x20_1000, DATA, EPT_PERM_RWX), Err(EptError::AlreadyMapped));
    e.map(0x60_0000, DATA, EPT_PERM_RWX).unwrap();
    assert_eq!(
        ept_map_2m(&mut e.mem, &mut e.alloc, e.root, 0x60_0000, 0x40_0000, EPT_PERM_RWX),
        Err(EptError::AlreadyMapped)
    );
    // the 2 MiB mapping is untouched
    assert_eq!(e.tr(0x20_0000, EPT_PERM_R), Ok(0x40_0000));
}

#[test]
fn remapping_overwrites_the_leaf() {
    let mut e = Ept::new();
    e.map(0x1000, DATA, EPT_PERM_RWX).unwrap();
    let used = e.tables_used();
    e.map(0x1000, DATA + 0x9000, EPT_PERM_R).unwrap();
    assert_eq!(e.tables_used(), used);
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Ok(DATA + 0x9000));
    assert!(e.tr(0x1000, EPT_PERM_W).is_err());
}

#[test]
fn alignment_is_checked() {
    let mut e = Ept::new();
    assert_eq!(e.map(0x1001, DATA, EPT_PERM_RWX), Err(EptError::Misaligned));
    assert_eq!(e.map(0x1000, DATA + 8, EPT_PERM_RWX), Err(EptError::Misaligned));
    assert_eq!(
        ept_map_2m(&mut e.mem, &mut e.alloc, e.root, 0x1000, 0x40_0000, EPT_PERM_RWX),
        Err(EptError::Misaligned)
    );
    assert_eq!(
        ept_map_2m(&mut e.mem, &mut e.alloc, e.root, 0x20_0000, 0x40_1000, EPT_PERM_RWX),
        Err(EptError::Misaligned)
    );
    assert_eq!(e.tables_used(), 1, "a refused mapping allocates nothing");
}

#[test]
fn out_of_table_memory() {
    let mut mem = MockGuestMem::new(MEM);
    let mut alloc = BumpAlloc::new(TABLES_START, TABLES_START + 2 * 4096); // root + one more
    let root = ept_new_table(&mut mem, &mut alloc).unwrap();
    assert_eq!(
        ept_map_page(&mut mem, &mut alloc, root, 0x1000, DATA, EPT_PERM_RWX),
        Err(EptError::OutOfMemory)
    );
    assert!(ept_translate(&mem, root, 0x1000, EPT_PERM_R).is_err());
}

#[test]
fn new_tables_are_zeroed() {
    let mut mem = MockGuestMem::new(MEM);
    mem.0[TABLES_START as usize..TABLES_START as usize + 8192].fill(0xaa);
    let mut alloc = BumpAlloc::new(TABLES_START, TABLES_END);
    let root = ept_new_table(&mut mem, &mut alloc).unwrap();
    ept_map_page(&mut mem, &mut alloc, root, 0x1000, DATA, EPT_PERM_RWX).unwrap();
    // PDPT slot 1 must read as "not present", not as stale 0xaa..
    assert_eq!(ept_translate(&mem, root, 1 << 30, EPT_PERM_R), Err(EptFault::Violation { level: 3, perms: 0 }));
}

#[test]
fn translate_reports_misconfig_and_bad_tables() {
    let mut e = Ept::new();
    e.map(0x1000, DATA, EPT_PERM_RWX).unwrap();
    let pt = e.entry(e.entry(e.entry(e.root, 0) & EPT_ADDR_MASK, 0) & EPT_ADDR_MASK, 0) & EPT_ADDR_MASK;
    e.set_entry(pt, 1, DATA | EPT_PERM_W); // W without R
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Err(EptFault::Misconfig { level: 1 }));
    e.set_entry(pt, 1, DATA | EPT_PERM_RWX | 2 << 3); // memtype 2
    assert_eq!(e.tr(0x1000, EPT_PERM_R), Err(EptFault::Misconfig { level: 1 }));
    // a root outside the memory model
    assert_eq!(
        ept_translate(&e.mem, MEM as u64 + 0x1000, 0x1000, EPT_PERM_R),
        Err(EptFault::BadTable { level: 4 })
    );
}

#[test]
fn violation_qualification_decode() {
    // data write to an unmapped page, linear address valid, access was the final translation
    let v = ept_violation_decode(0x182);
    assert!(v.write && !v.read && !v.fetch);
    assert!(!v.readable && !v.writable && !v.executable);
    assert!(v.gla_valid && v.translated);
    assert!(ept_violation_is_unmapped(&v));

    // instruction fetch from a readable+writable (not executable) page
    let v = ept_violation_decode(0x4 | 0x8 | 0x10);
    assert!(v.fetch && !v.read && !v.write);
    assert!(v.readable && v.writable && !v.executable);
    assert!(!v.gla_valid && !v.translated);
    assert!(!ept_violation_is_unmapped(&v));

    // read of a page the guest's own page table walk touched: not "translated"
    let v = ept_violation_decode(0x1 | 0x80);
    assert!(v.read && v.gla_valid && !v.translated);

    assert_eq!(ept_violation_decode(0), ept_violation::default());
    // bits above 8 (user/NX/NMI info) do not disturb the low fields
    assert_eq!(ept_violation_decode(0x1e00 | 0x2), ept_violation { write: true, ..Default::default() });
}

#[test]
fn invept_descriptor_layout() {
    let eptp = construct_eptp(0x123000, true);
    let d = invept_descriptor(eptp);
    assert_eq!(d.len(), 16);
    assert_eq!(u64::from_le_bytes(d[..8].try_into().unwrap()), eptp);
    assert_eq!(&d[8..], &[0u8; 8]);
    assert_eq!(VMX_EPT_EXTENT_CONTEXT, 1);
    assert_eq!(VMX_EPT_EXTENT_GLOBAL, 2);
}
