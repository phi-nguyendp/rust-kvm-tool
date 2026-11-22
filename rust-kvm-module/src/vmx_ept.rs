//! Stage 8: EPT, the second level of address translation.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (`construct_eptp`, `handle_ept_violation`,
//! `handle_ept_misconfig`), arch/x86/kvm/vmx/vmx_ops.h (`__invept`),
//! arch/x86/kvm/mmu/spte.c + tdp_mmu.c (building the entries and walking/populating the
//! tables; KVM calls them SPTEs).
//! Book: Intel SDM Vol. 3C, chapter 28 "VMX support for address translation":
//! 28.3.2 EPT translation, 28.3.3.1 EPT misconfigurations and violations,
//! Tables 28-1..28-6 (entry formats), 28.2.2 the EPT pointer (Table 25-9 / 24.6.11),
//! 28.3.3.4 / 30.3 INVEPT, Table 28-7 (EPT-violation exit qualification).
//!
//! Without EPT the guest's physical addresses are the machine's; with EPT every
//! guest-physical address (GPA) the guest produces, after its *own* paging, is
//! translated by a 4-level table you build: PML4 -> PDPT -> PD -> PT, 9 bits of the GPA per
//! level, exactly like x86-64 paging, with R/W/X bits instead of P/RW/US.
//!
//! "Host physical" addresses (HPA) here are offsets into a [`GuestMem`], so the tests can
//! build whole table trees in a `Vec<u8>`; in the kernel they are real physical
//! addresses of `PageBox`es and guest RAM.
#![allow(unused_variables, unused_imports)]

use crate::vmcs_ops::GuestMem;
use crate::vmx::*;

pub const EPT_PERM_R: u64 = VMX_EPT_READABLE_MASK;
pub const EPT_PERM_W: u64 = VMX_EPT_WRITABLE_MASK;
pub const EPT_PERM_X: u64 = VMX_EPT_EXECUTABLE_MASK;
pub const EPT_PERM_RWX: u64 = EPT_PERM_R | EPT_PERM_W | EPT_PERM_X;

/// Bits 51:12 of an entry or of the EPTP hold a (4 KiB-aligned) physical address.
pub const EPT_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

pub const PAGE_SIZE_4K: u64 = 1 << 12;
pub const PAGE_SIZE_2M: u64 = 1 << 21;
pub const PAGE_SIZE_1G: u64 = 1 << 30;
/// Entries per table.
pub const EPT_ENTRIES: usize = 512;

/// INVEPT types (kernel: VMX_EPT_EXTENT_*).
pub const VMX_EPT_EXTENT_CONTEXT: u64 = 1;
pub const VMX_EPT_EXTENT_GLOBAL: u64 = 2;

/// Why a translation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptFault {
    /// The walk hit an entry that is not legal (see [`ept_misconfig_check`]): the CPU
    /// would raise an EPT *misconfiguration* VM exit (reason 49).
    Misconfig { level: u8 },
    /// Not present (`perms == 0`), or present but the effective permissions (the AND of
    /// the permission bits of every entry on the path) lack the requested access:
    /// an EPT *violation* VM exit (reason 48). `perms` are the effective R/W/X bits
    /// at the level where the walk stopped.
    Violation { level: u8, perms: u8 },
    /// A table address pointed outside the memory model.
    BadTable { level: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptError {
    /// The table allocator ran dry.
    OutOfMemory,
    /// `gpa`/`hpa` is not aligned to the page size being mapped.
    Misaligned,
    /// The range is already covered by a mapping of a different size
    /// (mapping 4 KiB under a 2 MiB leaf, or a 2 MiB leaf over a table).
    AlreadyMapped,
    /// A table address pointed outside the memory model.
    BadTable,
}

/// Hands out zeroed-on-demand, 4 KiB-aligned tables ("host physical" addresses).
/// In the kernel this wraps `PageBox::new()`; the walker below zeroes the table itself.
pub trait EptTableAlloc {
    fn alloc_table(&mut self) -> Option<u64>;
}

/// PROVIDED: a bump allocator over `[next, end)`.
pub struct BumpAlloc {
    pub next: u64,
    pub end: u64,
}

impl BumpAlloc {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            next: (start + 0xfff) & !0xfff,
            end,
        }
    }
}

