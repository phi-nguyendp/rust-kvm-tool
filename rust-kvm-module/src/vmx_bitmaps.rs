//! Intercept configuration: I/O bitmaps, MSR bitmap, CR-access exits (guest/host
//! masks + read shadows) and VPID. Stage 10.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (vmx_disable_intercept_for_msr,
//! vmx_enable_intercept_for_msr, handle_cr, vmx_set_cr0, vmx_set_cr4, allocate_vpid,
//! free_vpid), arch/x86/kvm/vmx/vmx.h (vmx_msr_bitmap_*), arch/x86/include/asm/vmx.h
//! (VMX_VPID_EXTENT_*), arch/x86/kvm/vmx/vmx_ops.h (__invvpid).
//! Book: SDM Vol. 3C 25.6.4 (I/O bitmaps), 25.6.9 (MSR bitmaps), 25.1.3 + 25.6.6
//! (guest/host masks, read shadows), Table 27-3 (CR-access exit qualification),
//! 28.1 (VPID), INVVPID in 30.3.
//!
//! Everything here is pure bit-twiddling over byte arrays and a `VmcsOps`, so it is
//! host-testable. `module/s10_bitmaps.rs` allocates the pages and programs the VMCS.

use crate::vmcs_ops::VmcsOps;
#[allow(unused_imports)]
use crate::vmx::*;

// ---- I/O bitmaps (SDM 25.6.4) ---------------------------------------------------
// Two 4 KiB pages. Bitmap A covers ports 0x0000-0x7fff, bitmap B 0x8000-0xffff,
// one bit per port, bit set = a guest access to that port causes a VM exit.

/// Mark ports `port .. port+len` (len is the access size in bytes: 1, 2 or 4) as
/// intercepted (`intercept == true`) or passed through. Ports that would lie above
/// 0xffff are ignored here (see `io_bitmap_intercepted` for what the CPU does).
/// (kernel: there is no single function; vmx.c fills the bitmaps with memset/__set_bit)
pub fn io_bitmap_set(
    bitmap_a: &mut [u8; 4096],
    bitmap_b: &mut [u8; 4096],
    port: u16,
    len: u8,
    intercept: bool,
) {
    let _ = (bitmap_a, bitmap_b, port, len, intercept);
    todo!("stage 10: per-port bit in A (port < 0x8000) or B (port & 0x7fff)")
}

/// Would an I/O instruction of `len` bytes at `port` cause a VM exit because of the
/// bitmaps? SDM 25.1.3: it exits if *any* accessed port's bit is 1, and an access
/// that touches a port above 0xffff always exits. (`port as u32 + len > 0x10000`.)
pub fn io_bitmap_intercepted(
    bitmap_a: &[u8; 4096],
    bitmap_b: &[u8; 4096],
    port: u16,
    len: u8,
) -> bool {
    let _ = (bitmap_a, bitmap_b, port, len);
    todo!("stage 10: OR of the bits of every accessed port; wrap past 0xffff => true")
}

// ---- MSR bitmap (SDM 25.6.9) ----------------------------------------------------
// One 4 KiB page split in four 1 KiB quarters, one bit per MSR:
//   0x000: reads  of MSRs 0x00000000-0x00001fff
//   0x400: reads  of MSRs 0xc0000000-0xc0001fff
//   0x800: writes of MSRs 0x00000000-0x00001fff
//   0xc00: writes of MSRs 0xc0000000-0xc0001fff
// Bit set = rdmsr/wrmsr of that MSR exits. MSRs outside both ranges ALWAYS exit.

pub const MSR_TYPE_R: u8 = 1;
pub const MSR_TYPE_W: u8 = 2;
pub const MSR_TYPE_RW: u8 = 3;

/// Start of the "all MSRs intercepted" state: a fresh bitmap must be all ones,
/// otherwise the guest could read/write any MSR directly. (kernel: memset(.., 0xff))
pub fn msr_bitmap_init_all_intercept(bitmap: &mut [u8; 4096]) {
    let _ = bitmap;
    todo!("stage 10")
}

/// Location of the bit that controls `msr` for a read (`write == false`) or a write:
/// `Some((byte_index, bit_in_byte))`, or `None` if the MSR is outside the two ranges.
pub fn msr_bitmap_pos(msr: u32, write: bool) -> Option<(usize, u8)> {
    let _ = (msr, write);
    todo!("stage 10: quarter offset 0x000/0x400/0x800/0xc00 + (msr & 0x1fff) / 8")
}

/// Let the guest access `msr` without exiting for the directions in `ty`
/// (`MSR_TYPE_R`, `MSR_TYPE_W` or `MSR_TYPE_RW`). Out-of-range MSRs: no-op (they
/// stay intercepted, the hardware gives you no choice). (kernel: vmx_disable_intercept_for_msr)
pub fn vmx_disable_intercept_for_msr(bitmap: &mut [u8; 4096], msr: u32, ty: u8) {
    let _ = (bitmap, msr, ty);
    todo!("stage 10: clear the bit(s)")
}

