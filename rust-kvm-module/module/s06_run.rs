// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 06: the world switch. `ktvmx_vcpu_run` (assembly) enters the guest
//! with VMLAUNCH/VMRESUME and comes back after the next VM exit; `vmx_run_vcpu`
//! (src/vmx_run.rs) is the loop around it.
//! Kernel counterparts: arch/x86/kvm/vmx/vmenter.S `__vmx_vcpu_run` (this is the same
//! thing minus spec-ctrl/RSB/L1D-flush/CFI), vmx.c `vmx_vcpu_enter_exit()`/`vmx_vcpu_run()`.
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).
//!
//! WARNING: loading with `selftest>=6` runs a guest and makes the module fail to load,
//! or crash the (throw-away QEMU) kernel, until the stages 2-6 `todo!()`s are done.
//!
//! The guest of this selftest is a 64-bit one **without EPT**. Why: without EPT guest-
//! physical == host-physical (the CPU's own page walk is all there is), which lets a
//! guest run on kmalloc'ed pages before we have memory virtualization (stage 8). But
//! "unrestricted guest" needs EPT, so without EPT CR0.PE and CR0.PG must be 1 (see the
//! CR0 fixed bits, stage 5): no real-mode guest yet, so we hand-build 4-level page tables
//! that identity-map the guest code page's *host physical address*. Stage 8 replaces
//! this with EPT, and real-mode guests become possible.

use core::arch::{asm, global_asm};

use kernel::prelude::*;

use crate::{
    hw,
    s03_vmcs::HwVmcs,
    s05_state::{lgdt, lidt, read_host_state, sgdt, sidt, LoadedVmcs},
    vmcs_ops::VmcsOps,
    vmx::*,
    vmx_controls::*,
    vmx_run::*,
    vmx_state::*,
};

// int ktvmx_vcpu_run(struct vcpu_vmx *vcpu /* rdi */, u32 resume /* esi */)
//   returns 0 after a VM exit (guest GPRs saved into vcpu->regs), 1 if VMLAUNCH/VMRESUME
//   itself failed (VMfail: CF or ZF set; nothing was entered).
// Callee-saved registers of the host are preserved by hand because after a VM exit the CPU
// only restores RSP, RIP, segment/control state from the VMCS host area, not general
// registers. HOST_RSP is rewritten at every entry (kernel: vmx_update_host_rsp()) because
// the stack depth at the call site differs. HOST_RIP is the `2:` label below.
//
// NOTE: objtool may warn about this unannotated function (it has no ORC/CFI info). The
// kernel's version uses SYM_FUNC_START and friends; it is a warning, not an error.
global_asm!(
    ".globl ktvmx_vcpu_run",
    ".type ktvmx_vcpu_run,@function",
    "ktvmx_vcpu_run:",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push rbx",
    "push rdi",                    // [rsp] = vcpu: survives the guest, it is on the host stack
    "mov eax, {host_rsp}",
    "vmwrite rax, rsp",            // HOST_RSP = rsp
    "mov eax, {host_rip}",
    "lea rdx, [rip + 2f]",
    "vmwrite rax, rdx",            // HOST_RIP = exit handler
    "mov rax, rdi",                // rax = vcpu = &regs[0]
    "test esi, esi",               // ZF <- (resume == 0); `mov` below does not touch flags
    "mov rcx, [rax + 8]",
    "mov rdx, [rax + 16]",
    "mov rbx, [rax + 24]",
    "mov rbp, [rax + 40]",         // [rax + 32] is RSP: it comes from GUEST_RSP in the VMCS
    "mov rsi, [rax + 48]",
    "mov rdi, [rax + 56]",
    "mov r8,  [rax + 64]",
    "mov r9,  [rax + 72]",
    "mov r10, [rax + 80]",
    "mov r11, [rax + 88]",
    "mov r12, [rax + 96]",
    "mov r13, [rax + 104]",
    "mov r14, [rax + 112]",
    "mov r15, [rax + 120]",
    "mov rax, [rax]",              // guest RAX last: it held the pointer
    "jnz 3f",
    "vmlaunch",
    "jmp 4f",
    "3:",
    "vmresume",
    "4:",                          // only reached if VMLAUNCH/VMRESUME failed (VMfail)
    "mov eax, 1",
    "jmp 5f",
    "2:",                          // VM exit lands here, guest GPRs live, RSP = HOST_RSP
    "push rax",
    "mov rax, [rsp + 8]",          // vcpu
    "pop qword ptr [rax]",         // regs[RAX] = guest RAX
    "mov [rax + 8], rcx",
    "mov [rax + 16], rdx",
    "mov [rax + 24], rbx",
    "mov [rax + 40], rbp",
    "mov [rax + 48], rsi",
    "mov [rax + 56], rdi",
    "mov [rax + 64], r8",
    "mov [rax + 72], r9",
    "mov [rax + 80], r10",
    "mov [rax + 88], r11",
    "mov [rax + 96], r12",
    "mov [rax + 104], r13",
    "mov [rax + 112], r14",
    "mov [rax + 120], r15",
    "xor eax, eax",
    "5:",
    "add rsp, 8",                  // drop the saved vcpu pointer
    "pop rbx",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",
    "ret",
    ".size ktvmx_vcpu_run, . - ktvmx_vcpu_run",
    host_rsp = const HOST_RSP,
    host_rip = const HOST_RIP,
);

