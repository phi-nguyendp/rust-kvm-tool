//! Stage 5: guest-state and host-state areas, and the checks the CPU runs on them.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (vmx_set_segment, vmx_get_segment,
//! vmx_segment_access_rights, vmx_set_cr0/vmx_set_efer, enter_rmode,
//! vmx_set_constant_host_state, init_vmcs, kvm_vmx_segment_fields[]).
//! Book: Intel SDM Vol. 3C chapter 24.4 (guest-state area), 24.5 (host-state area),
//! 26.2.2-26.2.4 (checks on host state), 26.3.1 (checks on guest state),
//! Vol. 3A 3.4.5 (segment descriptors, access-rights format: Figure 24-2 in 3C).
#![allow(unused_variables)]

use crate::vmcs_ops::*;
use crate::vmx::*;
use crate::vmx_controls::vmcs_config;

// enum { VCPU_SREG_ES, ... } in arch/x86/include/asm/kvm_host.h (same order)
pub const VCPU_SREG_ES: usize = 0;
pub const VCPU_SREG_CS: usize = 1;
pub const VCPU_SREG_SS: usize = 2;
pub const VCPU_SREG_DS: usize = 3;
pub const VCPU_SREG_FS: usize = 4;
pub const VCPU_SREG_GS: usize = 5;
pub const VCPU_SREG_TR: usize = 6;
pub const VCPU_SREG_LDTR: usize = 7;

/// A segment register as userspace/the emulator thinks of it (kernel: `struct kvm_segment`,
/// include/uapi/linux/kvm.h). The VMCS stores the same information as four fields,
/// including a packed 32-bit "access rights" word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct kvm_segment {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    /// descriptor type, 4 bits
    pub type_: u8,
    pub present: bool,
    pub dpl: u8,
    /// D/B: default operand size (code) / big (stack)
    pub db: bool,
    /// S: 1 = code/data segment, 0 = system segment
    pub s: bool,
    /// L: 64-bit code segment
    pub l: bool,
    /// G: limit counts 4 KiB pages
    pub g: bool,
    pub avl: bool,
    /// VMX "segment unusable" (access rights bit 16): the register holds no valid descriptor
    pub unusable: bool,
}

/// The four VMCS fields of one segment register (kernel: `struct kvm_vmx_segment_field`).
#[derive(Clone, Copy, Debug)]
pub struct vmx_segment_field {
    pub selector: u32,
    pub base: u32,
    pub limit: u32,
    pub ar_bytes: u32,
}

/// PROVIDED: kernel `kvm_vmx_segment_fields[]`.
pub fn vmx_segment_fields(seg: usize) -> vmx_segment_field {
    let f = |selector, base, limit, ar_bytes| vmx_segment_field { selector, base, limit, ar_bytes };
    match seg {
        VCPU_SREG_ES => f(GUEST_ES_SELECTOR, GUEST_ES_BASE, GUEST_ES_LIMIT, GUEST_ES_AR_BYTES),
        VCPU_SREG_CS => f(GUEST_CS_SELECTOR, GUEST_CS_BASE, GUEST_CS_LIMIT, GUEST_CS_AR_BYTES),
        VCPU_SREG_SS => f(GUEST_SS_SELECTOR, GUEST_SS_BASE, GUEST_SS_LIMIT, GUEST_SS_AR_BYTES),
        VCPU_SREG_DS => f(GUEST_DS_SELECTOR, GUEST_DS_BASE, GUEST_DS_LIMIT, GUEST_DS_AR_BYTES),
        VCPU_SREG_FS => f(GUEST_FS_SELECTOR, GUEST_FS_BASE, GUEST_FS_LIMIT, GUEST_FS_AR_BYTES),
        VCPU_SREG_GS => f(GUEST_GS_SELECTOR, GUEST_GS_BASE, GUEST_GS_LIMIT, GUEST_GS_AR_BYTES),
        VCPU_SREG_TR => f(GUEST_TR_SELECTOR, GUEST_TR_BASE, GUEST_TR_LIMIT, GUEST_TR_AR_BYTES),
        VCPU_SREG_LDTR => {
            f(GUEST_LDTR_SELECTOR, GUEST_LDTR_BASE, GUEST_LDTR_LIMIT, GUEST_LDTR_AR_BYTES)
        }
        _ => panic!("bad segment register index"),
    }
}