/// Make accesses to `msr` exit again. (kernel: vmx_enable_intercept_for_msr)
pub fn vmx_enable_intercept_for_msr(bitmap: &mut [u8; 4096], msr: u32, ty: u8) {
    let _ = (bitmap, msr, ty);
    todo!("stage 10: set the bit(s)")
}

/// Does a read (`write == false`) / write of `msr` cause a VM exit with this bitmap?
/// (kernel: msr_write_intercepted / vmx_get_..., the "is it intercepted" query used
/// when merging L1 and L2 bitmaps). Out-of-range MSRs: `true`.
pub fn msr_bitmap_intercepted(bitmap: &[u8; 4096], msr: u32, write: bool) -> bool {
    let _ = (bitmap, msr, write);
    todo!("stage 10")
}

// ---- CR-access exits (SDM Table 27-3) -------------------------------------------

pub const CR_ACCESS_MOV_TO_CR: u8 = 0;
pub const CR_ACCESS_MOV_FROM_CR: u8 = 1;
pub const CR_ACCESS_CLTS: u8 = 2;
pub const CR_ACCESS_LMSW: u8 = 3;

/// Decoded EXIT_QUALIFICATION of a CR-access exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct cr_access {
    /// Control register number (0, 3, 4 or 8).
    pub cr: u8,
    /// One of `CR_ACCESS_*`.
    pub access_type: u8,
    /// General-purpose register for MOV CR: 0=RAX 1=RCX 2=RDX 3=RBX 4=RSP 5=RBP
    /// 6=RSI 7=RDI 8..15=R8..R15 (the usual x86 encoding, NOT the kernel's old order).
    pub gpr: u8,
    /// LMSW source data (16 bits).
    pub lmsw_data: u16,
    /// LMSW operand is memory (true) or register (false).
    pub lmsw_mem: bool,
}

/// Bits 3:0 cr number, 5:4 access type, 6 LMSW operand type, 11:8 register,
/// 31:16 LMSW source data. (kernel: handle_cr() open-codes this with
/// `exit_qualification & 15`, `(exit_qualification >> 4) & 3`, ...)
pub fn vmx_decode_cr_access(exit_qualification: u64) -> cr_access {
    let _ = exit_qualification;
    todo!("stage 10")
}

// The CR0/CR4 values the guest and the hardware see differ. For a CR bit owned by the
// host (guest/host mask bit = 1) the *hardware* value is what we program into
// GUEST_CR0/4, and the *read shadow* is what the guest believes it wrote.

/// Bits that VMX requires to be 1/0 in CR0/CR4 while in VMX operation (stage 1 read
/// these from IA32_VMX_CR*_FIXED{0,1}; with unrestricted guest the caller already
/// relaxed CR0.PE/PG in `cr0_fixed0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct vmx_cr_fixed {
    pub cr0_fixed0: u64,
    pub cr0_fixed1: u64,
    pub cr4_fixed0: u64,
    pub cr4_fixed1: u64,
}

/// Program GUEST_CR0 for a guest that asked for `cr0`: the read shadow gets `cr0`
/// verbatim (what the guest will read back), the hardware value gets the fixed bits
/// forced (`(cr0 | fixed0) & fixed1`). Also installs the guest/host mask = every bit
/// the VMM forced or must see (`fixed0 | !fixed1`). (kernel: vmx_set_cr0)
pub fn vmx_set_cr0<V: VmcsOps>(vmcs: &mut V, cr0: u64, fixed: &vmx_cr_fixed) {
    let _ = (vmcs, cr0, fixed);
    todo!("stage 10: CR0_READ_SHADOW, GUEST_CR0, CR0_GUEST_HOST_MASK")
}

/// Same for CR4. The guest must never see (or clear) CR4.VMXE in the read shadow
/// path *unless it set it itself*; hardware always runs with `fixed0` bits (VMXE is
/// in `cr4_fixed0` while VMX is on). (kernel: vmx_set_cr4)
pub fn vmx_set_cr4<V: VmcsOps>(vmcs: &mut V, cr4: u64, fixed: &vmx_cr_fixed) {
    let _ = (vmcs, cr4, fixed);
    todo!("stage 10: CR4_READ_SHADOW, GUEST_CR4, CR4_GUEST_HOST_MASK")
}

/// What `mov %cr0, %reg` / `mov %cr4, %reg` returns to the guest: bits owned by the
/// host (mask = 1) come from the read shadow, the other bits from the real guest CR.
/// SDM 25.6.6 / 27.x. (kernel: kvm_read_cr0_bits / vmx_cache_reg combination)
pub fn vmx_guest_cr_view(guest_cr: u64, shadow: u64, guest_host_mask: u64) -> u64 {
    let _ = (guest_cr, shadow, guest_host_mask);
    todo!("stage 10")
}

