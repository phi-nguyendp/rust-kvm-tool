// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 09: injecting an interrupt, and the VMX-preemption timer.
//! Kernel counterparts: arch/x86/kvm/x86.c `inject_pending_event()` / `vcpu_enter_guest()`
//! (decide what to inject before each entry, open an interrupt window if it cannot be
//! injected yet), arch/x86/kvm/vmx/vmx.c (`vmx_inject_irq`, `vmx_enable_irq_window`,
//! `vmx_complete_interrupts`, `vmx_set_hv_timer`).
//! NOT COMPILED by the course author (see CLAUDE.md): expect a small fix or two.
//! Uses `ept_guest` from stage 8 (real-mode guest behind EPT) and the stage-6 interface
//! assumed in `s07_exit.rs`.

#[allow(unused_imports)]
use kernel::prelude::*;

#[allow(unused_imports)]
use crate::{
    hw,
    s03_vmcs::HwVmcs,
    s08_ept::ept_guest,
    vmcs_ops::VmcsOps,
    vmx::*,
    vmx_events::*,
    vmx_exit::*,
};

extern "C" {
    // arch/x86/kernel/tsc.c: EXPORT_SYMBOL(tsc_khz)
    static tsc_khz: u32;
}

/// Run `g` until the guest halts, delivering external interrupt `vector` as soon as the guest can
/// take it. The loop, at most 100 iterations, each with a fresh `HwVmcs`:
/// 1. if the vector is still pending: if `vmx_interrupt_allowed()` -> `vmx_inject_irq()` and clear
///    `pending`; else `vmx_enable_irq_window()` (we want an exit when the guest becomes interruptible);
/// 2. `let raw = g.run()`;
/// 3. `vmx_complete_interrupts()` (a re-injection must not be lost; it fills the entry fields, so it
///    takes priority: do it before step 1 of the next iteration);
/// 4. `EXIT_REASON_INTERRUPT_WINDOW` -> `vmx_disable_irq_window()` and loop; `EXIT_REASON_HLT` ->
///    `skip_emulated_instruction()` and return `Ok(())`; `EXIT_REASON_IO_INSTRUCTION` -> handle it with
///    `handle_io` against `uart` (the guest's handler prints to COM1; build the `exit_ctx` as in
///    stage 7) and loop; anything else -> `Err`.
/// Returns the number of VM exits taken (the self test checks it is small).
pub(crate) fn run_with_irq(
    g: &mut ept_guest,
    uart: &mut uart_com1,
    vector: u8,
) -> core::result::Result<usize, &'static str> {
    let _ = (&g, &uart, vector);
    todo!("inject-or-open-window, run, complete_interrupts, dispatch the exit")
}

/// Run `g` (a guest that never exits by itself) until the VMX-preemption timer fires after about
/// `ns` nanoseconds: `value = preemption_timer_value(ns, tsc_khz, hw::rdmsr(MSR_IA32_VMX_MISC))`,
/// `vmx_arm_preemption_timer()`, then `g.run()` until the basic exit reason is
/// `EXIT_REASON_PREEMPTION_TIMER` (re-arm if an unrelated exit comes first: the timer value is not
/// saved unless the VM-exit control says so), finally `vmx_disarm_preemption_timer()`. Returns `Ok(())`
/// or `Err` if some other exit reason shows up. Needs `PIN_BASED_VMX_PREEMPTION_TIMER` in the allowed-1
/// half of IA32_VMX_PINBASED_CTLS (true-controls MSR if bit 55 of IA32_VMX_BASIC is set); return
/// `Err("unsupported")` if it is not.
pub(crate) fn run_with_timer(g: &mut ept_guest, ns: u64) -> core::result::Result<(), &'static str> {
    let _ = (&g, ns);
    todo!("arm the timer, run until EXIT_REASON_PREEMPTION_TIMER, disarm")
}

/// guest/09_events.S: main `hlt; jmp -3` at 0x1000, handler `mov $0x3f8,%dx; mov $'X',%al; out %al,%dx;
/// iret` at 0x2000.
const MAIN: [u8; 3] = [0xf4, 0xeb, 0xfd];
const HANDLER: [u8; 7] = [0xba, 0xf8, 0x03, 0xb0, 0x58, 0xee, 0xcf];
const SPIN: [u8; 2] = [0xeb, 0xfe];

/// PROVIDED.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    // --- part 1: inject vector 0x20 into a guest that is interruptible from the start ---------
    // IVT entry 0x20 lives at GPA 0x20 * 4 = 0x80: offset 0x2000, segment 0.
    let ivt: [u8; 4] = [0x00, 0x20, 0x00, 0x00];
    let mut g = ept_guest::new(&[(0x1000, &MAIN), (0x2000, &HANDLER), (0x80, &ivt)], &[])?;
    let mut uart = uart_com1::new();
    let exits = run_with_irq(&mut g, &mut uart, 0x20)?;
    if uart.output() != b"X" {
        pr_err!("ktvmx: console = {:?}\n", uart.output());
        return Err("the guest's vector-0x20 handler did not run (or ran more than once)");
    }
    if exits > 8 {
        return Err("too many exits for one injected interrupt");
    }
    drop(g);

    // --- part 2: preemption timer bounds a guest that spins forever -------------------------
    let mut g = ept_guest::new(&[(0x1000, &SPIN)], &[])?;
    match run_with_timer(&mut g, 2_000_000) {
        Ok(()) => {}
        Err("unsupported") => {
            pr_info!("ktvmx: VMX-preemption timer not supported here, skipping that part\n");
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

/// PROVIDED: the host TSC frequency in kHz (what `preemption_timer_value` needs).
pub(crate) fn tsc_khz_value() -> u64 {
    // SAFETY: tsc_khz is set during boot and only read afterwards.
    unsafe { tsc_khz as u64 }
}
