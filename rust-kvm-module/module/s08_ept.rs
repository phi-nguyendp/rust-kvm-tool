// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 08: EPT for a real guest.
//! Kernel counterparts: arch/x86/kvm/vmx/vmx.c (`vmx_vcpu_reset`/`vmx_set_cr0` for the
//! unrestricted-guest bits, `handle_ept_violation`, `construct_eptp`), arch/x86/kvm/vmx/vmx_ops.h
//! (`__invept`), arch/x86/kvm/mmu/tdp_mmu.c (what we do by hand: populate on violation).
//! NOT COMPILED by the course author (see CLAUDE.md): expect a small fix or two.
//! Uses the stage-6 interface assumed in `s07_exit.rs` (`vcpu_vmx::new_flat32`, `.run()`,
//! `.regs`); the guest is switched to real mode here, which requires EPT + "unrestricted guest".
//!
//! What the self test does: a real-mode guest (guest/08_ept.S) runs in 1 MiB of guest RAM that
//! is reachable only through the EPT you build, except for one page, GPA 0x9000, that the host
//! leaves out. The guest's store to it must cause an EPT violation; the host maps the page
//! (demand paging, exactly what KVM does for every page of guest RAM) and resumes.

#[allow(unused_imports)]
use core::arch::asm;

use kernel::prelude::*;

use crate::{
    hw::{self, PageBox},
    s03_vmcs::HwVmcs,
    s06_run::vcpu_vmx,
    vmcs_ops::{GuestMem, VmcsOps},
    vmx::*,
    vmx_ept::*,
    vmx_exit::*,
};

pub(crate) const GUEST_RAM_PAGES: usize = 256; // 1 MiB of guest-physical RAM at GPA 0

/// `GuestMem` over *host-physical* addresses, through the kernel's direct map
/// (virt = phys + off). Only ever used on pages we allocated ourselves (EPT tables).
pub(crate) struct host_phys {
    off: u64,
}

impl GuestMem for host_phys {
    fn read(&self, pa: u64, buf: &mut [u8]) -> bool {
        // SAFETY: the EPT code only follows addresses of tables we built from PageBoxes.
        unsafe { core::ptr::copy_nonoverlapping(pa.wrapping_add(self.off) as *const u8, buf.as_mut_ptr(), buf.len()) };
        true
    }
    fn write(&mut self, pa: u64, buf: &[u8]) -> bool {
        // SAFETY: as above.
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), pa.wrapping_add(self.off) as *mut u8, buf.len()) };
        true
    }
}

/// Table pages for the walker: each is a zeroed `PageBox`, kept alive here.
pub(crate) struct table_pages {
    pages: KVec<PageBox>,
}

impl EptTableAlloc for table_pages {
    fn alloc_table(&mut self) -> Option<u64> {
        let p = PageBox::new().ok()?;
        let pa = p.phys();
        self.pages.push(p, GFP_KERNEL).ok()?;
        Some(pa)
    }
}

/// INVEPT with type `ext` (`VMX_EPT_EXTENT_CONTEXT` or `_GLOBAL`) for `eptp`. Build the
/// descriptor with `invept_descriptor(eptp)` (stage 8 host half), then
/// `asm!("invept {t}, [{d}]", ...)` (Intel syntax: type register first, memory second) and decode
/// success from RFLAGS like `vmxon` did in stage 2 (CF=1 VMfailInvalid, ZF=1 VMfailValid).
/// Returns true on success. (kernel: `__invept`.)
///
/// # Safety
/// In VMX operation; the CPU must support INVEPT (IA32_VMX_EPT_VPID_CAP bit 20 for context,
/// bit 21 for global).
pub(crate) unsafe fn invept(ext: u64, eptp: u64) -> bool {
    let _ = (ext, eptp);
    todo!("descriptor + asm!(\"invept ...\") + check CF/ZF")
}

