# Stage 12 (planned): interrupt controllers and timers: boot Linux

State: planned, no code yet. When adding it: stub + test + this doc + `COURSE.md` row
(see `CLAUDE.md`), plus the `module/` and `qemu/` parts.

Goal: boot a real `vmlinux` (the one `rust-kvm-tool` already loads: ELF segments, `boot_params`,
command line, optional initramfs) on `/dev/ktvmx` with `console=ttyS0`, reach the initramfs
`/init`, and get a shell prompt.

Read: SDM Vol. 3A ch. 12 (*APIC*: 12.4 Local APIC, 12.5 interrupt handling, 12.6 timer, 12.9
spurious interrupt), Vol. 3C ch. 25.5-25.6 (*interrupt-window exiting*, *TPR shadow*), 29
(*APIC virtualization*, optional); the 8259A and 8254 (PIT) datasheets, the 16550 UART
datasheet, the Intel 82093AA IOAPIC datasheet. Kernel:
`arch/x86/kvm/lapic.c` (`kvm_lapic_reg_read/write`, `apic_update_ppr`,
`kvm_apic_accept_irq`, `start_apic_timer`), `ioapic.c`, `i8259.c`, `i8254.c`, `irq.c`,
`vmx.c` (`vmx_inject_irq`, `vmx_enable_irq_window`), `Documentation/virt/kvm/api.rst`
(`KVM_CREATE_IRQCHIP`, `KVM_IRQ_LINE`, `KVM_CREATE_PIT2`, `KVM_SET_CPUID2`,
`KVM_SET_TSS_ADDR`, `KVM_SET_IDENTITY_MAP_ADDR`). Reuse as reference:
`rust-kvm-tool/src/kvm.rs` `handle_io` (the COM1 UART and its THRE interrupt handling) and
the comments about IRQ4 there. QEMU: `hw/intc/apic.c` for a second implementation.

## Tasks (outline)
1. **CPUID.** A real kernel reads dozens of leaves. Implement `KTVMX_GET_SUPPORTED_CPUID` /
   `KTVMX_SET_CPUID2` and the CPUID exit handler (stage 7) from a table, hiding what you do
   not implement (VMX, PMU, x2APIC unless you do it, ...), showing the hypervisor bit.
2. **Local APIC page (MMIO).** Map no page at `0xfee00000` in the EPT: every access takes an
   EPT violation (stage 8) that you decode (`MMIO` exit or in-kernel handler) and emulate:
   ID, version, TPR, EOI, spurious vector, LVT entries, ICR, timer registers. IRR/ISR/TMR
   bitmaps; `apic_update_ppr`; priority arbitration.
3. **Interrupt delivery.** Find the highest pending vector above PPR; inject it with stage 9's
   `vmx_inject_irq` when `RFLAGS.IF` is set and the guest is not in an interrupt shadow
   (`GUEST_INTERRUPTIBILITY_INFO`), else request an *interrupt-window exit* and retry.
4. **APIC timer.** One-shot, periodic and TSC-deadline modes with an `hrtimer`; the timer
   must kick the vCPU out of `HLT` (what does your HLT handler do today?) and out of guest
   mode (VMX-preemption timer or an IPI + `external-interrupt exiting`).
5. **IOAPIC + PIC + PIT.** The legacy path Linux uses early: 8259 pair (ports 0x20/0xa0,
   ICW/OCW), 8254 channel 0 as the first tick source (Linux calibrates against it!), IOAPIC
   MMIO at `0xfec00000` with the redirection table. `KTVMX_IRQ_LINE` from userspace
   (rust-kvm-tool pulses IRQ4 for the UART).
6. **The odd KVM-isms:** `KVM_SET_TSS_ADDR` (a 3-page TSS for vm86 emulation on CPUs without
   unrestricted guest; you may only need to accept it), `KVM_SET_IDENTITY_MAP_ADDR` (identity
   page table for real-mode paging emulation, same remark), MTRRs/PAT, `IA32_MISC_ENABLE`,
   TSC and `RDTSC` offsetting, `XSETBV`/XCR0, `RDTSCP`.
7. Optional: **APICv** (virtual-interrupt delivery + posted interrupts: stage 13's outline).

Done when: `qemu/stage12.sh` boots a `vmlinux` + initramfs built with
`rust-kvm-tool/tools/mkinitramfs.py` to `/init` and `/bin/sh` answers `uname -a`.

## Think about
- Why does Linux hang in `calibrate_delay`/`tsc_refine_calibration_work` if the PIT does not
  tick, even though the APIC timer is available?
- What exactly is an *interrupt shadow* (after `sti` or `mov ss`), and what does the
  VM-entry check do if you inject into one? (Stage 5/9 checks.)
- Why does an EPT-violation exit for an APIC access carry no *instruction bytes*, and what
  does that force on an MMIO emulator? Compare `x86_emulate_instruction` in KVM with what
  you can do with `VM_EXIT_INSTRUCTION_LEN` and the exit qualification alone. (Hint: why does
  KVM's APIC-access virtualization, `SECONDARY_EXEC_VIRTUALIZE_APIC_ACCESSES`, exist?)
- Which component should own `EOI`: the LAPIC (for edge interrupts) or the IOAPIC (level
  interrupts via the EOI broadcast, `ioapic_eoi_broadcast`)? What happens to a level IRQ whose
  line is still asserted after EOI?
- The UART's THRE interrupt: rust-kvm-tool pulses IRQ4 only when `IER.THRI` is set. What
  does real 16550 hardware do when THRI is enabled while the transmitter is already empty?