// PROVIDED: access-rights word layout (SDM Figure 24-2 / table 24-2).
// 3:0 type, 4 S, 6:5 DPL, 7 P, 11:8 reserved, 12 AVL, 13 L, 14 D/B, 15 G, 16 unusable.
pub const AR_UNUSABLE: u32 = 1 << 16;
pub fn ar_type(ar: u32) -> u32 { ar & 0xf }
pub fn ar_s(ar: u32) -> bool { ar & (1 << 4) != 0 }
pub fn ar_dpl(ar: u32) -> u32 { (ar >> 5) & 3 }
pub fn ar_p(ar: u32) -> bool { ar & (1 << 7) != 0 }
pub fn ar_avl(ar: u32) -> bool { ar & (1 << 12) != 0 }
pub fn ar_l(ar: u32) -> bool { ar & (1 << 13) != 0 }
pub fn ar_db(ar: u32) -> bool { ar & (1 << 14) != 0 }
pub fn ar_g(ar: u32) -> bool { ar & (1 << 15) != 0 }
pub fn ar_unusable(ar: u32) -> bool { ar & AR_UNUSABLE != 0 }
/// PROVIDED: 48-bit canonical address test.
pub fn is_canonical(addr: u64) -> bool {
    (((addr as i64) << 16) >> 16) as u64 == addr
}

/// Pack a segment into the VMCS access-rights word (kernel: `vmx_segment_access_rights()`).
/// If the segment is `unusable` *or not present* the word is just `AR_UNUSABLE`
/// (every other bit is then ignored/undefined for the CPU, so write a clean value);
/// otherwise type | S<<4 | DPL<<5 | P<<7 | AVL<<12 | L<<13 | D/B<<14 | G<<15.
pub fn vmx_segment_access_rights(var: &kvm_segment) -> u32 {
    todo!()
}

/// Write one guest segment register (kernel: `__vmx_set_segment()`): selector, base,
/// limit and the access rights from `vmx_segment_access_rights`.
pub fn vmx_set_segment(vmcs: &mut impl VmcsOps, seg: usize, var: &kvm_segment) {
    todo!()
}

/// Read one guest segment register (kernel: `vmx_get_segment()`): the inverse of the above.
/// `present` comes from AR bit 7, `unusable` from bit 16.
pub fn vmx_get_segment(vmcs: &impl VmcsOps, seg: usize) -> kvm_segment {
    todo!()
}

/// State shared by every guest mode (kernel: the guest half of `init_vmcs()` plus
/// `vmx_vcpu_reset()`). Writes:
/// - `VMCS_LINK_POINTER` = all ones (no shadow VMCS; anything else makes VM entry fail),
/// - `GUEST_RIP`/`GUEST_RSP` from the arguments, `GUEST_RFLAGS` = 2 (bit 1 is reserved-1),
/// - `GUEST_ACTIVITY_STATE`, `GUEST_INTERRUPTIBILITY_INFO`, `GUEST_PENDING_DBG_EXCEPTIONS` = 0,
///   `GUEST_DR7` = 0x400, `GUEST_IA32_DEBUGCTL` = 0, `GUEST_SYSENTER_CS/ESP/EIP` = 0,
/// - `GUEST_GDTR_*`/`GUEST_IDTR_*`: base 0, limit 0xffff,
/// - LDTR: unusable (`kvm_segment { unusable: true, ..Default::default() }`),
/// - TR: selector 0, base 0, limit 0xffff, type 11 (busy 32/64-bit TSS), S=0, present:
///   the CPU insists on a usable TR even if the guest never uses tasks.
pub fn vmx_setup_guest_common(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64) {
    todo!()
}