/// Turn EPT on in the *current* VMCS. Steps:
/// 1. `IA32_VMX_EPT_VPID_CAP` (`hw::rdmsr(MSR_IA32_VMX_EPT_VPID_CAP)`) must have bit 6 (4-level page
///    walk) and bit 14 (write-back EPT paging structures): else `Err`.
/// 2. `IA32_VMX_PROCBASED_CTLS2` high dword = the secondary controls the CPU *allows* to be 1: it must
///    contain `SECONDARY_EXEC_ENABLE_EPT` and `SECONDARY_EXEC_UNRESTRICTED_GUEST` (needed for a
///    real-mode guest) else `Err`.
/// 3. Set `CPU_BASED_ACTIVATE_SECONDARY_CONTROLS` in the primary controls, OR the two secondary bits into
///    `SECONDARY_VM_EXEC_CONTROL`, write `EPT_POINTER = construct_eptp(root_pa, false)`.
/// (kernel: `vmx_exec_control`/`vmx_secondary_exec_control` pick bits from `vmcs_config`, and
/// `vmx_load_mmu_pgd` writes `EPT_POINTER`. A cleaner version of this goes through stage 4's
/// `adjust_vmx_controls`.)
pub(crate) fn ept_enable(vmcs: &mut impl VmcsOps, root_pa: u64) -> core::result::Result<(), &'static str> {
    let _ = (&vmcs, root_pa);
    todo!("check capabilities, set secondary controls, write EPT_POINTER")
}

/// Handle `EXIT_REASON_EPT_VIOLATION` for a not-present page of guest RAM: read
/// `GUEST_PHYSICAL_ADDRESS` and `EXIT_QUALIFICATION`, decode with `ept_violation_decode`; if the
/// entry allowed nothing (`ept_violation_is_unmapped`) and the GPA lies inside guest RAM, map that
/// page: `ept_map_page(&mut g.mem, &mut g.tables, g.root, gpa & !0xfff, g.ram[gpa >> 12].phys(),
/// EPT_PERM_RWX)`. Anything else (a protection fault, a GPA outside RAM): `Err`. After changing
/// an entry that was *not present* no INVEPT is needed; think about when it is (stage doc).
pub(crate) fn ept_handle_violation(g: &mut ept_guest) -> core::result::Result<(), &'static str> {
    let _ = g;
    todo!("decode the exit, map the page")
}

/// A real-mode guest with its RAM behind EPT. PROVIDED.
pub(crate) struct ept_guest {
    pub vmx: vcpu_vmx,
    pub root: u64,
    pub ram: KVec<PageBox>,
    pub tables: table_pages,
    pub mem: host_phys,
}