/// Would a guest `mov %reg, %cr0/cr4` of `new_value` cause a VM exit given the mask and
/// the current read shadow? SDM 25.1.3: yes iff it changes a mask-owned bit relative
/// to the shadow. (A guest write that only touches bits the guest owns never exits;
/// with the read shadow equal to the written value, even host-owned bits don't.)
pub fn cr_write_causes_exit(guest_host_mask: u64, read_shadow: u64, new_value: u64) -> bool {
    let _ = (guest_host_mask, read_shadow, new_value);
    todo!("stage 10")
}

/// LMSW loads only CR0 bits 3:0 (PE, MP, EM, TS) and can set but never clear PE.
pub fn lmsw_result(old_cr0: u64, data: u16) -> u64 {
    let _ = (old_cr0, data);
    todo!("stage 10")
}

/// General-purpose register file order used by `handle_cr_access`: index = the x86
/// encoding in the exit qualification (RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI, R8-R15).
/// RSP's slot is NOT kept up to date: the real guest RSP lives in GUEST_RSP.
pub type gprs_t = [u64; 16];

pub const VCPU_REGS_RSP: usize = 4;

/// Emulate a CR-access exit. Handles: MOV to CR0/CR4 (`vmx_set_cr0/4`), MOV to CR3
/// (write GUEST_CR3; we do not need CR3-load exiting with EPT, but handle it), MOV
/// from CR0/CR3/CR4 (use `vmx_guest_cr_view`), CLTS, LMSW. Reading RSP as the source
/// register must use `GUEST_RSP`. Returns `Err(())` for CR8 (unsupported here) or an
/// impossible encoding. The caller then skips the instruction (stage 7).
/// (kernel: handle_cr)
pub fn handle_cr_access<V: VmcsOps>(
    vmcs: &mut V,
    gprs: &mut gprs_t,
    exit_qualification: u64,
    fixed: &vmx_cr_fixed,
) -> Result<(), ()> {
    let _ = (vmcs, gprs, exit_qualification, fixed);
    todo!("stage 10: decode, then one match arm per access type")
}

// ---- VPID (SDM 28.1) ------------------------------------------------------------
// A 16-bit tag (VIRTUAL_PROCESSOR_ID) on cached translations so the TLB need not be
// flushed on every VM entry/exit. VPID 0 is reserved for the host.

/// Bitmap allocator over the 16-bit VPID space. (kernel: allocate_vpid/free_vpid
/// with `vmx_vpid_bitmap`.)
pub struct vpid_allocator {
    pub bitmap: [u64; 1024],
}

impl vpid_allocator {
    /// Empty allocator with VPID 0 already taken.
    pub fn new() -> Self {
        todo!("stage 10: [0; 1024] with bit 0 set")
    }
    /// Lowest free VPID in 1..=65535, or `None` when exhausted.
    pub fn vpid_alloc(&mut self) -> Option<u16> {
        todo!("stage 10")
    }
    /// Release a VPID. Freeing 0 or a VPID that is not allocated is a no-op.
    pub fn vpid_free(&mut self, vpid: u16) {
        let _ = vpid;
        todo!("stage 10")
    }
    pub fn vpid_in_use(&self, vpid: u16) -> bool {
        let _ = vpid;
        todo!("stage 10: bit test")
    }
}

// INVVPID types (SDM 30.3 INVVPID; asm/vmx.h VMX_VPID_EXTENT_*)
pub const VMX_VPID_EXTENT_INDIVIDUAL_ADDR: u64 = 0;
pub const VMX_VPID_EXTENT_SINGLE_CONTEXT: u64 = 1;
pub const VMX_VPID_EXTENT_ALL_CONTEXT: u64 = 2;
pub const VMX_VPID_EXTENT_SINGLE_NON_GLOBAL: u64 = 3;

/// The 128-bit INVVPID descriptor as two little-endian qwords: bits 15:0 of the first
/// are the VPID, bits 63:16 must be zero (else the instruction fails); the second is
/// the linear address (only used by the individual-address type).
/// (kernel: struct { u64 vpid : 16; u64 rsvd : 48; u64 gva; } in __invvpid)
pub fn invvpid_descriptor(vpid: u16, linear_addr: u64) -> [u64; 2] {
    let _ = (vpid, linear_addr);
    todo!("stage 10")
}

/// Does IA32_VMX_EPT_VPID_CAP (stage 1 reads it) allow INVVPID of this extent?
/// Bit 32: INVVPID supported at all; 40: individual-address; 41: single-context;
/// 42: all-context; 43: single-context retaining globals. (SDM A.10;
/// kernel: cpu_has_vmx_invvpid_*)
pub fn invvpid_type_supported(ept_vpid_cap: u64, ty: u64) -> bool {
    let _ = (ept_vpid_cap, ty);
    todo!("stage 10")
}