/// Guest in real mode with *unrestricted guest* (needs EPT, stage 8; kernel: `enter_rmode`
/// minus the vm86 emulation that pre-Westmere CPUs needed). `rip`/`rsp` are offsets into
/// segment 0. Writes `vmx_setup_guest_common`, then:
/// - CS: selector 0, base 0, limit 0xffff, type 0xb (execute/read, accessed), S=1, present;
///   ES/SS/DS/FS/GS likewise but type 3 (read/write, accessed),
/// - `GUEST_CR0` = `NE | ET` (the CPU insists on NE=1 via IA32_VMX_CR0_FIXED0),
///   `CR0_READ_SHADOW` = `ET` (what the guest *sees*), `CR0_GUEST_HOST_MASK` = `NE`
///   (bits owned by the host: the guest reads them from the shadow, writing them exits),
/// - `GUEST_CR4` = `VMXE` (same fixed-bit rule), `CR4_READ_SHADOW` = 0,
///   `CR4_GUEST_HOST_MASK` = `VMXE`, `GUEST_CR3` = 0,
/// - `GUEST_IA32_EFER` = 0 and the `VM_ENTRY_IA32E_MODE` bit of `VM_ENTRY_CONTROLS` cleared
///   (read-modify-write with `vmcs_clear_bits`).
pub fn vmx_setup_guest_real_mode(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64) {
    todo!()
}

/// Guest in 32-bit protected mode, flat, paging off (also needs unrestricted guest).
/// As `vmx_setup_guest_real_mode`, with:
/// - CS: selector 0x08, base 0, limit 0xffff_ffff, type 0xb, S=1, present, D=1, G=1;
///   ES/SS/DS/FS/GS: selector 0x10, same but type 3,
/// - `GUEST_CR0` = `PE | NE | ET`, `CR0_READ_SHADOW` = `PE | ET`.
pub fn vmx_setup_guest_protected_mode(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64) {
    todo!()
}

/// Guest in 64-bit long mode, paging on with the guest page tables at `cr3` (works with or
/// without unrestricted guest: PE and PG are 1 anyway; kernel: `vmx_set_efer` +
/// `vmx_set_cr0` + `vmx_set_cr4` for a long-mode vcpu). As `vmx_setup_guest_common`, with:
/// - CS: selector 0x08, base 0, limit 0xffff_ffff, type 0xb, S=1, present, **L=1, D=0**, G=1;
///   ES/SS/DS/FS/GS: selector 0x10, type 3, S=1, present, D/B=1, G=1, limit 0xffff_ffff,
/// - `GUEST_CR0` = `PE | PG | NE | ET`, shadow `PE | PG | ET`, host mask `NE`,
/// - `GUEST_CR4` = `PAE | VMXE`, shadow `PAE`, host mask `VMXE`; `GUEST_CR3` = `cr3`,
/// - `GUEST_IA32_EFER` = `LME | LMA` and `VM_ENTRY_IA32E_MODE` set in `VM_ENTRY_CONTROLS`.
pub fn vmx_setup_guest_long_mode(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64, cr3: u64) {
    todo!()
}

/// What the CPU must restore on VM exit: a snapshot of the host (kernel: the values
/// `vmx_set_constant_host_state()` / `vmx_vcpu_load_vmcs()` read from the running CPU).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct host_state {
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cs_sel: u16,
    pub ss_sel: u16,
    pub ds_sel: u16,
    pub es_sel: u16,
    pub fs_sel: u16,
    pub gs_sel: u16,
    pub tr_sel: u16,
    pub fs_base: u64,
    pub gs_base: u64,
    pub tr_base: u64,
    pub gdtr_base: u64,
    pub idtr_base: u64,
    pub sysenter_cs: u32,
    pub sysenter_esp: u64,
    pub sysenter_eip: u64,
    pub efer: u64,
    /// where the CPU resumes the host after a VM exit (stage 6)
    pub rsp: u64,
    pub rip: u64,
}

