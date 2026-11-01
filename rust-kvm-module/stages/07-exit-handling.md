# Stage 7: handling VM exits

Read: Intel SDM Vol. 3C, "VM exits" chapter: 27.2 *Recording VM-exit information*
(exit reason, exit qualification, instruction length), Table 27-5 *Exit qualification for
I/O instructions*, Appendix C (basic exit reasons); Vol. 3A for MSR semantics (`RDMSR`,
`WRMSR` use EDX:EAX) and `CPUID` leaf 1 / the hypervisor leaves. Kernel: `vmx_handle_exit()` /
`__vmx_handle_exit()` and `kvm_vmx_exit_handlers[]` in `arch/x86/kvm/vmx/vmx.c`,
`handle_io()`, `kvm_emulate_cpuid()` (`arch/x86/kvm/cpuid.c`), `kvm_skip_emulated_instruction()`
(`arch/x86/kvm/x86.c`).
(SDM section numbers are those of the 2023+ editions; the section *titles* are stable, search for them.)
Files: `src/vmx_exit.rs` (host half), `module/s07_exit.rs` (kernel half, `run_guest`).
Tests: `tests/stage07_exits.rs`; in QEMU `qemu/stage07.sh`. Guest: `guest/07_exit32.S`.
(Needs stages 1-6: a vCPU that can enter a guest and report an exit.)

## Tasks (host half, `cargo test --test stage07_exits`)
1. `skip_emulated_instruction`: RIP += `VM_EXIT_INSTRUCTION_LEN`, and clear the STI /
   MOV-SS interruptibility bits.
2. `uart_com1::pio_write` / `pio_read`: a UART that only knows "transmit a byte" and
   "transmitter ready", plus an "open bus" answer for ports with no device.
3. `decode_io_qualification` and `handle_io`: Table 27-5, OUT vs IN, how a 1/2/4-byte `IN`
   merges into RAX, and why string I/O is refused for now.
4. `handle_cpuid`: pass the host through, with two lies (hide VMX, show the hypervisor bit) and
   one invented leaf (`0x4000_0000`, your signature).
5. `vmx_get_msr` / `vmx_set_msr` / `handle_rdmsr` / `handle_wrmsr`: MSRs that live in the VMCS;
   everything else is `InjectGp` (RIP must *not* move).
6. `handle_hlt`, `basic_exit_reason`, and `vmx_handle_exit`: the dispatcher. Unknown exits must
   leave the loop, not silently resume.

## Tasks (kernel half, QEMU)
`run_guest` in `module/s07_exit.rs`: the loop around `vcpu_vmx::run()` and `vmx_handle_exit()`.
The selftest runs `guest/07_exit32.S` (two CPUIDs, three `out`s, `hlt`) and checks the console
says `Hi`, VMX is hidden, and the signature reads `KtVm`. The kernel half has **not been
compiled** by the course author: the `s06_run` interface it assumes is written at the top of the
file, adapt it to yours.

Design: like the TCP course keeps `tcp_v4_rcv` separate from the device, a handler here touches
no hardware: it gets the saved guest registers and a `VmcsOps`, and answers with an `exit_action`
(the kernel returns an `int`: 1 = resume, 0 = go to userspace).

## Think about
- Why does the CPU give you the instruction length instead of letting you decode the
  instruction at RIP? Which exits have no meaningful length (try a VM-entry failure, an EPT
  violation, an external interrupt)? What happens if you resume after `HLT` *without* skipping?
- `skip_emulated_instruction` clears STI / MOV-SS blocking. Construct a guest sequence where
  forgetting that lets an interrupt in one instruction too early. Why is NMI blocking *not*
  cleared?
- `IN` with a 4-byte operand zero-extends into RAX, but 1- and 2-byte `IN`s preserve the upper
  bits. Where else on x86-64 does a 32-bit register write behave like that, and why does the
  rule exist?
- Why does an OUT to an unknown port get ignored but an IN from it return all ones? What does
  real hardware (an ISA bus with nothing on it) do, and what does a guest driver probing for a
  device assume?
- Why hide `CPUID.1:ECX[5]` (VMX) from the guest? What would a guest kernel try to do if it saw
  it, and what would your module do about the `VMXON` it executes? And why set bit 31
  (hypervisor present)? Look at what `arch/x86/kernel/cpu/hypervisor.c` does with it.
- KVM answers unknown MSR reads with #GP unless `ignore_msrs` is set. What is the trade-off?
  Which MSR accesses does a Linux guest do early in boot that would break if you injected #GP?
- Why return `ExitToUser` for an exit reason you do not know, instead of `Resume`? What does the
  guest do when you resume a `VMCALL` exit without skipping it?
- `EXIT_REASON_EXTERNAL_INTERRUPT` needs no emulation, yet it is the most frequent exit. Who
  actually handles that interrupt, and when? (Stage 9 answers this for real.)
- `exit_ctx.host_cpuid` is a function pointer instead of calling `cpuid` directly. What does the
  pointer buy the *host tests*? What does CPUID cost on real hardware inside a guest, and why do
  hypervisors care about CPUID-heavy workloads?

## Run
```
cargo test --test stage07_exits
make -C guest 07_exit32.bin && xxd 07_exit32.bin    # the 33 bytes embedded in s07_exit.rs
qemu/stage07.sh                                     # needs the Rust kernel, see docs/setup.md
```
Expected: `ktvmx: stage07 exit: PASS` in `dmesg`. If the run never ends with
`Err(...)` but `pr_err!` prints an exit reason, look it up in Appendix C and in `asm/vmx.h`.
Oracle: `rust-kvm-tool` (real KVM) running a payload that does the same CPUID/OUT/HLT shows the
same console output and the same exits through its `KVM_EXIT_IO` / `KVM_EXIT_HLT` handling.
