//! Stage 5: guest-state and host-state areas, and the checks the CPU runs on them.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (vmx_set_segment, vmx_get_segment,
//! vmx_segment_access_rights, vmx_set_cr0/vmx_set_efer, enter_rmode,
//! vmx_set_constant_host_state, init_vmcs, kvm_vmx_segment_fields[]).
//! Book: Intel SDM Vol. 3C chapter 24.4 (guest-state area), 24.5 (host-state area),
//! 26.2.2-26.2.4 (checks on host state), 26.3.1 (checks on guest state),
//! Vol. 3A 3.4.5 (segment descriptors, access-rights format: Figure 24-2 in 3C).
#![allow(unused_variables)]

use crate::vmcs_fields::vmcs_field_type;
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
    let f = |selector, base, limit, ar_bytes| vmx_segment_field {
        selector,
        base,
        limit,
        ar_bytes,
    };
    match seg {
        VCPU_SREG_ES => f(
            GUEST_ES_SELECTOR,
            GUEST_ES_BASE,
            GUEST_ES_LIMIT,
            GUEST_ES_AR_BYTES,
        ),
        VCPU_SREG_CS => f(
            GUEST_CS_SELECTOR,
            GUEST_CS_BASE,
            GUEST_CS_LIMIT,
            GUEST_CS_AR_BYTES,
        ),
        VCPU_SREG_SS => f(
            GUEST_SS_SELECTOR,
            GUEST_SS_BASE,
            GUEST_SS_LIMIT,
            GUEST_SS_AR_BYTES,
        ),
        VCPU_SREG_DS => f(
            GUEST_DS_SELECTOR,
            GUEST_DS_BASE,
            GUEST_DS_LIMIT,
            GUEST_DS_AR_BYTES,
        ),
        VCPU_SREG_FS => f(
            GUEST_FS_SELECTOR,
            GUEST_FS_BASE,
            GUEST_FS_LIMIT,
            GUEST_FS_AR_BYTES,
        ),
        VCPU_SREG_GS => f(
            GUEST_GS_SELECTOR,
            GUEST_GS_BASE,
            GUEST_GS_LIMIT,
            GUEST_GS_AR_BYTES,
        ),
        VCPU_SREG_TR => f(
            GUEST_TR_SELECTOR,
            GUEST_TR_BASE,
            GUEST_TR_LIMIT,
            GUEST_TR_AR_BYTES,
        ),
        VCPU_SREG_LDTR => f(
            GUEST_LDTR_SELECTOR,
            GUEST_LDTR_BASE,
            GUEST_LDTR_LIMIT,
            GUEST_LDTR_AR_BYTES,
        ),
        _ => panic!("bad segment register index"),
    }
}

// PROVIDED: access-rights word layout (SDM Figure 24-2 / table 24-2).
// 3:0 type, 4 S, 6:5 DPL, 7 P, 11:8 reserved, 12 AVL, 13 L, 14 D/B, 15 G, 16 unusable.
pub const AR_UNUSABLE: u32 = 1 << 16;
pub fn ar_type(ar: u32) -> u32 {
    ar & 0xf
}
pub fn ar_s(ar: u32) -> bool {
    ar & (1 << 4) != 0
}
pub fn ar_dpl(ar: u32) -> u32 {
    (ar >> 5) & 3
}
pub fn ar_p(ar: u32) -> bool {
    ar & (1 << 7) != 0
}
pub fn ar_avl(ar: u32) -> bool {
    ar & (1 << 12) != 0
}
pub fn ar_l(ar: u32) -> bool {
    ar & (1 << 13) != 0
}
pub fn ar_db(ar: u32) -> bool {
    ar & (1 << 14) != 0
}
pub fn ar_g(ar: u32) -> bool {
    ar & (1 << 15) != 0
}
pub fn ar_unusable(ar: u32) -> bool {
    ar & AR_UNUSABLE != 0
}
/// PROVIDED: 48-bit canonical address test.
pub fn is_canonical(addr: u64) -> bool {
    (((addr as i64) << 16) >> 16) as u64 == addr
}