/// Write the host-state area (kernel: `vmx_set_constant_host_state`). All fields map 1:1 to
/// `HOST_*`, with **one trap**: the host segment selectors must have RPL = 0 and TI = 0
/// (SDM 26.2.3), but the running host's DS/ES are often `0x2b` (RPL 3): write
/// `sel & !7`. Also `HOST_IA32_SYSENTER_*`, `HOST_IA32_EFER`, `HOST_RSP`, `HOST_RIP`.
pub fn vmx_set_host_state(vmcs: &mut impl VmcsOps, hs: &host_state) {
    todo!()
}

// Why VM entry would fail. PROVIDED so tests and you agree on the strings.
pub const ERR_LINK_POINTER: &str = "VMCS link pointer must be all ones";
pub const ERR_CR0_FIXED: &str = "guest CR0 violates IA32_VMX_CR0_FIXED0/1";
pub const ERR_CR0_PG_NOPE: &str = "guest CR0.PG=1 requires CR0.PE=1";
pub const ERR_CR4_FIXED: &str = "guest CR4 violates IA32_VMX_CR4_FIXED0/1";
pub const ERR_CR3_RESERVED: &str = "guest CR3 bits 63:52 must be 0";
pub const ERR_IA32E_PG: &str = "IA-32e mode guest requires CR0.PG=1";
pub const ERR_IA32E_PAE: &str = "IA-32e mode guest requires CR4.PAE=1";
pub const ERR_EFER_RESERVED: &str = "guest IA32_EFER reserved bits must be 0";
pub const ERR_EFER_LMA_ENTRY: &str = "guest EFER.LMA must equal the IA-32e mode guest entry control";
pub const ERR_EFER_LMA_LME: &str = "guest EFER.LMA must equal EFER.LME when CR0.PG=1";
pub const ERR_RFLAGS: &str = "guest RFLAGS: bit 1 must be 1, reserved bits must be 0";
pub const ERR_RFLAGS_VM: &str = "guest RFLAGS.VM must be 0 in IA-32e mode or with CR0.PE=0";
pub const ERR_RIP: &str = "guest RIP not valid for this mode";
pub const ERR_CS_L_D: &str = "IA-32e guest with CS.L=1 must have CS.D=0";
pub const ERR_SS_RPL: &str = "SS.RPL must equal CS.RPL";
pub const ERR_SEG_AR_RESERVED: &str = "segment access rights reserved bits must be 0";
pub const ERR_SEG_GRANULARITY: &str = "segment limit and G bit are inconsistent";
pub const ERR_SEG_TYPE: &str = "invalid segment type";
pub const ERR_SEG_S: &str = "usable CS/SS/DS/ES/FS/GS must have S=1";
pub const ERR_SEG_P: &str = "usable segment must be present";
pub const ERR_SEG_BASE: &str = "invalid segment base address";
pub const ERR_SEG_DPL: &str = "invalid segment DPL";
pub const ERR_TR_UNUSABLE: &str = "TR must be usable";
pub const ERR_TR_TI: &str = "TR selector must have TI=0";
pub const ERR_TR_TYPE: &str = "TR must be a busy TSS (S=0, present)";
pub const ERR_LDTR_TYPE: &str = "usable LDTR must be type 2, S=0, present, TI=0";
pub const ERR_DTR_LIMIT: &str = "GDTR/IDTR limit bits 31:16 must be 0";
pub const ERR_DTR_BASE: &str = "GDTR/IDTR base must be canonical";

