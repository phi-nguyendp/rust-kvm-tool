# Stage 1: Does this CPU do VMX, and what does it support?

Read: Intel SDM Vol. 3C ch. 23 ("Discovering Support for VMX", "Enabling and Entering
VMX Operation"), Appendix A.1 (IA32_VMX_BASIC), A.3 (the controls MSRs, skim),
A.10 (IA32_VMX_EPT_VPID_CAP). SDM chapter/section numbers move between editions; the
appendix titles are stable. Kernel: `arch/x86/kernel/cpu/feat_ctl.c`
(`init_ia32_feat_ctl`), `arch/x86/include/asm/vmx.h` (`vmx_basic_vmcs_*`),
`arch/x86/kvm/vmx/capabilities.h` (`cpu_has_vmx_ept_*`).
Files: `src/vmx_caps.rs` (+ `module/s01_caps.rs`, provided). Test:
`tests/stage01_vmx_caps.rs`. QEMU: `qemu/stage01.sh`.
First, read `docs/setup.md`: the whole course runs in a nested-VMX QEMU guest.

## Tasks
1. `cpu_has_vmx`: CPUID.1:ECX bit 5.
2. `vmx_feat_ctl_state` / `vmx_disabled_by_bios`: the three states of
   IA32_FEATURE_CONTROL (locked and enabled; unlocked, so the OS may enable and lock
   it; locked but disabled). Mirror what `init_ia32_feat_ctl` does.
3. `vmx_basic_decode`: every field of IA32_VMX_BASIC. The revision id and the VMCS size
   are used from stage 2 on; the memory type and the "true controls" bit in stage 4.
4. `vmx_ept_vpid_decode`: IA32_VMX_EPT_VPID_CAP. Stage 8 needs `pwl4`, `wb`; stage 10
   needs `invvpid`.
5. `vmx_probe`: tie it together. Mind the *order*: never read an MSR that may not exist.

The tests feed `MockMsrs` with real Skylake values (no `/dev/kvm`, no root needed).
`module/s01_caps.rs` runs the same `vmx_probe` against the real CPU at `insmod` time.

## Think about
- Why is IA32_FEATURE_CONTROL "lock once and forget until reset"? Who is it protecting
  from whom? What would an OS that finds it unlocked and VMX-disabled do, and why
  does the Linux kernel not simply write it on every boot?
- Bit 1 (VMXON inside SMX) vs bit 2 (outside SMX): when would a normal OS ever care
  about SMX? (Look up Intel TXT / `tboot`.)
- Why do the capability MSRs come in "allowed-0 / allowed-1" pairs (stage 4 builds on
  this)? In IA32_VMX_PROCBASED_CTLS, which dword says "may be 1"?
- `rdmsr` of a nonexistent MSR is a #GP. KVM uses `rdmsr_safe` with an exception-table
  fixup. Why does `vmx_probe` still not read IA32_VMX_EPT_VPID_CAP unconditionally?
- Why is IA32_VMX_BASIC[48] (32-bit physical addresses) irrelevant on every CPU you
  can buy, yet worth modelling?
- You run under QEMU, which runs under KVM, which runs under QEMU/KVM (this laptop is a VM
  too). Which of the values you decode are the real CPU's, and which were rewritten by
  a hypervisor in between?

## Run in QEMU (needs the Rust kernel build; see docs/setup.md)
```
qemu/stage01.sh
```
It boots the guest, runs `insmod ktvmx.ko selftest=1` and expects
`ktvmx: stage01 caps: PASS` in dmesg plus the `vmx_basic:`/`ept:` lines. If it says
`CPUID.1:ECX.VMX is clear`, QEMU was not started with `-cpu host,vmx=on`, or the
outer VM does not expose VMX.
`module/s01_caps.rs` was written without a Rust-enabled kernel at hand and has not been
compiled: a small compile fix is possible.