/// Pack a segment into the VMCS access-rights word (kernel: `vmx_segment_access_rights()`).
/// If the segment is `unusable` *or not present* the word is just `AR_UNUSABLE`
/// (every other bit is then ignored/undefined for the CPU, so write a clean value);
/// otherwise type | S<<4 | DPL<<5 | P<<7 | AVL<<12 | L<<13 | D/B<<14 | G<<15.
pub fn vmx_segment_access_rights(var: &kvm_segment) -> u32 {
    if !var.present || var.unusable {
        return AR_UNUSABLE;
    }

    let mut ar: u32 = var.type_ as u32;
    ar |= (var.s as u32) << 4;
    ar |= (var.dpl as u32) << 5;
    ar |= (var.present as u32) << 7;
    ar |= (var.avl as u32) << 12;
    ar |= (var.l as u32) << 13;
    ar |= (var.db as u32) << 14;
    ar |= (var.g as u32) << 15;

    ar
}

/// Write one guest segment register (kernel: `__vmx_set_segment()`): selector, base,
/// limit and the access rights from `vmx_segment_access_rights`.
pub fn vmx_set_segment(vmcs: &mut impl VmcsOps, seg: usize, var: &kvm_segment) {
    let sf = vmx_segment_fields(seg);
    // base
    vmcs.vmcs_writel(sf.base, var.base);
    // limit
    vmcs.vmcs_write32(sf.limit, var.limit);
    // selector
    vmcs.vmcs_write16(sf.selector, var.selector);
    // access right
    vmcs.vmcs_write32(sf.ar_bytes, vmx_segment_access_rights(var));
}

/// Read one guest segment register (kernel: `vmx_get_segment()`): the inverse of the above.
/// `present` comes from AR bit 7, `unusable` from bit 16.
pub fn vmx_get_segment(vmcs: &impl VmcsOps, seg: usize) -> kvm_segment {
    let sf = vmx_segment_fields(seg);
    let base = vmcs.vmcs_readl(sf.base);
    let limit = vmcs.vmcs_read32(sf.limit);
    let selector = vmcs.vmcs_read16(sf.selector);
    let ar = vmcs.vmcs_read32(sf.ar_bytes);

    kvm_segment {
        base,
        limit,
        selector,
        type_: ar_type(ar) as u8,
        present: ar_p(ar),
        dpl: ar_dpl(ar) as u8,
        db: ar_db(ar),
        s: ar_s(ar),
        l: ar_l(ar),
        g: ar_g(ar),
        avl: ar_avl(ar),
        unusable: ar_unusable(ar),
    }
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
    vmcs.vmcs_write64(VMCS_LINK_POINTER, u64::MAX);
    vmcs.vmcs_write64(GUEST_RIP, rip);
    vmcs.vmcs_write64(GUEST_RSP, rsp);
    vmcs.vmcs_write64(GUEST_RFLAGS, 0x2);
    vmcs.vmcs_write32(GUEST_ACTIVITY_STATE, 0);
    vmcs.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, 0);
    vmcs.vmcs_write64(GUEST_PENDING_DBG_EXCEPTIONS, 0);
    vmcs.vmcs_write64(GUEST_DR7, 0x400);
    vmcs.vmcs_write64(GUEST_IA32_DEBUGCTL, 0);
    vmcs.vmcs_write32(GUEST_SYSENTER_CS, 0);
    vmcs.vmcs_write64(GUEST_SYSENTER_ESP, 0);
    vmcs.vmcs_write64(GUEST_SYSENTER_EIP, 0);
    vmcs.vmcs_write64(GUEST_GDTR_BASE, 0);
    vmcs.vmcs_write64(GUEST_IDTR_BASE, 0);
    vmcs.vmcs_write32(GUEST_GDTR_LIMIT, 0xffff);
    vmcs.vmcs_write32(GUEST_IDTR_LIMIT, 0xffff);
    // LDRT
    let ldrt_segment = kvm_segment {
        unusable: true,
        ..Default::default()
    };
    vmx_set_segment(vmcs, VCPU_SREG_LDTR, &ldrt_segment);

    // TR
    let tr_segment = kvm_segment {
        selector: 0,
        base: 0,
        limit: 0xffff,
        type_: 11,
        s: false,
        present: true,
        ..Default::default()
    };
    vmx_set_segment(vmcs, VCPU_SREG_TR, &tr_segment);
}