extern "C" {
    fn ktvmx_vcpu_run(vcpu: *mut vcpu_vmx, resume: u32) -> u64;
}

/// One world switch. Returns `true` if the entry VMfailed.
///
/// # Safety
/// In VMX operation with a fully initialised current VMCS; interrupts disabled.
pub(crate) unsafe fn vmx_vcpu_enter(vcpu: &mut vcpu_vmx, flags: u32) -> bool {
    // SAFETY: forwarded; `vcpu_vmx` is repr(C) with `regs` at offset 0.
    unsafe { ktvmx_vcpu_run(vcpu as *mut vcpu_vmx, flags & VMX_RUN_VMRESUME) != 0 }
}

/// Run `f` with local interrupts off (pushfq/cli ... restore), and put GDTR/IDTR back
/// afterwards: a VM exit sets both limits to 0xffff. (KVM also reloads TR, because the exit
/// sets its limit to 0x67 (`invalidate_tss_limit()`); we skip that: it only breaks
/// ioperm() users of the throw-away guest kernel.)
pub(crate) fn with_irqs_off<R>(f: impl FnOnce() -> R) -> R {
    let flags: u64;
    // SAFETY: saves RFLAGS, clears IF.
    unsafe { asm!("pushfq", "pop {0}", "cli", out(reg) flags) };
    let gdt = sgdt();
    let idt = sidt();
    let r = f();
    // SAFETY: both descriptors were read from this CPU a moment ago.
    unsafe {
        lgdt(&gdt);
        lidt(&idt);
        if flags & X86_EFLAGS_IF != 0 {
            asm!("sti", options(nomem, nostack));
        }
    }
    r
}

/// On an EXTERNAL_INTERRUPT exit the interrupt is still pending (we did not set
/// "acknowledge interrupt on exit"). Open an interrupt window for one instruction so the
/// host's normal IDT handler runs, then close it again. (kernel:
/// `handle_external_interrupt_irqoff()`; `sti` has a one-instruction shadow, hence the nop.)
pub(crate) fn host_take_interrupt() {
    // SAFETY: the host handler runs at the nop; IF is off again afterwards.
    unsafe { asm!("sti", "nop", "cli", options(nomem, nostack)) };
}

/// Glue used by the kernel halves of stages 7-10 (they were written assuming exactly this
/// interface). TASK for you, after the stage 6 selftest passes: turn what the selftest does
/// by hand into a reusable vcpu object.
///
/// `new_flat32(entry_phys, rsp)`: `vmx_enable_cpu()` (stage 2), a `LoadedVmcs::new()`
/// (stage 5) made current, `vmx_setup_guest_protected_mode(..)` with RIP = `entry_phys`
/// (flat 32-bit, paging off: guest-physical == host-physical, no EPT yet), controls with
/// HLT / unconditional-I/O / MSR exiting on (stage 4 control writes). Dropping the object
/// must VMCLEAR and VMXOFF (keep the `VmxCpu` guard and the `LoadedVmcs` in a wrapper that
/// owns the `vcpu_vmx`).
/// `run`: `with_irqs_off(|| vmx_vcpu_enter(..))`, `vmx_post_run`, `host_take_interrupt()` on
/// EXTERNAL_INTERRUPT, return the raw `VM_EXIT_REASON` with IRQs enabled again.
impl vcpu_vmx {
    pub(crate) fn new_flat32(_entry_phys: u64, _rsp: u64) -> core::result::Result<Self, &'static str> {
        todo!("stage 6 task 5: see the doc comment")
    }

    pub(crate) fn run(&mut self) -> u32 {
        todo!("stage 6 task 5: see the doc comment")
    }
}

