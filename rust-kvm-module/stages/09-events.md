# Stage 9: events: interrupts, exceptions, windows, the preemption timer

Read: Intel SDM Vol. 3C: *VM-entry interruption-information field* (25.8.3, Table 25-17) and
event injection rules (26.6); *Information for VM exits due to vectored events* (27.2.2,
Tables 27-15, 27-16: exit interruption information and IDT-vectoring information);
*Guest interruptibility state* (24.4.2, Table 24-3); *Interrupt-window / NMI-window exiting*
(25.3, 25.4 area: primary processor-based controls); *Exception bitmap* (24.6.3); *VMX-preemption
timer* (25.5.1, 26.7.4 activity, Appendix A.6 `IA32_VMX_MISC[4:0]`). Vol. 3A chapter 6 for the
IVT/IDT delivery and the list of exceptions with error codes.
Kernel: `vmx_inject_irq()`, `vmx_inject_nmi()`, `vmx_inject_exception()`,
`__vmx_complete_interrupts()`, `vmx_interrupt_blocked()`, `vmx_nmi_blocked()`,
`vmx_enable_irq_window()`, `vmx_update_exception_bitmap()`, `vmx_set_hv_timer()` in
`arch/x86/kvm/vmx/vmx.c`; `is_exception_n()` in `vmx.h`; the policy in `inject_pending_event()`
and `vcpu_enter_guest()` in `arch/x86/kvm/x86.c`.
(SDM section numbers are those of the 2023+ editions; the section *titles* are stable, search for them.)
Files: `src/vmx_events.rs` (host half), `module/s09_events.rs` (kernel half: `run_with_irq`,
`run_with_timer`). Tests: `tests/stage09_events.rs`; QEMU: `qemu/stage09.sh`. Guest: `guest/09_events.S`.
(Needs stages 7 and 8: this stage's guest is the real-mode guest behind EPT.)

## Tasks (host half, `cargo test --test stage09_events`)
1. `exception_has_error_code`, `vmx_inject_irq`, `vmx_inject_nmi`, `vmx_inject_soft_intr`,
   `vmx_inject_exception`: the VM-entry interruption-information encoding, including which
   events need `VM_ENTRY_INSTRUCTION_LEN` and which need the error-code bit.
2. `decode_intr_info`, `vmx_exit_intr_info`, `is_exception_n`: the same word, read after an exit.
3. `vmx_complete_interrupts`: if an exit interrupted the *delivery* of an event, queue it again.
4. `vmx_interrupt_allowed`, `vmx_nmi_allowed`; `vmx_enable_irq_window` / `vmx_disable_irq_window`.
5. `vmx_update_exception_bitmap`, `vmx_exception_intercepted`.
6. `preemption_timer_value`, `vmx_arm_preemption_timer`, `vmx_disarm_preemption_timer`.

## Tasks (kernel half, QEMU)
- `run_with_irq`: before each entry either inject the pending vector (if the guest can take it) or
  open an interrupt window; after each exit, `vmx_complete_interrupts`; handle window / I/O / HLT.
- `run_with_timer`: bound a guest that spins forever with the preemption timer.
The selftest starts a real-mode guest whose IVT entry 0x20 points at a handler that prints `X`, with
`IF = 1`, injects vector 0x20 on the first entry and checks the console says exactly `X`; then spins
a guest and expects exit reason 52 within ~2 ms. **Not compiled** by the course author. If your
`vcpu_vmx::run()` leaves host interrupts disabled after an exit, external interrupts will be handled
late: see `local_irq_enable()` in `vcpu_enter_guest()` and the question below.

## Think about
- Which VM-entry fields must be written for a *software* interrupt (`INT n`) but not for an external
  one? What goes wrong on the guest stack if `VM_ENTRY_INSTRUCTION_LEN` is wrong? Why is #BP/#OF
  injected as a *software exception* (type 6) and #GP as a hardware exception (type 3)?
- The "deliver error code" bit set for #UD makes VM entry fail with VM-instruction error 7. Why does
  the CPU check this instead of just ignoring the bit? Which vectors push an error code, and why does
  #DF's error code always read 0?
- You cannot inject an interrupt into a guest with `RFLAGS.IF = 0`, nor right after `STI`/`MOV SS`.
  Why do those two instructions defer interrupts by one instruction (look up the `STI; HLT` idiom and
  `MOV SS, ax; MOV SP, bx`)? How does interrupt-window exiting avoid *polling* the guest's IF?
- Why can only one event be injected per entry? How does KVM prioritize (NMI vs interrupt vs
  exception, `inject_pending_event`), and what happens to the others?
- Take a guest `#PF` whose push to the stack faults with an EPT violation: which VM-exit information
  fields tell you the #PF was being delivered, and what exactly would the guest see if you resumed
  without `vmx_complete_interrupts`? When is copying the field verbatim *wrong* (hint: the
  contributory/double-fault rules of SDM 6.15, and `kvm_multiple_exception`)?
- After an NMI exit, further NMIs are blocked until the guest executes IRET. How does the CPU report
  that (the NMI-unblocking bit), and what does KVM do with `vmx_set_nmi_mask`? What if the NMI hit
  *during* the delivery of another exception?
- Why does the VM exit on an external interrupt at all, instead of the guest handling it through its
  IDT? Who handles the interrupt, and what must the host do between `VM exit` and the next `VM entry`
  for it to be delivered (RFLAGS.IF after the exit; "acknowledge interrupt on exit"; `local_irq_enable()`;
  how does `-smp 1` and a preemptible kernel interact with that)?
- The preemption timer ticks at TSC >> X. Why not exactly at TSC rate, and why must `ns * tsc_khz` be
  computed in 128 bits? What happens to its value across a VM exit if the "save VMX-preemption timer
  value" exit control is clear, and what does KVM use the timer for (hint: the guest LAPIC timer
  deadline, `vmx_set_hv_timer`)?
- Intercepting #PF is special: `PAGE_FAULT_ERROR_CODE_MASK/MATCH` decide *which* page faults exit.
  What do mask = match = 0 mean for bit 14 of the exception bitmap, and what do they mean if the bit
  is clear? When would a VMM want only some page faults?

## Run
```
cargo test --test stage09_events
make -C guest 09_events.bin && objdump -D -b binary -mi8086 -M intel guest/09_events.bin | head -40
qemu/stage09.sh        # ktvmx: stage09 events: PASS
```
If the console shows nothing: the guest probably took the injection but its handler ran in the wrong
mode; check the IVT bytes at GPA 0x80 and `GUEST_CS_BASE`. If the guest never gets the interrupt, print
`vmx_interrupt_allowed()` and `GUEST_RFLAGS` before the entry. Oracle: `rust-kvm-tool`'s `KVM_INTERRUPT`
(not implemented there yet) would be the real KVM equivalent of `vmx_inject_irq`.