fn vmx_config_segments(vmcs: &mut impl VmcsOps, cs: &kvm_segment, others: &kvm_segment) {
    // cs
    vmx_set_segment(vmcs, VCPU_SREG_CS, cs);
    // switch for es/ss/ds/fs/gs
    vmx_set_segment(vmcs, VCPU_SREG_ES, others);
    vmx_set_segment(vmcs, VCPU_SREG_SS, others);
    vmx_set_segment(vmcs, VCPU_SREG_DS, others);
    vmx_set_segment(vmcs, VCPU_SREG_FS, others);
    vmx_set_segment(vmcs, VCPU_SREG_GS, others);
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
    vmx_setup_guest_common(vmcs, rip, rsp);
    let code_segment = kvm_segment {
        base: 0,
        selector: 0,
        limit: 0xffff,
        type_: 0xb,
        s: true,
        present: true,
        ..Default::default()
    };
    let mut others_segment = code_segment.clone();
    others_segment.type_ = 3;
    vmx_config_segments(vmcs, &code_segment, &others_segment);

    vmcs.vmcs_write64(GUEST_CR0, X86_CR0_ET | X86_CR0_NE);
    vmcs.vmcs_write64(CR0_READ_SHADOW, X86_CR0_ET);
    vmcs.vmcs_write64(CR0_GUEST_HOST_MASK, X86_CR0_NE);

    vmcs.vmcs_write64(GUEST_CR4, X86_CR4_VMXE);
    vmcs.vmcs_write64(CR4_READ_SHADOW, 0);
    vmcs.vmcs_write64(CR4_GUEST_HOST_MASK, X86_CR4_VMXE);

    vmcs.vmcs_write64(GUEST_CR3, 0);
    vmcs.vmcs_write64(GUEST_IA32_EFER, 0);

    // This function loads the vmcs field, clears bit and writes back
    vmcs.vmcs_clear_bits(VM_ENTRY_CONTROLS, VM_ENTRY_IA32E_MODE as u64);
}

