# Stage 6: VM entry and exit: the first guest runs

Read: Intel SDM Vol. 3C chapter 26 (VM entries, 26.8 "VM-entry failures during or after loading
guest state"), chapter 27 (VM exits: 27.2 recording information, 27.5 loading host state),
30.3 (VMLAUNCH/VMRESUME), 30.4 (VM-instruction error numbers, Table 30-1), Appendix C (basic exit
reasons). Kernel: `arch/x86/kvm/vmx/vmenter.S` (`__vmx_vcpu_run`), `vmx.c` (`vmx_vcpu_run()`,
`vmx_vcpu_enter_exit()`, the failure branches of `__vmx_handle_exit()`, `handle_external_interrupt_irqoff()`),
`union vmx_exit_reason` in `arch/x86/kvm/vmx/vmx.h`.
Files: `src/vmx_run.rs`, `module/s06_run.rs` (the assembly is provided, read it). Test:
`tests/stage06_run.rs`. Demo: `qemu/stage06.sh`, guest `guest/06_hlt.S`. (Needs stages 1-5.)

Everything so far prepared an environment. Now the CPU enters it: `VMLAUNCH` the first time,
`VMRESUME` ever after, and control returns when the guest does something the controls say is
the host's business (a *VM exit*), landing at `HOST_RIP` with `HOST_RSP`. This stage is the
world switch and the decisions around it: launched or not, did the entry even happen, did it
fail late, was it just a host interrupt, or a real exit for stage 7.

## Tasks (host side, `src/vmx_run.rs`)
1. `vmx_exit_reason::basic()`/`failed_vmentry()`, `vmx_exit_reason_str()`,
   `vmx_instruction_error_str()`: read exit reason and error number like a human (the tables
   are given).
2. `vmx_vcpu_run_flags()` and `vmx_post_run()`: the VMLAUNCH/VMRESUME bookkeeping. A VMfail
   must not mark the VMCS launched.
3. `vm_entry_failure()` and `vmx_vcpu_run_decide()`: the two ways an entry fails and the
   classification of a normal exit (host interrupt vs "stage 7's problem").
4. `vmx_run_vcpu()`: the loop.

The tests drive these with a fake VMCS and a scripted fake CPU (`enter` closures that "exit" with a
chosen reason or VMfail), including interrupt storms and failures in the middle of a run.

## Tasks (kernel side, `module/s06_run.rs`: read, then run)
The assembly `ktvmx_vcpu_run` is given because getting a world switch wrong is not instructive,
reading it is. Follow one entry on paper: what is on the stack, which register holds what at
`vmlaunch`, what the CPU has loaded by the time `2:` runs and what it has *not* (all of the
guest's general registers are still in the CPU!). The selftest builds a 64-bit guest whose code is a
single `hlt`, **without EPT** (see the long comment at the top of the file for why: no real mode
before EPT) and expects `EXIT_REASON_HLT` with RIP still on the `hlt`.

5. (Needed from stage 7 on.) Fill the `todo!()` `vcpu_vmx::new_flat32` / `run` glue at the end
   of `module/s06_run.rs`: stages 7-10's kernel halves call exactly these two. Start from
   what the selftest does by hand; the doc comment there lists what each must do. If you
   prefer another shape, adapt the calls at the top of `s07_exit.rs` instead.

## Think about
- Why is the host `RSP` rewritten before every entry and not once at setup? And why does the
  CPU not just save and restore the host's general registers itself?
- Which registers does the guest *not* get from `vcpu_vmx.regs`, and where does the guest's RSP
  live instead?
- After `VMLAUNCH` fails with VMfailValid, where does the error number live? And after
  VMfailInvalid? (Hint: what is the difference between the two cases in SDM 30.2?)
- What is the difference between VM-instruction error 7 and an exit with bit 31 set and reason
  33 (`INVALID_STATE`)? Which of your stage 4/5 mistakes produces which?
- The launch state. What does VMCLEAR do to it, what does VMLAUNCH on a launched VMCS give, and
  which of the two unlucky VMfails (4, 5) did you just avoid by writing `vmx_post_run` correctly?
  What does SDM 26.8 say about the launch state after an entry that fails *after* it started?
- We run the guest with interrupts disabled and `ext-int exiting` on. What happens to a host
  timer interrupt that fires mid-guest, what would happen with `ext-int exiting` off, and why
  `sti; nop; cli`?
- The `hlt` exit leaves `GUEST_RIP` on the `hlt`. When would a hypervisor advance it by
  `VM_EXIT_INSTRUCTION_LEN`, and what goes wrong if it forgets? (Stage 7.)
- Pitfalls the selftest dodges by clearing two controls: `ENABLE_VPID` with
  `VIRTUAL_PROCESSOR_ID == 0`, and `VMX_PREEMPTION_TIMER` with a timer value of 0. What does each
  produce if you leave it on (error number / exit reason)?

## Run the demo
```
cargo test --test stage06_run
make -C module && qemu/stage06.sh     # boots QEMU, loads the module with selftest=6
```
Expected: `ktvmx: stage06 run: PASS`. If it says `FAIL (VMLAUNCH failed (see dmesg))`, dmesg has the
VM-instruction error and its text; use `vmx_check_guest_state` output and `docs/setup.md`.