/// PROVIDED. Put the current VMCS into real mode: all segments base = selector << 4 (here 0),
/// limit 0xffff, CS/data access rights 0x9b/0x93, TR/LDTR as the entry checks need, CR0 without
/// PE/PG (legal only with "unrestricted guest"), RFLAGS.IF = 1, no pending events.
pub(crate) fn vmx_set_real_mode(vmcs: &mut impl VmcsOps, rip: u64, rsp: u64) {
    const AR_CODE: u64 = 0x9b;
    const AR_DATA: u64 = 0x93;
    let segs: [(u32, u32, u32, u32, u64); 6] = [
        (GUEST_ES_SELECTOR, GUEST_ES_BASE, GUEST_ES_LIMIT, GUEST_ES_AR_BYTES, AR_DATA),
        (GUEST_CS_SELECTOR, GUEST_CS_BASE, GUEST_CS_LIMIT, GUEST_CS_AR_BYTES, AR_CODE),
        (GUEST_SS_SELECTOR, GUEST_SS_BASE, GUEST_SS_LIMIT, GUEST_SS_AR_BYTES, AR_DATA),
        (GUEST_DS_SELECTOR, GUEST_DS_BASE, GUEST_DS_LIMIT, GUEST_DS_AR_BYTES, AR_DATA),
        (GUEST_FS_SELECTOR, GUEST_FS_BASE, GUEST_FS_LIMIT, GUEST_FS_AR_BYTES, AR_DATA),
        (GUEST_GS_SELECTOR, GUEST_GS_BASE, GUEST_GS_LIMIT, GUEST_GS_AR_BYTES, AR_DATA),
    ];
    for (sel, base, limit, ar, arv) in segs {
        vmcs.vmcs_writel(sel, 0);
        vmcs.vmcs_writel(base, 0);
        vmcs.vmcs_writel(limit, 0xffff);
        vmcs.vmcs_writel(ar, arv);
    }
    vmcs.vmcs_writel(GUEST_LDTR_SELECTOR, 0);
    vmcs.vmcs_writel(GUEST_LDTR_BASE, 0);
    vmcs.vmcs_writel(GUEST_LDTR_LIMIT, 0xffff);
    vmcs.vmcs_writel(GUEST_LDTR_AR_BYTES, 0x10000); // unusable
    vmcs.vmcs_writel(GUEST_TR_SELECTOR, 0);
    vmcs.vmcs_writel(GUEST_TR_BASE, 0);
    vmcs.vmcs_writel(GUEST_TR_LIMIT, 0xff);
    vmcs.vmcs_writel(GUEST_TR_AR_BYTES, 0x8b); // busy 32-bit TSS
    vmcs.vmcs_writel(GUEST_GDTR_BASE, 0);
    vmcs.vmcs_writel(GUEST_GDTR_LIMIT, 0xffff);
    vmcs.vmcs_writel(GUEST_IDTR_BASE, 0);
    vmcs.vmcs_writel(GUEST_IDTR_LIMIT, 0xffff);
    vmcs.vmcs_writel(GUEST_CR0, X86_CR0_NE | X86_CR0_ET);
    vmcs.vmcs_writel(CR0_READ_SHADOW, X86_CR0_NE | X86_CR0_ET);
    vmcs.vmcs_writel(GUEST_RIP, rip);
    vmcs.vmcs_writel(GUEST_RSP, rsp);
    vmcs.vmcs_writel(GUEST_RFLAGS, X86_EFLAGS_FIXED | X86_EFLAGS_IF);
    vmcs.vmcs_writel(GUEST_ACTIVITY_STATE, 0);
    vmcs.vmcs_writel(GUEST_INTERRUPTIBILITY_INFO, 0);
    vmcs.vmcs_writel(GUEST_PENDING_DBG_EXCEPTIONS, 0);
    vmcs.vmcs_writel(GUEST_IA32_EFER, 0);
    vmcs.vmcs_clear_bits(VM_ENTRY_CONTROLS, VM_ENTRY_IA32E_MODE as u64);
}

