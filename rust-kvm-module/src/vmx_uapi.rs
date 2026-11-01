//! The userspace ABI of `/dev/ktvmx`, modelled on KVM's so that a VMM written for
//! /dev/kvm (your `rust-kvm-tool`) can drive your module with small changes.
//! Stage 11. Kernel counterparts: include/uapi/linux/kvm.h (struct kvm_regs, kvm_sregs,
//! kvm_segment, kvm_run, KVM_* ioctls, KVM_EXIT_*), arch/x86/include/uapi/asm/kvm.h,
//! arch/x86/kvm/vmx/vmx.c (vmx_set_segment, vmx_get_segment, vmx_segment_access_rights),
//! virt/kvm/kvm_main.c (kvm_vcpu_ioctl, kvm_vm_ioctl, check_memory_region_flags,
//! kvm_set_memory_region).
//! Book: SDM Vol. 3C 24.4.1 (segment AR format, Table 24-2), 24.4.2; Vol. 3A 3.4.5.
//!
//! The `#[repr(C)]` structs and the numeric constants are PROVIDED (they must match the
//! real KVM byte-for-byte, that is the whole point). You implement the conversions.

use crate::vmcs_ops::VmcsOps;
#[allow(unused_imports)]
use crate::vmx::*;

// ---- ioctl numbers (asm-generic/ioctl.h on x86) ---------------------------------
// bits 7:0 nr, 15:8 type, 29:16 size, 31:30 direction.
pub const IOC_NONE: u32 = 0;
pub const IOC_WRITE: u32 = 1;
pub const IOC_READ: u32 = 2;
/// KVMIO. Same magic as KVM: rust-kvm-tool's `KVMIO`.
pub const KTVMX_IOCTL_TYPE: u32 = 0xae;

pub const KTVMX_API_VERSION: i32 = 12;
pub const KTVMX_GET_API_VERSION: u32 = 0xae00; // _IO(0xAE, 0x00)
pub const KTVMX_CREATE_VM: u32 = 0xae01; // _IO(0xAE, 0x01)
pub const KTVMX_GET_VCPU_MMAP_SIZE: u32 = 0xae04; // _IO(0xAE, 0x04)
pub const KTVMX_SET_USER_MEMORY_REGION: u32 = 0x4020_ae46; // _IOW(0xAE, 0x46, region)
pub const KTVMX_CREATE_VCPU: u32 = 0xae41; // _IO(0xAE, 0x41)
pub const KTVMX_RUN: u32 = 0xae80; // _IO(0xAE, 0x80)
pub const KTVMX_GET_REGS: u32 = 0x8090_ae81; // _IOR(0xAE, 0x81, regs)
pub const KTVMX_SET_REGS: u32 = 0x4090_ae82; // _IOW(0xAE, 0x82, regs)
pub const KTVMX_GET_SREGS: u32 = 0x8138_ae83; // _IOR(0xAE, 0x83, sregs)
pub const KTVMX_SET_SREGS: u32 = 0x4138_ae84; // _IOW(0xAE, 0x84, sregs)

/// The vcpu mmap: page 0 is `ktvmx_run`, page 1 is the PIO data buffer.
pub const KTVMX_PIO_DATA_OFFSET: u64 = 4096;
pub const KTVMX_VCPU_MMAP_SIZE: usize = 8192;

// ---- exit reasons (include/uapi/linux/kvm.h KVM_EXIT_*, same numbers) ----------
pub const KTVMX_EXIT_UNKNOWN: u32 = 0;
pub const KTVMX_EXIT_IO: u32 = 2;
pub const KTVMX_EXIT_HLT: u32 = 5;
pub const KTVMX_EXIT_MMIO: u32 = 6;
pub const KTVMX_EXIT_SHUTDOWN: u32 = 8;
pub const KTVMX_EXIT_FAIL_ENTRY: u32 = 9;
pub const KTVMX_EXIT_INTERNAL_ERROR: u32 = 17;
pub const KTVMX_EXIT_IO_IN: u8 = 0;
pub const KTVMX_EXIT_IO_OUT: u8 = 1;

pub const EINVAL: i32 = 22;
pub const ENOMEM: i32 = 12;