/// Guest in 32-bit protected mode, flat, paging off (also needs unrestricted guest).
/// As `vmx_setup_guest_real_mode`, with:
/// - CS: selector 0x08, base 0, limit 0xffff_ffff, type 0xb, S=1, present, D=1, G=1;
///   ES/SS/DS/FS/GS: selector 0x10, same but type 3,
/// - `GUEST_CR0` = `PE | NE | ET`, `CR0_READ_SHADOW` = `PE | ET`.
pub fn vmx_setup_guest_protected_mode(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64) {
    vmx_setup_guest_common(vmcs, rip, rsp);
    let code_segment = kvm_segment {
        selector: 0x08,
        base: 0,
        limit: 0xffff_ffff,
        type_: 0xb,
        s: true,
        present: true,
        db: true,
        g: true,
        ..Default::default()
    };
    let mut others_segment = code_segment.clone();
    others_segment.type_ = 3;
    others_segment.selector = 0x10;
    vmx_config_segments(vmcs, &code_segment, &others_segment);

    vmcs.vmcs_write64(GUEST_CR0, X86_CR0_ET | X86_CR0_NE | X86_CR0_PE);
    vmcs.vmcs_write64(CR0_READ_SHADOW, X86_CR0_ET | X86_CR0_PE);
    vmcs.vmcs_write64(CR0_GUEST_HOST_MASK, X86_CR0_NE);

    vmcs.vmcs_write64(GUEST_CR4, X86_CR4_VMXE);
    vmcs.vmcs_write64(CR4_READ_SHADOW, 0);
    vmcs.vmcs_write64(CR4_GUEST_HOST_MASK, X86_CR4_VMXE);

    vmcs.vmcs_write64(GUEST_CR3, 0);
    vmcs.vmcs_write64(GUEST_IA32_EFER, 0);

    // This function loads the vmcs field, clears bit and writes back
    vmcs.vmcs_clear_bits(VM_ENTRY_CONTROLS, VM_ENTRY_IA32E_MODE as u64);
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
    vmx_setup_guest_common(vmcs, rip, rsp);
    let code_segment = kvm_segment {
        selector: 0x08,
        base: 0,
        limit: 0xffff_ffff,
        type_: 0xb,
        s: true,
        present: true,
        l: true,
        db: false,
        g: true,
        ..Default::default()
    };
    let mut others_segment = code_segment.clone();
    others_segment.type_ = 3;
    others_segment.selector = 0x10;
    others_segment.l = false;
    others_segment.db = true;
    vmx_config_segments(vmcs, &code_segment, &others_segment);

    vmcs.vmcs_write64(GUEST_CR0, X86_CR0_ET | X86_CR0_NE | X86_CR0_PE | X86_CR0_PG);
    vmcs.vmcs_write64(CR0_READ_SHADOW, X86_CR0_ET | X86_CR0_PE | X86_CR0_PG);
    vmcs.vmcs_write64(CR0_GUEST_HOST_MASK, X86_CR0_NE);

    vmcs.vmcs_write64(GUEST_CR4, X86_CR4_PAE | X86_CR4_VMXE);
    vmcs.vmcs_write64(CR4_READ_SHADOW, X86_CR4_PAE);
    vmcs.vmcs_write64(CR4_GUEST_HOST_MASK, X86_CR4_VMXE);

    vmcs.vmcs_write64(GUEST_CR3, cr3);
    vmcs.vmcs_write64(GUEST_IA32_EFER, EFER_LME | EFER_LMA);

    // This function loads the vmcs field, set bit and writes back
    vmcs.vmcs_set_bits(VM_ENTRY_CONTROLS, VM_ENTRY_IA32E_MODE as u64);
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
    // 1. Control registers (Natural width / 64-bit)
    vmcs.vmcs_writel(HOST_CR0, hs.cr0);
    vmcs.vmcs_writel(HOST_CR3, hs.cr3);
    vmcs.vmcs_writel(HOST_CR4, hs.cr4);

    // 2. Segment selectors (16-bit)
    // Trap: SDM 26.2.3 requires host segment selectors to have RPL = 0 and TI = 0.
    // We mask with !7 (0xFFF8) to clear the bottom 3 privilege/table bits.
    vmcs.vmcs_write16(HOST_CS_SELECTOR, hs.cs_sel & !7);
    vmcs.vmcs_write16(HOST_SS_SELECTOR, hs.ss_sel & !7);
    vmcs.vmcs_write16(HOST_DS_SELECTOR, hs.ds_sel & !7);
    vmcs.vmcs_write16(HOST_ES_SELECTOR, hs.es_sel & !7);
    vmcs.vmcs_write16(HOST_FS_SELECTOR, hs.fs_sel & !7);
    vmcs.vmcs_write16(HOST_GS_SELECTOR, hs.gs_sel & !7);
    vmcs.vmcs_write16(HOST_TR_SELECTOR, hs.tr_sel & !7);

    // 3. Segment and descriptor table base addresses (Natural width)
    vmcs.vmcs_writel(HOST_FS_BASE, hs.fs_base);
    vmcs.vmcs_writel(HOST_GS_BASE, hs.gs_base);
    vmcs.vmcs_writel(HOST_TR_BASE, hs.tr_base);
    vmcs.vmcs_writel(HOST_GDTR_BASE, hs.gdtr_base);
    vmcs.vmcs_writel(HOST_IDTR_BASE, hs.idtr_base);

    // 4. SYSENTER MSR state
    vmcs.vmcs_write32(HOST_IA32_SYSENTER_CS, hs.sysenter_cs);
    vmcs.vmcs_writel(HOST_IA32_SYSENTER_ESP, hs.sysenter_esp);
    vmcs.vmcs_writel(HOST_IA32_SYSENTER_EIP, hs.sysenter_eip);

    // 5. Extended Feature Enable Register (EFER)
    vmcs.vmcs_writel(HOST_IA32_EFER, hs.efer);

    // 6. Host stack pointer and VM-exit handler entry point RIP
    vmcs.vmcs_writel(HOST_RSP, hs.rsp);
    vmcs.vmcs_writel(HOST_RIP, hs.rip);
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
pub const ERR_EFER_LMA_ENTRY: &str =
    "guest EFER.LMA must equal the IA-32e mode guest entry control";
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
pub fn vmx_check_guest_state(vmcs: &impl VmcsOps, cfg: &vmcs_config) -> Result<(), &'static str> {
    // 1. VMCS link pointer must be all ones (no shadow VMCS)
    if vmcs.vmcs_read64(VMCS_LINK_POINTER) != !0 {
        return Err(ERR_LINK_POINTER);
    }

    let vm_entry_controls = vmcs.vmcs_read32(VM_ENTRY_CONTROLS);
    let cpu_based_vm_exec_control = vmcs.vmcs_read32(CPU_BASED_VM_EXEC_CONTROL);
    let secondary_vm_exec_control =
        if cpu_based_vm_exec_control & CPU_BASED_ACTIVATE_SECONDARY_CONTROLS != 0 {
            vmcs.vmcs_read32(SECONDARY_VM_EXEC_CONTROL)
        } else {
            0
        };
    let unrestricted_guest = secondary_vm_exec_control & SECONDARY_EXEC_UNRESTRICTED_GUEST != 0;
    let ia32e_mode_guest = vm_entry_controls & VM_ENTRY_IA32E_MODE != 0;

    // 2. CR0 fixed bits; unrestricted guest removes PE and PG from fixed0 (SDM 26.3.1.1)
    let cr0 = vmcs.vmcs_readl(GUEST_CR0);
    let mut cr0_fixed0 = cfg.cr0_fixed0;
    if unrestricted_guest {
        cr0_fixed0 &= !(X86_CR0_PE | X86_CR0_PG);
    }
    if (cr0 & cr0_fixed0) != cr0_fixed0 || (cr0 & !cfg.cr0_fixed1) != 0 {
        return Err(ERR_CR0_FIXED);
    }

    // 3. CR0.PG=1 requires CR0.PE=1
    if cr0 & X86_CR0_PG != 0 && cr0 & X86_CR0_PE == 0 {
        return Err(ERR_CR0_PG_NOPE);
    }

    // 4. CR4 fixed bits (no unrestricted-guest exception here)
    let cr4 = vmcs.vmcs_readl(GUEST_CR4);
    if (cr4 & cfg.cr4_fixed0) != cfg.cr4_fixed0 || (cr4 & !cfg.cr4_fixed1) != 0 {
        return Err(ERR_CR4_FIXED);
    }

    // 5. IA-32e guest: CR0.PG=1, CR4.PAE=1
    if ia32e_mode_guest {
        if cr0 & X86_CR0_PG == 0 {
            return Err(ERR_IA32E_PG);
        }
        if cr4 & X86_CR4_PAE == 0 {
            return Err(ERR_IA32E_PAE);
        }
    }

    // 6. CR3 bits 63:52 must be clear
    let cr3 = vmcs.vmcs_readl(GUEST_CR3);
    if cr3 & 0xfff0_0000_0000_0000 != 0 {
        return Err(ERR_CR3_RESERVED);
    }

    // 7. EFER (only when it is loaded on entry)
    if vm_entry_controls & VM_ENTRY_LOAD_IA32_EFER != 0 {
        let efer = vmcs.vmcs_read64(GUEST_IA32_EFER);
        if efer & !(EFER_SCE | EFER_LME | EFER_LMA | EFER_NX) != 0 {
            return Err(ERR_EFER_RESERVED);
        }
        let lma = efer & EFER_LMA != 0;
        let lme = efer & EFER_LME != 0;
        if lma != ia32e_mode_guest {
            return Err(ERR_EFER_LMA_ENTRY);
        }
        if cr0 & X86_CR0_PG != 0 && lma != lme {
            return Err(ERR_EFER_LMA_LME);
        }
    }

    // 8. RFLAGS: bit 1 set; bits 63:22, 15, 5, 3 clear; VM clear in IA-32e or with CR0.PE=0
    let rflags = vmcs.vmcs_readl(GUEST_RFLAGS);
    const RFLAGS_RESERVED: u64 = 0xffff_ffff_ffc0_0000 | (1 << 15) | (1 << 5) | (1 << 3);
    if rflags & 0x2 == 0 || rflags & RFLAGS_RESERVED != 0 {
        return Err(ERR_RFLAGS);
    }
    if (ia32e_mode_guest || cr0 & X86_CR0_PE == 0) && rflags & X86_EFLAGS_VM != 0 {
        return Err(ERR_RFLAGS_VM);
    }

    // 9. RIP: bits 63:32 clear unless IA-32e with CS.L=1, then canonical
    let rip = vmcs.vmcs_readl(GUEST_RIP);
    let cs = vmx_get_segment(vmcs, VCPU_SREG_CS);
    let ss = vmx_get_segment(vmcs, VCPU_SREG_SS);
    if ia32e_mode_guest && cs.l {
        if !is_canonical(rip) {
            return Err(ERR_RIP);
        }
    } else if rip >> 32 != 0 {
        return Err(ERR_RIP);
    }

    // 10. Segments.
    // The privilege rules (RPL/DPL) are waived for an unrestricted guest that is in real mode
    // (CR0.PE=0) or uses a type-3 (read/write data) CS, i.e. a "big real mode" style guest.
    let privilege_waived = unrestricted_guest && (cr0 & X86_CR0_PE == 0 || cs.type_ == 3);
    let rpl = |sel: u16| (sel & 3) as u8;

    // Selector checks come before the access-rights checks in SDM 26.3.1.2.
    if !privilege_waived && rpl(ss.selector) != rpl(cs.selector) {
        return Err(ERR_SS_RPL);
    }

    for &seg in &[
        VCPU_SREG_CS,
        VCPU_SREG_SS,
        VCPU_SREG_DS,
        VCPU_SREG_ES,
        VCPU_SREG_FS,
        VCPU_SREG_GS,
    ] {
        let s = vmx_get_segment(vmcs, seg);
        if s.unusable {
            continue;
        }
        // The raw AR word: kvm_segment drops the reserved bits, so read the field itself.
        let ar = vmcs.vmcs_read32(vmx_segment_fields(seg).ar_bytes);
        if ar & 0x0000_0f00 != 0 || ar & 0xfffe_0000 != 0 {
            return Err(ERR_SEG_AR_RESERVED);
        }
        if !s.present {
            return Err(ERR_SEG_P);
        }
        if !s.s {
            return Err(ERR_SEG_S);
        }
        check_granularity(&s)?;
        let base_ok = match seg {
            VCPU_SREG_FS | VCPU_SREG_GS => is_canonical(s.base),
            _ => s.base >> 32 == 0,
        };
        if !base_ok {
            return Err(ERR_SEG_BASE);
        }

        match seg {
            VCPU_SREG_CS => {
                match s.type_ {
                    3 if unrestricted_guest => {}
                    9 | 11 | 13 | 15 => {}
                    _ => return Err(ERR_SEG_TYPE),
                }
                let dpl_ok = match s.type_ {
                    3 => s.dpl == 0,
                    9 | 11 => s.dpl == ss.dpl,
                    _ => s.dpl <= ss.dpl, // 13 | 15: conforming code
                };
                if !dpl_ok {
                    return Err(ERR_SEG_DPL);
                }
                if ia32e_mode_guest && s.l && s.db {
                    return Err(ERR_CS_L_D);
                }
            }
            VCPU_SREG_SS => {
                if s.type_ != 3 && s.type_ != 7 {
                    return Err(ERR_SEG_TYPE);
                }
                if !privilege_waived && s.dpl != rpl(s.selector) {
                    return Err(ERR_SEG_DPL);
                }
            }
            _ => {
                // DS/ES/FS/GS: accessed, and if code then readable
                if s.type_ & 0x1 == 0 || (s.type_ & 0x8 != 0 && s.type_ & 0x2 == 0) {
                    return Err(ERR_SEG_TYPE);
                }
                // Data and non-conforming code (types 0..=11): DPL >= RPL
                if !privilege_waived && s.type_ <= 11 && s.dpl < rpl(s.selector) {
                    return Err(ERR_SEG_DPL);
                }
            }
        }
    }

    // 11. TR: always checked, must be usable
    let tr = vmx_get_segment(vmcs, VCPU_SREG_TR);
    if tr.selector & 0x4 != 0 {
        return Err(ERR_TR_TI);
    }
    if tr.unusable {
        return Err(ERR_TR_UNUSABLE);
    }
    let tr_type_ok = tr.type_ == 11 || (tr.type_ == 3 && !ia32e_mode_guest);
    if !tr_type_ok || tr.s || !tr.present {
        return Err(ERR_TR_TYPE);
    }
    let tr_ar = vmcs.vmcs_read32(GUEST_TR_AR_BYTES);
    if tr_ar & 0x0000_0f00 != 0 || tr_ar & 0xfffe_0000 != 0 {
        return Err(ERR_SEG_AR_RESERVED);
    }
    check_granularity(&tr)?;
    if !is_canonical(tr.base) {
        return Err(ERR_SEG_BASE);
    }

    // LDTR: only if usable
    let ldtr = vmx_get_segment(vmcs, VCPU_SREG_LDTR);
    if !ldtr.unusable && (ldtr.selector & 0x4 != 0 || ldtr.type_ != 2 || ldtr.s || !ldtr.present) {
        return Err(ERR_LDTR_TYPE);
    }

    // 12. GDTR/IDTR
    for (limit, base) in [
        (GUEST_GDTR_LIMIT, GUEST_GDTR_BASE),
        (GUEST_IDTR_LIMIT, GUEST_IDTR_BASE),
    ] {
        if vmcs.vmcs_read32(limit) & 0xffff_0000 != 0 {
            return Err(ERR_DTR_LIMIT);
        }
        if !is_canonical(vmcs.vmcs_readl(base)) {
            return Err(ERR_DTR_BASE);
        }
    }

    Ok(())
}

/// SDM 26.3.1.2: if any of limit[11:0] is 0 then G must be 0; if any of limit[31:20] is 1
/// then G must be 1.
fn check_granularity(s: &kvm_segment) -> Result<(), &'static str> {
    if (s.limit & 0xfff != 0xfff && s.g) || (s.limit >> 20 != 0 && !s.g) {
        return Err(ERR_SEG_GRANULARITY);
    }
    Ok(())
}