/// Run the checks the CPU performs on the guest-state area (SDM 26.3.1), in this order, and
/// return the first violated rule's `ERR_*`. A subset, but one that catches the mistakes
/// people actually make. Use `cfg` for the CR fixed bits; read the *entry controls*
/// (`VM_ENTRY_CONTROLS`: IA-32e mode guest, load-EFER) and *unrestricted guest*
/// (`SECONDARY_VM_EXEC_CONTROL`, but only if `CPU_BASED_VM_EXEC_CONTROL` activates it)
/// from the VMCS itself, because that is what the CPU looks at.
///
/// 1. `VMCS_LINK_POINTER` == !0                                          -> `ERR_LINK_POINTER`
/// 2. CR0: `(cr0 & fixed0) == fixed0` and `cr0 & !fixed1 == 0`, except that with
///    unrestricted guest `PE` and `PG` are removed from `fixed0` first   -> `ERR_CR0_FIXED`
/// 3. CR0.PG=1 needs CR0.PE=1                                            -> `ERR_CR0_PG_NOPE`
/// 4. CR4 against its fixed bits                                         -> `ERR_CR4_FIXED`
/// 5. IA-32e guest: CR0.PG=1, CR4.PAE=1                                  -> `ERR_IA32E_PG`, `ERR_IA32E_PAE`
/// 6. CR3 bits 63:52 clear                                               -> `ERR_CR3_RESERVED`
/// 7. if the *load IA32_EFER* entry control is on: only SCE/LME/LMA/NX may be set
///    (`ERR_EFER_RESERVED`); LMA == IA-32e entry control (`ERR_EFER_LMA_ENTRY`); if CR0.PG,
///    LMA == LME (`ERR_EFER_LMA_LME`)
/// 8. RFLAGS: bit 1 set and bits 63:22, 15, 5, 3 clear (`ERR_RFLAGS`); VM (bit 17) clear in
///    IA-32e mode or when CR0.PE=0 (`ERR_RFLAGS_VM`)
/// 9. RIP: if not (IA-32e mode and CS.L=1) bits 63:32 clear, else canonical (`ERR_RIP`)
/// 10. segments, in order CS, SS, DS, ES, FS, GS (usable ones only, i.e. unusable bit clear),
///     with `ar`/`limit`/`base`/`selector` taken from the VMCS:
///     reserved AR bits 11:8 and 31:17 clear (`ERR_SEG_AR_RESERVED`); present
///     (`ERR_SEG_P`); S=1 (`ERR_SEG_S`); granularity: if limit[11:0] != 0xfff then G must be 0,
///     if limit[31:20] != 0 then G must be 1 (`ERR_SEG_GRANULARITY`); base: CS/SS/DS/ES bits
///     63:32 clear, FS/GS canonical (`ERR_SEG_BASE`); type: CS in {3,9,11,13,15} (3 only with
///     unrestricted guest), SS in {3,7}, DS/ES/FS/GS accessed (bit 0) and, if code (bit 3),
///     readable (bit 1) (`ERR_SEG_TYPE`); DPL: CS type 3 -> 0, types 9/11 -> == SS.DPL,
///     13/15 -> <= SS.DPL; data segments (types 0..=11) DPL >= selector RPL
///     (`ERR_SEG_DPL`), SS.DPL == SS.RPL; the SS/DPL/data-DPL rules are waived with
///     unrestricted guest when CR0.PE=0 or CS type is 3. SS.RPL == CS.RPL (same waiver;
///     `ERR_SS_RPL`). IA-32e with CS.L=1 needs CS.D=0 (`ERR_CS_L_D`).
/// 11. TR (always checked): selector TI clear (`ERR_TR_TI`), usable (`ERR_TR_UNUSABLE`), type
///     11 (or 3 outside IA-32e), S=0, present (`ERR_TR_TYPE`), reserved AR bits, granularity,
///     base canonical. LDTR if usable: selector TI clear, type 2, S=0, present
///     (`ERR_LDTR_TYPE`).
/// 12. GDTR/IDTR: limit bits 31:16 clear (`ERR_DTR_LIMIT`), base canonical (`ERR_DTR_BASE`).
pub fn vmx_check_guest_state(
    vmcs: &impl VmcsOps,
    cfg: &vmcs_config,
) -> Result<(), &'static str> {
    todo!()
}