fn set_pte(page: &mut hw::PageBox, idx: u64, val: u64) {
    let o = (idx as usize) * 8;
    page.as_mut_slice()[o..o + 8].copy_from_slice(&val.to_le_bytes());
}

/// PROVIDED. Guest = one `hlt` in a 64-bit page at a linear address equal to its host
/// physical address. Expect: VM exit EXIT_REASON_HLT with RIP still on the `hlt`.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let lv = LoadedVmcs::new()?;
    let mut cfg = lv.cfg;
    // No EPT in this stage (see the module comment), and:
    //  - VPID on would require a non-zero VIRTUAL_PROCESSOR_ID (error 7 otherwise),
    //  - the preemption timer on would expire at once (value 0): exit reason 52, no guest code.
    cfg.cpu_based_2nd_exec_ctrl &= !(SECONDARY_EXEC_ENABLE_EPT
        | SECONDARY_EXEC_UNRESTRICTED_GUEST
        | SECONDARY_EXEC_ENABLE_VPID);
    cfg.pin_based_exec_ctrl &= !PIN_BASED_VMX_PREEMPTION_TIMER;
    let mut v = HwVmcs;
    vmx_vmcs_controls_write(&mut v, &cfg);

    // guest memory: code page + a 4-level walk to it
    let mut code = hw::PageBox::new().map_err(|_| "alloc")?;
    let mut pml4 = hw::PageBox::new().map_err(|_| "alloc")?;
    let mut pdpt = hw::PageBox::new().map_err(|_| "alloc")?;
    let mut pd = hw::PageBox::new().map_err(|_| "alloc")?;
    let mut pt = hw::PageBox::new().map_err(|_| "alloc")?;
    code.as_mut_slice()[0] = 0xf4; // hlt
    let lin = code.phys(); // guest linear address == host physical address
    const P_RW: u64 = 0x3; // present | writable
    set_pte(&mut pml4, (lin >> 39) & 511, pdpt.phys() | P_RW);
    set_pte(&mut pdpt, (lin >> 30) & 511, pd.phys() | P_RW);
    set_pte(&mut pd, (lin >> 21) & 511, pt.phys() | P_RW);
    set_pte(&mut pt, (lin >> 12) & 511, lin | P_RW);

    vmx_setup_guest_long_mode(&mut v, lin, lin + 0x800, pml4.phys());
    let hs = read_host_state(); // rsp/rip: written by the assembly at each entry
    vmx_set_host_state(&mut v, &hs);
    vmx_check_guest_state(&v, &cfg).map_err(|e| {
        pr_err!("ktvmx: guest state check: {}\n", e);
        "guest state rejected by vmx_check_guest_state"
    })?;

    let mut vcpu = vcpu_vmx::default();
    let action = with_irqs_off(|| {
        vmx_run_vcpu(
            &mut vcpu,
            &mut v,
            // SAFETY: VMX on, VMCS current and complete, IRQs off (with_irqs_off).
            &mut |c, _v, flags| c.fail = unsafe { vmx_vcpu_enter(c, flags) },
            &mut host_take_interrupt,
        )
    });
    match action {
        vmx_run_action::Exit(EXIT_REASON_HLT) => {}
        vmx_run_action::Exit(b) => {
            pr_err!("ktvmx: unexpected exit {} ({})\n", b, vmx_exit_reason_str(b));
            return Err("guest did not exit with HLT");
        }
        vmx_run_action::EntryFailed(vmx_entry_failure::InstructionError(e)) => {
            pr_err!("ktvmx: VM entry failed: VM-instruction error {}: {}\n", e, vmx_instruction_error_str(e));
            return Err("VMLAUNCH failed (see dmesg)");
        }
        vmx_run_action::EntryFailed(vmx_entry_failure::ExitFailed { basic, qualification }) => {
            pr_err!(
                "ktvmx: VM entry failed after start: {} qualification {:#x}\n",
                vmx_exit_reason_str(basic),
                qualification
            );
            return Err("VM entry failed on guest state (see dmesg)");
        }
        vmx_run_action::HostInterrupt => return Err("loop returned HostInterrupt"),
    }
    if v.vmcs_readl(GUEST_RIP) != lin {
        return Err("HLT exit must leave RIP on the hlt instruction");
    }
    if v.vmcs_read32(VM_EXIT_INSTRUCTION_LEN) != 1 {
        return Err("HLT is one byte long");
    }
    if !vcpu.launched {
        return Err("launched flag not set");
    }
    Ok(())
}