impl EptTableAlloc for BumpAlloc {
    fn alloc_table(&mut self) -> Option<u64> {
        if self.next + PAGE_SIZE_4K > self.end {
            return None;
        }
        let pa = self.next;
        self.next += PAGE_SIZE_4K;
        Some(pa)
    }
}

// ---------------------------------------------------------------------------------
// Entries and the EPTP
// ---------------------------------------------------------------------------------

/// A *leaf* entry mapping `pa` (SDM Tables 28-5 (2 MiB PDE), 28-6 (4 KiB PTE)):
/// bits 2:0 = `perms` (R/W/X), bits 5:3 = EPT memory type `memtype`
/// (`VMX_EPT_MT_WB` for RAM, `VMX_EPT_MT_UC` for devices), b2it 6 (ignore PAT) left 0, bit 7
/// = 1 if `large` (a 2 MiB / 1 GiB leaf in a PD / PDPT; for a 4 KiB PTE the bit is
/// ignored, keep it 0), bits 51:12 = `pa`.
/// Only the R/W/X bits of `perms` are used (mask it); `pa` is masked with `EPT_ADDR_MASK`.
/// (kernel: `make_spte` in arch/x86/kvm/mmu/spte.c, for the EPT case.)
pub fn ept_entry(pa: u64, perms: u64, memtype: u64, large: bool) -> u64 {
    // create an ept entry with the given physical address, permissions, memory type, and large
    // page flag
    let is_large = if large { 1u64 } else { 0u64 };
    let perms = perms & EPT_PERM_RWX; // only 3 bits for permissions
    let memtype = memtype & 0x7; // only 3 bits for memory type
                                 //0:2 | 3:5 | 6 | 7 | 12:51
    perms | memtype << 3 | (is_large << 7) | (pa & EPT_ADDR_MASK)
}

/// A *non-leaf* entry pointing at the next-level table at `table_pa` (Tables 28-1, 28-3,
/// 28-4): R, W and X all set (so the leaf alone decides the permissions), bits 6:3 and
/// bit 7 zero (they are reserved in a non-leaf entry), bits 51:12 = `table_pa`.
/// (kernel: `make_nonleaf_spte`.)
pub fn ept_table_entry(table_pa: u64) -> u64 {
    let ret = EPT_PERM_RWX; // to clear bits 6:3 and 7, we can just start with 0 and set the R/W/X bits
    ret | (table_pa & EPT_ADDR_MASK)
}

/// The EPT pointer for the `EPT_POINTER` VMCS field (SDM 24.6.11, Table 25-9):
/// bits 2:0 = memory type used to access the PML4 (6 = write-back),
/// bits 5:3 = page-walk length minus 1 (4 levels -> 3), bit 6 = enable accessed/dirty
/// flags (`ad`), bits 51:12 = physical address of the PML4 table.
/// (kernel: `construct_eptp`.)
pub fn construct_eptp(root_pa: u64, ad: bool) -> u64 {
    let ret = root_pa & EPT_ADDR_MASK;
    ret | VMX_EPT_MT_WB | VMX_EPTP_PWL_4 | ((ad as u64) << 6)
}

/// Index into the table of `level` (4 = PML4 ... 1 = PT) for `gpa`: 9 bits starting at bit
/// `12 + 9 * (level - 1)`.
pub fn ept_index(gpa: u64, level: u8) -> usize {
    let shift = 12 + 9 * (level - 1) as u64;
    // index of the table at the given level for the given guest physical address
    ((gpa >> shift) & 0x1ff) as usize
}

/// An entry is "present" for the CPU if any of R/W/X is set (there is no P bit).
pub fn ept_present(entry: u64) -> bool {
    entry & EPT_PERM_RWX != 0
}

