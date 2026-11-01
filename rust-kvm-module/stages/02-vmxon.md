# Stage 2: Entering VMX operation (VMXON / VMXOFF)

Read: SDM Vol. 3C ch. 23 ("Enabling and Entering VMX Operation": CR4.VMXE, the VMXON
region, fixed bits, IA32_FEATURE_CONTROL), Appendix A.7/A.8 (CR0/CR4 fixed MSRs), ch. 30
"Conventions" (VMsucceed / VMfailInvalid / VMfailValid), the VMXON and VMXOFF pages of
Vol. 2. Kernel: `arch/x86/kvm/vmx/vmx.c` (`vmx_hardware_enable`, `kvm_cpu_vmxon`),
`arch/x86/kvm/vmx/vmx_ops.h` (`vmx_asm1`).
Files: `src/vmx_enable.rs`, `module/s02_vmxon.rs`. Test: `tests/stage02_vmxon.rs`.
QEMU: `qemu/stage02.sh`. Needs stage 1.

## Tasks (host half, `cargo test --test stage02_vmxon`)
1. `vmx_read_cr_fixed`, `cr_fixed_adjust`, `cr_fixed_ok`: the CR0/CR4 fixed bits.
2. `vmxon_region_init`: revision id in bits 30:0 of dword 0, bit 31 clear.
3. `vmx_fail_kind`: decode CF/ZF into the three results.
4. `vmx_cpu_state::{vmfail, vmxon, vmxoff}`: the SDM pseudo-code as a state machine, with
   its precise order of #UD, #GP, VMfailInvalid and VMfailValid. The doc comment on
   `vmxon` lists the order; the tests check it.

## Tasks (kernel half, `module/s02_vmxon.rs`)
5. `vmxon(phys)` and `vmxoff()` as `asm!`; decode the result from RFLAGS.
6. `vmx_enable_cpu()`: the whole `vmx_hardware_enable` sequence, using your stage 1 and
   stage 2 host logic (probe, FEATURE_CONTROL, fixed bits, region, CR4.VMXE, VMXON).
   `VmxCpu`'s `Drop` (provided) does VMXOFF and restores CR4.

Until you replace the `todo!()` in `module/`, loading with `selftest=2` panics the
guest: that is what `todo!()` does in a kernel. The guest is disposable.

## Think about
- Why must CR4.VMXE be set *before* VMXON, and why can you not clear it while in VMX
  operation? Who else could write CR4 meanwhile (hint: `cr4_set_bits` and the CR4 shadow)?
- The fixed-bit MSRs say CR0.NE and CR0.PE/PG must be 1. Why does the SDM allow
  unrestricted-guest to relax that for the *guest* but never for the host?
- Why does the VMXON region need the VMCS revision id? Where else does the revision id
  matter (stage 3)? What would VMXON do on a CPU stepping with another id?
- VMXON takes its operand from memory, VMPTRLD too, VMWRITE takes registers. Why a
  *physical* address, and why 4 KiB alignment?
- Why is a failing VMXON while already in VMX operation VMfailInvalid, not
  VMfailValid, until some VMCS is current? What does that tell you about where the
  error number lives?
- Why is the state per *logical processor*? What happens to your module if the scheduler
  moves the task between CPUs after `vmx_enable_cpu` (and why does `-smp 1` hide it)?
  How does KVM solve this (`kvm_arch_enable_virtualization`, CPU hotplug callbacks,
  `vmx_vcpu_load`)?
- VMX operation is global to the CPU: why must `kvm_intel` and your module never both own
  VMXON on one CPU? (That is why `scripts/build_kernel.sh` builds without KVM_INTEL.)
- What does `VMXOFF` do to a launched VMCS in memory? Who must `VMCLEAR` it first, and why?

## Run in QEMU
```
qemu/stage02.sh
```
Expect `ktvmx: stage01 caps: PASS` and `stage02 vmxon: PASS`. A hang or a reboot is
a #GP/#UD from a wrong CR4/CR0/FEATURE_CONTROL or from running VMXON twice: the
selftest guest uses `oops=panic panic=-1`, so look at `qemu.log`.
`module/s02_vmxon.rs` has not been compiled by the course author.