impl ept_guest {
    /// PROVIDED. 1 MiB of RAM at GPA 0; `code` = (gpa, bytes) pieces copied into it (each inside one
    /// page); every page is mapped read/write/execute **except** those in `holes`. The vCPU starts in
    /// real mode at 0000:1000 with SP = 0x8000.
    pub(crate) fn new(code: &[(u64, &[u8])], holes: &[u64]) -> core::result::Result<Self, &'static str> {
        let mut ram = KVec::with_capacity(GUEST_RAM_PAGES, GFP_KERNEL).map_err(|_| "alloc ram")?;
        for _ in 0..GUEST_RAM_PAGES {
            ram.push(PageBox::new().map_err(|_| "alloc ram page")?, GFP_KERNEL).map_err(|_| "alloc ram")?;
        }
        for (gpa, bytes) in code {
            let off = (*gpa & 0xfff) as usize;
            ram[(*gpa >> 12) as usize].as_mut_slice()[off..off + bytes.len()].copy_from_slice(bytes);
        }
        let probe = PageBox::new().map_err(|_| "alloc probe")?;
        let mut mem = host_phys { off: (probe.as_ptr() as u64).wrapping_sub(probe.phys()) };
        let mut tables = table_pages { pages: KVec::new() };
        let root = ept_new_table(&mut mem, &mut tables).map_err(|_| "alloc ept root")?;
        for i in 0..GUEST_RAM_PAGES {
            let gpa = (i as u64) << 12;
            if holes.contains(&gpa) {
                continue;
            }
            ept_map_page(&mut mem, &mut tables, root, gpa, ram[i].phys(), EPT_PERM_RWX).map_err(|_| "ept_map_page")?;
        }
        let mut vmx = vcpu_vmx::new_flat32(0x1000, 0x8000)?;
        let mut vmcs = HwVmcs;
        ept_enable(&mut vmcs, root)?;
        vmx_set_real_mode(&mut vmcs, 0x1000, 0x8000);
        Ok(Self { vmx, root, ram, tables, mem })
    }

    /// PROVIDED. Enter the guest; host interrupts that hit meanwhile (external-interrupt exits) are
    /// skipped. Returns the raw `VM_EXIT_REASON`.
    pub(crate) fn run(&mut self) -> u32 {
        loop {
            let raw = self.vmx.run();
            if raw & 0xffff != EXIT_REASON_EXTERNAL_INTERRUPT {
                return raw;
            }
        }
    }
}

/// guest/08_ept.S: `mov $0x1234,%ax; mov %al,0x9000; hlt`
const GUEST_CODE: [u8; 7] = [0xb8, 0x34, 0x12, 0xa2, 0x00, 0x90, 0xf4];
const HOLE: u64 = 0x9000;

/// PROVIDED.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let mut g = ept_guest::new(&[(0x1000, &GUEST_CODE)], &[HOLE])?;

    // First entry: the store to the unmapped page violates.
    let raw = g.run();
    if raw & 0xffff != EXIT_REASON_EPT_VIOLATION {
        pr_err!("ktvmx: expected EPT violation, got exit reason {:#x}\n", raw);
        return Err("first exit is not an EPT violation");
    }
    let vmcs = HwVmcs;
    let gpa = vmcs.vmcs_read64(GUEST_PHYSICAL_ADDRESS);
    let q = ept_violation_decode(vmcs.vmcs_readl(EXIT_QUALIFICATION));
    if gpa != HOLE {
        pr_err!("ktvmx: violation at gpa {:#x}\n", gpa);
        return Err("EPT violation at the wrong guest-physical address");
    }
    if !q.write || q.read || q.fetch || !ept_violation_is_unmapped(&q) {
        return Err("exit qualification does not describe a write to an unmapped page");
    }

    // Map it, invalidate, resume: the CPU re-executes the same store.
    ept_handle_violation(&mut g)?;
    let eptp = construct_eptp(g.root, false);
    // SAFETY: in VMX operation (g.vmx is alive); global invalidation is always legal if supported.
    if !unsafe { invept(VMX_EPT_EXTENT_GLOBAL, eptp) } {
        return Err("invept failed");
    }
    let raw = g.run();
    if raw & 0xffff != EXIT_REASON_HLT {
        pr_err!("ktvmx: expected HLT, got exit reason {:#x}\n", raw);
        return Err("second exit is not HLT");
    }
    if g.vmx.regs[VCPU_REGS_RAX] as u16 != 0x1234 {
        return Err("AX lost");
    }
    if g.ram[(HOLE >> 12) as usize].as_slice()[0] != 0x34 {
        return Err("the store did not reach the page behind the EPT mapping");
    }
    // Software walk agrees with what the hardware just did.
    match ept_translate(&g.mem, g.root, HOLE + 5, EPT_PERM_W) {
        Ok(hpa) if hpa == g.ram[(HOLE >> 12) as usize].phys() + 5 => Ok(()),
        _ => Err("ept_translate disagrees with the mapping"),
    }
}