/// Is `entry`, found in a table of `level` (4..=1), an EPT *misconfiguration*
/// (SDM 28.3.3.1)? A not-present entry (R=W=X=0) is never misconfigured. A present one is when:
/// - W is set without R (an execute-only entry, R=0 W=0 X=1, is allowed on CPUs that
///   report it in IA32_VMX_EPT_VPID_CAP bit 0; treat it as legal here);
/// - it is a non-leaf entry (level 4, or level 3/2 with bit 7 clear) and any of bits 6:3 is
///   set, or (level 4 only) bit 7 is set: those bits are reserved;
/// - it is a leaf (level 1, or level 3/2 with bit 7 set) and the memory type (bits 5:3) is
///   2, 3 or 7 (reserved; 0, 1, 4, 5, 6 are legal);
/// - it is a large leaf whose address bits below the page size are set: level 3 (1 GiB) bits
///   29:12, level 2 (2 MiB) bits 20:12.
/// (kernel: the checks are in the hardware; `handle_ept_misconfig` reports them via
/// `kvm_mmu_page_fault`; the spte rules are in `is_shadow_present_pte` etc.)
pub fn ept_misconfig_check(entry: u64, level: u8) -> bool {
    if !ept_present(entry) {
        return false;
    }

    let r = entry & EPT_PERM_R != 0;
    let w = entry & EPT_PERM_W != 0;
    let x = entry & EPT_PERM_X != 0;
    let is_leaf = level == 1 || (level == 2 || level == 3) && (entry & (1 << 7) != 0);
    let is_large_leaf = (level == 2 || level == 3) && (entry & (1 << 7) != 0);
    let memtype = (entry >> 3) & 0x7;
    let addr = entry & EPT_ADDR_MASK;

    // Check for W without R
    if w && !r {
        return true;
    }
    // Check for reserved bits in non-leaf entries
    if !is_leaf {
        if (entry & 0x78) != 0 {
            return true;
        }
        if level == 4 && (entry & (1 << 7) != 0) {
            return true;
        }
    }
    // Check for reserved memory types in leaf entries
    if is_leaf && (memtype == 2 || memtype == 3 || memtype == 7) {
        return true;
    }
    // Check for misaligned addresses in large leaf entries
    if is_large_leaf {
        let page_size = if level == 3 {
            PAGE_SIZE_1G
        } else {
            PAGE_SIZE_2M
        };
        if addr & (page_size - 1) != 0 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------------
// Building and walking tables inside a GuestMem
// ---------------------------------------------------------------------------------

fn read_entry(mem: &impl GuestMem, table: u64, idx: usize) -> Option<u64> {
    let mut b = [0u8; 8];
    if mem.read(table + 8 * idx as u64, &mut b) {
        Some(u64::from_le_bytes(b))
    } else {
        None
    }
}

fn write_entry(mem: &mut impl GuestMem, table: u64, idx: usize, val: u64) -> bool {
    mem.write(table + 8 * idx as u64, &val.to_le_bytes())
}

/// Allocate and zero a table. PROVIDED.
pub fn ept_new_table(
    mem: &mut impl GuestMem,
    alloc: &mut impl EptTableAlloc,
) -> Result<u64, EptError> {
    let pa = alloc.alloc_table().ok_or(EptError::OutOfMemory)?;
    if !mem.write(pa, &[0u8; 4096]) {
        return Err(EptError::BadTable);
    }
    Ok(pa)
}

/// Map `size` bytes (`PAGE_SIZE_4K` or `PAGE_SIZE_2M`) at `gpa` to `hpa` below the PML4 at
/// `root`, creating intermediate tables from `alloc` as needed (new tables are zeroed,
/// `ept_table_entry`). Leaves use `VMX_EPT_MT_WB`. Remapping the same page with the same
/// size just overwrites the leaf.
/// Errors: `Misaligned` if `gpa` or `hpa` is not a multiple of `size`; `AlreadyMapped` if
/// an intermediate entry on the way is already a large leaf (mapping 4 KiB under a
/// 2 MiB page) or, when mapping 2 MiB, the PD slot already points to a PT.
/// Write the leaf last so a failed walk leaves no half-installed leaf (new empty
/// tables may remain, harmless). (kernel: `tdp_mmu_map_handle_target_level`, `kvm_tdp_mmu_map`.)
fn ept_map(
    mem: &mut impl GuestMem,
    alloc: &mut impl EptTableAlloc,
    root: u64,
    gpa: u64,
    hpa: u64,
    perms: u64,
    size: u64,
) -> Result<(), EptError> {
    // check alignment
    if gpa % size != 0 || hpa % size != 0 {
        return Err(EptError::Misaligned);
    }
    // walk the table levels 4..=1
    let mut table = root;
    let target_level = if size == PAGE_SIZE_4K { 1 } else { 2 };
    for level in (1..=4).rev() {
        // on each level, we read the entry based on the index from the gpa,
        // validate it, after that:
        // if we are at the target level, we write a leaf entry with the hpa and perms
        // otherwise, we check if the entry is a large leaf, if so we return AlreadyMapped
        // if not, we get the next table address, if it's 0 we allocate a new table and write the
        // entry, otherwise we continue to the next level
        let entry_idx = ept_index(gpa, level);
        let entry = read_entry(mem, table, entry_idx).ok_or(EptError::BadTable)?;
        if ept_misconfig_check(entry, level) {
            return Err(EptError::BadTable);
        }
        if level == target_level {
            // if the size is 2M, we need to check if the entry is a present non-leaf entry (a PT)
            if size == PAGE_SIZE_2M && ept_present(entry) && entry & (1 << 7) == 0 {
                return Err(EptError::AlreadyMapped);
            }
            // write the leaf entry
            let leaf_entry = ept_entry(hpa, perms, VMX_EPT_MT_WB, size == PAGE_SIZE_2M);
            if !write_entry(mem, table, entry_idx, leaf_entry) {
                return Err(EptError::BadTable);
            }
            return Ok(());
        } else {
            // intermediate level: check if it's a large leaf
            if entry & (1 << 7) != 0 {
                return Err(EptError::AlreadyMapped);
            }
            // get the next table address
            let next_table_pa = entry & EPT_ADDR_MASK;
            if next_table_pa == 0 {
                // allocate a new table
                let new_table_pa = ept_new_table(mem, alloc)?;
                let new_entry = ept_table_entry(new_table_pa);
                if !write_entry(mem, table, entry_idx, new_entry) {
                    return Err(EptError::BadTable);
                }
                table = new_table_pa;
            } else {
                table = next_table_pa;
            }
        }
    }

    Ok(())
}

/// Map one 4 KiB page. See [`ept_map`] (private; call it with `PAGE_SIZE_4K`).
pub fn ept_map_page(
    mem: &mut impl GuestMem,
    alloc: &mut impl EptTableAlloc,
    root: u64,
    gpa: u64,
    hpa: u64,
    perms: u64,
) -> Result<(), EptError> {
    ept_map(mem, alloc, root, gpa, hpa, perms, PAGE_SIZE_4K)
}

/// Map one 2 MiB page (a PD entry with bit 7 set). See [`ept_map`].
pub fn ept_map_2m(
    mem: &mut impl GuestMem,
    alloc: &mut impl EptTableAlloc,
    root: u64,
    gpa: u64,
    hpa: u64,
    perms: u64,
) -> Result<(), EptError> {
    ept_map(mem, alloc, root, gpa, hpa, perms, PAGE_SIZE_2M)
}

/// Software EPT walk: translate `gpa` for an access of kind `access` (a mask of
/// `EPT_PERM_R/W/X`) the way the CPU would (SDM 28.3.2). Start at `root`; at each level
/// 4..=1:
/// 1. read the entry; if `ept_misconfig_check` -> `Err(Misconfig{level})`;
/// 2. not present -> `Err(Violation{level, perms: 0})`;
/// 3. accumulate the *effective* permissions: AND of the R/W/X bits of all entries so far;
/// 4. if the entry is a leaf (level 1, or level 3/2 with bit 7 set): if the effective
///    permissions do not include all of `access` -> `Err(Violation{level, perms: effective})`,
///    otherwise the result is `(entry & EPT_ADDR_MASK) + offset within the page` (offset =
///    low 12/21/30 bits of `gpa`, careful with large pages: the entry's address field
///    is already aligned).
/// 5. otherwise descend into `entry & EPT_ADDR_MASK`.
/// A table address outside the memory model is `Err(BadTable{level})`.
/// (kernel: this is hardware; the closest software walkers are `kvm_tdp_mmu_get_walk` /
/// `tdp_iter_*`, and `vmx_get_ept_mapping`-style debug helpers.)
pub fn ept_translate(
    mem: &impl GuestMem,
    root: u64,
    gpa: u64,
    access: u64,
) -> Result<u64, EptFault> {
    let mut table = root;
    let mut effective_perms = EPT_PERM_RWX; // start with all permissions
    for level in (1..=4).rev() {
        let entry_idx = ept_index(gpa, level);
        let entry = read_entry(mem, table, entry_idx).ok_or(EptFault::BadTable { level })?;
        if ept_misconfig_check(entry, level) {
            return Err(EptFault::Misconfig { level });
        }
        if !ept_present(entry) {
            return Err(EptFault::Violation { level, perms: 0 });
        }
        effective_perms &= entry & EPT_PERM_RWX;
        let is_leaf = level == 1 || (level == 2 || level == 3) && (entry & (1 << 7) != 0);
        if is_leaf {
            if effective_perms & access != access {
                return Err(EptFault::Violation {
                    level,
                    perms: effective_perms as u8,
                });
            }
            let page_size = if level == 3 {
                PAGE_SIZE_1G
            } else if level == 2 {
                PAGE_SIZE_2M
            } else {
                PAGE_SIZE_4K
            };
            let offset = gpa & (page_size - 1);
            return Ok((entry & EPT_ADDR_MASK) + offset);
        } else {
            table = entry & EPT_ADDR_MASK;
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------------------------
// VM exits: EPT violation / misconfiguration, INVEPT
// ---------------------------------------------------------------------------------

/// Decoded EXIT_QUALIFICATION of an EPT violation (SDM Table 28-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ept_violation {
    /// The access that caused it (bits 0, 1, 2): data read, data write, instruction fetch.
    pub read: bool,
    pub write: bool,
    pub fetch: bool,
    /// What the EPT entry for the GPA allowed (bits 3, 4, 5).
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    /// Bit 7: `GUEST_LINEAR_ADDRESS` is valid.
    pub gla_valid: bool,
    /// Bit 8 (meaningful when `gla_valid`): the access was to the final physical address
    /// of the guest's linear-address translation (1), rather than to one of the guest's own
    /// paging structures (0).
    pub translated: bool,
}

/// Decode Table 28-7. (kernel: `handle_ept_violation` builds `error_code` from the same
/// bits: `EPT_VIOLATION_ACC_READ`, `_WRITE`, `_INSTR`, `_PROT_*`...)
pub fn ept_violation_decode(q: u64) -> ept_violation {
    ept_violation {
        read: q & (1 << 0) != 0,
        write: q & (1 << 1) != 0,
        fetch: q & (1 << 2) != 0,
        readable: q & (1 << 3) != 0,
        writable: q & (1 << 4) != 0,
        executable: q & (1 << 5) != 0,
        gla_valid: q & (1 << 7) != 0,
        translated: q & (1 << 8) != 0,
    }
}

/// Is the violation a pure "not mapped" fault (the entry allowed nothing at all)? A VMM
/// uses this to decide between "populate the page" and "report a protection error".
pub fn ept_violation_is_unmapped(v: &ept_violation) -> bool {
    !v.readable && !v.writable && !v.executable
}

/// The 16-byte INVEPT descriptor (SDM 30.3, INVEPT): bytes 0..8 = EPT pointer, bytes
/// 8..16 reserved (must be 0; the kernel's `__invept` writes a gpa there that the
/// hardware ignores). Little-endian. The instruction's *type* operand
/// (`VMX_EPT_EXTENT_CONTEXT` / `GLOBAL`) goes in a register, not here.
pub fn invept_descriptor(eptp: u64) -> [u8; 16] {
    let mut desc = [0u8; 16];
    desc[0..8].copy_from_slice(&eptp.to_le_bytes());
    desc
}