// ---- structs (PROVIDED; layouts equal the real KVM's) --------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ktvmx_regs {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ktvmx_segment {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    pub type_: u8,
    pub present: u8,
    pub dpl: u8,
    pub db: u8,
    pub s: u8,
    pub l: u8,
    pub g: u8,
    pub avl: u8,
    pub unusable: u8,
    pub padding: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ktvmx_dtable {
    pub base: u64,
    pub limit: u16,
    pub padding: [u16; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ktvmx_sregs {
    pub cs: ktvmx_segment,
    pub ds: ktvmx_segment,
    pub es: ktvmx_segment,
    pub fs: ktvmx_segment,
    pub gs: ktvmx_segment,
    pub ss: ktvmx_segment,
    pub tr: ktvmx_segment,
    pub ldt: ktvmx_segment,
    pub gdt: ktvmx_dtable,
    pub idt: ktvmx_dtable,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
    pub apic_base: u64,
    pub interrupt_bitmap: [u64; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ktvmx_userspace_memory_region {
    pub slot: u32,
    pub flags: u32,
    pub guest_phys_addr: u64,
    pub memory_size: u64,
    pub userspace_addr: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ktvmx_exit_io {
    pub direction: u8,
    pub size: u8,
    pub port: u16,
    pub count: u32,
    /// Offset from the start of `ktvmx_run` to the data (KTVMX_PIO_DATA_OFFSET).
    pub data_offset: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ktvmx_exit_mmio {
    pub phys_addr: u64,
    pub data: [u8; 8],
    pub len: u32,
    pub is_write: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ktvmx_exit_fail_entry {
    pub hardware_entry_failure_reason: u64,
    pub cpu: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ktvmx_exit_internal {
    pub suberror: u32,
    pub ndata: u32,
    pub data: [u64; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union ktvmx_run_exit {
    pub io: ktvmx_exit_io,
    pub mmio: ktvmx_exit_mmio,
    pub fail_entry: ktvmx_exit_fail_entry,
    pub internal: ktvmx_exit_internal,
    pub padding: [u8; 256],
}

/// The shared page userspace mmaps from the vcpu fd. First 32 bytes + a 256-byte union
/// is the head of the real `struct kvm_run`; that is what rust-kvm-tool reads.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ktvmx_run {
    pub request_interrupt_window: u8,
    pub immediate_exit: u8,
    pub padding1: [u8; 6],
    pub exit_reason: u32,
    pub ready_for_interrupt_injection: u8,
    pub if_flag: u8,
    pub flags: u16,
    pub cr8: u64,
    pub apic_base: u64,
    pub exit: ktvmx_run_exit,
    pub kvm_valid_regs: u64,
    pub kvm_dirty_regs: u64,
}

impl ktvmx_run {
    pub fn zeroed() -> Self {
        // SAFETY: all fields are plain integers / unions of integers; all-zero is valid.
        unsafe { core::mem::zeroed() }
    }
}

/// What the module's run loop (stage 7) hands back to userspace. This is the logical
/// exit; `exit_to_run` is its serialisation into the shared `ktvmx_run` page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ktvmx_exit {
    /// Non-string port I/O (`in`/`out`): `count` is always 1 here. For `is_out`,
    /// `data[..size]` is the value written; for IN userspace fills the pio buffer.
    Io { is_out: bool, size: u8, port: u16, count: u32, data: [u8; 4] },
    Hlt,
    /// Triple fault.
    Shutdown,
    /// VM entry failed (VM_EXIT_REASON bit 31): `reason` is the full exit reason word.
    FailEntry { reason: u64 },
    InternalError { suberror: u32 },
    Mmio { gpa: u64, is_write: bool, len: u32, data: [u8; 8] },
}

// ---- 1. ioctl number arithmetic --------------------------------------------------

/// Build an ioctl number: `(dir << 30) | (size << 16) | (ty << 8) | nr`
/// (kernel: `_IOC()` in include/uapi/asm-generic/ioctl.h; the x86 layout above).
/// `size` must fit in 14 bits.
pub fn ioc(dir: u32, ty: u32, nr: u32, size: usize) -> u32 {
    let _ = (dir, ty, nr, size);
    todo!("stage 11")
}
pub fn ioc_dir(cmd: u32) -> u32 {
    let _ = cmd;
    todo!("stage 11")
}
pub fn ioc_type(cmd: u32) -> u32 {
    let _ = cmd;
    todo!("stage 11")
}
pub fn ioc_nr(cmd: u32) -> u32 {
    let _ = cmd;
    todo!("stage 11")
}
pub fn ioc_size(cmd: u32) -> usize {
    let _ = cmd;
    todo!("stage 11")
}

// ---- 2. segment <-> access-rights ---------------------------------------------

/// KVM's `struct kvm_segment` -> VMCS access-rights word (SDM Table 24-2):
/// bits 3:0 type, 4 S, 6:5 DPL, 7 P, 12 AVL, 13 L, 14 D/B, 15 G, 16 "unusable".
/// A segment the guest marked unusable, or one that is not present, must have bit 16
/// set (`vmx_segment_access_rights`: `(var->unusable || !var->present) << 16`).
pub fn segment_to_ar(seg: &ktvmx_segment) -> u32 {
    let _ = seg;
    todo!("stage 11")
}

/// The reverse: VMCS (selector, base, limit, access rights) -> `ktvmx_segment`
/// (kernel: vmx_get_segment). `unusable` = AR bit 16; `present` = AR bit 7.
pub fn ar_to_segment(selector: u16, base: u64, limit: u32, ar: u32) -> ktvmx_segment {
    let _ = (selector, base, limit, ar);
    todo!("stage 11")
}

// ---- 3. KTVMX_SET_SREGS / GET_SREGS --------------------------------------------

/// Write `sregs` into the *current VMCS*: selector/base/limit/AR of CS DS ES FS GS SS TR
/// LDTR, GDTR and IDTR base+limit, CR0, CR3, CR4 and EFER (`GUEST_IA32_EFER`).
/// CR2, CR8, APIC base and the interrupt bitmap are not VMCS state: ignore them here.
/// This writes the raw values. The kernel half runs CR0/CR4 through stage 10's
/// `vmx_set_cr0/4` afterwards so the hardware gets its fixed bits. (kernel:
/// kvm_arch_vcpu_ioctl_set_sregs -> __set_sregs_common -> vmx_set_segment, vmx_set_cr0...)
pub fn sregs_to_vmcs<V: VmcsOps>(sregs: &ktvmx_sregs, vmcs: &mut V) {
    let _ = (sregs, vmcs);
    todo!("stage 11: loop over the 8 segments with a table of VMCS field encodings")
}

/// Read the same state back. Fields that are not VMCS state come back as 0.
/// (kernel: __get_sregs -> vmx_get_segment ...)
pub fn vmcs_to_sregs<V: VmcsOps>(vmcs: &V) -> ktvmx_sregs {
    let _ = vmcs;
    todo!("stage 11")
}

// ---- 4. KTVMX_SET_REGS / GET_REGS ----------------------------------------------

/// Software-saved general-purpose registers in x86 encoding order: RAX RCX RDX RBX RSP
/// RBP RSI RDI R8..R15 (the layout `vmx_vcpu_run` of stage 6 saves/restores). RSP's slot
/// is unused, the guest RSP lives in the VMCS.
pub type gprs16 = [u64; 16];

/// Distribute `regs`: RIP -> GUEST_RIP, RSP -> GUEST_RSP, RFLAGS -> GUEST_RFLAGS with
/// the always-one bit 1 forced on (SDM 26.3.1.4), everything else into `gprs`.
/// (kernel: kvm_arch_vcpu_ioctl_set_regs, kvm_register_write)
pub fn regs_to_vmcs<V: VmcsOps>(regs: &ktvmx_regs, vmcs: &mut V, gprs: &mut gprs16) {
    let _ = (regs, vmcs, gprs);
    todo!("stage 11")
}

/// Collect them back. (kernel: kvm_arch_vcpu_ioctl_get_regs)
pub fn vmcs_to_regs<V: VmcsOps>(vmcs: &V, gprs: &gprs16) -> ktvmx_regs {
    let _ = (vmcs, gprs);
    todo!("stage 11")
}

// ---- 5. KTVMX_SET_USER_MEMORY_REGION --------------------------------------------

/// Validate a memory-region request. We support exactly one slot (0) and no flags;
/// `max_size` is how much RAM the module is willing to back. Rules:
///  - `slot != 0` or `flags != 0`         -> `Err(EINVAL)`
///  - gpa, size or userspace_addr not 4 KiB aligned, or size == 0 -> `Err(EINVAL)`
///  - `gpa + size` overflows u64          -> `Err(EINVAL)`
///  - `size > max_size`                   -> `Err(ENOMEM)`
/// (kernel: check_memory_region_flags + __kvm_set_memory_region)
pub fn memslot_check(
    region: &ktvmx_userspace_memory_region,
    max_size: u64,
) -> Result<(), i32> {
    let _ = (region, max_size);
    todo!("stage 11")
}

// ---- 6. KTVMX_RUN result -------------------------------------------------------

/// Serialise `exit` into the shared run page: set `run.exit_reason` and the matching
/// union member (zero the union first), and for an `Io` exit put the data bytes at
/// `pio_page[0..size]` for OUT (IN: leave the page alone) with
/// `data_offset = KTVMX_PIO_DATA_OFFSET`. `FailEntry` fills `hardware_entry_failure_reason`
/// (cpu = 0); `InternalError` fills `suberror` (ndata = 0); `Mmio` fills `phys_addr`,
/// `data`, `len`, `is_write`. (kernel: kvm_run exit setup in handle_io() and friends)
pub fn exit_to_run(exit: &ktvmx_exit, run: &mut ktvmx_run, pio_page: &mut [u8; 4096]) {
    let _ = (exit, run, pio_page);
    todo!("stage 11")
}
