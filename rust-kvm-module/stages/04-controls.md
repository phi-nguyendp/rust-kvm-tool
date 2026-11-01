# Stage 4: execution, exit and entry controls

Read: Intel SDM Vol. 3C Appendix A.2-A.5 (the capability MSRs: pin-based, primary and
secondary processor-based, exit, entry; "allowed 0-settings / allowed 1-settings"), A.1
(IA32_VMX_BASIC, bit 55), chapter 24.6-24.8 (the control fields themselves), 26.2.1 (the
checks the CPU runs on them). Section numbers are for recent SDMs; the appendices are stable.
Kernel: `arch/x86/kvm/vmx/vmx.c`: `adjust_vmx_controls()`, `setup_vmcs_config()`, `init_vmcs()`;
`struct vmcs_config` in `vmx.h`; the control bits in `arch/x86/include/asm/vmx.h`.
Files: `src/vmx_controls.rs`, `module/s04_controls.rs` (provided). Test: `tests/stage04_controls.rs`.
Demo: `qemu/stage04.sh` (logs the controls your CPU gives you). Needs stages 1-3 only for the demo.

A VMCS has dozens of control bits that decide *what makes the guest exit* and how entry and exit
behave. Almost every one is optional on some CPU and forced on others, so the first job of a
hypervisor is to ask the hardware what is possible and keep the answer.

## Tasks
1. `adjust_vmx_controls(min, opt, msr)`: the one-liner every VMX hypervisor has. The MSR's low
   half says which bits **must be 1**, the high half which bits **may be 1**. Request
   `min | opt`, clear what is not allowed, set what is forced, fail if a `min` bit is gone.
2. `vmx_ctl_msr`: pick the TRUE_* MSR or the legacy one, depending on IA32_VMX_BASIC bit 55.
3. `setup_vmcs_config(&impl MsrOps)`: decode IA32_VMX_BASIC, reject hardware we cannot use,
   choose all five control words, read the CR fixed-bit MSRs. The `MIN_*`/`OPT_*` constants are
   given: the interesting part is that **unrestricted guest requires EPT**.
4. `vmx_vmcs_controls_write(vmcs, cfg)`: put them into the current VMCS, plus the
   "nothing special" defaults so that nothing stale survives. Do not write the secondary field
   if the primary controls don't activate it.

The tests run on a fake MSR file (`MockMsrs`) and a fake VMCS (`MockVmcs`). No root, no KVM.
`module/s04_controls.rs` runs the very same `setup_vmcs_config` on the real `rdmsr` in QEMU.

## Think about
- Why does a capability MSR report "allowed 0" and "allowed 1" separately instead of one
  bitmask? What does a bit that is 1 in both halves mean, and 0 in both?
- Before "true" controls existed (`IA32_VMX_BASIC[55]`), some bits were reported as must-be-1
  even though the CPU would accept 0. What did that cost a hypervisor, and what does the kernel
  do with the TRUE MSRs that it could not do with the plain ones?
- We do not set `CPU_BASED_USE_MSR_BITMAPS` or `CPU_BASED_USE_IO_BITMAPS` yet. What does the
  guest's `rdmsr` then do? What does `CPU_BASED_UNCOND_IO_EXITING` do and why is it not
  redundant with the bitmaps? (Stage 10.)
- `VM_EXIT_ACK_INTR_ON_EXIT` is off. Who takes an interrupt that arrives while the guest runs,
  and when? (Stage 6 gives the mechanism, stage 9 the alternative.)
- Why is `VM_ENTRY_IA32E_MODE` not part of the machine-wide `vmcs_config`?
- The kernel enables EFER save/load only if the CPU offers it and falls back to `wrmsr`
  around the entry otherwise. We make it a `MIN_*` bit instead. What do we lose?

## Run the demo
```
cargo test --test stage04_controls
make -C module && qemu/stage04.sh      # in-QEMU: logs "ktvmx: vmcs_config: ..."
```
Compare the logged `cpu2` value with `grep -o 'ept\|vpid\|unrestricted_guest' /proc/cpuinfo`
on the machine running QEMU (the VMX flags line lists the secondary controls the CPU supports).
